import { common } from '@kit.AbilityKit';
import { UIContext } from '@kit.ArkUI';
import { util } from '@kit.ArkTS';
import {
  AgentCardDto,
  AgentStageDeltaDto,
  AskSideQuestionRequestDto,
  BriefingDecisionDto,
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
  ConnectedHostSummaryDto,
  ControlGoalRunRequestDto,
  ConversationSearchHitDto,
  CreateSchedulePayloadDto,
  CreateSessionRequestDto,
  CreateWorkspaceEntryRequestDto,
  DagStepSpecDto,
  DeleteWorkspaceEntryRequestDto,
  DiffFileDto,
  DiffLineDto,
  ExportOptionsDto,
  ExportedTranscriptDto,
  FleetOverviewDto,
  FleetSidebarDto,
  ForkAgentRequestDto,
  ForkAgentResultDto,
  GitCommitResultDto,
  GitConflictEntryDto,
  GitCreatePrResultDto,
  GitGeneratedCommitMessageDto,
  GitPrStatusDto,
  GitRebaseStatusDto,
  GitReviewDto,
  GitStashEntryDto,
  GitStatusSnapshotDto,
  GithubDeviceFlowPollDto,
  GithubDeviceFlowStartDto,
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
  OrbitMeshPeerRow,
  PendingPlanApprovalDto,
  PermissionBatchItemDto,
  PersonalAgentActivityEntryDto,
  PersonalAgentBriefingDto,
  PersonalRouterConfigDto,
  PromptBadgeDto,
  ProviderModelCatalogDto,
  ProviderModelSpecDto,
  ProviderSubagentTimelineDto,
  PushPreferencesDto,
  QueuedPromptDto,
  QueuedPromptItem,
  QuotedDiffSnippet,
  RegisterPushDeviceRequestDto,
  RenameWorkspaceEntryRequestDto,
  RenderDiagramRequestDto,
  RenderDiagramViewDto,
  ResolvedModelSelectionDto,
  RewindAgentResultDto,
  RouterHostCapabilitiesDto,
  RouterStatusSnapshotDto,
  ScheduleRunRecordDto,
  ScheduleSpecDto,
  SearchMessagesRequestDto,
  SearchMessagesResponseDto,
  SessionPlanStepDto,
  SessionSubagentEntryDto,
  SessionUsageDto,
  SlashCommandDto,
  SpawnWorkspacePeerRequestDto,
  StagedAttachmentDto,
  StartDagWorkflowRequestDto,
  StartGoalRunRequestDto,
  StartMonitorWatchRequestDto,
  StopMonitorWatchRequestDto,
  TerminalRowDto,
  TerminalScreenDto,
  ThreadPrefSeedDto,
  TimelineItemDto,
  TimerConditionDto,
  ToolStepDescriptorDto,
  WorkflowRunRecordDto,
  WorkspaceBoardSnapshotDto,
  WorkspaceDto,
  WorkspaceEventRecordDto,
  WorkspaceFileViewDto,
  WriteWorkspaceFileRequestDto,
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
import { OrbitCoreBridge } from './OrbitCoreBridge';
import { OrbitVoiceInputController } from './OrbitVoiceInput';
import { orbitLogInfo, orbitLogWarn } from './OrbitLogger';
import { OrbitPreferencesStore } from './OrbitPreferences';
import { OrbitPerfProbe } from './OrbitPerfProbe';
import {
  OrbitEventFlushHandler,
  OrbitEventFrameScheduler,
  isStageScopeVisibleForEvent
} from './OrbitEventFrameScheduler';
import {
  orbitApplySystemBarTheme,
  orbitBindThemeApplicationContext,
  orbitNormalizeThemeMode,
  orbitSetThemeModePreference
} from './OrbitWindowTheme';
import {
  OrbitStageDeltaAppliedCallback,
  OrbitStageSession,
  OrbitStageStore,
  resolveStageTailLimit
} from './stores/OrbitStageStore';
import {
  OrbitStageTurnModel,
  OrbitTimelineItemModel
} from './viewmodels/OrbitStageModels';
import { OrbitFleetStore } from './stores/OrbitFleetStore';
import {
  OrbitBriefingAppliedCallback,
  OrbitPersonalAgentStore,
  OrbitPersonalAgentVisitStorage
} from './stores/OrbitPersonalAgentStore';
import {
  OrbitComposerStore,
  OrbitComposerToastSink
} from './stores/OrbitComposerStore';
import {
  DeepLinkAgentValidation,
  parseOrbitOpenDeepLink,
  resolveDeepLinkPostNavigationAction,
  validateDeepLinkAgentInDetail
} from './OrbitDeepLink';
import { buildDecisionFocusTarget } from './OrbitDecisionFocusModel';
import {
  gitTargetPathsEqual,
  isGitCollabResponseCurrent,
  shouldApplyGitDiffForTarget
} from './OrbitDiffInspectorModel';
import {
  DEFAULT_LOCAL_NOTIFICATION_COPY,
  LocalNotificationCopyStrings,
  NotificationVisibilityContext,
  OrbitLocalNotificationPayload,
  OrbitNotificationEngine,
  clonePushPreferences,
  defaultPushPreferences
} from './OrbitNotificationEngine';
import {
  BatchApprovalHostOutcome,
  FrozenBatchApprovalTarget,
  aggregateBatchApprovalOutcomes,
  classifyBatchApprovalFeedback,
  intersectFrozenBatchTargets,
  normalizePersonalRouterConfig,
  parseBatchApprovalHostResponse,
  reconcileBriefingPresentationOnResize,
  resolvePaIdentityArrivalPresentation,
  resolvePaSessionSelectPresentation,
  snapshotBatchApprovalTargets
} from '../components/personal-agent/PersonalAgentBriefingModel';
import {
  checkHarmonyNotificationEnabled,
  publishOrbitDeepLinkNotification,
  readAuthorizedClipboardText,
  readUriBytesAsync,
  registerHarmonyPushToken,
  requestHarmonyNotificationPermission,
  scanPairingQrCode,
  setHarmonyAppBadgeNumber,
  triggerNativeHaptic,
  writeClipboardText
} from './OrbitDeviceBridge';

const EMPTY_STAGE_TURNS: OrbitStageTurnModel[] = [];
const EMPTY_TIMELINE_MODELS: OrbitTimelineItemModel[] = [];
const EMPTY_DIFF_FILES: DiffFileDto[] = [];
const EMPTY_PLAN_STEPS: SessionPlanStepDto[] = [];
const EMPTY_SUBAGENTS: SessionSubagentEntryDto[] = [];

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

class OrbitStoreEventFlushAdapter implements OrbitEventFlushHandler {
  private readonly owner: OrbitHarmonyStore;

  constructor(owner: OrbitHarmonyStore) {
    this.owner = owner;
  }

  getSelectedHostId(): string {
    return this.owner.fleetStore.selectedHostId;
  }

  isStageScopeVisible(hostId: string, agentId: string): boolean {
    return isStageScopeVisibleForEvent(
      this.owner.isInBackgroundMode(),
      this.owner.isNewSessionDraft,
      this.owner.fleetStore.selectedHostId,
      this.owner.selectedAgentId,
      hostId,
      agentId
    );
  }

  isGitScopeVisible(hostId: string, cwd: string): boolean {
    if (!this.owner.showGitDiffSheet && !this.owner.showInlineDiffInspector) {
      return false;
    }
    const targetHost = this.owner.resolvedDiffHostId();
    if (targetHost !== hostId || hostId.length === 0) {
      return false;
    }
    return cwd.length === 0 || cwdPathsEquivalent(cwd, this.owner.resolvedDiffCwd());
  }

  isTerminalScopeVisible(hostId: string, slot: number): boolean {
    if (!this.owner.showPtySheet || this.owner.fleetStore.selectedHostId !== hostId || hostId.length === 0) {
      return false;
    }
    const detail = this.owner.fleetStore.currentHostDetail;
    const activeSlot = detail !== null && detail.active_terminal_slot > 0
      ? detail.active_terminal_slot
      : 1;
    return slot === activeSlot;
  }

  onFleetDirty(): void {
    this.owner.handleSchedulerFleetDirty();
  }

  onBriefingDirty(): void {
    this.owner.personalAgentStore.refreshBriefing(false);
  }

  onSelectedHostDirty(hostId: string): void {
    this.owner.handleSchedulerSelectedHostDirty(hostId);
  }

  onBackgroundHostDirty(hostId: string): void {
    this.owner.handleSchedulerBackgroundHostDirty(hostId);
  }

  onVisibleTimelineDirty(hostId: string, agentId: string): void {
    this.owner.handleSchedulerVisibleTimelineDirty(hostId, agentId);
  }

  onBackgroundTimelineDirty(hostId: string, agentId: string): void {
    this.owner.handleSchedulerBackgroundTimelineDirty(hostId, agentId);
  }

  onVisibleGitDirty(hostId: string, cwd: string): void {
    this.owner.handleSchedulerVisibleGitDirty(hostId, cwd);
  }

  onBackgroundGitDirty(hostId: string, cwd: string): void {
    this.owner.handleSchedulerBackgroundGitDirty(hostId, cwd);
  }

  onVisibleTerminalDirty(hostId: string, slot: number): void {
    this.owner.handleSchedulerVisibleTerminalDirty(hostId, slot);
  }

  onResync(): void {
    this.owner.handleSchedulerResync();
  }
}

class OrbitStoreStageDeltaAdapter implements OrbitStageDeltaAppliedCallback {
  private readonly owner: OrbitHarmonyStore;

  constructor(owner: OrbitHarmonyStore) {
    this.owner = owner;
  }

  onDeltaApplied(session: OrbitStageSession, delta: AgentStageDeltaDto): void {
    this.owner.handleStageDeltaAppliedFromStore(session, delta);
  }
}

class OrbitStoreComposerToastAdapter implements OrbitComposerToastSink {
  private readonly owner: OrbitHarmonyStore;

  constructor(owner: OrbitHarmonyStore) {
    this.owner = owner;
  }

  onDraftToast(message: string, isError: boolean): void {
    this.owner.showToast(message, isError);
  }
}

class OrbitStoreBriefingAppliedAdapter implements OrbitBriefingAppliedCallback {
  private readonly owner: OrbitHarmonyStore;

  constructor(owner: OrbitHarmonyStore) {
    this.owner = owner;
  }

  onBriefingApplied(briefing: PersonalAgentBriefingDto): void {
    this.owner.handleBriefingApplied(briefing);
  }
}

class OrbitStorePaVisitStorageAdapter implements OrbitPersonalAgentVisitStorage {
  private readonly owner: OrbitHarmonyStore;

  constructor(owner: OrbitHarmonyStore) {
    this.owner = owner;
  }

  getLastVisitMs(hostId: string, agentId: string): number {
    return this.owner.readPersistedPersonalAgentVisitMs(hostId, agentId);
  }

  setLastVisitMs(hostId: string, agentId: string, tsMs: number): void {
    this.owner.writePersistedPersonalAgentVisitMs(hostId, agentId, tsMs);
  }
}

@ObservedV2
export class OrbitHarmonyStore {
  private static instance: OrbitHarmonyStore | null = null;
  private bridge: OrbitCoreBridge | null = null;
  private abilityContext: common.Context | undefined = undefined;
  private isAppInBackground: boolean = false;
  private notificationSeq: number = 1000;
  private readonly notificationEngine: OrbitNotificationEngine = new OrbitNotificationEngine();
  private readonly pushPrefsByHost: Map<string, PushPreferencesDto> = new Map<string, PushPreferencesDto>();
  private routerDiscoveryInFlight: Promise<void> | null = null;
  private prefsStore: OrbitPreferencesStore = new OrbitPreferencesStore();
  private voiceController: OrbitVoiceInputController = new OrbitVoiceInputController();
  private voiceTimerId: number = -1;
  private pendingDeepLink: string = '';
  private pendingDeepLinkEventKind: string = '';
  private pendingDeepLinkRequestId: string = '';
  private pendingDeepLinkSeq: number = 0;
  private pendingOpenHostId: string = '';
  private pendingOpenAgentId: string = '';
  private pendingOpenEventKind: string = '';
  private pendingOpenRequestId: string = '';
  private pendingOpenSeq: number = 0;
  private pendingAgentValidationHostId: string = '';
  private pendingAgentValidationAgentId: string = '';
  private pendingAgentValidationEventKind: string = '';
  private pendingAgentValidationRequestId: string = '';
  private pendingAgentValidationSeq: number = 0;
  private pendingReplayHostId: string = '';
  private pendingReplayAgentId: string = '';
  private pendingReplayFramesJson: string = '';
  private replayFinishTimerId: number = -1;
  private gitDirty: boolean = false;
  private gitCollabRequestSeq: number = 0;
  private briefingDismissedByUserForPaKey: string = '';
  private automationsRefreshSeq: number = 0;
  private sideQuestionGen: number = 0;

  // Domain Stores (PR-1 composition root)
  public readonly stageStore: OrbitStageStore = new OrbitStageStore();
  public readonly fleetStore: OrbitFleetStore = new OrbitFleetStore();
  public readonly personalAgentStore: OrbitPersonalAgentStore = new OrbitPersonalAgentStore();
  public readonly composerStore: OrbitComposerStore = new OrbitComposerStore();
  public readonly eventScheduler: OrbitEventFrameScheduler;

  @Trace public isNewSessionDraft: boolean = false;
  @Trace public keyProtectionMode: string = 'huks_hardware';
  @Trace public isFoldableDevice: boolean = false;
  @Trace public foldPosture: string = 'unknown';
  @Trace public showGlobalOverview: boolean = false;
  @Trace public reducedMotion: boolean = false;
  @Trace public showPersonalAgentSetup: boolean = false;
  @Trace public briefingViewingOpen: boolean = false;
  @Trace public showBriefingPanel: boolean = true;
  @Trace public showBriefingSheet: boolean = false;
  @Trace public decisionFocusRevision: number = 0;
  @Trace public pendingFocusDecisionHostId: string = '';
  @Trace public pendingFocusDecisionAgentId: string = '';
  @Trace public pendingFocusDecisionEventKind: string = '';
  @Trace public pendingFocusDecisionRequestId: string = '';
  @Trace public pendingFocusDecisionSeq: number = 0;
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
  private diffInspectorOpenedByUser: boolean = false;
  @Trace public showPtySheet: boolean = false;
  @Trace public showModelPickerSheet: boolean = false;
  @Trace public showContextPickerSheet: boolean = false;
  @Trace public showHostSettingsSheet: boolean = false;
  @Trace public showDirBrowser: boolean = false;
  @Trace public showAutomationsSheet: boolean = false;
  @Trace public showWorkspaceBoardSheet: boolean = false;
  @Trace public showSideQuestionSheet: boolean = false;
  @Trace public showArchivedSessionsSheet: boolean = false;
  @Trace public showFileExplorerSheet: boolean = false;
  @Trace public showGlobalSearchSheet: boolean = false;
  @Trace public showRouterMeshSheet: boolean = false;
  @Trace public showExportTranscriptSheet: boolean = false;
  @Trace public showSubagentSheet: boolean = false;
  @Trace public activeSessionUsage: SessionUsageDto | null = null;
  @Trace public inspectedSubagentId: string = '';
  @Trace public inspectedSubagentTitle: string = '';
  @Trace public inspectedSubagentItems: TimelineItemDto[] = [];
  @Trace public isVoiceRecording: boolean = false;
  @Trace public voiceInterimTranscript: string = '';
  @Trace public voiceElapsedSeconds: number = 0;
  @Trace public terminalRawMode: boolean = true;
  @Trace public inspectedFilePath: string = '';
  @Trace public inspectedFileView: WorkspaceFileViewDto | null = null;
  @Trace public fileExplorerBusy: boolean = false;
  @Trace public gitWriteBusy: boolean = false;
  @Trace public gitPrStatus: GitPrStatusDto | null = null;
  @Trace public gitStashes: GitStashEntryDto[] = [];
  @Trace public gitConflicts: GitConflictEntryDto[] = [];
  @Trace public gitRebaseStatus: GitRebaseStatusDto | null = null;
  @Trace public globalSearchQuery: string = '';
  @Trace public globalSearchHits: ConversationSearchHitDto[] = [];
  @Trace public globalSearchBusy: boolean = false;
  @Trace public globalSearchRemoteQueried: number = 0;
  @Trace public globalSearchRemoteFailed: number = 0;
  @Trace public deviceLinkUri: string = '';
  @Trace public githubDeviceFlow: GithubDeviceFlowStartDto | null = null;
  @Trace public pushSyncedHostIds: string[] = [];
  @Trace public pushPrefs: PushPreferencesDto = {
    enabled: true,
    notify_permission_required: true,
    notify_turn_completed: true,
    notify_agent_error: true,
    notify_goal_finished: true,
    notify_timer_fired: true,
    include_message_preview: false
  };
  @Trace public scheduleRunsById: Record<string, ScheduleRunRecordDto[]> = {};
  @Trace public collapsedProjectIds: string[] = [];
  @Trace public slashCommands: SlashCommandDto[] = [];
  private cachedFallbackCatalogs: ProviderModelCatalogDto[] = [];
  private diagramRenderCache: Map<string, RenderDiagramViewDto> = new Map<string, RenderDiagramViewDto>();
  private diagramRenderOrder: string[] = [];
  private pinnedThreadsFilePath: string = '';
  private draftPrefsFilePath: string = '';
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

  // Multi-Host Fleet & Workspace Facade State
  @Trace public nativeAvailable: boolean = false;
  @Trace public selectedAgentId: string = '';
  @Trace public currentAgent: AgentCardDto | null = null;
  @Trace public activeCwd: string = '~';

  // OmniComposer & Context State (draft text & slash directive delegated to OrbitComposerStore)
  @Trace public composerProvider: string = 'antigravity';
  @Trace public composerModel: string = '';
  @Trace public composerEffort: string = '';
  @Trace public composerMode: string = 'accept_edits';
  @Trace public composerBadges: PromptBadgeDto[] = [];
  @Trace public stagedAttachments: ComposerAttachmentItem[] = [];
  @Trace public inlineMentions: InlineMentionRegistration[] = [];
  @Trace public isStagingAttachment: boolean = false;
  @Trace public quotedSnippets: QuotedDiffSnippet[] = [];
  @Trace public worktreeBranch: string = '';
  @Trace public workspaceFiles: string[] = [];
  @Trace public activeQueuedPrompts: QueuedPromptItem[] = [];
  public queuedPromptsByAgent: Map<string, QueuedPromptItem[]> = new Map<string, QueuedPromptItem[]>();

  // Git Working Tree Inspector State (independent target scope so decision Diff previews
  // are never overwritten by unrelated PA Stage deltas).
  @Trace public diffTargetHostId: string = '';
  @Trace public diffTargetCwd: string = '';
  @Trace public isDiffTargetPinned: boolean = false;
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

  constructor() {
    this.eventScheduler = new OrbitEventFrameScheduler(new OrbitStoreEventFlushAdapter(this));
    this.stageStore.setDeltaAppliedCallback(new OrbitStoreStageDeltaAdapter(this));
    this.composerStore.setToastSink(new OrbitStoreComposerToastAdapter(this));
    this.personalAgentStore.setBriefingAppliedCallback(new OrbitStoreBriefingAppliedAdapter(this));
    this.personalAgentStore.bindVisitStorage(new OrbitStorePaVisitStorageAdapter(this));
  }

  public readPersistedPersonalAgentVisitMs(hostId: string, agentId: string): number {
    return this.prefsStore.getPersonalAgentLastVisitMs(hostId, agentId);
  }

  public writePersistedPersonalAgentVisitMs(hostId: string, agentId: string, tsMs: number): void {
    this.prefsStore.setPersonalAgentLastVisitMs(hostId, agentId, tsMs);
  }

  public markPersonalAgentVisitedNow(hostId: string, agentId: string): void {
    this.personalAgentStore.recordPersonalAgentVisitNow(hostId, agentId, Date.now());
  }

  public clearPendingDecisionFocus(): void {
    this.pendingFocusDecisionHostId = '';
    this.pendingFocusDecisionAgentId = '';
    this.pendingFocusDecisionEventKind = '';
    this.pendingFocusDecisionRequestId = '';
    this.pendingFocusDecisionSeq = 0;
  }

  private resolveResourceText(res: Resource, fallback: string): string {
    if (this.abilityContext === undefined) {
      return fallback;
    }
    try {
      const text = this.abilityContext.resourceManager.getStringSync(res.id);
      if (typeof text === 'string' && text.trim().length > 0) {
        return text;
      }
    } catch (_err) {
      // Fall back cleanly when running outside a full ability resource context
    }
    return fallback;
  }

  public showLocalizedToast(
    res: Resource,
    fallback: string,
    isError: boolean = false,
    args?: (string | number)[]
  ): void {
    let text = this.resolveResourceText(res, fallback);
    if (args !== undefined && args.length > 0) {
      let argIdx = 0;
      text = text.replace(/%([1-9][0-9]*\$)?[sd]/g, (_m: string, posGroup: string | undefined): string => {
        if (posGroup !== undefined && posGroup.length > 1) {
          const slot = Number.parseInt(posGroup.substring(0, posGroup.length - 1), 10) - 1;
          if (slot >= 0 && slot < args.length) {
            return String(args[slot]);
          }
        }
        if (argIdx < args.length) {
          const val = String(args[argIdx]);
          argIdx += 1;
          return val;
        }
        return _m;
      });
    }
    this.showToast(text, isError);
  }

  private resolveLocalNotificationCopyStrings(): LocalNotificationCopyStrings {
    return {
      permissionTitle: this.resolveResourceText(
        $r('app.string.notify_permission_title'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.permissionTitle
      ),
      permissionBody: this.resolveResourceText(
        $r('app.string.notify_permission_body'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.permissionBody
      ),
      turnCompletedTitle: this.resolveResourceText(
        $r('app.string.notify_turn_completed_title'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.turnCompletedTitle
      ),
      turnCompletedBody: this.resolveResourceText(
        $r('app.string.notify_turn_completed_body'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.turnCompletedBody
      ),
      agentErrorTitle: this.resolveResourceText(
        $r('app.string.notify_agent_error_title'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.agentErrorTitle
      ),
      agentErrorBody: this.resolveResourceText(
        $r('app.string.notify_agent_error_body'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.agentErrorBody
      ),
      goalFinishedTitle: this.resolveResourceText(
        $r('app.string.notify_goal_finished_title'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.goalFinishedTitle
      ),
      goalFinishedBody: this.resolveResourceText(
        $r('app.string.notify_goal_finished_body'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.goalFinishedBody
      ),
      timerFiredTitle: this.resolveResourceText(
        $r('app.string.notify_timer_fired_title'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.timerFiredTitle
      ),
      timerFiredBody: this.resolveResourceText(
        $r('app.string.notify_timer_fired_body'),
        DEFAULT_LOCAL_NOTIFICATION_COPY.timerFiredBody
      )
    };
  }

  private buildNotificationVisibilityContext(): NotificationVisibilityContext {
    return {
      isAppInBackground: this.isAppInBackground,
      isNewSessionDraft: this.isNewSessionDraft,
      showPersonalAgentSetup: this.showPersonalAgentSetup,
      selectedHostId: this.selectedHostId,
      selectedAgentId: this.selectedAgentId
    };
  }

  private hydratePushPreferencesFromStorage(): void {
    const persisted = this.prefsStore.getPushPreferencesMap();
    this.pushPrefsByHost.clear();
    const keys = Object.keys(persisted);
    for (let i = 0; i < keys.length; i += 1) {
      const k = keys[i];
      this.pushPrefsByHost.set(k, clonePushPreferences(persisted[k]));
    }
    this.pushPrefs = clonePushPreferences(this.getPushPreferencesForHost(this.selectedHostId));
  }

  public getPushPreferencesForHost(hostId: string): PushPreferencesDto {
    const cleanHost = hostId.trim();
    if (cleanHost.length > 0) {
      const hostScoped = this.pushPrefsByHost.get(cleanHost);
      if (hostScoped !== undefined) {
        return hostScoped;
      }
    }
    const fallback = this.pushPrefsByHost.get('__default__');
    if (fallback !== undefined) {
      return fallback;
    }
    return defaultPushPreferences();
  }

  public handleSchedulerFleetDirty(): void {
    this.reloadFleetAsync();
    this.personalAgentStore.refreshBriefing(false);
  }

  public handleSchedulerSelectedHostDirty(hostId: string): void {
    this.reloadHostAsync(hostId);
    this.reloadWorkspaceFiles();
    this.personalAgentStore.refreshBriefing(false);
  }

  public handleSchedulerBackgroundHostDirty(_hostId: string): void {
    // Strictly avoid polling full HostDetail for unselected background hosts (§3.3):
    // refresh only zero-IO in-memory sidebar & briefing projections to drive background badges/notifications.
    this.refreshSidebarAsync();
    this.personalAgentStore.refreshBriefing(false);
  }

  public handleSchedulerBackgroundTimelineDirty(hostId: string, agentId: string): void {
    // Strictly comply with §3.3: invisible timeline events ONLY mark the row dirty (0 HostDetail RPCs).
    this.fleetStore.markConversationDirty(hostId, agentId);
  }

  public isInBackgroundMode(): boolean {
    return this.isAppInBackground;
  }

  public handleBriefingApplied(briefing: PersonalAgentBriefingDto): void {
    const pa = briefing.personal_agent;
    if (pa !== null && pa.host_id.length > 0 && pa.agent_id.length > 0) {
      this.fleetStore.setPersonalAgentKey(pa.host_id, pa.agent_id);
      if (
        !this.isNewSessionDraft &&
        this.selectedHostId === pa.host_id &&
        this.selectedAgentId === pa.agent_id
      ) {
        if (!this.personalAgentStore.activeVisitKeyMatches(pa.host_id, pa.agent_id)) {
          // Hydrate persisted previous-visit timestamp when cold-start deep link selected the PA
          // before the initial briefing snapshot arrived.
          this.personalAgentStore.enterPersonalAgentVisit(pa.host_id, pa.agent_id);
        }
        const paKey = `${pa.host_id}::${pa.agent_id}`;
        const userOpenedDiff = this.showInlineDiffInspector &&
          (this.diffInspectorOpenedByUser || this.isDiffTargetPinned);
        const arrived = resolvePaIdentityArrivalPresentation(
          this.windowWidthVp,
          true,
          this.briefingViewingOpen,
          this.showBriefingPanel,
          this.showBriefingSheet,
          userOpenedDiff,
          this.briefingDismissedByUserForPaKey === paKey,
          OrbitTokens.BP_EXPANDED_MIN
        );
        this.briefingViewingOpen = arrived.briefingViewingOpen;
        this.showBriefingPanel = arrived.showBriefingPanel;
        this.showBriefingSheet = arrived.showBriefingSheet;
        if (arrived.shouldCloseDefaultDiffInspector && !userOpenedDiff) {
          this.showInlineDiffInspector = false;
        }
      }
    } else {
      this.fleetStore.setPersonalAgentKey('', '');
    }
    const needsCount = briefing.counts !== undefined && briefing.counts !== null
      ? briefing.counts.needs_you
      : 0;
    setHarmonyAppBadgeNumber(needsCount).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'setHarmonyAppBadgeNumber failed', err);
    });

    const payloads = this.notificationEngine.observeBriefing(
      briefing,
      this.buildNotificationVisibilityContext(),
      (hid: string): PushPreferencesDto => this.getPushPreferencesForHost(hid),
      this.resolveLocalNotificationCopyStrings()
    );
    this.dispatchLocalNotificationPayloads(payloads);
  }

  private nextNotificationId(): number {
    this.notificationSeq += 1;
    if (this.notificationSeq >= 999999) {
      this.notificationSeq = 1000;
    }
    return this.notificationSeq;
  }

  private dispatchLocalNotificationPayloads(payloads: readonly OrbitLocalNotificationPayload[]): void {
    if (this.abilityContext === undefined || payloads.length === 0) {
      return;
    }
    for (let i = 0; i < payloads.length; i += 1) {
      const p = payloads[i];
      if (!this.getPushPreferencesForHost(p.hostId).enabled || p.hostId.length === 0) {
        continue;
      }
      publishOrbitDeepLinkNotification(
        this.abilityContext,
        this.nextNotificationId(),
        p.title,
        p.body,
        p.hostId,
        p.agentId,
        p.eventKind,
        p.requestId,
        p.seq
      ).catch((err: Object) => {
        orbitLogWarn('OrbitStore', 'dispatchLocalNotificationPayloads failed', err);
      });
    }
  }

  private observeHostDetailForNotifications(hostId: string, detail: HostDetailDto): void {
    const prefs = this.getPushPreferencesForHost(hostId);
    const payloads = this.notificationEngine.observeHostDetail(
      hostId,
      detail,
      this.buildNotificationVisibilityContext(),
      prefs,
      this.resolveLocalNotificationCopyStrings()
    );
    this.dispatchLocalNotificationPayloads(payloads);
  }

  public handleSchedulerVisibleTimelineDirty(hostId: string, agentId: string): void {
    const session = this.stageStore.getOrCreateSession(hostId, agentId);
    this.stageStore.fetchIncremental(session, this.stageTailLimit()).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'Incremental stage fetch failed', err);
    });
  }

  public handleSchedulerVisibleGitDirty(_hostId: string, _cwd: string): void {
    this.gitDirty = false;
    this.reloadGitDiff(this.resolvedDiffCwd(), this.resolvedDiffHostId());
    if (!this.isDiffTargetPinned) {
      this.reloadWorkspaceFiles();
    }
  }

  public handleSchedulerBackgroundGitDirty(_hostId: string, _cwd: string): void {
    this.gitDirty = true;
  }

  public handleSchedulerVisibleTerminalDirty(_hostId: string, _slot: number): void {
    this.reloadTerminal();
  }

  public handleSchedulerResync(): void {
    this.handleFullResync();
  }

  public handleStageDeltaAppliedFromStore(session: OrbitStageSession, delta: AgentStageDeltaDto): void {
    this.onStageDeltaApplied(session, delta);
  }

  // Facade accessors delegating cleanly to domain sub-stores

  public get hosts(): HostCardDto[] {
    return this.fleetStore.hosts;
  }

  public get projects(): ClientProjectDto[] {
    return this.fleetStore.projects;
  }

  public get totalPendingApprovals(): number {
    return this.fleetStore.totalPendingApprovals;
  }

  public get selectedHostId(): string {
    return this.fleetStore.selectedHostId;
  }

  public get currentHostDetail(): HostDetailDto | null {
    return this.fleetStore.currentHostDetail;
  }

  public get sidebarView(): FleetSidebarDto | null {
    return this.fleetStore.sidebarView;
  }

  public get sidebarProjectSort(): string {
    return this.fleetStore.sidebarProjectSort;
  }

  public set sidebarProjectSort(val: string) {
    this.fleetStore.sidebarProjectSort = val;
  }

  public get sidebarThreadFilter(): string {
    return this.fleetStore.sidebarThreadFilter;
  }

  public set sidebarThreadFilter(val: string) {
    this.fleetStore.sidebarThreadFilter = val;
  }

  public get sidebarSearchQuery(): string {
    return this.fleetStore.sidebarSearchQuery;
  }

  public set sidebarSearchQuery(val: string) {
    this.fleetStore.sidebarSearchQuery = val;
  }

  public get stageRevision(): number {
    return this.stageStore.activeRevision;
  }

  public get activeStageTurnKey(): string {
    return this.stageStore.activeTurnKey;
  }

  public get stageTurns(): OrbitStageTurnModel[] {
    return this.stageStore.activeSession !== null
      ? this.stageStore.activeSession.turns
      : EMPTY_STAGE_TURNS;
  }

  public get pendingPermissions(): OrbitTimelineItemModel[] {
    return this.stageStore.activeSession !== null
      ? this.stageStore.activeSession.pendingPermissions
      : EMPTY_TIMELINE_MODELS;
  }

  public get pendingQuestions(): OrbitTimelineItemModel[] {
    return this.stageStore.activeSession !== null
      ? this.stageStore.activeSession.pendingQuestions
      : EMPTY_TIMELINE_MODELS;
  }

  public get stagePendingPlanApproval(): PendingPlanApprovalDto | null {
    return this.stageStore.activePendingPlanApproval;
  }

  public get stagePlanSteps(): SessionPlanStepDto[] {
    const insights = this.stageStore.activeSession?.insights ?? null;
    return insights !== null && insights.plan_steps !== undefined
      ? insights.plan_steps
      : EMPTY_PLAN_STEPS;
  }

  public get stageSubagents(): SessionSubagentEntryDto[] {
    const insights = this.stageStore.activeSession?.insights ?? null;
    return insights !== null && insights.subagents !== undefined
      ? insights.subagents
      : EMPTY_SUBAGENTS;
  }

  public get stageHasOlderHistory(): boolean {
    const sess = this.stageStore.activeSession;
    return sess !== null ? (sess.hasOlderTurns || sess.hasOlderHistory) : false;
  }

  public get stageLoadingOlder(): boolean {
    return this.stageStore.activeSession !== null ? this.stageStore.activeSession.loadingOlder : false;
  }

  public get sessionDiffs(): DiffFileDto[] {
    return this.stageStore.activeSession !== null
      ? this.stageStore.activeSession.sessionDiffs
      : EMPTY_DIFF_FILES;
  }

  public get briefing(): PersonalAgentBriefingDto | null {
    return this.personalAgentStore.briefing;
  }

  public get briefingNeedsYou(): BriefingDecisionDto[] {
    return this.personalAgentStore.needsYou;
  }

  public get personalAgentActivity(): PersonalAgentActivityEntryDto[] {
    return this.personalAgentStore.activity;
  }

  public get routerHosts(): RouterHostCapabilitiesDto[] {
    return this.personalAgentStore.routerHosts;
  }

  public set routerHosts(val: RouterHostCapabilitiesDto[]) {
    this.personalAgentStore.setRouterHosts(val);
  }

  public get activeRouterStatus(): RouterStatusSnapshotDto | null {
    return this.personalAgentStore.activeRouterStatus;
  }

  public set activeRouterStatus(val: RouterStatusSnapshotDto | null) {
    this.personalAgentStore.activeRouterStatus = val;
  }

  public get primaryRouterHostId(): string {
    return this.personalAgentStore.primaryRouterHostId;
  }

  public set primaryRouterHostId(val: string) {
    this.personalAgentStore.primaryRouterHostId = val;
  }

  public get fleetMeshPeers(): OrbitMeshPeerRow[] {
    return this.personalAgentStore.fleetMeshPeers;
  }

  public set fleetMeshPeers(val: OrbitMeshPeerRow[]) {
    this.personalAgentStore.fleetMeshPeers = val;
  }

  public get routerMeshBusy(): boolean {
    return this.personalAgentStore.busy;
  }

  public set routerMeshBusy(val: boolean) {
    this.personalAgentStore.busy = val;
  }

  public get composerText(): string {
    return this.composerStore.text;
  }

  public set composerText(val: string) {
    this.composerStore.setText(val);
  }

  public get activeSlashDirective(): string {
    return this.composerStore.slashDirective;
  }

  public set activeSlashDirective(val: string) {
    this.composerStore.setSlashDirective(val);
  }

  public flushComposerDraftNow(): void {
    this.composerStore.flushNow();
  }

  public stageTailLimit(): number {
    return resolveStageTailLimit(this.windowWidthVp);
  }

  public bindUIContext(ctx: UIContext | null): void {
    this.eventScheduler.bindUIContext(ctx);
  }

  public static getInstance(): OrbitHarmonyStore {
    if (OrbitHarmonyStore.instance === null) {
      OrbitHarmonyStore.instance = new OrbitHarmonyStore();
    }
    return OrbitHarmonyStore.instance;
  }

  public initialize(dbPath: string, masterKeyB64: string = ''): void {
    if (this.pinnedThreadsFilePath.length === 0 && dbPath.length > 0) {
      this.pinnedThreadsFilePath = `${dbPath}.pinned_threads.json`;
    }
    if (this.draftPrefsFilePath.length === 0 && dbPath.length > 0) {
      this.draftPrefsFilePath = `${dbPath}.draft_prefs.json`;
      this.loadDraftPrefsFromDisk();
    }
    if (this.bridge !== null) {
      if (this.pendingDeepLink.length > 0) {
        const uri = this.pendingDeepLink;
        const evKind = this.pendingDeepLinkEventKind;
        const reqId = this.pendingDeepLinkRequestId;
        const seq = this.pendingDeepLinkSeq;
        this.pendingDeepLink = '';
        this.pendingDeepLinkEventKind = '';
        this.pendingDeepLinkRequestId = '';
        this.pendingDeepLinkSeq = 0;
        this.handleDeepLinkUri(uri, evKind, reqId, seq);
      }
      return;
    }
    try {
      this.bridge = OrbitCoreBridge.openWithMasterKeyOrFallback(dbPath, masterKeyB64);
      this.nativeAvailable = true;
      this.stageStore.bindBridge(this.bridge);
      this.stageStore.setTailLimit(this.stageTailLimit());
      this.fleetStore.bindBridge(this.bridge);
      this.personalAgentStore.bindBridge(this.bridge);
      this.composerStore.bindBridge(this.bridge);
      this.composerStore.hydrate();

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
            this.bridge.seedThreadPrefsJson(JSON.stringify(seeds));
          }
        }
        this.prefsStore.markMigratedThreadPrefsV1();
      }

      try {
        this.cachedFallbackCatalogs = JSON.parse(
          this.bridge.fallbackProviderCatalogsJson()
        ) as ProviderModelCatalogDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'Failed to parse fallback catalogs', err as Object);
      }
      try {
        this.slashCommands = JSON.parse(
          this.bridge.slashCommandsJson('harmony', null)
        ) as SlashCommandDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'Failed to parse slash commands', err as Object);
      }

      this.bridge.subscribeEvents((err: Error | null, payloadJson: string) => {
        if (err !== null || payloadJson.length === 0) {
          return;
        }
        this.eventScheduler.enqueue(payloadJson);
      });
      this.reloadFleet();
      this.refreshSidebar();
      this.personalAgentStore.refreshBriefing(true);
      this.refreshCloudSyncStatus();
      this.bridge.connectAllHosts();
      if (this.pushToken.length > 0) {
        this.syncPushTokenToAllHosts(false);
      }
      if (this.pendingDeepLink.length > 0) {
        const uri = this.pendingDeepLink;
        const evKind = this.pendingDeepLinkEventKind;
        const reqId = this.pendingDeepLinkRequestId;
        const seq = this.pendingDeepLinkSeq;
        this.pendingDeepLink = '';
        this.pendingDeepLinkEventKind = '';
        this.pendingDeepLinkRequestId = '';
        this.pendingDeepLinkSeq = 0;
        this.handleDeepLinkUri(uri, evKind, reqId, seq);
      }
      if (this.pendingReplayFramesJson.length > 0) {
        const rHost = this.pendingReplayHostId;
        const rAgent = this.pendingReplayAgentId;
        const rFrames = this.pendingReplayFramesJson;
        this.pendingReplayHostId = '';
        this.pendingReplayAgentId = '';
        this.pendingReplayFramesJson = '';
        this.startDebugStageReplay(rHost, rAgent, rFrames);
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
    this.abilityContext = ctx;
    this.prefsStore.init(ctx);
    this.hydratePushPreferencesFromStorage();
  }

  public bindApplicationContext(ctx: common.ApplicationContext): void {
    orbitBindThemeApplicationContext(ctx);
  }

  public applyPersistedThemeMode(): void {
    const saved = orbitNormalizeThemeMode(this.prefsStore.getThemeMode());
    this.themeMode = saved;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
    orbitSetThemeModePreference(saved);
    orbitApplySystemBarTheme(this.palette.isDark);
  }

  public setThemeMode(mode: string): void {
    const normalized = orbitNormalizeThemeMode(mode);
    this.themeMode = normalized;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
    this.prefsStore.setThemeMode(normalized);
    orbitSetThemeModePreference(normalized);
    orbitApplySystemBarTheme(this.palette.isDark);
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
    this.stageStore.trimBackgroundCaches();
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
    if (this.voiceTimerId !== -1) {
      clearInterval(this.voiceTimerId);
      this.voiceTimerId = -1;
    }
    if (this.replayFinishTimerId !== -1) {
      clearTimeout(this.replayFinishTimerId);
      this.replayFinishTimerId = -1;
    }
    this.voiceController.cancel();
    this.composerStore.dispose();
    this.personalAgentStore.dispose();
    this.eventScheduler.dispose();
  }

  public updateFoldPosture(isFoldable: boolean, posture: string): void {
    this.isFoldableDevice = isFoldable;
    this.foldPosture = posture;
    if (posture === 'expanded' && !this.isSidebarCollapsed) {
      this.sidebarOpen = true;
    } else if (posture === 'folded') {
      this.sidebarOpen = false;
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
      this.briefingViewingOpen = false;
      this.showBriefingPanel = false;
      this.showBriefingSheet = false;
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
    return this.fleetStore.otherHostsPendingApprovals();
  }

  public refreshSidebar(): void {
    this.refreshSidebarAsync();
  }

  public refreshSidebarAsync(): void {
    if (this.bridge === null) {
      return;
    }
    const activeAgent = (!this.isNewSessionDraft && this.selectedAgentId.length > 0)
      ? this.selectedAgentId
      : null;
    const activeCwdArg = this.activeCwd.length > 0 ? this.activeCwd : null;
    this.fleetStore.refreshSidebarAsync(activeAgent, activeCwdArg).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'refreshSidebarAsync failed', err);
    });
  }

  public setSidebarSearchQuery(query: string): void {
    this.fleetStore.setSidebarSearchQuery(query);
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
    return this.fleetStore.isProjectCollapsed(projectId);
  }

  public toggleProjectCollapsed(projectId: string): void {
    this.fleetStore.toggleProjectCollapsed(projectId);
    if (this.fleetStore.isProjectCollapsed(projectId)) {
      if (this.collapsedProjectIds.indexOf(projectId) < 0) {
        this.collapsedProjectIds = this.collapsedProjectIds.concat([projectId]);
      }
    } else {
      this.collapsedProjectIds = this.collapsedProjectIds.filter((id: string) => id !== projectId);
    }
    this.triggerHaptic(6);
  }

  public toggleProjectMoreExpanded(projectId: string): void {
    this.fleetStore.toggleProjectMoreExpanded(projectId);
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
    if (this.bridge === null || hostId.length === 0 || agentId.length === 0) {
      return;
    }
    this.bridge.togglePinAgent(hostId, agentId);
    this.refreshSidebar();
    this.triggerHaptic(8);
  }

  public markAgentRead(hostId: string, agentId: string): void {
    if (this.bridge === null || hostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.bridge.markAgentRead(hostId, agentId);
      this.refreshSidebarAsync();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'markAgentRead failed', err as Object);
    }
  }

  public markAgentUnread(hostId: string, agentId: string): void {
    if (this.bridge === null || hostId.length === 0 || agentId.length === 0) {
      return;
    }
    this.bridge.markAgentUnread(hostId, agentId);
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
    const prevWidthVp = this.windowWidthVp;
    const prevWasPhone = prevWidthVp < OrbitTokens.BP_FOLDABLE_MIN;
    const nextIsPhone = widthVp < OrbitTokens.BP_FOLDABLE_MIN;
    const prevWasExpanded = prevWidthVp >= OrbitTokens.BP_EXPANDED_MIN;
    const nextIsExpanded = widthVp >= OrbitTokens.BP_EXPANDED_MIN;
    this.windowWidthVp = widthVp;
    this.stageStore.setTailLimit(resolveStageTailLimit(widthVp));
    if (prevWasPhone && !nextIsPhone) {
      this.sidebarOpen = !this.isSidebarCollapsed;
    } else if (!prevWasPhone && nextIsPhone) {
      this.sidebarOpen = false;
    }
    const reconciledBriefing = reconcileBriefingPresentationOnResize(
      prevWidthVp,
      widthVp,
      this.isCurrentSessionPersonalAgent(),
      this.showBriefingSheet,
      this.showBriefingPanel,
      OrbitTokens.BP_EXPANDED_MIN,
      this.briefingViewingOpen
    );
    this.briefingViewingOpen = reconciledBriefing.briefingViewingOpen;
    this.showBriefingSheet = reconciledBriefing.showBriefingSheet;
    this.showBriefingPanel = reconciledBriefing.showBriefingPanel;

    if (!prevWasExpanded && nextIsExpanded) {
      const promotedFromUserSheet = this.showGitDiffSheet;
      this.showGitDiffSheet = false;
      this.showInlineDiffInspector = !(this.isCurrentSessionPersonalAgent() && this.showBriefingPanel);
      this.diffInspectorOpenedByUser =
        this.showInlineDiffInspector &&
        (promotedFromUserSheet || this.diffInspectorOpenedByUser || this.isDiffTargetPinned);
      if (this.selectedHostId.length > 0 && this.showInlineDiffInspector) {
        this.refreshGitReview(
          this.gitStagedMode,
          this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
        );
      }
    } else if (!this.canShowInlineDiffInspector() && this.showInlineDiffInspector) {
      this.showInlineDiffInspector = false;
      this.diffInspectorOpenedByUser = false;
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
    this.isAppInBackground = false;
    this.eventScheduler.setBackgroundMode(false);
    if (this.bridge !== null) {
      try {
        this.bridge.onAppForeground();
        this.reloadFleetAsync();
        this.personalAgentStore.refreshBriefing(false);
        if (this.selectedHostId.length > 0) {
          this.reloadHostAsync(this.selectedHostId);
        }
        if (this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
          this.reloadStage();
        }
      } catch (err) {
        orbitLogWarn('OrbitStore', 'onAppForeground failed', err as Object);
      }
    }
  }

  public onAppBackground(): void {
    this.isAppInBackground = true;
    this.eventScheduler.setBackgroundMode(true);
    if (this.isCurrentSessionPersonalAgent()) {
      this.markPersonalAgentVisitedNow(this.selectedHostId, this.selectedAgentId);
    }
    this.composerStore.flushNow();
    if (this.bridge !== null) {
      try {
        this.bridge.onAppBackground();
      } catch (err) {
        orbitLogWarn('OrbitStore', 'onAppBackground failed', err as Object);
      }
    }
  }

  public onNetworkChanged(): void {
    if (this.bridge !== null) {
      try {
        this.bridge.onNetworkChanged();
      } catch (err) {
        orbitLogWarn('OrbitStore', 'onNetworkChanged failed', err as Object);
      }
    }
  }

  public handleDeepLinkUri(
    uri: string,
    eventKind: string = '',
    requestId: string = '',
    targetSeq: number = 0
  ): void {
    const trimmed = uri.trim();
    const hashIdx = trimmed.indexOf('#');
    const clean = hashIdx >= 0 ? trimmed.substring(0, hashIdx).trim() : trimmed;
    const lower = clean.toLowerCase();
    if (!lower.startsWith('orbit://')) {
      return;
    }
    if (this.bridge === null) {
      this.pendingDeepLink = clean;
      this.pendingDeepLinkEventKind = eventKind;
      this.pendingDeepLinkRequestId = requestId;
      this.pendingDeepLinkSeq = targetSeq;
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
    const isDeviceLinkRoute =
      lower.startsWith('orbit://sync') ||
      lower.startsWith('orbit://link') ||
      lower.startsWith('orbit://cloud-sync');

    if (isDeviceLinkRoute) {
      this.importCloudSyncDeviceLinkUri(clean);
      return;
    }

    if (isOpenRoute) {
      const parsedOpen = parseOrbitOpenDeepLink(clean, eventKind, requestId, targetSeq);
      if (parsedOpen !== null) {
        this.openSessionFromDeepLink(
          parsedOpen.hostId,
          parsedOpen.agentId,
          parsedOpen.eventKind,
          parsedOpen.requestId,
          parsedOpen.targetSeq
        );
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

  /**
   * Tri-state validation for deep-linked agent targets:
   * - `'selected'`: target agent verified and selected.
   * - `'pending'`: host detail not yet cached/online; caller must defer side effects until loaded.
   * - `'not_found'`: host is online and cached, and agent does NOT exist; caller MUST abort.
   */
  public validateDeepLinkAgentInDetail(
    detail: HostDetailDto | null,
    hostId: string,
    agentId: string
  ): DeepLinkAgentValidation {
    return validateDeepLinkAgentInDetail(detail, hostId, agentId);
  }

  public openSessionFromDeepLink(
    hostId: string,
    agentId: string,
    eventKind: string = '',
    requestId: string = '',
    targetSeq: number = 0
  ): void {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    const cleanEvent = eventKind.trim().toLowerCase();
    const cleanReqId = requestId.trim();
    const cleanSeq = Number.isFinite(targetSeq) && targetSeq > 0 ? Math.floor(targetSeq) : 0;
    if (cleanHost.length === 0) {
      return;
    }
    if (this.bridge === null) {
      this.pendingOpenHostId = cleanHost;
      this.pendingOpenAgentId = cleanAgent;
      this.pendingOpenEventKind = cleanEvent;
      this.pendingOpenRequestId = cleanReqId;
      this.pendingOpenSeq = cleanSeq;
      return;
    }
    const hostExists = this.hosts.some((h: HostCardDto): boolean => h.host_id === cleanHost);
    if (!hostExists) {
      this.pendingOpenHostId = cleanHost;
      this.pendingOpenAgentId = cleanAgent;
      this.pendingOpenEventKind = cleanEvent;
      this.pendingOpenRequestId = cleanReqId;
      this.pendingOpenSeq = cleanSeq;
      this.reloadFleet();
      return;
    }
    this.pendingOpenHostId = '';
    this.pendingOpenAgentId = '';
    this.pendingOpenEventKind = '';
    this.pendingOpenRequestId = '';
    this.pendingOpenSeq = 0;

    // Validate target agent BEFORE switching stage or emitting side effects (`markAgentRead`).
    if (cleanAgent.length > 0) {
      const targetDetail = this.fleetStore.getCachedHostDetail(cleanHost);
      const validation = this.validateDeepLinkAgentInDetail(targetDetail, cleanHost, cleanAgent);
      if (validation === 'not_found') {
        this.showLocalizedToast($r('app.string.briefing_sessionNotFound'), 'Session not found', true);
        return;
      }
      if (validation === 'pending') {
        this.pendingAgentValidationHostId = cleanHost;
        this.pendingAgentValidationAgentId = cleanAgent;
        this.pendingAgentValidationEventKind = cleanEvent;
        this.pendingAgentValidationRequestId = cleanReqId;
        this.pendingAgentValidationSeq = cleanSeq;
        if (this.selectedHostId !== cleanHost) {
          this.selectHost(cleanHost);
        } else {
          this.reloadHostAsync(cleanHost);
        }
        return;
      }
    }

    this.pendingAgentValidationHostId = '';
    this.pendingAgentValidationAgentId = '';
    this.pendingAgentValidationEventKind = '';
    this.pendingAgentValidationRequestId = '';
    this.pendingAgentValidationSeq = 0;
    this.showGlobalOverview = false;
    this.showPersonalAgentSetup = false;
    this.showBriefingSheet = false;
    if (this.selectedHostId !== cleanHost) {
      this.selectHost(cleanHost);
    }
    if (cleanAgent.length > 0) {
      this.selectAgent(cleanAgent);
      this.markAgentRead(cleanHost, cleanAgent);
    }
    const focusTarget = buildDecisionFocusTarget(
      cleanEvent,
      cleanHost,
      cleanAgent,
      cleanReqId,
      cleanSeq
    );
    if (focusTarget !== null) {
      this.pendingFocusDecisionHostId = focusTarget.hostId;
      this.pendingFocusDecisionAgentId = focusTarget.agentId;
      this.pendingFocusDecisionEventKind = focusTarget.eventKind;
      this.pendingFocusDecisionRequestId = focusTarget.requestId;
      this.pendingFocusDecisionSeq = focusTarget.seq;
      this.decisionFocusRevision += 1;
    } else {
      this.clearPendingDecisionFocus();
    }
    const postAction = resolveDeepLinkPostNavigationAction(cleanEvent, cleanAgent);
    if (postAction === 'permission_required') {
      this.reloadStage();
      this.decisionFocusRevision += 1;
      this.showLocalizedToast(
        $r('app.string.briefing_approvalWaiting'),
        'Approval waiting for your decision',
        false
      );
    } else if (postAction === 'automations') {
      this.openAutomationsSheet();
    } else if (postAction === 'agent_error') {
      this.reloadStage();
      this.showLocalizedToast(
        $r('app.string.briefing_agentStoppedError'),
        'Agent stopped with an error',
        true
      );
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

  // Core Event Loop & Performance Replay Hooks

  private handleFullResync(): void {
    this.reloadFleetAsync();
    this.personalAgentStore.refreshBriefing(true);
    if (this.selectedHostId.length > 0) {
      this.reloadHostAsync(this.selectedHostId);
    }
    if (!this.isNewSessionDraft && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0) {
      const session = this.stageStore.getOrCreateSession(this.selectedHostId, this.selectedAgentId);
      this.stageStore.reloadFull(session, { limit: this.stageTailLimit() }, 'resync').catch((err: Object) => {
        orbitLogWarn('OrbitStore', 'Resync stage reload failed', err);
      });
    }
    if (this.showGitDiffSheet || this.showInlineDiffInspector) {
      this.reloadGitDiff(this.resolvedDiffCwd(), this.resolvedDiffHostId());
    }
    if (this.showPtySheet) {
      this.reloadTerminal();
    }
  }

  public scheduleDebugStageReplay(hostId: string, agentId: string, framesJson: string): void {
    if (this.bridge === null) {
      this.pendingReplayHostId = hostId;
      this.pendingReplayAgentId = agentId;
      this.pendingReplayFramesJson = framesJson;
      return;
    }
    this.startDebugStageReplay(hostId, agentId, framesJson);
  }

  public startDebugStageReplay(hostId: string, agentId: string, framesJson: string): void {
    if (this.bridge === null || hostId.length === 0 || agentId.length === 0) {
      return;
    }
    if (this.replayFinishTimerId !== -1) {
      clearTimeout(this.replayFinishTimerId);
      this.replayFinishTimerId = -1;
    }
    this.fleetStore.injectSyntheticHostIfEmpty(hostId, 'Perf Replay');
    this.fleetStore.setSelectedHostId(hostId);
    this.showGlobalOverview = false;
    this.isNewSessionDraft = false;
    this.selectedAgentId = agentId;
    this.stageStore.activateSession(hostId, agentId);
    try {
      OrbitPerfProbe.mark('perf.replay_started');
      this.bridge.debugReplayStageDeltasJson(hostId, agentId, framesJson);
      // 500 frames @ 40 fps = 12.5s; flush OrbitPerf metrics and release replay state after 15.5s.
      this.replayFinishTimerId = setTimeout((): void => {
        this.replayFinishTimerId = -1;
        OrbitPerfProbe.mark('perf.replay_finished');
        OrbitPerfProbe.flushNow();
        try {
          this.bridge?.debugReplayStageDeltasJson(hostId, agentId, '[]');
        } catch (_cleanupErr) {
          // Ignore cleanup errors on shutdown
        }
      }, 15500);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'debugReplayStageDeltasJson failed', err as Object);
    }
  }

  // Fleet, Host & Directory Operations

  public reloadFleet(): void {
    if (this.bridge === null) {
      return;
    }
    const snap = this.fleetStore.reloadFleetSync();
    if (snap !== null) {
      this.onFleetOverviewLoaded(snap);
    }
  }

  public reloadFleetAsync(): void {
    if (this.bridge === null) {
      return;
    }
    this.fleetStore.reloadFleetAsync().then((snap: FleetOverviewDto | null): void => {
      if (snap !== null) {
        this.onFleetOverviewLoaded(snap);
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'reloadFleetAsync failed', err);
    });
  }

  private onFleetOverviewLoaded(snap: FleetOverviewDto): void {
    if (this.pendingOpenHostId.length > 0) {
      const targetHost = this.pendingOpenHostId;
      const targetAgent = this.pendingOpenAgentId;
      const targetEvent = this.pendingOpenEventKind;
      const targetReq = this.pendingOpenRequestId;
      const targetSeq = this.pendingOpenSeq;
      this.pendingOpenHostId = '';
      this.pendingOpenAgentId = '';
      this.pendingOpenEventKind = '';
      this.pendingOpenRequestId = '';
      this.pendingOpenSeq = 0;
      if (snap.hosts.some((h: HostCardDto): boolean => h.host_id === targetHost)) {
        this.openSessionFromDeepLink(targetHost, targetAgent, targetEvent, targetReq, targetSeq);
      } else {
        this.showToast('Host not found', true);
        if (this.selectedHostId.length === 0 && snap.hosts.length > 0) {
          this.selectHost(snap.hosts[0].host_id);
        } else {
          this.refreshSidebarAsync();
        }
      }
    } else if (this.selectedHostId.length === 0 && snap.hosts.length > 0) {
      this.selectHost(snap.hosts[0].host_id);
    } else {
      this.refreshSidebarAsync();
    }
    this.personalAgentStore.refreshBriefing(false);
    if (this.pushToken.length > 0) {
      this.syncPushTokenToAllHosts(false);
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
    if (this.bridge === null || hostId.length === 0 || agent.agent_id.length === 0) {
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
      this.bridge.sendPromptWithMeta(
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
    const prevWasPa = this.isCurrentSessionPersonalAgent();
    const prevHostId = this.selectedHostId;
    const prevAgentId = this.selectedAgentId;
    const changed = this.fleetStore.setSelectedHostId(hostId);
    this.pushPrefs = clonePushPreferences(this.getPushPreferencesForHost(hostId));
    if (changed) {
      if (prevWasPa && prevHostId.length > 0 && prevAgentId.length > 0) {
        this.markPersonalAgentVisitedNow(prevHostId, prevAgentId);
        this.personalAgentStore.clearActiveVisitKey();
      }
      this.briefingViewingOpen = false;
      this.showBriefingSheet = false;
      this.briefingDismissedByUserForPaKey = '';
      if (this.isDiffTargetPinned) {
        this.closeGitInspector();
      } else {
        this.resetGitCollabState();
      }
      this.clearPendingDecisionFocus();
      this.automationsRefreshSeq += 1;
      this.sideQuestionGen += 1;
      this.sideQuestionStreaming = false;
      this.selectedAgentId = '';
      this.currentAgent = null;
      this.stageStore.clearActiveSession();
      this.composerStore.setActiveScope(hostId, '');
      this.schedules = [];
      this.conditionalTimers = [];
      this.goalRuns = [];
      this.monitorWatches = [];
      this.workflowRuns = [];
      this.automationsLoading = false;
      this.resetWorkspaceBoardState();
      this.syncActiveQueuedPrompts();
    }
    this.reloadHostAsync(hostId);
    this.refreshSidebarAsync();
    if (this.bridge !== null) {
      this.bridge.connectHost(hostId);
      this.bridge.refreshHost(hostId);
      this.bridge.refreshQuotas(hostId);
      this.bridge.refreshProviderModels(hostId, false);
    }
  }

  public reloadHost(hostId: string): void {
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    const detail = this.fleetStore.reloadHostSync(hostId);
    if (detail !== null) {
      this.onHostDetailLoaded(hostId, detail);
    }
  }

  public reloadHostAsync(hostId: string): void {
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    this.fleetStore.reloadHostAsync(hostId).then((detail: HostDetailDto | null): void => {
      if (detail !== null && hostId === this.selectedHostId) {
        this.onHostDetailLoaded(hostId, detail);
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'reloadHostAsync failed', err);
    });
  }

  private onHostDetailLoaded(hostId: string, detail: HostDetailDto): void {
    if (this.bridge === null) {
      return;
    }
    try {
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
          const oldOptId = this.selectedAgentId;
          const oldKey = this.agentQueueKey(hostId, oldOptId);
          const newAgentId = replacedId;
          const newKey = this.agentQueueKey(hostId, newAgentId);
          const pendingQueue = this.queuedPromptsByAgent.get(oldKey);
          if (pendingQueue !== undefined && pendingQueue.length > 0) {
            const existing = this.queuedPromptsByAgent.get(newKey) ?? [];
            this.queuedPromptsByAgent.delete(oldKey);
            this.queuedPromptsByAgent.set(newKey, existing.concat(pendingQueue));
          }
          this.composerStore.remapScope(hostId, oldOptId, hostId, newAgentId);
          this.stageStore.dropSession(hostId, oldOptId);
          this.selectedAgentId = newAgentId;
          this.isNewSessionDraft = false;
          this.stageStore.activateSession(hostId, newAgentId);
          this.syncActiveQueuedPrompts();
          this.bridge.openAgentTimeline(hostId, this.selectedAgentId);
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
            this.bridge.markAgentSeen(hostId, this.selectedAgentId, detail.agents[i].latest_seq);
            break;
          }
        }
      }
      this.refreshSidebarAsync();

      this.observeHostDetailForNotifications(hostId, detail);

      if (this.pendingAgentValidationHostId === hostId && this.pendingAgentValidationAgentId.length > 0) {
        const valResult = this.validateDeepLinkAgentInDetail(
          detail,
          hostId,
          this.pendingAgentValidationAgentId
        );
        if (valResult !== 'pending') {
          const pHost = this.pendingAgentValidationHostId;
          const pAgent = this.pendingAgentValidationAgentId;
          const pEvent = this.pendingAgentValidationEventKind;
          const pReq = this.pendingAgentValidationRequestId;
          const pSeq = this.pendingAgentValidationSeq;
          this.pendingAgentValidationHostId = '';
          this.pendingAgentValidationAgentId = '';
          this.pendingAgentValidationEventKind = '';
          this.pendingAgentValidationRequestId = '';
          this.pendingAgentValidationSeq = 0;
          if (valResult === 'selected') {
            this.openSessionFromDeepLink(pHost, pAgent, pEvent, pReq, pSeq);
          } else {
            this.showLocalizedToast($r('app.string.briefing_sessionNotFound'), 'Session not found', true);
          }
        }
      }

      for (let i = 0; i < detail.agents.length; i++) {
        this.drainQueuedPromptsForAgentIfIdle(hostId, detail.agents[i]);
      }
    } catch (e) {
      orbitLogWarn('OrbitStore', 'onHostDetailLoaded failed', e as Object);
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
    if (this.bridge !== null && this.selectedHostId.length > 0) {
      const canUpdateGitDiff = shouldApplyGitDiffForTarget(
        this.isDiffTargetPinned,
        this.diffTargetHostId,
        this.diffTargetCwd,
        this.selectedHostId,
        cwd,
        this.resolveHostHomeDir(this.selectedHostId)
      );
      try {
        if (canUpdateGitDiff) {
          this.bridge.fetchGitStatus(this.selectedHostId, cwd);
          this.bridge.fetchGitDiff(this.selectedHostId, cwd, null, this.gitStagedMode);
        }
        this.bridge.fetchWorkspaceFiles(this.selectedHostId, cwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'setActiveCwd fetch failed', err as Object);
      }
      if (canUpdateGitDiff) {
        this.reloadGitDiff(cwd, this.selectedHostId);
      }
      this.reloadWorkspaceFiles();
    }
  }

  public startNewSessionDraft(cwd?: string): void {
    if (this.isCurrentSessionPersonalAgent() && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0) {
      this.markPersonalAgentVisitedNow(this.selectedHostId, this.selectedAgentId);
      this.personalAgentStore.clearActiveVisitKey();
    }
    this.briefingViewingOpen = false;
    this.showBriefingSheet = false;
    this.briefingDismissedByUserForPaKey = '';
    if (this.isDiffTargetPinned) {
      this.closeGitInspector();
    }
    this.clearPendingDecisionFocus();
    this.showGlobalOverview = false;
    this.showPersonalAgentSetup = false;
    this.isNewSessionDraft = true;
    this.selectedAgentId = '';
    this.currentAgent = null;
    this.stageStore.clearActiveSession();
    this.composerStore.setActiveScope(this.selectedHostId, '');
    this.activeSessionUsage = null;
    this.syncDefaultModelAndEffort(this.composerProvider);
    this.syncActiveQueuedPrompts();
    if (cwd !== undefined && cwd.length > 0) {
      this.setActiveCwd(cwd);
    } else if (this.currentHostDetail !== null) {
      this.syncActiveWorkspaceBoardFromDetail(this.currentHostDetail);
    } else {
      this.resetWorkspaceBoardState();
    }
    this.refreshSidebarAsync();
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.triggerHaptic(10);
  }

  public selectAgent(agentId: string): void {
    if (agentId.length === 0) {
      return;
    }
    const prevWasPa = this.isCurrentSessionPersonalAgent();
    const prevHostId = this.selectedHostId;
    const prevAgentId = this.selectedAgentId;
    const sameSession = !this.isNewSessionDraft && prevAgentId === agentId;
    if (prevWasPa && !sameSession && prevHostId.length > 0 && prevAgentId.length > 0) {
      this.markPersonalAgentVisitedNow(prevHostId, prevAgentId);
      this.personalAgentStore.clearActiveVisitKey();
    }
    if (this.isDiffTargetPinned && !sameSession) {
      this.closeGitInspector();
    }
    if (
      this.pendingFocusDecisionHostId !== this.selectedHostId ||
      this.pendingFocusDecisionAgentId !== agentId
    ) {
      this.clearPendingDecisionFocus();
    }
    this.showGlobalOverview = false;
    this.showPersonalAgentSetup = false;
    this.isNewSessionDraft = false;
    this.selectedAgentId = agentId;
    this.activeSessionUsage = null;
    if (this.selectedHostId.length > 0) {
      this.fleetStore.clearConversationDirty(this.selectedHostId, agentId);
      this.composerStore.setActiveScope(this.selectedHostId, agentId);
      this.stageStore.activateSession(this.selectedHostId, agentId);
    }
    // Unified PA visit & Briefing presentation lifecycle across ALL session navigation entries
    // (including cold-start deep links, sidebar taps, and cross-host switches).
    if (this.isCurrentSessionPersonalAgent()) {
      const paKey = `${this.selectedHostId}::${agentId}`;
      const userDismissedCurrentPa = this.briefingDismissedByUserForPaKey === paKey;
      const userOpenedDiff = this.showInlineDiffInspector &&
        (this.diffInspectorOpenedByUser || this.isDiffTargetPinned);
      if (
        !prevWasPa ||
        !sameSession ||
        !this.personalAgentStore.activeVisitKeyMatches(this.selectedHostId, agentId)
      ) {
        this.personalAgentStore.enterPersonalAgentVisit(this.selectedHostId, agentId);
      }
      if (!sameSession) {
        this.briefingDismissedByUserForPaKey = '';
      }
      const nextPres = resolvePaSessionSelectPresentation(
        this.windowWidthVp,
        sameSession,
        this.briefingViewingOpen,
        this.showBriefingPanel,
        this.showBriefingSheet,
        userDismissedCurrentPa,
        userOpenedDiff,
        OrbitTokens.BP_EXPANDED_MIN
      );
      this.briefingViewingOpen = nextPres.briefingViewingOpen;
      this.showBriefingPanel = nextPres.showBriefingPanel;
      this.showBriefingSheet = nextPres.showBriefingSheet;
      if (nextPres.shouldCloseDefaultDiffInspector && !userOpenedDiff) {
        this.showInlineDiffInspector = false;
      }
    } else {
      this.briefingViewingOpen = false;
      this.showBriefingSheet = false;
      this.briefingDismissedByUserForPaKey = '';
    }
    if (this.bridge !== null && this.selectedHostId.length > 0) {
      const agents = this.currentHostDetail?.agents ?? [];
      let matchedSeq = 0;
      for (let i = 0; i < agents.length; i++) {
        if (agents[i].agent_id === agentId) {
          matchedSeq = agents[i].latest_seq;
          break;
        }
      }
      this.bridge.markAgentSeen(this.selectedHostId, agentId, matchedSeq);
    }
    this.syncActiveQueuedPrompts();
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    if (this.bridge !== null && this.selectedHostId.length > 0) {
      this.bridge.openAgentTimeline(this.selectedHostId, agentId);
      this.bridge.listProviderSubagents(this.selectedHostId, agentId);
      this.reloadStage();
      this.refreshSessionUsage();
      if (this.currentHostDetail !== null) {
        this.syncActiveWorkspaceBoardFromDetail(this.currentHostDetail);
      } else {
        this.resetWorkspaceBoardState();
      }
      this.refreshSidebarAsync();
    }
    this.triggerHaptic(8);
  }

  public reloadStage(forceFull: boolean = false): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    const session = this.stageStore.activateSession(this.selectedHostId, this.selectedAgentId);
    if (session === null) {
      return;
    }
    const limit = this.stageTailLimit();
    if (forceFull) {
      this.stageStore.reloadFull(session, { limit: limit }, 'reload').catch((err: Object) => {
        orbitLogWarn('OrbitStore', 'reloadStage full failed', err);
      });
    } else {
      this.stageStore.fetchIncremental(session, limit).catch((err: Object) => {
        orbitLogWarn('OrbitStore', 'reloadStage incremental failed', err);
      });
    }
  }

  private onStageDeltaApplied(session: OrbitStageSession, delta: AgentStageDeltaDto): void {
    if (
      this.bridge === null ||
      session.hostId !== this.selectedHostId ||
      session.agentId !== this.selectedAgentId
    ) {
      return;
    }
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
        this.bridge.markAgentSeen(
          this.selectedHostId,
          delta.agent.agent_id,
          Math.max(delta.agent.latest_seq, delta.latest_seq)
        );
        if (!delta.agent.is_running) {
          this.refreshSidebarAsync();
        }
      }
      // Unified stage-delta notification observation via OrbitNotificationEngine (zero duplicate state machine)
      const stageHostCard = this.fleetStore.getHostById(session.hostId);
      const stageHostLabel = stageHostCard !== null && stageHostCard.display_name.trim().length > 0
        ? stageHostCard.display_name.trim()
        : session.hostId;
      const stagePayloads = this.notificationEngine.observeStageDelta(
        session.hostId,
        stageHostLabel,
        delta.agent,
        delta.pending_permissions ?? [],
        this.buildNotificationVisibilityContext(),
        this.getPushPreferencesForHost(session.hostId),
        this.resolveLocalNotificationCopyStrings()
      );
      this.dispatchLocalNotificationPayloads(stagePayloads);
      this.drainQueuedPromptsForAgentIfIdle(this.selectedHostId, delta.agent);
    } else if (!this.isNewSessionDraft && this.selectedAgentId.length > 0) {
      this.bridge.markAgentSeen(this.selectedHostId, this.selectedAgentId, delta.latest_seq);
    }
  }

  public describeToolStepFallback(item: TimelineItemDto): ToolStepDescriptorDto | null {
    return item.step_descriptor ?? null;
  }

  // Pairing, Host Configuration & Projects

  public pairHost(uri: string): void {
    const clean = uri.trim();
    if (clean.length === 0) {
      return;
    }
    if (this.bridge === null) {
      this.showToast('Native core unavailable in previewer', true);
      return;
    }
    try {
      const card = JSON.parse(this.bridge.pairHostJson(clean)) as HostCardDto;
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
    if (this.bridge === null) {
      return;
    }
    try {
      const uri = this.bridge.discoverLocalDaemonUri();
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
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    try {
      this.bridge.updateHostConfigJson(
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
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    try {
      this.bridge.connectHost(hostId);
      this.bridge.refreshHost(hostId);
      this.bridge.refreshQuotas(hostId);
      this.bridge.refreshProviderModels(hostId, false);
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
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    this.bridge.refreshQuotas(this.selectedHostId);
    this.reloadHost(this.selectedHostId);
    this.showToast('Refreshing account quotas…', false);
    this.triggerHaptic(8);
  }

  public refreshModelsAndQuotas(forceModels: boolean = true): void {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    this.bridge.refreshQuotas(this.selectedHostId);
    this.bridge.refreshProviderModels(this.selectedHostId, forceModels);
    this.reloadHost(this.selectedHostId);
    this.showToast('Refreshing models…', false);
    this.triggerHaptic(8);
  }

  public removeHost(hostId: string): void {
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    try {
      this.bridge.removeHost(hostId);
      this.fleetStore.removeCachedHost(hostId);
      if (this.selectedHostId === hostId) {
        this.fleetStore.setSelectedHostId('');
        this.selectedAgentId = '';
        this.currentAgent = null;
        this.stageStore.clearActiveSession();
        this.composerStore.setActiveScope('', '');
      }
      this.reloadFleet();
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public pinDirectoryAsProject(dirPath: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || dirPath.trim().length === 0) {
      return;
    }
    try {
      const name = basenameOfPath(dirPath);
      this.bridge.upsertProjectJson(null, this.selectedHostId, name, JSON.stringify([dirPath]));
      this.reloadFleet();
      this.setActiveCwd(dirPath);
      const displayName = name.trim().length > 0 ? name.trim() : name;
      this.showToast(`Pinned ${displayName}`, false);
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
    if (this.bridge === null || projectId.length === 0) {
      return;
    }
    try {
      this.bridge.removeProject(projectId);
      this.reloadFleet();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'removePinnedProject failed', err as Object);
    }
  }

  public browseDirectory(path: string | null): void {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    this.showDirBrowser = true;
    try {
      this.bridge.listDirectory(this.selectedHostId, path);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'browseDirectory failed', err as Object);
    }
  }

  // Composer Context, Provider/Model/Effort & Prompt Submission

  public reloadWorkspaceFiles(): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.activeCwd.length === 0) {
      return;
    }
    try {
      const files = JSON.parse(
        this.bridge.workspaceFilesJson(this.selectedHostId, this.activeCwd)
      ) as string[];
      this.workspaceFiles = files;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'reloadWorkspaceFiles failed', err as Object);
    }
  }

  public ensureWorkspaceFilesLoaded(): void {
    if (
      this.workspaceFiles.length === 0 &&
      this.bridge !== null &&
      this.selectedHostId.length > 0 &&
      this.activeCwd.length > 0
    ) {
      try {
        this.bridge.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'ensureWorkspaceFilesLoaded failed', err as Object);
      }
      this.reloadWorkspaceFiles();
    }
  }

  public openContextPicker(): void {
    if (this.bridge !== null && this.selectedHostId.length > 0) {
      this.bridge.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
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
    if (this.terminalRows.length === 0 && this.bridge !== null && this.selectedHostId.length > 0) {
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
    if (this.bridge !== null) {
      try {
        return JSON.parse(
          this.bridge.slashCommandsJson('harmony', clean.length > 0 ? clean : null)
        ) as SlashCommandDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'filterSlashCommands failed', err as Object);
      }
    }
    return this.slashCommands;
  }

  public rankMentionCandidates(query: string, limit: number = 11): MentionCandidateDto[] {
    if (this.bridge === null || this.selectedHostId.length === 0) {
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
        this.bridge.rankMentionCandidatesJson(JSON.stringify(req))
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
    if (kind === 'terminal' && this.terminalRows.length === 0 && this.bridge !== null && this.selectedHostId.length > 0) {
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
    if (this.bridge === null || this.selectedHostId.length === 0) {
      this.showToast('Select a host first to attach files', true);
      return;
    }
    this.isStagingAttachment = true;
    try {
      const loaded = await readUriBytesAsync(uri, fallbackName, fallbackMime);
      const isImage = forceImage || loaded.mimeType.startsWith('image/');
      const rawJson = await this.bridge.stageAttachmentJson(
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
    if (this.bridge !== null) {
      if (this.selectedHostId.length > 0 && this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
        this.bridge.setSessionMode(this.selectedHostId, this.selectedAgentId, normalized);
      } else {
        this.bridge.setAccountDefaultMode(normalized, Date.now());
      }
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
    if (this.bridge !== null) {
      try {
        return JSON.parse(
          this.bridge.fallbackProviderCatalogJson(provider)
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
    if (this.bridge === null) {
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
        this.bridge.resolveModelSelectionJson(JSON.stringify(req))
      ) as ResolvedModelSelectionDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resolveModelSelectionJson failed', err as Object);
      return null;
    }
  }

  public effortDescriptionText(effort: string): string {
    if (this.bridge !== null && effort.trim().length > 0) {
      try {
        return this.bridge.defaultEffortDescription(effort);
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
    if (this.bridge !== null && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
      const modelOpt = resolved.model.trim().length > 0 ? resolved.model.trim() : null;
      const effortOpt = resolved.effort.trim().length > 0 ? resolved.effort.trim() : null;
      if (prevProvider !== provider) {
        this.bridge.switchAgentProvider(
          this.selectedHostId,
          this.selectedAgentId,
          provider,
          modelOpt,
          effortOpt,
          null
        );
      } else {
        this.bridge.setAgentConfig(this.selectedHostId, this.selectedAgentId, modelOpt, effortOpt);
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

  public isActiveAgentPersonalAgent(): boolean {
    if (this.isNewSessionDraft || this.selectedAgentId.length === 0 || this.currentAgent === null) {
      return false;
    }
    return this.currentAgent.labels !== undefined && this.currentAgent.labels['orbit.role'] === 'personal_router';
  }

  public isActiveAgentRunning(): boolean {
    if (this.isNewSessionDraft || this.selectedAgentId.length === 0) {
      return false;
    }
    if (this.stageStore.activeTurnKey.length > 0) {
      return true;
    }
    const turns = this.stageTurns;
    for (let i = 0; i < turns.length; i++) {
      if (turns[i].hasActiveItem) {
        return true;
      }
    }
    if (this.currentAgent === null) {
      return false;
    }
    return (
      this.currentAgent.is_running ||
      this.currentAgent.is_awaiting_permission ||
      this.currentAgent.status.state === 'running' ||
      this.currentAgent.status.state === 'initializing' ||
      this.currentAgent.status.state === 'awaiting_permission'
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
    if (this.bridge === null) {
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
      return JSON.parse(this.bridge.composePromptJson(JSON.stringify(req))) as ComposedPromptDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'composePromptJson failed', err as Object);
      return null;
    }
  }

  public submitSlashCompact(extraFocus: string = ''): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.isNewSessionDraft || this.selectedAgentId.length === 0) {
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
      const composed = JSON.parse(this.bridge.composePromptJson(JSON.stringify(req))) as ComposedPromptDto;
      const badgesJson = composed.badges.length > 0 ? JSON.stringify(composed.badges) : null;
      this.bridge.sendPromptWithMeta(
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
      } else {
        this.openSideQuestionSheet();
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
    if (this.selectedHostSupportsServerQueue() && this.bridge !== null) {
      try {
        this.bridge.enqueuePrompt(
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

    this.composerStore.clearActiveDraft(true);
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
    if (this.selectedHostSupportsServerQueue() && this.bridge !== null) {
      try {
        this.bridge.removeQueuedPrompt(this.selectedHostId, this.selectedAgentId, itemId);
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
    if (this.selectedHostSupportsServerQueue() && this.bridge !== null) {
      try {
        this.bridge.removeQueuedPrompt(this.selectedHostId, this.selectedAgentId, itemId);
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
      this.bridge === null ||
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
        this.bridge.flushQueuedPromptNow(this.selectedHostId, this.selectedAgentId, itemId);
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
        this.bridge.interruptAgent(this.selectedHostId, this.selectedAgentId);
      }
      this.bridge.sendPromptWithMeta(
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
    if (this.bridge === null || this.selectedHostId.length === 0) {
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
      } else {
        this.openSideQuestionSheet();
      }
      return;
    }
    if (overridePrompt === undefined && composed.is_compact) {
      this.composerText = '';
      this.activeSlashDirective = '';
      this.submitSlashCompact(composed.display_prompt);
      return;
    }

    if (this.isActiveAgentRunning()) {
      if (overridePrompt === undefined) {
        this.enqueueCurrentDraft();
        return;
      }
      if (this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
        const badgesJsonOverride = composed.badges.length > 0 ? JSON.stringify(composed.badges) : null;
        const queueId = `q_${Date.now()}_${Math.floor(Math.random() * 100000)}`;
        this.bridge.enqueuePrompt(
          this.selectedHostId,
          this.selectedAgentId,
          composed.wire_prompt,
          composed.display_prompt,
          badgesJsonOverride,
          queueId
        );
        this.reloadStage();
        return;
      }
    }

    const badgesJson = composed.badges.length > 0 ? JSON.stringify(composed.badges) : null;

    try {
      if (this.selectedAgentId.length > 0 && !this.isNewSessionDraft) {
        this.bridge.sendPromptWithMeta(
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
        const optId = this.bridge.createSessionJson(JSON.stringify(createReq));
        this.composerStore.clearActiveDraft(true);
        this.selectedAgentId = optId;
        this.isNewSessionDraft = false;
        this.composerStore.setActiveScope(this.selectedHostId, optId);
        this.stageStore.activateSession(this.selectedHostId, optId);
        this.syncActiveQueuedPrompts();
        this.reloadHostAsync(this.selectedHostId);
        this.reloadStage();
      }
      if (overridePrompt === undefined) {
        this.composerStore.clearActiveDraft(true);
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
    if (this.bridge === null) {
      return;
    }
    this.reloadFleet();
    if (this.selectedHostId.length > 0) {
      try {
        this.bridge.connectHost(this.selectedHostId);
        this.bridge.refreshHost(this.selectedHostId);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'pullRefreshStage host refresh failed', err as Object);
      }
      this.reloadHost(this.selectedHostId);
      if (this.selectedAgentId.length > 0) {
        try {
          this.bridge.openAgentTimeline(this.selectedHostId, this.selectedAgentId);
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
        this.bridge.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'pullRefreshStage workspace files failed', err as Object);
      }
      this.reloadWorkspaceFiles();
    }
  }

  public pullRefreshGitReview(): void {
    this.triggerHaptic(10);
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return;
    }
    try {
      this.bridge.refreshHost(targetHost);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'pullRefreshGitReview host refresh failed', err as Object);
    }
    this.refreshGitReview(
      this.gitStagedMode,
      this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null
    );
    if (!this.isDiffTargetPinned) {
      try {
        this.bridge.fetchWorkspaceFiles(targetHost, targetCwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'pullRefreshGitReview workspace files failed', err as Object);
      }
      this.reloadWorkspaceFiles();
    }
  }

  public interruptCurrentAgent(): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      this.bridge.interruptAgent(this.selectedHostId, this.selectedAgentId);
      this.triggerHaptic(16);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'interruptCurrentAgent failed', err as Object);
      this.showToast('Failed to stop agent', true);
    }
  }

  public archiveSession(agentId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.bridge.archiveAgent(this.selectedHostId, agentId);
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
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      this.bridge.resolvePermission(this.selectedHostId, this.selectedAgentId, requestId, decision);
      this.triggerHaptic(14);
      this.reloadStage();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resolvePermission failed', err as Object);
      this.showToast('Failed to send approval decision', true);
    }
  }

  public answerQuestion(requestId: string, answers: string[][]): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      this.bridge.answerQuestion(
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

  private resolveHostHomeDir(_hostId: string): string {
    // Only expand `~` when an authoritative daemon home directory is explicitly provided by the host;
    // never guess from arbitrary sidebar `/home/<user>` groups when multiple users may coexist on a host.
    return '';
  }

  public resolvedDiffHostId(): string {
    return this.isDiffTargetPinned && this.diffTargetHostId.length > 0
      ? this.diffTargetHostId
      : this.selectedHostId;
  }

  public resolvedDiffCwd(): string {
    return this.isDiffTargetPinned && this.diffTargetCwd.length > 0
      ? this.diffTargetCwd
      : this.activeCwd;
  }

  private resetGitCollabState(): void {
    this.gitCollabRequestSeq += 1;
    this.gitPrStatus = null;
    this.gitStashes = [];
    this.gitConflicts = [];
    this.gitRebaseStatus = null;
  }

  public closeGitInspector(): void {
    this.showInlineDiffInspector = false;
    this.showGitDiffSheet = false;
    this.diffInspectorOpenedByUser = false;
    this.isDiffTargetPinned = false;
    this.diffTargetHostId = '';
    this.diffTargetCwd = '';
    this.resetGitCollabState();
  }

  public toggleTopBarGitDiff(): void {
    if (this.canShowInlineDiffInspector() && this.showInlineDiffInspector) {
      this.closeGitInspector();
      this.triggerHaptic(8);
      return;
    }
    // Reuse `openGitInspector()` so wide-screen right-rail mutual exclusion (yielding docked
    // Briefing panel and marking `diffInspectorOpenedByUser = true`) stays strictly uniform.
    this.openGitInspector();
  }

  public openGitInspector(focusPath?: string): void {
    this.isDiffTargetPinned = false;
    this.diffTargetHostId = this.selectedHostId;
    this.diffTargetCwd = this.activeCwd;
    this.diffInspectorOpenedByUser = true;
    this.resetGitCollabState();
    if (focusPath !== undefined) {
      this.gitSelectedPath = focusPath;
    }
    this.refreshGitReview(this.gitStagedMode, this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null);
    if (this.canShowInlineDiffInspector()) {
      // Right rail is mutually exclusive: yield docked Briefing rail so Diff is immediately visible.
      if (this.isCurrentSessionPersonalAgent()) {
        this.briefingDismissedByUserForPaKey = `${this.selectedHostId}::${this.selectedAgentId}`;
      }
      this.briefingViewingOpen = false;
      this.showBriefingPanel = false;
      this.showBriefingSheet = false;
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
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return;
    }
    try {
      this.bridge.fetchGitStatus(targetHost, targetCwd);
      this.bridge.fetchGitDiff(targetHost, targetCwd, path, staged);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshGitReview failed', err as Object);
    }
    this.reloadGitDiff(targetCwd, targetHost);
  }

  public reloadGitDiff(cwd: string, hostId?: string): void {
    const targetHost = (hostId !== undefined && hostId.trim().length > 0)
      ? hostId.trim()
      : this.selectedHostId;
    if (this.bridge === null || targetHost.length === 0) {
      return;
    }
    if (
      !shouldApplyGitDiffForTarget(
        this.isDiffTargetPinned,
        this.diffTargetHostId,
        this.diffTargetCwd,
        targetHost,
        cwd,
        this.resolveHostHomeDir(targetHost)
      )
    ) {
      return;
    }
    try {
      const pathOpt = this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null;
      const gr = JSON.parse(this.bridge.gitReviewJson(targetHost, cwd, pathOpt)) as GitReviewDto;
      if (this.isDiffTargetPinned) {
        const hostHomeDir = this.resolveHostHomeDir(targetHost);
        const respHost = (gr.host_id ?? '').trim();
        const respCwd = gr.cwd ?? '';
        const respStatus = gr.status;
        const hostMismatch = respHost.length === 0 || respHost !== this.diffTargetHostId.trim();
        const cwdMismatch =
          respCwd.length === 0 || !gitTargetPathsEqual(respCwd, this.diffTargetCwd, hostHomeDir);
        let statusCwdMismatch = false;
        if (respStatus !== null && respStatus !== undefined) {
          const statusCwd = respStatus.cwd ?? '';
          statusCwdMismatch =
            statusCwd.length === 0 || !gitTargetPathsEqual(statusCwd, this.diffTargetCwd, hostHomeDir);
        }
        if (hostMismatch || cwdMismatch || statusCwdMismatch) {
          this.gitStatus = null;
          this.gitDiffFiles = [];
          return;
        }
      }
      this.gitStatus = gr.status;
      this.gitDiffFiles = gr.diff_files;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'reloadGitDiff failed', err as Object);
    }
  }

  public totalDiffFileCount(): number {
    if (this.isDiffTargetPinned) {
      return this.gitDiffFiles.length;
    }
    const source = this.gitDiffFiles.length > 0 ? this.gitDiffFiles : this.sessionDiffs;
    return source.length;
  }

  public activeTerminalCount(): number {
    return this.currentHostDetail !== null ? this.currentHostDetail.terminals.length : 0;
  }

  public openFullGitDiffSheet(): void {
    this.diffInspectorOpenedByUser = true;
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
      this.setActiveCwd(cwd);
    }
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.openGitInspector('');
  }

  /**
   * Opens Git Diff for a specific `(hostId, cwd)` targeted from a Briefing decision card.
   * Pins the Diff inspector's independent `(diffTargetHostId, diffTargetCwd)` so incoming PA
   * Stage deltas NEVER overwrite the inspected worker's directory across staged/unstaged switches,
   * refreshes, or Git actions. On wide screens (`>= 1024 vp`) yields the docked Briefing rail.
   */
  public openDecisionDiffForHostCwd(hostId: string, cwd: string): void {
    const cleanHost = hostId.trim();
    const trimmedCwdCheck = cwd.trim();
    if (cleanHost.length === 0 || trimmedCwdCheck.length === 0 || trimmedCwdCheck === '~') {
      this.showLocalizedToast(
        $r('app.string.briefing_diffTargetUnavailable'),
        'Workspace path unavailable for this request',
        true
      );
      return;
    }
    this.diffTargetHostId = cleanHost;
    this.diffTargetCwd = cwd;
    this.isDiffTargetPinned = true;
    this.diffInspectorOpenedByUser = true;
    this.gitSelectedPath = '';
    this.gitStatus = null;
    this.gitDiffFiles = [];
    this.resetGitCollabState();
    this.refreshGitReview(this.gitStagedMode, null);
    this.refreshGitCollabStatus();
    if (this.isCurrentSessionPersonalAgent()) {
      this.briefingDismissedByUserForPaKey = `${this.selectedHostId}::${this.selectedAgentId}`;
    }
    this.briefingViewingOpen = false;
    this.showBriefingPanel = false;
    this.showBriefingSheet = false;
    if (this.canShowInlineDiffInspector()) {
      this.showInlineDiffInspector = true;
      this.showGitDiffSheet = false;
    } else {
      this.showGitDiffSheet = true;
    }
    this.triggerHaptic(8);
  }

  public openPtyTerminalInCwd(cwd: string): void {
    const hasCwd = cwd.trim().length > 0;
    if (hasCwd) {
      this.setActiveCwd(cwd);
    }
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.showPtySheet = true;
    if (this.bridge !== null && this.selectedHostId.length > 0) {
      const detail = this.currentHostDetail;
      let matchedTermId = '';
      if (detail !== null && hasCwd) {
        for (let i = 0; i < detail.terminals.length; i++) {
          if (cwdPathsEquivalent(detail.terminals[i].cwd, cwd)) {
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
    if (this.bridge !== null && this.selectedHostId.length > 0) {
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
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    try {
      this.bridge.startTerminal(
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
    if (this.bridge === null || this.selectedHostId.length === 0 || terminalId.length === 0) {
      return;
    }
    try {
      this.bridge.attachTerminal(this.selectedHostId, terminalId, Math.max(1, slot));
      this.reloadHost(this.selectedHostId);
      this.reloadTerminal();
      this.triggerHaptic(8);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'attachTerminal failed', err as Object);
    }
  }

  public killTerminal(terminalId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || terminalId.length === 0) {
      return;
    }
    try {
      this.bridge.killTerminal(this.selectedHostId, terminalId);
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
    if (this.bridge !== null && this.selectedHostId.length > 0) {
      try {
        this.bridge.resizeTerminal(this.selectedHostId, clampedCols, clampedRows);
        this.reloadTerminal();
      } catch (err) {
        orbitLogWarn('OrbitStore', 'resizeTerminal failed', err as Object);
      }
    }
  }

  public reloadTerminal(): void {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    try {
      const screen = JSON.parse(
        this.bridge.terminalScreenJson(this.selectedHostId)
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

  private encodeCtrlByte(ch: string): string | null {
    if (ch.length !== 1) {
      return null;
    }
    const upper = ch.toUpperCase().charCodeAt(0);
    if (upper >= 64 && upper <= 95) {
      return String.fromCharCode(upper - 64);
    }
    if (ch === ' ') {
      return '\x00';
    }
    if (ch === '/') {
      return '\x1f';
    }
    if (ch === '?') {
      return '\x7f';
    }
    return null;
  }

  public sendTerminalKeys(rawKeys: string): void {
    this.sendTerminalRawUtf8(rawKeys);
  }

  public sendTerminalRawUtf8(rawKeys: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || rawKeys.length === 0) {
      return;
    }
    let out = rawKeys;
    const ctrlArmed = this.ctrlSticky;
    const altArmed = this.altSticky;
    if (ctrlArmed) {
      const ctrlSeq = this.encodeCtrlByte(rawKeys);
      if (ctrlSeq !== null) {
        out = ctrlSeq;
      }
      this.ctrlSticky = false;
    }
    if (altArmed) {
      out = `\x1b${out}`;
      this.altSticky = false;
    }
    try {
      const slot = this.currentHostDetail !== null && this.currentHostDetail.active_terminal_slot > 0
        ? this.currentHostDetail.active_terminal_slot
        : 1;
      const encoder = new util.TextEncoder();
      const bytes = encoder.encodeInto(out);
      this.bridge.sendTerminalBytes(this.selectedHostId, slot, bytes);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'sendTerminalRawUtf8 failed', err as Object);
    }
  }

  public sendTerminalRawBytes(bytes: Uint8Array): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || bytes.length === 0) {
      return;
    }
    try {
      const slot = this.currentHostDetail !== null && this.currentHostDetail.active_terminal_slot > 0
        ? this.currentHostDetail.active_terminal_slot
        : 1;
      this.bridge.sendTerminalBytes(this.selectedHostId, slot, bytes);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'sendTerminalRawBytes failed', err as Object);
    }
  }

  // Client Bridge Contract v2 Wrappers

  public async rewindAgent(toBeforeSeq: number, restoreFiles: boolean): Promise<RewindAgentResultDto | null> {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return null;
    }
    try {
      const json = await this.bridge.rewindAgentJson(
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
    if (this.bridge === null || this.selectedHostId.length === 0 || path.trim().length === 0) {
      return null;
    }
    const targetCwd = cwdOverride !== undefined && cwdOverride.length > 0 ? cwdOverride : this.activeCwd;
    try {
      const json = await this.bridge.readWorkspaceFileJson(this.selectedHostId, targetCwd, path);
      return JSON.parse(json) as WorkspaceFileViewDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'readWorkspaceFileJson failed', err as Object);
      this.showToast((err as Error).message, true);
      return null;
    }
  }

  public buildSideQuestionContext(maxChars: number = 4000): string {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return '';
    }
    try {
      return this.bridge.buildSideQuestionContext(this.selectedHostId, this.selectedAgentId, maxChars);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'buildSideQuestionContext failed', err as Object);
      return '';
    }
  }

  public askSideQuestion(question: string, contextSummary?: string): string {
    const cleanQ = question.trim();
    if (this.bridge === null || this.selectedHostId.length === 0 || cleanQ.length === 0) {
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
      return this.bridge.askSideQuestion(JSON.stringify(req), (err: Error | null, eventJson: string) => {
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
    if (this.bridge === null) {
      return null;
    }
    try {
      return this.bridge.detectDiagramKind(language, source);
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
    if (this.bridge === null || kind.length === 0) {
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
      const json = await this.bridge.renderDiagramJson(JSON.stringify(req));
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
      this.bridge !== null &&
      this.selectedHostId.length > 0 &&
      this.selectedAgentId.length > 0 &&
      !this.isNewSessionDraft
    ) {
      try {
        this.bridge.setAgentFastMode(this.selectedHostId, this.selectedAgentId, next);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'setAgentFastMode failed', err as Object);
      }
    }
    this.showToast(next ? 'Fast Mode enabled' : 'Fast Mode disabled', false);
    this.triggerHaptic(8);
  }

  public async forkCurrentSession(useWorktree: boolean): Promise<void> {
    if (
      this.bridge === null ||
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
      const json = await this.bridge.forkAgentJson(JSON.stringify(req));
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
      this.bridge === null ||
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
      const json = this.bridge.exportAgentTranscriptJson(
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
    if (this.bridge !== null && this.selectedHostId.length > 0) {
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
    if (this.bridge === null || this.selectedHostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.bridge.unarchiveAgent(this.selectedHostId, agentId);
      this.reloadHost(this.selectedHostId);
      this.showToast('Session restored', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'unarchiveSession failed', err as Object);
      this.showToast('Failed to restore session', true);
    }
  }

  public deleteSessionPermanently(agentId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.bridge.deleteAgent(this.selectedHostId, agentId);
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
    if (this.bridge === null || this.selectedHostId.length === 0 || wsId.length === 0) {
      this.resetWorkspaceBoardState();
      return;
    }
    try {
      this.bridge.fetchWorkspaceBoard(this.selectedHostId, wsId);
      this.bridge.fetchWorkspaceEvents(this.selectedHostId, wsId, 30);
      const snapJson = this.bridge.workspaceBoardJson(this.selectedHostId, wsId);
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
      const eventsJson = this.bridge.workspaceEventsJson(this.selectedHostId, wsId);
      this.workspaceEvents = JSON.parse(eventsJson) as WorkspaceEventRecordDto[];
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshWorkspaceBoard failed', err as Object);
    }
  }

  public saveWorkspaceBoard(content: string): void {
    const wsId = this.activeWorkspaceId();
    if (this.bridge === null || this.selectedHostId.length === 0 || wsId.length === 0) {
      this.showToast('No active workspace selected', true);
      return;
    }
    try {
      this.bridge.updateWorkspaceBoard(
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
      this.bridge === null ||
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
      const optId = this.bridge.spawnWorkspacePeerJson(JSON.stringify(req));
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
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const seq = ++this.automationsRefreshSeq;
    this.automationsLoading = true;
    try {
      this.bridge.refreshHost(hostId);
      this.reloadHost(hostId);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshAutomations host refresh failed', err as Object);
    }

    const bridgeRef = this.bridge;
    const settled: (string | null)[] = await Promise.all<string | null>([
      this.settleAutomationJson(
        bridgeRef.listConditionalTimersJson(hostId, null, true),
        'listConditionalTimersJson'
      ),
      this.settleAutomationJson(
        bridgeRef.listGoalRunsJson(hostId, null),
        'listGoalRunsJson'
      ),
      this.settleAutomationJson(
        bridgeRef.listMonitorWatchesJson(hostId, null),
        'listMonitorWatchesJson'
      ),
      this.settleAutomationJson(
        bridgeRef.listWorkflowRunsJson(hostId, true),
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
    if (this.bridge === null || this.selectedHostId.length === 0) {
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
      this.bridge.createScheduleJson(JSON.stringify(payload));
      this.showToast(`Created schedule "${cleanName}"`, false);
      this.bridge.refreshHost(this.selectedHostId);
      this.triggerHaptic(12);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createScheduleJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to create schedule', true);
      return false;
    }
  }

  public pauseSchedule(scheduleId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.pauseSchedule(this.selectedHostId, scheduleId);
      this.showToast('Schedule paused', false);
      this.triggerHaptic(8);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'pauseSchedule failed', err as Object);
      this.showToast((err as Error).message || 'Failed to pause schedule', true);
    }
  }

  public resumeSchedule(scheduleId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.resumeSchedule(this.selectedHostId, scheduleId);
      this.showToast('Schedule resumed', false);
      this.triggerHaptic(8);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resumeSchedule failed', err as Object);
      this.showToast((err as Error).message || 'Failed to resume schedule', true);
    }
  }

  public runScheduleOnce(scheduleId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.runScheduleOnce(this.selectedHostId, scheduleId);
      this.showToast('Triggered schedule run', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'runScheduleOnce failed', err as Object);
      this.showToast((err as Error).message || 'Failed to trigger schedule', true);
    }
  }

  public deleteSchedule(scheduleId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.deleteSchedule(this.selectedHostId, scheduleId);
      this.reloadHost(this.selectedHostId);
      this.showToast('Schedule deleted', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'deleteSchedule failed', err as Object);
      this.showToast((err as Error).message || 'Failed to delete schedule', true);
    }
  }

  public async cancelConditionalTimer(timerId: string): Promise<void> {
    if (this.bridge === null || this.selectedHostId.length === 0 || timerId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    try {
      const json = await this.bridge.cancelConditionalTimerJson(hostId, timerId);
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
    if (this.bridge === null || this.selectedHostId.length === 0 || timerId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    try {
      await this.bridge.fireConditionalTimerNowJson(hostId, timerId);
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
    if (this.bridge === null || this.selectedHostId.length === 0 || goalId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const req: ControlGoalRunRequestDto = {
      host_id: hostId,
      goal_id: goalId,
      action: action
    };
    try {
      const json = await this.bridge.controlGoalRunJson(JSON.stringify(req));
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
    if (this.bridge === null || this.selectedHostId.length === 0 || monitorId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const req: StopMonitorWatchRequestDto = {
      host_id: hostId,
      monitor_id: monitorId
    };
    try {
      const json = await this.bridge.stopMonitorWatchJson(JSON.stringify(req));
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
    if (this.bridge === null || this.selectedHostId.length === 0 || runId.length === 0) {
      return;
    }
    const hostId = this.selectedHostId;
    const req: CancelWorkflowRunRequestDto = {
      host_id: hostId,
      run_id: runId
    };
    try {
      const json = await this.bridge.cancelWorkflowRunJson(JSON.stringify(req));
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
    if (this.bridge === null) {
      return;
    }
    try {
      this.cloudSyncStatus = JSON.parse(this.bridge.cloudSyncStatusJson()) as CloudSyncStatusDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncStatusJson failed', err as Object);
    }
  }

  public generateCloudSyncPassphrase(): string {
    if (this.bridge === null) {
      return '';
    }
    try {
      return this.bridge.cloudSyncGeneratePassphrase();
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncGeneratePassphrase failed', err as Object);
      return '';
    }
  }

  public async cloudSyncLoginWithToken(token: string): Promise<boolean> {
    const clean = token.trim();
    if (this.bridge === null || clean.length === 0) {
      return false;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.bridge.cloudSyncLoginWithTokenJson(clean);
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
    if (this.bridge === null || clean.length === 0) {
      return false;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.bridge.cloudSyncUnlockWithPassphraseJson(clean);
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
    if (this.bridge === null) {
      return;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.bridge.cloudSyncNowJson();
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
    if (this.bridge === null) {
      return;
    }
    try {
      this.cloudSyncStatus = JSON.parse(
        this.bridge.cloudSyncSetAutoSyncJson(enabled)
      ) as CloudSyncStatusDto;
      this.triggerHaptic(6);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncSetAutoSyncJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to update auto-sync', true);
    }
  }

  public cloudSyncDisconnect(clearLocalHosts: boolean): void {
    if (this.bridge === null) {
      return;
    }
    this.cloudSyncBusy = true;
    try {
      this.cloudSyncStatus = JSON.parse(
        this.bridge.cloudSyncLogoutJson(clearLocalHosts)
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

  // 7. HarmonyOS Push Kit Host Registration & Deep-Link Notifications

  public registerPushTokenSilently(): void {
    registerHarmonyPushToken().then((outcome) => {
      if (outcome.token.length > 0) {
        this.pushToken = outcome.token;
        this.pushStatusMessage = 'Push Kit token ready';
        this.registerPushTokenInCore(outcome.token);
        this.syncPushTokenToAllHosts(false);
      } else {
        this.pushStatusMessage = outcome.errorMessage;
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'registerPushTokenSilently failed', err);
    });
  }

  private registerPushTokenInCore(token: string): void {
    if (this.bridge === null || token.trim().length === 0) {
      return;
    }
    try {
      this.bridge.registerPushTokenJson('harmony_push_kit', token.trim());
    } catch (err) {
      orbitLogWarn('OrbitStore', 'registerPushTokenJson failed', err as Object);
    }
  }

  public async syncPushTokenToAllHosts(force: boolean = false): Promise<void> {
    if (this.bridge === null || this.pushToken.trim().length === 0) {
      return;
    }
    const deviceId = this.prefsStore.getOrCreateDeviceId();
    const req: RegisterPushDeviceRequestDto = {
      device_id: deviceId,
      platform: 'harmony_push_kit',
      device_name: this.isFoldableDevice ? 'HarmonyOS Foldable' : 'HarmonyOS Device',
      device_token: this.pushToken.trim(),
      live_activity_token: null,
      sandbox: false
    };
    const reqJson = JSON.stringify(req);
    let syncedCount = 0;
    const nextSynced: string[] = force ? [] : this.pushSyncedHostIds.slice();
    for (let i = 0; i < this.hosts.length; i++) {
      const host = this.hosts[i];
      const online = host.connection_state === 'online_direct' || host.connection_state === 'online_relay';
      if (!online) {
        continue;
      }
      if (!force && nextSynced.indexOf(host.host_id) >= 0) {
        continue;
      }
      try {
        await this.bridge.registerPushDeviceJson(host.host_id, reqJson);
        if (nextSynced.indexOf(host.host_id) < 0) {
          nextSynced.push(host.host_id);
        }
        syncedCount += 1;
      } catch (err) {
        orbitLogWarn('OrbitStore', `registerPushDeviceJson failed for ${host.host_id}`, err as Object);
      }
    }
    this.pushSyncedHostIds = nextSynced;
    if (syncedCount > 0) {
      this.pushStatusMessage = `Synced to ${nextSynced.length} host${nextSynced.length === 1 ? '' : 's'}`;
      orbitLogInfo('OrbitStore', `Registered HarmonyOS Push Kit token on ${syncedCount} host(s)`);
    }
  }

  public async updatePushPreferences(nextPrefs: PushPreferencesDto): Promise<void> {
    const copy = clonePushPreferences(nextPrefs);
    this.pushPrefs = copy;
    const scopeKey = this.selectedHostId.trim().length > 0 ? this.selectedHostId.trim() : '__default__';
    this.pushPrefsByHost.set(scopeKey, copy);
    this.prefsStore.savePushPreferencesForHost(scopeKey, copy);
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    try {
      await this.bridge.updatePushPreferencesJson(this.selectedHostId, JSON.stringify(copy));
      this.showToast('Push preferences updated', false);
      this.triggerHaptic(6);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'updatePushPreferencesJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to update push preferences', true);
    }
  }

  public async sendTestPushNotification(): Promise<void> {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      this.showToast('Connect a host first', true);
      return;
    }
    try {
      const deviceId = this.prefsStore.getOrCreateDeviceId();
      await this.bridge.testPushNotificationJson(this.selectedHostId, deviceId);
      this.showToast('Test push sent from host', false);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'testPushNotificationJson failed', err as Object);
      this.showToast((err as Error).message || 'Test push failed', true);
    }
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
      this.registerPushTokenInCore(outcome.token);
      await this.syncPushTokenToAllHosts(true);
      this.pushStatusMessage = this.pushSyncedHostIds.length > 0
        ? `Registered on ${this.pushSyncedHostIds.length} host(s)`
        : 'Push Kit token registered';
    } else {
      this.pushStatusMessage = outcome.errorMessage;
    }
    if (notifOk && outcome.token.length > 0) {
      this.showToast('Push Kit synced & notifications ready', false);
    } else if (notifOk) {
      this.showToast(
        outcome.errorMessage.length > 0
          ? `Notifications enabled · ${outcome.errorMessage}`
          : 'Notifications enabled',
        false
      );
    } else if (outcome.token.length > 0) {
      this.showToast('Push Kit synced · enable notifications in Settings', false);
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

  // 8. Global Personal Agent Briefing & Safe Batch Approval

  public openGlobalOverview(): void {
    this.openPersonalAgentHome();
  }

  public isCurrentSessionPersonalAgent(): boolean {
    if (this.isNewSessionDraft || this.showPersonalAgentSetup) {
      return false;
    }
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return false;
    }
    const pa = this.personalAgentStore.briefing?.personal_agent ?? null;
    if (pa !== null && pa.host_id === this.selectedHostId && pa.agent_id === this.selectedAgentId) {
      return true;
    }
    const routerStatus = this.personalAgentStore.activeRouterStatus;
    if (
      routerStatus !== null &&
      this.primaryRouterHostId === this.selectedHostId &&
      routerStatus.config.router_agent_id !== null &&
      routerStatus.config.router_agent_id === this.selectedAgentId
    ) {
      return true;
    }
    return false;
  }

  public selectAgentOnHost(hostId: string, agentId: string, markRead: boolean = true): void {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      return;
    }
    this.showPersonalAgentSetup = false;
    this.showGlobalOverview = false;
    if (this.selectedHostId !== cleanHost) {
      this.selectHost(cleanHost);
    }
    this.selectAgent(cleanAgent);
    if (markRead) {
      this.markAgentRead(cleanHost, cleanAgent);
    }
    if (this.isCurrentSessionPersonalAgent()) {
      // Reuse fresh briefing if already loaded on entry (zero trailing duplicate RPC).
      this.personalAgentStore.ensureFreshBriefing();
      this.refreshPersonalAgentActivity(cleanHost, 50, null, false);
    }
  }

  public openPersonalAgentHome(): void {
    // Reuse fresh briefing when available; fetch once only when missing/stale/invalidated.
    this.personalAgentStore.ensureFreshBriefing();
    const pa = this.personalAgentStore.briefing?.personal_agent ?? null;
    if (pa !== null && pa.host_id.length > 0 && pa.agent_id.length > 0) {
      this.showPersonalAgentSetup = false;
      this.showGlobalOverview = false;
      this.selectAgentOnHost(pa.host_id, pa.agent_id, true);
      if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
        this.sidebarOpen = false;
      }
      this.triggerHaptic(8);
      return;
    }
    this.showPersonalAgentSetup = true;
    this.showGlobalOverview = false;
    this.briefingViewingOpen = false;
    this.showBriefingSheet = false;
    // Discovery is owned by PersonalAgentSetupCard.aboutToAppear() via coalesced refreshRouterAndMesh().
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.triggerHaptic(8);
  }

  public toggleBriefingPanel(): void {
    const nextOpen = !this.showBriefingPanel;
    this.briefingViewingOpen = nextOpen;
    this.showBriefingPanel = nextOpen;
    if (nextOpen) {
      this.briefingDismissedByUserForPaKey = '';
      this.closeGitInspector();
      this.showBriefingSheet = false;
      this.personalAgentStore.ensureFreshBriefing();
    } else if (this.isCurrentSessionPersonalAgent()) {
      this.briefingDismissedByUserForPaKey = `${this.selectedHostId}::${this.selectedAgentId}`;
    }
    this.triggerHaptic(6);
  }

  public closeBriefingPanel(): void {
    if (this.isCurrentSessionPersonalAgent()) {
      this.briefingDismissedByUserForPaKey = `${this.selectedHostId}::${this.selectedAgentId}`;
    }
    this.briefingViewingOpen = false;
    this.showBriefingPanel = false;
    this.showBriefingSheet = false;
  }

  public openBriefingSheet(): void {
    this.briefingDismissedByUserForPaKey = '';
    this.briefingViewingOpen = true;
    if (this.windowWidthVp >= OrbitTokens.BP_EXPANDED_MIN) {
      // Wide screen uses the 304 vp docked rail exclusively; never open Sheet simultaneously.
      this.showBriefingSheet = false;
      this.closeGitInspector();
      this.showBriefingPanel = true;
      this.personalAgentStore.ensureFreshBriefing();
      this.triggerHaptic(6);
      return;
    }
    this.showBriefingPanel = false;
    this.showBriefingSheet = true;
    this.personalAgentStore.ensureFreshBriefing();
    const pa = this.personalAgentStore.briefing?.personal_agent ?? null;
    if (pa !== null && pa.host_id.length > 0) {
      this.refreshPersonalAgentActivity(pa.host_id, 50, null, false);
    }
    this.triggerHaptic(6);
  }

  public closeBriefingSheet(): void {
    this.showBriefingSheet = false;
    // Only mark user-dismissed when closing on compact width; programmatic Sheet close during
    // a 1023 -> 1024 vp promotion MUST preserve `briefingViewingOpen = true` for the docked rail.
    if (this.windowWidthVp < OrbitTokens.BP_EXPANDED_MIN) {
      if (this.isCurrentSessionPersonalAgent()) {
        this.briefingDismissedByUserForPaKey = `${this.selectedHostId}::${this.selectedAgentId}`;
      }
      this.briefingViewingOpen = false;
      this.showBriefingPanel = false;
    }
  }

  public archiveAgentOnHost(hostId: string, agentId: string): void {
    if (this.bridge === null || hostId.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.bridge.archiveAgent(hostId, agentId);
      if (this.selectedHostId === hostId && this.selectedAgentId === agentId) {
        this.startNewSessionDraft();
      }
      this.reloadHostAsync(hostId);
      this.refreshSidebarAsync();
      this.personalAgentStore.refreshBriefing(true);
      this.triggerHaptic(10);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'archiveAgentOnHost failed', err as Object);
      this.showToast('Failed to archive session', true);
    }
  }

  public providerCatalogsForHost(hostId: string): ProviderModelCatalogDto[] {
    const detail = hostId.length > 0
      ? this.fleetStore.getCachedHostDetail(hostId)
      : this.currentHostDetail;
    if (detail !== null && detail.provider_catalogs !== undefined && detail.provider_catalogs.length > 0) {
      return detail.provider_catalogs;
    }
    return this.cachedFallbackCatalogs;
  }

  public ensureFreshPersonalAgentBriefing(): void {
    this.personalAgentStore.ensureFreshBriefing();
  }

  public refreshPersonalAgentBriefing(force: boolean = false): void {
    this.personalAgentStore.refreshBriefing(force);
  }

  public async refreshPersonalAgentActivity(
    hostId?: string,
    limit: number = 50,
    sinceMs: number | null = null,
    force: boolean = false
  ): Promise<void> {
    const targetHost = (hostId !== undefined && hostId.trim().length > 0)
      ? hostId.trim()
      : (this.primaryRouterHostId.length > 0 ? this.primaryRouterHostId : this.selectedHostId);
    if (targetHost.length === 0) {
      return;
    }
    await this.personalAgentStore.refreshActivity(targetHost, limit, sinceMs, force);
  }

  public resolveBriefingDecision(item: BriefingDecisionDto, decision: string): void {
    if (this.bridge === null || item.request_id === null || item.request_id.length === 0) {
      return;
    }
    try {
      this.bridge.resolvePermission(item.host_id, item.agent_id, item.request_id, decision);
      this.triggerHaptic(10);
      this.reloadFleetAsync();
      this.personalAgentStore.refreshBriefing(true);
      if (this.selectedHostId === item.host_id && this.selectedAgentId === item.agent_id) {
        this.reloadStage();
      }
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resolveBriefingDecision failed', err as Object);
      this.showToast((err as Error).message || 'Approval failed', true);
    }
  }

  public async approveSafeBatchPermissions(
    frozenTargets?: readonly FrozenBatchApprovalTarget[]
  ): Promise<void> {
    if (this.bridge === null) {
      return;
    }
    const liveDecisions = this.personalAgentStore.needsYou;
    const confirmedTargets = frozenTargets !== undefined
      ? frozenTargets
      : snapshotBatchApprovalTargets(liveDecisions);
    const byHost = intersectFrozenBatchTargets(confirmedTargets, liveDecisions);
    if (byHost.size === 0) {
      this.showLocalizedToast(
        $r('app.string.briefing_batchNothingEligible'),
        'No confirmed requests remain eligible',
        false
      );
      return;
    }
    const outcomes: BatchApprovalHostOutcome[] = [];
    const hostIds = Array.from(byHost.keys());
    for (let i = 0; i < hostIds.length; i++) {
      const hid = hostIds[i];
      const batch = byHost.get(hid) ?? [];
      if (batch.length === 0) {
        continue;
      }
      try {
        const rawReceipt = await this.bridge.resolvePermissionsBatchJson(hid, JSON.stringify(batch), 'allow');
        outcomes.push(parseBatchApprovalHostResponse(hid, rawReceipt));
      } catch (err) {
        orbitLogWarn('OrbitStore', `resolvePermissionsBatchJson failed on ${hid}`, err as Object);
        outcomes.push({
          hostId: hid,
          resolvedCount: 0,
          skippedCount: 0,
          firstSkipReason: '',
          failed: true
        });
      }
    }
    const summary = aggregateBatchApprovalOutcomes(outcomes);
    const feedback = classifyBatchApprovalFeedback(summary);
    this.reloadFleetAsync();
    this.personalAgentStore.refreshBriefing(true);
    if (this.selectedAgentId.length > 0) {
      this.reloadStage();
    }
    if (feedback.kind === 'approved_all') {
      this.showLocalizedToast(
        $r('app.string.briefing_batchApprovedCount', feedback.resolvedCount),
        `Approved ${feedback.resolvedCount} safe request(s)`,
        false,
        [feedback.resolvedCount]
      );
      this.triggerHaptic(14);
    } else if (feedback.kind === 'approved_with_skipped') {
      this.showLocalizedToast(
        $r('app.string.briefing_batchApprovedWithSkipped', feedback.resolvedCount, feedback.skippedCount),
        `Approved ${feedback.resolvedCount}, skipped ${feedback.skippedCount}`,
        false,
        [feedback.resolvedCount, feedback.skippedCount]
      );
      this.triggerHaptic(12);
    } else if (feedback.kind === 'partial_failure') {
      this.showLocalizedToast(
        $r(
          'app.string.briefing_batchPartialFailure',
          feedback.resolvedCount,
          feedback.skippedCount,
          feedback.failedHostCount
        ),
        `Approved ${feedback.resolvedCount}, skipped ${feedback.skippedCount}; ${feedback.failedHostCount} host(s) failed`,
        true,
        [feedback.resolvedCount, feedback.skippedCount, feedback.failedHostCount]
      );
    } else if (feedback.kind === 'skipped_and_failed') {
      const baseMsg = this.resolveResourceText(
        $r('app.string.briefing_batchSkippedAndFailed', feedback.skippedCount, feedback.failedHostCount),
        `Skipped ${feedback.skippedCount} request(s); ${feedback.failedHostCount} host(s) failed`
      )
        .replace(/%1\$d/g, String(feedback.skippedCount))
        .replace(/%2\$d/g, String(feedback.failedHostCount));
      const fullMsg = feedback.firstSkipReason.length > 0
        ? `${baseMsg} (${feedback.firstSkipReason})`
        : baseMsg;
      this.showToast(fullMsg, true);
    } else if (feedback.kind === 'all_skipped') {
      const baseMsg = this.resolveResourceText(
        $r('app.string.briefing_batchAllSkipped'),
        'Skipped %d request(s); none approved'
      ).replace(/%([1-9][0-9]*\$)?d/g, String(feedback.skippedCount));
      const fullMsg = feedback.firstSkipReason.length > 0
        ? `${baseMsg} (${feedback.firstSkipReason})`
        : baseMsg;
      this.showToast(fullMsg, true);
    } else if (feedback.kind === 'failed') {
      this.showLocalizedToast(
        $r('app.string.briefing_batchFailed'),
        'Batch approval failed',
        true
      );
    }
  }

  public submitPersonalAgentPrompt(promptText: string): void {
    const clean = promptText.trim();
    if (clean.length === 0) {
      return;
    }
    // Route to configured Primary Personal Router when available
    if (this.primaryRouterHostId.length > 0) {
      const targetRouter = this.routerHosts.find(
        (h: RouterHostCapabilitiesDto): boolean => h.host_id === this.primaryRouterHostId
      );
      if (targetRouter !== undefined && targetRouter.active_router_agent_id !== undefined &&
        targetRouter.active_router_agent_id !== null && targetRouter.active_router_agent_id.length > 0) {
        this.selectHost(targetRouter.host_id);
        this.selectAgent(targetRouter.active_router_agent_id);
        this.composerText = clean;
        this.submitComposerPrompt(clean);
        return;
      }
    }
    // Otherwise start a fresh conversation on the active host
    this.startNewSessionDraft();
    this.composerText = clean;
    this.submitComposerPrompt(clean);
  }

  // 9. Codex-Style Voice Dictation in Composer

  public startVoiceDictation(context: common.Context | undefined): void {
    if (this.isVoiceRecording) {
      this.finishVoiceDictation();
      return;
    }
    this.voiceInterimTranscript = '';
    this.voiceElapsedSeconds = 0;
    this.triggerHaptic(10);
    this.voiceController.start(context, {
      onPartialResult: (partial: string): void => {
        this.voiceInterimTranscript = partial;
      },
      onFinalResult: (finalText: string): void => {
        this.stopVoiceTimer();
        this.isVoiceRecording = false;
        this.voiceInterimTranscript = '';
        const clean = finalText.trim();
        if (clean.length > 0) {
          const prefix = this.composerText.trim().length > 0 ? `${this.composerText.trimEnd()} ` : '';
          this.composerText = `${prefix}${clean}`;
          this.triggerHaptic(8);
        }
      },
      onStateChanged: (recording: boolean): void => {
        this.isVoiceRecording = recording;
        if (recording) {
          this.startVoiceTimer();
        } else {
          this.stopVoiceTimer();
        }
      },
      onError: (msg: string): void => {
        this.stopVoiceTimer();
        this.isVoiceRecording = false;
        this.voiceInterimTranscript = '';
        this.showToast(msg, true);
      }
    }).catch((err: Object) => {
      this.stopVoiceTimer();
      this.isVoiceRecording = false;
      orbitLogWarn('OrbitStore', 'startVoiceDictation error', err);
    });
  }

  public finishVoiceDictation(): void {
    const pending = this.voiceController.finish().trim();
    const candidate = pending.length > 0 ? pending : this.voiceInterimTranscript.trim();
    this.stopVoiceTimer();
    this.isVoiceRecording = false;
    this.voiceInterimTranscript = '';
    if (candidate.length > 0) {
      const prefix = this.composerText.trim().length > 0 ? `${this.composerText.trimEnd()} ` : '';
      this.composerText = `${prefix}${candidate}`;
      this.triggerHaptic(10);
    }
  }

  public cancelVoiceDictation(): void {
    this.voiceController.cancel();
    this.stopVoiceTimer();
    this.isVoiceRecording = false;
    this.voiceInterimTranscript = '';
    this.triggerHaptic(6);
  }

  private startVoiceTimer(): void {
    this.stopVoiceTimer();
    this.voiceElapsedSeconds = 0;
    this.voiceTimerId = setInterval(() => {
      this.voiceElapsedSeconds += 1;
    }, 1000);
  }

  private stopVoiceTimer(): void {
    if (this.voiceTimerId !== -1) {
      clearInterval(this.voiceTimerId);
      this.voiceTimerId = -1;
    }
  }

  // 10. Stage Tail Pagination, Plan Approval, Subagents, Stall Recovery & Usage

  public loadOlderHistory(): number {
    const active = this.stageStore.activeSession;
    if (active === null || active.loadingOlder) {
      return 0;
    }
    const inserted = this.stageStore.loadOlder(active);
    if (inserted > 0) {
      this.triggerHaptic(6);
    }
    return inserted;
  }

  public async resolvePlanApproval(
    requestId: string,
    action: string,
    editedPlanMarkdown: string | null,
    feedback: string | null,
    targetMode: string
  ): Promise<boolean> {
    if (
      this.bridge === null ||
      this.selectedHostId.length === 0 ||
      this.selectedAgentId.length === 0
    ) {
      return false;
    }
    try {
      await this.bridge.resolvePlanApprovalJson(
        this.selectedHostId,
        this.selectedAgentId,
        requestId,
        JSON.stringify(action),
        editedPlanMarkdown,
        feedback,
        JSON.stringify(targetMode)
      );
      if (this.stageStore.activeSession !== null) {
        this.stageStore.activeSession.pendingPlanApproval = null;
      }
      this.stageStore.activePendingPlanApproval = null;
      this.reloadStage();
      this.showToast(
        action === 'approve_and_build'
          ? 'Plan approved · Building'
          : (action === 'request_changes' ? 'Requested plan revisions' : 'Plan dismissed'),
        false
      );
      this.triggerHaptic(12);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resolvePlanApprovalJson failed', err as Object);
      this.showToast((err as Error).message || 'Plan resolution failed', true);
      return false;
    }
  }

  public interruptAndContinueCurrentAgent(): void {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    try {
      this.bridge.interruptAgentAndContinue(this.selectedHostId, this.selectedAgentId);
      this.showToast('Interrupted & continuing turn', false);
      this.triggerHaptic(12);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'interruptAgentAndContinue failed', err as Object);
      this.showToast((err as Error).message || 'Interrupt failed', true);
    }
  }

  public async refreshSessionUsage(): Promise<void> {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      this.activeSessionUsage = null;
      return;
    }
    try {
      const json = await this.bridge.getSessionUsageJson(this.selectedHostId, this.selectedAgentId);
      this.activeSessionUsage = JSON.parse(json) as SessionUsageDto;
    } catch (_err) {
      this.activeSessionUsage = null;
    }
  }

  public inspectSubagent(sub: SessionSubagentEntryDto): void {
    if (sub.child_agent_id !== null && sub.child_agent_id.length > 0) {
      const targetHostId = (
        sub.host_id !== undefined &&
        sub.host_id !== null &&
        sub.host_id.trim().length > 0
      ) ? sub.host_id.trim() : this.selectedHostId;
      if (targetHostId.length > 0) {
        this.selectAgentOnHost(targetHostId, sub.child_agent_id, true);
      } else {
        this.selectAgent(sub.child_agent_id);
      }
      return;
    }
    const subId = (sub.provider_subagent_id !== undefined && sub.provider_subagent_id !== null && sub.provider_subagent_id.length > 0)
      ? sub.provider_subagent_id
      : sub.id;
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    this.inspectedSubagentId = subId;
    this.inspectedSubagentTitle = sub.role.length > 0 ? sub.role : 'Subagent';
    this.inspectedSubagentItems = [];
    this.showSubagentSheet = true;
    try {
      this.bridge.openProviderSubagentTimeline(this.selectedHostId, this.selectedAgentId, subId);
      const raw = this.bridge.providerSubagentTimelineJson(this.selectedHostId, this.selectedAgentId, subId);
      const parsed = JSON.parse(raw) as ProviderSubagentTimelineDto;
      this.inspectedSubagentItems = parsed.items ?? [];
    } catch (err) {
      orbitLogWarn('OrbitStore', 'providerSubagentTimelineJson failed', err as Object);
    }
    this.triggerHaptic(8);
  }

  // 11. File Explorer & Code Viewer (Read / Edit / Create / Rename / Delete)

  public openFileExplorer(initialPath?: string): void {
    this.ensureWorkspaceFilesLoaded();
    this.showFileExplorerSheet = true;
    if (initialPath !== undefined && initialPath.trim().length > 0) {
      this.openWorkspaceFileInViewer(initialPath.trim());
    }
    this.triggerHaptic(8);
  }

  public async openWorkspaceFileInViewer(path: string): Promise<void> {
    this.inspectedFilePath = path;
    this.fileExplorerBusy = true;
    try {
      const view = await this.readWorkspaceFile(path);
      this.inspectedFileView = view;
    } finally {
      this.fileExplorerBusy = false;
    }
  }

  public async saveWorkspaceFile(path: string, content: string): Promise<boolean> {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return false;
    }
    this.fileExplorerBusy = true;
    try {
      const req: WriteWorkspaceFileRequestDto = {
        host_id: this.selectedHostId,
        cwd: this.activeCwd,
        path: path,
        content: content,
        expected_etag: null,
        force: true
      };
      await this.bridge.writeWorkspaceFileJson(JSON.stringify(req));
      await this.openWorkspaceFileInViewer(path);
      this.showToast(`Saved ${basenameOfPath(path)}`, false);
      this.triggerHaptic(10);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'writeWorkspaceFileJson failed', err as Object);
      this.showToast((err as Error).message || 'Save failed', true);
      return false;
    } finally {
      this.fileExplorerBusy = false;
    }
  }

  public async createWorkspaceEntry(
    path: string,
    isDir: boolean,
    initialContent: string | null
  ): Promise<boolean> {
    const clean = path.trim();
    if (this.bridge === null || this.selectedHostId.length === 0 || clean.length === 0) {
      return false;
    }
    this.fileExplorerBusy = true;
    try {
      const req: CreateWorkspaceEntryRequestDto = {
        host_id: this.selectedHostId,
        cwd: this.activeCwd,
        path: clean,
        is_dir: isDir,
        initial_content: initialContent
      };
      await this.bridge.createWorkspaceEntryJson(JSON.stringify(req));
      this.bridge?.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      this.reloadWorkspaceFiles();
      this.showToast(`Created ${basenameOfPath(clean)}`, false);
      this.triggerHaptic(8);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createWorkspaceEntryJson failed', err as Object);
      this.showToast((err as Error).message || 'Create failed', true);
      return false;
    } finally {
      this.fileExplorerBusy = false;
    }
  }

  public async renameWorkspaceEntry(oldPath: string, newPath: string): Promise<boolean> {
    const cleanNew = newPath.trim();
    if (this.bridge === null || this.selectedHostId.length === 0 || cleanNew.length === 0) {
      return false;
    }
    this.fileExplorerBusy = true;
    try {
      const req: RenameWorkspaceEntryRequestDto = {
        host_id: this.selectedHostId,
        cwd: this.activeCwd,
        old_path: oldPath,
        new_path: cleanNew
      };
      await this.bridge.renameWorkspaceEntryJson(JSON.stringify(req));
      this.bridge?.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      this.reloadWorkspaceFiles();
      this.inspectedFilePath = cleanNew;
      await this.openWorkspaceFileInViewer(cleanNew);
      this.showToast(`Renamed to ${basenameOfPath(cleanNew)}`, false);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'renameWorkspaceEntryJson failed', err as Object);
      this.showToast((err as Error).message || 'Rename failed', true);
      return false;
    } finally {
      this.fileExplorerBusy = false;
    }
  }

  public async deleteWorkspaceEntry(path: string): Promise<boolean> {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return false;
    }
    this.fileExplorerBusy = true;
    try {
      const req: DeleteWorkspaceEntryRequestDto = {
        host_id: this.selectedHostId,
        cwd: this.activeCwd,
        path: path
      };
      await this.bridge.deleteWorkspaceEntryJson(JSON.stringify(req));
      if (this.inspectedFilePath === path) {
        this.inspectedFilePath = '';
        this.inspectedFileView = null;
      }
      this.bridge?.fetchWorkspaceFiles(this.selectedHostId, this.activeCwd);
      this.reloadWorkspaceFiles();
      this.showToast(`Deleted ${basenameOfPath(path)}`, false);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'deleteWorkspaceEntryJson failed', err as Object);
      this.showToast((err as Error).message || 'Delete failed', true);
      return false;
    } finally {
      this.fileExplorerBusy = false;
    }
  }

  // 12. Git Write / Commit / PR / Stash / Conflicts / Rebase

  public async gitStagePaths(paths: string[], all: boolean): Promise<boolean> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return false;
    }
    this.gitWriteBusy = true;
    try {
      await this.bridge.gitStagePathsJson(targetHost, targetCwd, paths, all);
      this.pullRefreshGitReview();
      this.showToast(all ? 'Staged all changes' : 'Staged file', false);
      this.triggerHaptic(8);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitStagePathsJson failed', err as Object);
      this.showToast((err as Error).message || 'Stage failed', true);
      return false;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async gitUnstagePaths(paths: string[], all: boolean): Promise<boolean> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return false;
    }
    this.gitWriteBusy = true;
    try {
      await this.bridge.gitUnstagePathsJson(targetHost, targetCwd, paths, all);
      this.pullRefreshGitReview();
      this.showToast(all ? 'Unstaged all changes' : 'Unstaged file', false);
      this.triggerHaptic(8);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitUnstagePathsJson failed', err as Object);
      this.showToast((err as Error).message || 'Unstage failed', true);
      return false;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async gitGenerateCommitMessage(): Promise<string> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return '';
    }
    this.gitWriteBusy = true;
    try {
      const json = await this.bridge.gitGenerateCommitMessageJson(targetHost, targetCwd);
      const parsed = JSON.parse(json) as GitGeneratedCommitMessageDto;
      if (parsed.message !== undefined && parsed.message.trim().length > 0) {
        return parsed.message.trim();
      }
      if (parsed.subject !== undefined && parsed.subject.trim().length > 0) {
        const body = (parsed.body ?? '').trim();
        return body.length > 0 ? `${parsed.subject.trim()}\n\n${body}` : parsed.subject.trim();
      }
      return '';
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitGenerateCommitMessageJson failed', err as Object);
      this.showToast((err as Error).message || 'Unable to generate commit message', true);
      return '';
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async gitCommit(message: string, amend: boolean): Promise<boolean> {
    const clean = message.trim();
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0 || clean.length === 0) {
      return false;
    }
    this.gitWriteBusy = true;
    try {
      const json = await this.bridge.gitCommitJson(targetHost, targetCwd, clean, amend);
      const res = JSON.parse(json) as GitCommitResultDto;
      this.pullRefreshGitReview();
      this.refreshGitCollabStatus();
      this.showToast(`Committed ${res.short_sha}`, false);
      this.triggerHaptic(12);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitCommitJson failed', err as Object);
      this.showToast((err as Error).message || 'Commit failed', true);
      return false;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async refreshGitCollabStatus(): Promise<void> {
    const hid = this.resolvedDiffHostId();
    if (this.bridge === null || hid.length === 0) {
      return;
    }
    const cwd = this.resolvedDiffCwd();
    this.gitCollabRequestSeq += 1;
    const reqSeq = this.gitCollabRequestSeq;
    const stillCurrent = (): boolean => isGitCollabResponseCurrent(
      reqSeq,
      this.gitCollabRequestSeq,
      hid,
      this.resolvedDiffHostId(),
      cwd,
      this.resolvedDiffCwd(),
      this.resolveHostHomeDir(hid)
    );
    try {
      const prJson = await this.bridge.gitPrStatusJson(hid, cwd);
      if (!stillCurrent()) {
        return;
      }
      this.gitPrStatus = JSON.parse(prJson) as GitPrStatusDto;
    } catch (_e) {
      if (!stillCurrent()) {
        return;
      }
      this.gitPrStatus = null;
    }
    try {
      const stashJson = await this.bridge.gitStashListJson(hid, cwd);
      if (!stillCurrent()) {
        return;
      }
      this.gitStashes = JSON.parse(stashJson) as GitStashEntryDto[];
    } catch (_e) {
      if (!stillCurrent()) {
        return;
      }
      this.gitStashes = [];
    }
    try {
      const confJson = await this.bridge.gitConflictsJson(hid, cwd);
      if (!stillCurrent()) {
        return;
      }
      this.gitConflicts = JSON.parse(confJson) as GitConflictEntryDto[];
    } catch (_e) {
      if (!stillCurrent()) {
        return;
      }
      this.gitConflicts = [];
    }
    try {
      const rebJson = await this.bridge.gitRebaseStatusJson(hid, cwd);
      if (!stillCurrent()) {
        return;
      }
      this.gitRebaseStatus = JSON.parse(rebJson) as GitRebaseStatusDto;
    } catch (_e) {
      if (!stillCurrent()) {
        return;
      }
      this.gitRebaseStatus = null;
    }
  }

  public async gitCreatePr(
    title: string,
    body: string,
    base: string | null,
    draft: boolean
  ): Promise<GitCreatePrResultDto | null> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0 || title.trim().length === 0) {
      return null;
    }
    this.gitWriteBusy = true;
    try {
      const json = await this.bridge.gitCreatePrJson(
        targetHost,
        targetCwd,
        title.trim(),
        body.trim(),
        base !== null && base.trim().length > 0 ? base.trim() : null,
        draft
      );
      const res = JSON.parse(json) as GitCreatePrResultDto;
      this.refreshGitCollabStatus();
      this.showToast(`Created PR: ${res.title}`, false);
      this.triggerHaptic(12);
      return res;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitCreatePrJson failed', err as Object);
      this.showToast((err as Error).message || 'Create PR failed', true);
      return null;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async gitSaveStash(message: string | null, includeUntracked: boolean): Promise<boolean> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return false;
    }
    this.gitWriteBusy = true;
    try {
      await this.bridge.gitStashSaveJson(targetHost, targetCwd, message, includeUntracked);
      this.pullRefreshGitReview();
      this.refreshGitCollabStatus();
      this.showToast('Saved working tree stash', false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Stash failed', true);
      return false;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async gitPopStash(stashRef: string, expectedOid: string | null): Promise<boolean> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return false;
    }
    this.gitWriteBusy = true;
    try {
      await this.bridge.gitStashPopJson(targetHost, targetCwd, stashRef, expectedOid);
      this.pullRefreshGitReview();
      this.refreshGitCollabStatus();
      this.showToast(`Popped ${stashRef}`, false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Pop stash failed', true);
      return false;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async gitRebaseContinue(): Promise<boolean> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return false;
    }
    this.gitWriteBusy = true;
    try {
      await this.bridge.gitRebaseContinueJson(targetHost, targetCwd);
      this.pullRefreshGitReview();
      this.refreshGitCollabStatus();
      this.showToast('Continued rebase', false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Rebase continue failed', true);
      return false;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  public async gitRebaseAbort(): Promise<boolean> {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return false;
    }
    this.gitWriteBusy = true;
    try {
      await this.bridge.gitRebaseAbortJson(targetHost, targetCwd);
      this.pullRefreshGitReview();
      this.refreshGitCollabStatus();
      this.showToast('Aborted rebase', false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Rebase abort failed', true);
      return false;
    } finally {
      this.gitWriteBusy = false;
    }
  }

  // 13. Global Cross-Host Search

  public openGlobalSearch(): void {
    this.showGlobalSearchSheet = true;
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.triggerHaptic(8);
  }

  public runLocalGlobalSearch(query: string): void {
    this.globalSearchQuery = query;
    const clean = query.trim();
    if (this.bridge === null || clean.length === 0) {
      this.globalSearchHits = [];
      this.globalSearchRemoteQueried = 0;
      this.globalSearchRemoteFailed = 0;
      return;
    }
    try {
      const json = this.bridge.searchMessagesJson(clean, 40);
      this.globalSearchHits = JSON.parse(json) as ConversationSearchHitDto[];
    } catch (err) {
      orbitLogWarn('OrbitStore', 'searchMessagesJson failed', err as Object);
    }
  }

  public async runRemoteGlobalSearch(query: string): Promise<void> {
    const clean = query.trim();
    this.globalSearchQuery = query;
    if (this.bridge === null || clean.length === 0) {
      return;
    }
    this.globalSearchBusy = true;
    try {
      const req: SearchMessagesRequestDto = {
        query: clean,
        host_id: null,
        agent_id: null,
        limit: 50
      };
      const json = await this.bridge.searchMessagesRemoteJson(JSON.stringify(req));
      const res = JSON.parse(json) as SearchMessagesResponseDto;
      this.globalSearchHits = res.hits ?? [];
      this.globalSearchRemoteQueried = res.remote_hosts_queried ?? 0;
      this.globalSearchRemoteFailed = res.remote_hosts_failed ?? 0;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'searchMessagesRemoteJson failed', err as Object);
      this.showToast((err as Error).message || 'Remote search failed', true);
    } finally {
      this.globalSearchBusy = false;
    }
  }

  public jumpToSearchHit(hit: ConversationSearchHitDto): void {
    this.showGlobalSearchSheet = false;
    this.openSessionFromDeepLink(hit.host_id, hit.agent_id, '');
    if (hit.is_unloaded) {
      this.loadOlderHistory();
    }
  }

  // 14. Personal Router & Fleet Mesh

  public openRouterMeshSheet(): void {
    this.showRouterMeshSheet = true;
    this.refreshRouterAndMesh();
    this.triggerHaptic(8);
  }

  public async refreshRouterAndMesh(): Promise<void> {
    if (this.bridge === null) {
      return;
    }
    if (this.routerDiscoveryInFlight !== null) {
      return this.routerDiscoveryInFlight;
    }
    const work = this.executeRouterAndMeshRefresh();
    this.routerDiscoveryInFlight = work;
    try {
      await work;
    } finally {
      if (this.routerDiscoveryInFlight === work) {
        this.routerDiscoveryInFlight = null;
      }
    }
  }

  private async executeRouterAndMeshRefresh(): Promise<void> {
    if (this.bridge === null) {
      return;
    }
    this.routerMeshBusy = true;
    try {
      try {
        const routersJson = await this.bridge.discoverFleetRouterHostsJson();
        this.routerHosts = JSON.parse(routersJson) as RouterHostCapabilitiesDto[];
      } catch (err) {
        orbitLogWarn('OrbitStore', 'discoverFleetRouterHostsJson failed', err as Object);
      }

      const targetHost = this.primaryRouterHostId.length > 0
        ? this.primaryRouterHostId
        : (this.routerHosts.length > 0 ? this.routerHosts[0].host_id : this.selectedHostId);
      if (targetHost.length > 0) {
        await this.loadRouterStatusForHost(targetHost);
      }

      try {
        const meshJson = await this.bridge.getFleetMeshSnapshotJson();
        const meshMap = JSON.parse(meshJson) as Record<string, ConnectedHostSummaryDto[]>;
        const rows: OrbitMeshPeerRow[] = [];
        const keys = Object.keys(meshMap);
        for (let i = 0; i < keys.length; i++) {
          const ownerId = keys[i];
          const peers = meshMap[ownerId] ?? [];
          const ownerCard = this.hosts.find((h: HostCardDto): boolean => h.host_id === ownerId);
          const ownerName = ownerCard !== undefined ? ownerCard.display_name : ownerId;
          for (let j = 0; j < peers.length; j++) {
            rows.push({
              owner_host_id: ownerId,
              owner_host_name: ownerName,
              peer: peers[j]
            });
          }
        }
        this.fleetMeshPeers = rows;
      } catch (err) {
        orbitLogWarn('OrbitStore', 'getFleetMeshSnapshotJson failed', err as Object);
      }
    } finally {
      this.routerMeshBusy = false;
    }
  }

  /**
   * Fetches the target host's current `PersonalRouterConfigDto` without mutating
   * `primaryRouterHostId`. Throws on RPC/parse failure so setup flows never overwrite an
   * existing host's security/webhook configuration with fallback defaults.
   */
  public async fetchHostRouterConfig(hostId: string): Promise<PersonalRouterConfigDto | null> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return null;
    }
    const json = await this.bridge.getRouterConfigJson(cleanHost);
    const snap = JSON.parse(json) as RouterStatusSnapshotDto;
    if (snap === null || snap === undefined || snap.config === undefined || snap.config === null) {
      throw new Error('Missing router config in host response');
    }
    return normalizePersonalRouterConfig(snap.config);
  }

  public async loadRouterStatusForHost(hostId: string): Promise<void> {
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    try {
      const json = await this.bridge.getRouterConfigJson(hostId);
      this.activeRouterStatus = JSON.parse(json) as RouterStatusSnapshotDto;
      this.primaryRouterHostId = hostId;
      this.prefsStore.setPrimaryRouterHostId(hostId);
    } catch (err) {
      orbitLogWarn('OrbitStore', `getRouterConfigJson failed for ${hostId}`, err as Object);
    }
  }

  public async saveRouterConfig(
    hostId: string,
    config: PersonalRouterConfigDto,
    activate: boolean
  ): Promise<boolean> {
    if (this.bridge === null || hostId.length === 0) {
      return false;
    }
    this.routerMeshBusy = true;
    try {
      const json = await this.bridge.updateRouterConfigJson(
        hostId,
        JSON.stringify(config),
        null,
        activate
      );
      this.activeRouterStatus = JSON.parse(json) as RouterStatusSnapshotDto;
      this.primaryRouterHostId = hostId;
      this.prefsStore.setPrimaryRouterHostId(hostId);
      this.showLocalizedToast(
        $r('app.string.briefing_settingsSaved'),
        'Personal Agent settings saved',
        false
      );
      this.triggerHaptic(10);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'updateRouterConfigJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to update router', true);
      return false;
    } finally {
      this.routerMeshBusy = false;
    }
  }

  public async toggleMeshPeerEnabled(
    ownerHostId: string,
    registrationId: string,
    label: string,
    enabled: boolean
  ): Promise<void> {
    if (this.bridge === null) {
      return;
    }
    try {
      await this.bridge.updateMeshPeerJson(ownerHostId, registrationId, label, enabled);
      await this.refreshRouterAndMesh();
      this.showToast(enabled ? `Enabled ${label}` : `Paused ${label}`, false);
      this.triggerHaptic(6);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'updateMeshPeerJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to update mesh peer', true);
    }
  }

  // 15. Zero-Passphrase Cloud Sync + Device Link QR + GitHub Device Flow + Daemon Upgrade

  public exportCloudSyncDeviceLinkUri(): string {
    if (this.bridge === null) {
      return '';
    }
    try {
      const uri = this.bridge.cloudSyncExportDeviceLinkUri();
      this.deviceLinkUri = uri;
      return uri;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncExportDeviceLinkUri failed', err as Object);
      this.showToast((err as Error).message || 'Device Link QR unavailable', true);
      return '';
    }
  }

  public async importCloudSyncDeviceLinkUri(uri: string): Promise<boolean> {
    const clean = uri.trim();
    if (this.bridge === null || clean.length === 0) {
      return false;
    }
    this.cloudSyncBusy = true;
    try {
      await this.bridge.cloudSyncImportDeviceLinkUriJson(clean);
      this.refreshCloudSyncStatus();
      this.reloadFleet();
      this.showToast('Device linked & vault synced', false);
      this.triggerHaptic(14);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncImportDeviceLinkUriJson failed', err as Object);
      this.showToast((err as Error).message || 'Device Link failed', true);
      return false;
    } finally {
      this.cloudSyncBusy = false;
    }
  }

  public scanDeviceLinkQr(context: common.Context | undefined): void {
    scanPairingQrCode(context).then((outcome) => {
      if (outcome.cancelled) {
        return;
      }
      if (outcome.uri.length > 0) {
        this.importCloudSyncDeviceLinkUri(outcome.uri);
      } else if (outcome.errorMessage.length > 0) {
        this.showToast(outcome.errorMessage, true);
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'scanDeviceLinkQr failed', err);
    });
  }

  public async setCloudSyncManualPassphraseMode(requireManual: boolean): Promise<void> {
    if (this.bridge === null) {
      return;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.bridge.cloudSyncSetManualPassphraseModeJson(requireManual);
      this.cloudSyncStatus = JSON.parse(json) as CloudSyncStatusDto;
      this.showToast(
        requireManual ? 'Manual passphrase required' : 'Zero-passphrase instant unlock enabled',
        false
      );
      this.triggerHaptic(8);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncSetManualPassphraseModeJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to update unlock mode', true);
    } finally {
      this.cloudSyncBusy = false;
    }
  }

  public async startGithubDeviceFlow(): Promise<void> {
    if (this.bridge === null) {
      return;
    }
    this.cloudSyncBusy = true;
    try {
      const json = await this.bridge.cloudSyncStartGithubDeviceFlowJson(null);
      this.githubDeviceFlow = JSON.parse(json) as GithubDeviceFlowStartDto;
      this.copyToClipboard(this.githubDeviceFlow.user_code, `Copied code ${this.githubDeviceFlow.user_code}`);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncStartGithubDeviceFlowJson failed', err as Object);
      this.showToast((err as Error).message || 'GitHub sign-in unavailable', true);
    } finally {
      this.cloudSyncBusy = false;
    }
  }

  public async pollGithubDeviceFlow(): Promise<string> {
    if (this.bridge === null || this.githubDeviceFlow === null) {
      return 'idle';
    }
    try {
      const json = await this.bridge.cloudSyncPollGithubDeviceFlowJson(
        this.githubDeviceFlow.device_code,
        null
      );
      const polled = JSON.parse(json) as GithubDeviceFlowPollDto;
      if (polled.status === 'authorized') {
        this.githubDeviceFlow = null;
        this.refreshCloudSyncStatus();
        this.reloadFleet();
        this.showToast('Signed in with GitHub', false);
        this.triggerHaptic(14);
      } else if (polled.status === 'expired' || polled.status === 'denied') {
        this.githubDeviceFlow = null;
        this.showToast(`GitHub sign-in ${polled.status}`, true);
      }
      return polled.status;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'cloudSyncPollGithubDeviceFlowJson failed', err as Object);
      return 'error';
    }
  }

  public async upgradeHostDaemon(hostId: string): Promise<void> {
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    try {
      this.showToast('Upgrading host daemon…', false);
      const summary = await this.bridge.upgradeHostDaemonOverWire(hostId, null);
      this.showToast(summary.length > 0 ? summary : 'Host daemon upgraded', false);
      this.reloadFleet();
      this.triggerHaptic(12);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'upgradeHostDaemonOverWire failed', err as Object);
      this.showToast((err as Error).message || 'Daemon upgrade failed', true);
    }
  }

  // 16. Automations Creation & Schedule Run History

  public loadScheduleRuns(scheduleId: string): void {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    try {
      this.bridge.fetchScheduleRuns(this.selectedHostId, scheduleId, 10);
      const raw = this.bridge.scheduleRunsJson(this.selectedHostId, scheduleId);
      const parsed = JSON.parse(raw) as ScheduleRunRecordDto[];
      const next: Record<string, ScheduleRunRecordDto[]> = {};
      const keys = Object.keys(this.scheduleRunsById);
      for (let i = 0; i < keys.length; i++) {
        next[keys[i]] = this.scheduleRunsById[keys[i]];
      }
      next[scheduleId] = parsed;
      this.scheduleRunsById = next;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'scheduleRunsJson failed', err as Object);
    }
  }

  public async createConditionalTimer(
    durationSeconds: number,
    prompt: string,
    conditionKind: string
  ): Promise<boolean> {
    const cleanPrompt = prompt.trim();
    if (
      this.bridge === null ||
      this.selectedHostId.length === 0 ||
      this.selectedAgentId.length === 0 ||
      cleanPrompt.length === 0
    ) {
      this.showToast('Select an active session to arm a timer', true);
      return false;
    }
    try {
      const condObj: TimerConditionDto = { kind: conditionKind };
      const condJson = JSON.stringify(condObj);
      await this.bridge.createTimerJson(
        this.selectedHostId,
        this.selectedAgentId,
        Math.max(10, Math.round(durationSeconds)),
        cleanPrompt,
        condJson
      );
      await this.refreshAutomations();
      this.showToast('Conditional timer armed', false);
      this.triggerHaptic(10);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createTimerJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to arm timer', true);
      return false;
    }
  }

  public async startGoalRun(
    objective: string,
    criteriaLines: string[],
    verifyCmd: string | null,
    maxIterations: number
  ): Promise<boolean> {
    const cleanObj = objective.trim();
    if (
      this.bridge === null ||
      this.selectedHostId.length === 0 ||
      this.selectedAgentId.length === 0 ||
      cleanObj.length === 0
    ) {
      this.showToast('Select an active session to start a goal', true);
      return false;
    }
    try {
      const req: StartGoalRunRequestDto = {
        host_id: this.selectedHostId,
        agent_id: this.selectedAgentId,
        objective: cleanObj,
        criteria: criteriaLines,
        verify_command: verifyCmd !== null && verifyCmd.trim().length > 0 ? verifyCmd.trim() : null,
        max_iterations: Math.max(1, maxIterations),
        max_duration_secs: 7200
      };
      await this.bridge.startGoalJson(JSON.stringify(req));
      await this.refreshAutomations();
      this.showToast('Goal loop started', false);
      this.triggerHaptic(10);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'startGoalJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to start goal', true);
      return false;
    }
  }

  public async createMonitorWatch(
    name: string,
    kind: string,
    target: string,
    pattern: string | null
  ): Promise<boolean> {
    const cleanName = name.trim();
    const cleanTarget = target.trim();
    if (
      this.bridge === null ||
      this.selectedHostId.length === 0 ||
      this.selectedAgentId.length === 0 ||
      cleanName.length === 0 ||
      cleanTarget.length === 0
    ) {
      this.showToast('Select an active session and provide a target', true);
      return false;
    }
    try {
      const req: StartMonitorWatchRequestDto = {
        host_id: this.selectedHostId,
        agent_id: this.selectedAgentId,
        name: cleanName,
        kind: kind === 'http_health'
          ? { kind: 'http_health', url: cleanTarget }
          : (kind === 'log_file_tail'
            ? { kind: 'log_file_tail', path: cleanTarget, pattern: pattern }
            : { kind: 'command_stream', command: cleanTarget, pattern: pattern }),
        expires_in_secs: 3600
      };
      await this.bridge.createMonitorJson(JSON.stringify(req));
      await this.refreshAutomations();
      this.showToast('Monitor watch started', false);
      this.triggerHaptic(10);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createMonitorJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to start monitor', true);
      return false;
    }
  }

  public async startDagWorkflow(name: string, stepPrompt: string): Promise<boolean> {
    const cleanName = name.trim();
    const cleanPrompt = stepPrompt.trim();
    if (
      this.bridge === null ||
      this.selectedHostId.length === 0 ||
      cleanName.length === 0 ||
      cleanPrompt.length === 0
    ) {
      return false;
    }
    try {
      const step: DagStepSpecDto = {
        step_id: 'step_1',
        name: cleanName,
        prompt: cleanPrompt,
        depends_on: []
      };
      const req: StartDagWorkflowRequestDto = {
        host_id: this.selectedHostId,
        name: cleanName,
        cwd: this.activeCwd,
        steps: [step],
        default_provider: this.composerProvider,
        default_mode: this.composerMode,
        max_concurrency: 2
      };
      await this.bridge.startWorkflowJson(JSON.stringify(req));
      await this.refreshAutomations();
      this.showToast('Workflow started', false);
      this.triggerHaptic(10);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'startWorkflowJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to start workflow', true);
      return false;
    }
  }

  // 17. Transcript Export (Markdown / JSON / HTML)

  public openExportTranscriptSheet(): void {
    this.showExportTranscriptSheet = true;
    this.triggerHaptic(8);
  }

  public exportCurrentSessionTranscript(
    format: string,
    includeThinking: boolean,
    includeTools: boolean,
    includeTimestamps: boolean
  ): ExportedTranscriptDto | null {
    if (this.bridge === null || this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return null;
    }
    try {
      const opts: ExportOptionsDto = {
        format: format,
        include_thinking: includeThinking,
        include_tool_calls: includeTools,
        include_timestamps: includeTimestamps
      };
      const raw = this.bridge.exportAgentTranscriptJson(
        this.selectedHostId,
        this.selectedAgentId,
        JSON.stringify(opts)
      );
      return JSON.parse(raw) as ExportedTranscriptDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'exportAgentTranscriptJson failed', err as Object);
      this.showToast((err as Error).message || 'Export failed', true);
      return null;
    }
  }
}
