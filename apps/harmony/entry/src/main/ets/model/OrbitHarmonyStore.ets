import { common } from '@kit.AbilityKit';
import { UIContext } from '@kit.ArkUI';
import { util } from '@kit.ArkTS';
import {
  AcpAgentAvailabilityDto,
  AcpAgentSpecDto,
  AcpAgentTestResultDto,
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
  CreateWorkspaceV2RequestDto,
  DagStepSpecDto,
  DeleteWorkspaceEntryRequestDto,
  DiffFileDto,
  DiffLineDto,
  ExecuteWorktreeHandoffRequestDto,
  ExportOptionsDto,
  ExportedTranscriptDto,
  ExternalMcpServerRecordDto,
  FleetOverviewDto,
  FleetSidebarDto,
  ForkAgentRequestDto,
  ForkAgentResultDto,
  GitCommitResultDto,
  GitConflictEntryDto,
  GitCreatePrResultDto,
  GitDiscardFilesRequestDto,
  GitGeneratedCommitMessageDto,
  GitPrStatusDto,
  GitPushRequestDto,
  GitPushResultDto,
  GitRebaseStatusDto,
  GitReviewDto,
  GitStashEntryDto,
  GitStatusSnapshotDto,
  GithubDeviceFlowPollDto,
  GithubDeviceFlowStartDto,
  GoalRunRecordDto,
  GuardianPolicyConfigDto,
  HighlightWorkspaceCodeRequestDto,
  HighlightedCodeDto,
  HostCardDto,
  HostDetailDto,
  InlineMentionRegistration,
  InstalledPluginDto,
  LaunchpadProjectDto,
  ManagedSkillRecordDto,
  MentionCandidateDto,
  MentionRankRequestDto,
  ModelOptionDto,
  ModelSelectionRequestDto,
  MonitorWatchRecordDto,
  OrbitMeshPeerRow,
  OrbitProjectDirectoryGroupInput,
  OrbitScopedActionOutcome,
  OrbitWorkspaceBoardDraft,
  PendingPlanApprovalDto,
  PermissionBatchItemDto,
  PermissionRuleRecordDto,
  PersonalAgentActivityEntryDto,
  PersonalAgentBriefingDto,
  PersonalRouterConfigDto,
  PreviewWorktreeHandoffRequestDto,
  ProjectMemoryItemDto,
  PromptBadgeDto,
  ProviderModelCatalogDto,
  ProviderModelSpecDto,
  ProviderSubagentTimelineDto,
  PushPreferencesDto,
  QueuedPromptDto,
  QueuedPromptItem,
  QuotedDiffSnippet,
  ReadWorkspaceFileRequestDto,
  RegisterPushDeviceRequestDto,
  RenameWorkspaceEntryRequestDto,
  RenderDiagramRequestDto,
  RenderDiagramViewDto,
  ResolvedModelSelectionDto,
  RewindAgentResultDto,
  RouterHostCapabilitiesDto,
  RouterStatusSnapshotDto,
  SandboxPolicyDto,
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
  TerminalScreenDeltaDto,
  TerminalScreenDto,
  ThreadPrefSeedDto,
  TimelineItemDto,
  TimerConditionDto,
  ToolStepDescriptorDto,
  UpdateWorkspaceBoardCasRequestDto,
  UpsertPermissionRuleInputDto,
  UsageInsightsSnapshotDto,
  WorkflowRunRecordDto,
  WorkspaceBoardCasResultDto,
  WorkspaceBoardSnapshotDto,
  WorkspaceDto,
  WorkspaceEventRecordDto,
  WorkspaceFileViewDto,
  WorkspaceFileWriteResultDto,
  WorkspaceScriptInfoDto,
  WorktreeHandoffOutcomeDto,
  WorktreeHandoffPreviewDto,
  WriteWorkspaceFileRequestDto,
  basenameOfPath,
  clipCodeForSyncHighlightPreview,
  cwdPathsEquivalent,
  formatEffortLabel,
  formatModeLabel,
  gitStatusMayHaveDiffForSlot,
  hasBoardCasConflict,
  isSubagentSettledStatus,
  isWorkspaceFileEditable,
  nonBlankPath,
  normalizeAbsoluteCwd,
  normalizeModeSlug,
  normalizeWorkspaceRelPath,
  parseProviderSubagentTimelineJson,
  resolveAuthoritativeProjectWorkspaces,
  resolveSubagentActualHostId,
  resolveSubagentDirectParentAgentId,
  resolveSubagentDirectParentHostId
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
import {
  OrbitHostDefaultAgentPrefs,
  OrbitKeyValuePrefsBackend,
  OrbitPreferencesStore
} from './OrbitPreferences';
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
  OrbitAgentParentEdge,
  OrbitAgentParentLookup,
  OrbitOpenSidePanelTabOptions,
  OrbitSidePanelKind,
  OrbitSidePanelStore,
  OrbitSidePanelTab,
  OrbitSidePanelVisibilityListener,
  canonicalCwdToolTabId,
  canonicalFileTabId,
  canonicalManagedSubagentTabId,
  canonicalPeerTabId,
  canonicalProviderSubagentTabId,
  canonicalSideQuestionTabId,
  encodeSidePanelKeyTuple
} from './stores/OrbitSidePanelStore';
import {
  OrbitBreakpoint,
  OrbitModalRoute,
  OrbitShellStore
} from './stores/OrbitShellStore';
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
  OrbitHapticPreset,
  applyNativeAppLanguage,
  checkHarmonyNotificationEnabled,
  normalizeOrbitAppLanguage,
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

export interface OrbitScopedBoardData {
  workspace: WorkspaceDto | null;
  workspaceId: string;
  workspaceVerified: boolean;
  snapshotLoaded: boolean;
  content: string;
  revision: number;
  updatedBy: string;
  boardPath: string;
  peers: AgentCardDto[];
  events: WorkspaceEventRecordDto[];
}

export interface OrbitScopedAutomationsData {
  schedules: ScheduleSpecDto[];
  timers: ConditionalTimerRecordDto[];
  goals: GoalRunRecordDto[];
  monitors: MonitorWatchRecordDto[];
  workflows: WorkflowRunRecordDto[];
}

export interface OrbitWorkspaceFileWriteOutcome {
  ok: boolean;
  conflict: boolean;
  etag: string;
  currentContent: string | null;
  errorMessage: string;
}

export interface OrbitScopedBoardSaveOutcome {
  ok: boolean;
  conflict: boolean;
  currentRevision: number;
  currentContent: string;
  errorMessage: string;
  errorCode: string;
}

export interface OrbitWorktreeHandoffContext {
  hostId: string;
  workspaceId: string;
  sourceAgentId: string | null;
  cwd: string;
  branch: string;
  isWorktree: boolean;
  title: string;
}

export interface OrbitScopedGitCollabData {
  prStatus: GitPrStatusDto | null;
  stashes: GitStashEntryDto[];
  conflicts: GitConflictEntryDto[];
  rebaseStatus: GitRebaseStatusDto | null;
}

export interface OrbitScopedSideQuestionState {
  scopeKey: string;
  hostId: string;
  agentId: string | null;
  cwd: string;
  provider: string;
  model: string | null;
  draft: string;
  prompt: string;
  answer: string;
  error: string;
  streaming: boolean;
  requestId: string;
  requestToken: number;
  generation: number;
}

export interface OrbitSubagentBatchArchiveOutcome {
  archivedCount: number;
  queuedCount: number;
  dismissedCount: number;
  failedCount: number;
}

export interface OrbitHydratedRootResult {
  status: 'resolved' | 'failed';
  hostId: string;
  rootAgentId: string;
  unresolvedHostId: string | null;
}

export type OrbitGitDiffModeState = 'idle' | 'loading' | 'confirmed' | 'unverified';

const ORBIT_MAX_GIT_DIFF_TRACKERS: number = 16;

interface OrbitGitDiffModeTracker {
  hostId: string;
  normCwd: string;
  rawCwd: string;
  focusPath: string | null;
  staged: boolean;
  requestSeq: number;
  modeState: OrbitGitDiffModeState;
  baselineDiffRevision: number;
  confirmedDiffRevision: number;
  dirtyWhileHidden: boolean;
  lastStatus: GitStatusSnapshotDto | null;
  cachedSnapshot: GitReviewDto | null;
  settleTimerId: number;
  lastTouchedAt: number;
  nonErrorLoadingRecoveryBudget: number;
  lastObservedIdentityEpoch: number;
  lastObservedStatusRevision: number;
  lastHandledErrorRevision: number;
  pendingErrorRecovery: boolean;
  errorObservedStatusRevision: number;
}

class OrbitStoreParentLookupAdapter implements OrbitAgentParentLookup {
  private readonly owner: OrbitHarmonyStore;

  constructor(owner: OrbitHarmonyStore) {
    this.owner = owner;
  }

  hasCachedHostDetail(hostId: string): boolean {
    const clean = hostId.trim();
    return clean.length > 0 && this.owner.fleetStore.getCachedHostDetail(clean) !== null;
  }

  hasAgentCard(hostId: string, agentId: string): boolean {
    const detail = this.owner.fleetStore.getCachedHostDetail(hostId);
    if (detail === null) {
      return false;
    }
    for (let i = 0; i < detail.agents.length; i++) {
      if (detail.agents[i].agent_id === agentId) {
        return true;
      }
    }
    return false;
  }

  getParentEdge(hostId: string, agentId: string): OrbitAgentParentEdge | null {
    // Pure cache read (no sync N-API in lookup path); preserves cross-host `parent_host_id`.
    const detail = this.owner.fleetStore.getCachedHostDetail(hostId);
    if (detail === null) {
      return null;
    }
    for (let i = 0; i < detail.agents.length; i++) {
      const ag = detail.agents[i];
      if (ag.agent_id === agentId) {
        const archived = (
          ag.archived_at_rfc3339 !== undefined &&
          ag.archived_at_rfc3339 !== null &&
          ag.archived_at_rfc3339.length > 0
        ) || ag.status.state === 'closed';
        const resolvedParentHost = (
          ag.parent_host_id !== undefined &&
          ag.parent_host_id !== null &&
          ag.parent_host_id.trim().length > 0
        )
          ? ag.parent_host_id.trim()
          : hostId;
        return {
          parentHostId: resolvedParentHost,
          parentAgentId: ag.parent_agent_id,
          archived: archived
        };
      }
    }
    return null;
  }
}

class OrbitStoreSidePanelVisibilityAdapter implements OrbitSidePanelVisibilityListener {
  private readonly owner: OrbitHarmonyStore;

  constructor(owner: OrbitHarmonyStore) {
    this.owner = owner;
  }

  onSidePanelVisibilityChanged(isOpen: boolean): void {
    this.owner.shellStore.onSidePanelOpenChanged(isOpen);
  }

  onSidePanelTabClosed(_scopeKey: string, closedTab: OrbitSidePanelTab): void {
    if (closedTab.kind === 'review') {
      this.owner.releaseGitDiffTrackerIfUnused(closedTab.hostId, closedTab.cwd, closedTab.path);
    }
  }
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
    if (hostId.length === 0 || agentId.length === 0) {
      return false;
    }
    if (
      isStageScopeVisibleForEvent(
        this.owner.isInBackgroundMode(),
        this.owner.isNewSessionDraft,
        this.owner.fleetStore.selectedHostId,
        this.owner.selectedAgentId,
        hostId,
        agentId
      )
    ) {
      return true;
    }
    if (!this.owner.isInBackgroundMode() && this.owner.isSidePanelActuallyVisible()) {
      const tab = this.owner.sidePanelStore.activeTab;
      if (tab !== null && (tab.kind === 'subagent' || tab.kind === 'peer')) {
        if (tab.hostId === hostId && (tab.agentId === agentId || tab.childAgentId === agentId)) {
          return true;
        }
        // Provider-native subagent timelines refresh when their direct parent receives timeline updates.
        const hasProviderId = (
          tab.providerSubagentId !== undefined &&
          tab.providerSubagentId !== null &&
          tab.providerSubagentId.length > 0
        );
        if (hasProviderId) {
          const directParentHost = (
            tab.directParentHostId !== undefined &&
            tab.directParentHostId !== null &&
            tab.directParentHostId.length > 0
          )
            ? tab.directParentHostId
            : (tab.parentHostId ?? tab.hostId);
          const directParentAgent = (
            tab.directParentAgentId !== undefined &&
            tab.directParentAgentId !== null &&
            tab.directParentAgentId.length > 0
          )
            ? tab.directParentAgentId
            : (tab.parentAgentId ?? '');
          if (directParentHost === hostId && directParentAgent === agentId) {
            return true;
          }
        }
      }
    }
    return false;
  }

  isGitScopeVisible(hostId: string, cwd: string): boolean {
    return (
      this.owner.isReviewScopeVisible(hostId, cwd) ||
      this.owner.isFilesScopeVisible(hostId, cwd)
    );
  }

  isTerminalScopeVisible(hostId: string, slot: number): boolean {
    if (!this.owner.isSidePanelActuallyVisible() || hostId.length === 0) {
      return false;
    }
    const tab = this.owner.sidePanelStore.activeTab;
    if (tab === null || tab.kind !== 'terminal' || tab.hostId !== hostId) {
      return false;
    }
    const tabSlot = (tab.terminalSlot !== undefined && tab.terminalSlot !== null && tab.terminalSlot > 0)
      ? tab.terminalSlot
      : 1;
    return slot === tabSlot;
  }

  onFleetDirty(): void {
    this.owner.handleSchedulerFleetDirty();
  }

  onBriefingDirty(): void {
    this.owner.handleSchedulerBriefingDirty();
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
  private pendingDeepLinkNavSeq: number = 0;
  private pendingDeepLinkRequestId: string = '';
  private pendingDeepLinkSeq: number = 0;
  private pendingOpenHostId: string = '';
  private pendingOpenAgentId: string = '';
  private pendingOpenEventKind: string = '';
  private pendingOpenNavSeq: number = 0;
  private pendingOpenRequestId: string = '';
  private pendingOpenSeq: number = 0;
  private pendingAgentValidationHostId: string = '';
  private pendingAgentValidationAgentId: string = '';
  private pendingAgentValidationEventKind: string = '';
  private pendingAgentValidationRequestId: string = '';
  private pendingAgentValidationSeq: number = 0;
  private pendingAgentValidationNavSeq: number = 0;
  private pendingReplayHostId: string = '';
  private pendingReplayAgentId: string = '';
  private pendingReplayFramesJson: string = '';
  private replayFinishTimerId: number = -1;
  private gitDirty: boolean = false;
  private gitCollabRequestSeq: number = 0;
  private briefingDismissedByUserForPaKey: string = '';
  private automationsRefreshSeq: number = 0;
  private sideQuestionGen: number = 0;

  // Domain Stores (composition root)
  public readonly stageStore: OrbitStageStore = new OrbitStageStore();
  public readonly fleetStore: OrbitFleetStore = new OrbitFleetStore();
  public readonly personalAgentStore: OrbitPersonalAgentStore = new OrbitPersonalAgentStore();
  public readonly composerStore: OrbitComposerStore = new OrbitComposerStore();
  public readonly sidePanelStore: OrbitSidePanelStore = new OrbitSidePanelStore();
  public readonly shellStore: OrbitShellStore = new OrbitShellStore();
  public readonly eventScheduler: OrbitEventFrameScheduler;

  @Trace public isNewSessionDraft: boolean = false;
  @Trace public keyProtectionMode: string = 'huks_hardware';
  @Trace public showGlobalOverview: boolean = false;
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
  @Trace public panelDataRevision: number = 0;
  @Trace public gitPanelRevision: number = 0;
  @Trace public terminalPanelRevision: number = 0;
  @Trace public providerSubagentRevision: number = 0;
  @Trace public sideQuestionRevision: number = 0;
  @Trace public automationSubmitRevision: number = 0;
  @Trace public archivedSubagentKeys: string[] = [];
  private readonly archivedSubagentIndex: Set<string> = new Set<string>();
  private readonly archivedManagedMarkRevByKey: Map<string, number> = new Map<string, number>();
  private readonly sideQuestionByScope: Map<string, OrbitScopedSideQuestionState> =
    new Map<string, OrbitScopedSideQuestionState>();
  private readonly activeSideQuestionScopeByToken: Map<number, string> = new Map<number, string>();
  private sideQuestionTokenSeq: number = 0;
  private navigationSeq: number = 0;
  private inFlightAgentHydrationNavSeq: number = 0;
  private readonly createdOptimisticAgentKeys: Set<string> = new Set<string>();
  private readonly automationSubmitTokenByKey: Map<string, number> = new Map<string, number>();
  private automationSubmitSeq: number = 0;
  private readonly gitDiffModeTrackers: Map<string, OrbitGitDiffModeTracker> =
    new Map<string, OrbitGitDiffModeTracker>();
  private gitDiffTrackerClock: number = 0;
  private readonly scopedTerminalGridByKey: Map<string, string> = new Map<string, string>();
  private draftSelectionByProvider: Map<string, DraftModelSelection> = new Map<string, DraftModelSelection>();
  private hasUserCustomizedDraftModel: boolean = false;
  private hostDefaultPrefsByHost: Map<string, OrbitHostDefaultAgentPrefs> = new Map<string, OrbitHostDefaultAgentPrefs>();
  @Trace public hostDefaultPrefsRevision: number = 0;

  // Theme & Window Safe Area State
  @Trace public themeMode: string = 'system';
  @Trace public appLanguage: string = 'system';
  @Trace public activeTurnSendBehavior: string = 'queue';
  @Trace public systemIsDark: boolean = false;
  @Trace public palette: OrbitThemePalette = LIGHT_PALETTE;
  private appContext: common.ApplicationContext | null = null;
  @Trace public safeTopVp: number = 0;
  @Trace public safeBottomVp: number = 0;
  @Trace public safeLeftVp: number = 0;
  @Trace public safeRightVp: number = 0;
  @Trace public keyboardHeightVp: number = 0;

  // Composer model popover visibility (non-sheet popup anchored to OmniComposer)
  private diffInspectorOpenedByUser: boolean = false;
  @Trace public showModelPickerSheet: boolean = false;
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
  // R2-P1-1: Persist unfinished BOARD.md drafts (content + original baseRevision) in Store
  // keyed by verified `(hostId, workspaceId)` so drafts survive component unmount/remount.
  private workspaceBoardDrafts: Map<string, OrbitWorkspaceBoardDraft> = new Map<string, OrbitWorkspaceBoardDraft>();
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
  // R7-P1-1: Isolate active workspace directory per hostId so switching hosts never leaks h1's activeCwd to h2/h3.
  private hostActiveCwdMap: Map<string, string> = new Map<string, string>();
  private activeCwdHostId: string = '';
  // R3-P1-4: Monotonic epoch bumped on every host switch so late Archive/Restore/Rename
  // outcomes from h1 never surface toast/haptics/sheet-close after h1->h2 or h1->h2->h1.
  @Trace public hostSelectionEpoch: number = 0;

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
  @Trace public toastResource: Resource | null = null;
  @Trace public toastIsError: boolean = false;
  @Trace public toastRevision: number = 0;
  private toastTimerId: number = -1;

  constructor() {
    this.eventScheduler = new OrbitEventFrameScheduler(new OrbitStoreEventFlushAdapter(this));
    this.stageStore.setDeltaAppliedCallback(new OrbitStoreStageDeltaAdapter(this));
    this.composerStore.setToastSink(new OrbitStoreComposerToastAdapter(this));
    this.sidePanelStore.bindParentLookup(new OrbitStoreParentLookupAdapter(this));
    this.sidePanelStore.setVisibilityListener(new OrbitStoreSidePanelVisibilityAdapter(this));
    this.personalAgentStore.setBriefingAppliedCallback(new OrbitStoreBriefingAppliedAdapter(this));
    this.personalAgentStore.bindVisitStorage(new OrbitStorePaVisitStorageAdapter(this));
  }

  public isSidePanelActuallyVisible(): boolean {
    if (!this.sidePanelStore.isOpen) {
      return false;
    }
    const layout = this.shellStore.resolveLayout(true);
    if (!layout.sidePanelInline && this.shellStore.isModalSheetPresented) {
      return false;
    }
    return true;
  }

  public isHostVisibleInSidePanel(hostId: string): boolean {
    if (!this.isSidePanelActuallyVisible() || hostId.length === 0) {
      return false;
    }
    const tab = this.sidePanelStore.activeTab;
    return tab !== null && tab.hostId === hostId;
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
    if (this.isSidePanelActuallyVisible()) {
      const kind = this.sidePanelStore.activeTab?.kind;
      if (kind === 'briefing' || kind === 'board') {
        this.panelDataRevision += 1;
      }
    }
  }

  public handleSchedulerBriefingDirty(): void {
    this.personalAgentStore.refreshBriefing(false);
    if (this.isSidePanelActuallyVisible() && this.sidePanelStore.activeTab?.kind === 'briefing') {
      this.panelDataRevision += 1;
    }
  }

  public handleSchedulerSelectedHostDirty(hostId: string): void {
    this.reloadHostAsync(hostId);
    if (this.isHostVisibleInSidePanel(hostId)) {
      this.panelDataRevision += 1;
    }
    this.reloadWorkspaceFiles();
    this.personalAgentStore.refreshBriefing(false);
  }

  public handleSchedulerBackgroundHostDirty(hostId: string): void {
    // Strictly avoid polling full HostDetail for unselected background hosts (§3.3),
    // except when that host is actively visible inside the right side panel.
    this.refreshSidebarAsync();
    this.personalAgentStore.refreshBriefing(false);
    if (hostId.length > 0 && this.isHostVisibleInSidePanel(hostId)) {
      this.reloadHostAsync(hostId);
    }
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
    if (this.isSidePanelActuallyVisible()) {
      const tab = this.sidePanelStore.activeTab;
      if (
        tab !== null &&
        tab.kind === 'subagent' &&
        tab.providerSubagentId !== undefined &&
        tab.providerSubagentId !== null &&
        tab.providerSubagentId.length > 0
      ) {
        const directParentHost = (tab.directParentHostId !== undefined && tab.directParentHostId !== null && tab.directParentHostId.length > 0)
          ? tab.directParentHostId
          : (tab.parentHostId ?? tab.hostId);
        const directParentAgent = (tab.directParentAgentId !== undefined && tab.directParentAgentId !== null && tab.directParentAgentId.length > 0)
          ? tab.directParentAgentId
          : (tab.parentAgentId ?? '');
        if (directParentHost === hostId && directParentAgent === agentId) {
          this.providerSubagentRevision += 1;
        }
      }
    }
  }

  public isReviewScopeVisible(hostId: string, cwd: string): boolean {
    if (!this.isSidePanelActuallyVisible() || hostId.length === 0) {
      return false;
    }
    const tab = this.sidePanelStore.activeTab;
    if (tab === null || tab.kind !== 'review' || tab.hostId !== hostId) {
      return false;
    }
    const tabCwd = tab.cwd !== null ? tab.cwd : '';
    return cwd.length === 0 || tabCwd.length === 0 || cwdPathsEquivalent(cwd, tabCwd);
  }

  public isFilesScopeVisible(hostId: string, cwd: string): boolean {
    if (!this.isSidePanelActuallyVisible() || hostId.length === 0) {
      return false;
    }
    const tab = this.sidePanelStore.activeTab;
    if (tab === null || tab.kind !== 'files' || tab.hostId !== hostId) {
      return false;
    }
    const tabCwd = tab.cwd !== null ? tab.cwd : '';
    return cwd.length === 0 || tabCwd.length === 0 || cwdPathsEquivalent(cwd, tabCwd);
  }

  public handleSchedulerVisibleGitDirty(hostId: string, cwd: string): void {
    const reviewVisible = this.isReviewScopeVisible(hostId, cwd);
    const filesVisible = this.isFilesScopeVisible(hostId, cwd);
    if (!reviewVisible && !filesVisible) {
      this.handleSchedulerBackgroundGitDirty(hostId, cwd);
      return;
    }
    if (reviewVisible) {
      this.gitDirty = false;
      this.onScopedGitDirtyArrived(hostId, cwd);
      this.gitPanelRevision += 1;
      this.panelDataRevision += 1;
      return;
    }
    // R5-P1-1: Only `Files` is visible while `Review` is hidden — mark hidden Review trackers dirty
    // with ZERO synchronous `gitReviewJson` calls, and refresh only the directory listing.
    this.gitDirty = true;
    this.markHiddenGitTrackersDirty(hostId, cwd);
    this.panelDataRevision += 1;
    if (hostId === this.selectedHostId && cwd.length > 0 && cwdPathsEquivalent(cwd, this.activeCwd)) {
      this.reloadWorkspaceFiles();
    }
  }

  public handleSchedulerBackgroundGitDirty(hostId: string, cwd: string): void {
    // §3.3 (R5-P1-1): Invisible Git scope marks dirty with ZERO synchronous `gitReviewJson` calls.
    this.gitDirty = true;
    this.markHiddenGitTrackersDirty(hostId, cwd);
  }

  public handleSchedulerVisibleTerminalDirty(hostId: string, slot: number): void {
    this.terminalPanelRevision += 1;
    this.panelDataRevision += 1;
    if (hostId === this.selectedHostId && slot === 1) {
      this.reloadTerminal();
    }
  }

  // Shell & Single-Modal Route Facade Accessors (delegating cleanly to OrbitShellStore & OrbitSidePanelStore)

  public get windowWidthVp(): number {
    return this.shellStore.windowWidthVp;
  }

  public set windowWidthVp(val: number) {
    this.shellStore.windowWidthVp = val;
  }

  public get windowHeightVp(): number {
    return this.shellStore.windowHeightVp;
  }

  public set windowHeightVp(val: number) {
    this.shellStore.windowHeightVp = val;
  }

  public get isFoldableDevice(): boolean {
    return this.shellStore.isFoldableDevice;
  }

  public set isFoldableDevice(val: boolean) {
    this.shellStore.isFoldableDevice = val;
  }

  public get reducedMotion(): boolean {
    return this.shellStore.reducedMotion;
  }

  public get foldPosture(): string {
    return this.shellStore.foldPosture;
  }

  public set foldPosture(val: string) {
    this.shellStore.foldPosture = val;
  }

  public get breakpoint(): OrbitBreakpoint {
    return this.shellStore.breakpoint;
  }

  public get sidebarOpen(): boolean {
    return this.shellStore.sidebarOpen;
  }

  public set sidebarOpen(val: boolean) {
    this.shellStore.sidebarOpen = val;
  }

  public get isSidebarCollapsed(): boolean {
    return this.shellStore.isSidebarCollapsed;
  }

  public set isSidebarCollapsed(val: boolean) {
    this.shellStore.isSidebarCollapsed = val;
  }

  public get modalRoute(): OrbitModalRoute | null {
    return this.shellStore.modalRoute;
  }

  public get showHostSettingsSheet(): boolean {
    return this.shellStore.isModalKindOpen('host_settings');
  }

  public set showHostSettingsSheet(val: boolean) {
    if (val) {
      this.openHostSettings();
    } else if (this.shellStore.isModalKindOpen('host_settings')) {
      this.shellStore.closeModal();
    }
  }

  public get showGlobalSearchSheet(): boolean {
    return this.shellStore.isModalKindOpen('global_search');
  }

  public set showGlobalSearchSheet(val: boolean) {
    if (val) {
      this.openGlobalSearch();
    } else if (this.shellStore.isModalKindOpen('global_search')) {
      this.shellStore.closeModal();
    }
  }

  public get showArchivedSessionsSheet(): boolean {
    return this.shellStore.isModalKindOpen('archived');
  }

  public set showArchivedSessionsSheet(val: boolean) {
    if (val) {
      this.openArchivedSessions();
    } else if (this.shellStore.isModalKindOpen('archived')) {
      this.shellStore.closeModal();
    }
  }

  public get showExportTranscriptSheet(): boolean {
    return this.shellStore.isModalKindOpen('export_transcript');
  }

  public set showExportTranscriptSheet(val: boolean) {
    if (val) {
      this.openExportTranscriptSheet();
    } else if (this.shellStore.isModalKindOpen('export_transcript')) {
      this.shellStore.closeModal();
    }
  }

  public get showDirBrowser(): boolean {
    return this.shellStore.isModalKindOpen('directory_browser');
  }

  public set showDirBrowser(val: boolean) {
    if (val) {
      this.shellStore.openModal({
        kind: 'directory_browser',
        hostId: this.selectedHostId,
        cwd: this.activeCwd
      });
    } else if (this.shellStore.isModalKindOpen('directory_browser')) {
      this.shellStore.closeModal();
    }
  }

  public get showContextPickerSheet(): boolean {
    return this.shellStore.isModalKindOpen('work_picker');
  }

  public set showContextPickerSheet(val: boolean) {
    if (val) {
      this.shellStore.openModal({
        kind: 'work_picker',
        hostId: this.selectedHostId,
        cwd: this.activeCwd
      });
    } else if (this.shellStore.isModalKindOpen('work_picker')) {
      this.shellStore.closeModal();
    }
  }

  public get showRouterMeshSheet(): boolean {
    return (
      this.shellStore.isModalKindOpen('host_settings') &&
      this.shellStore.modalRoute?.initialPath === 'router_mesh'
    );
  }

  public set showRouterMeshSheet(val: boolean) {
    if (val) {
      this.openRouterMeshSheet();
    } else if (this.showRouterMeshSheet) {
      this.shellStore.closeModal();
    }
  }

  public get showInlineDiffInspector(): boolean {
    return (
      this.sidePanelStore.isOpen &&
      this.sidePanelStore.activeTab?.kind === 'review' &&
      this.shellStore.breakpoint !== 'sm'
    );
  }

  public set showInlineDiffInspector(val: boolean) {
    if (val) {
      this.openSidePanelToolTab('review', null, 'user');
    } else if (this.sidePanelStore.activeTab?.kind === 'review') {
      this.sidePanelStore.dismissPanel();
    }
  }

  public get showGitDiffSheet(): boolean {
    return (
      this.sidePanelStore.isOpen &&
      this.sidePanelStore.activeTab?.kind === 'review' &&
      this.shellStore.breakpoint === 'sm'
    );
  }

  public set showGitDiffSheet(val: boolean) {
    if (val) {
      this.openSidePanelToolTab('review', null, 'user');
    } else if (this.sidePanelStore.activeTab?.kind === 'review') {
      this.sidePanelStore.dismissPanel();
    }
  }

  public get showPtySheet(): boolean {
    return this.sidePanelStore.isOpen && this.sidePanelStore.activeTab?.kind === 'terminal';
  }

  public set showPtySheet(val: boolean) {
    if (val) {
      this.openSidePanelToolTab('terminal', null, 'user');
    } else if (this.sidePanelStore.activeTab?.kind === 'terminal') {
      this.sidePanelStore.dismissPanel();
    }
  }

  public get showFileExplorerSheet(): boolean {
    const kind = this.sidePanelStore.activeTab?.kind;
    return this.sidePanelStore.isOpen && (kind === 'files' || kind === 'file');
  }

  public set showFileExplorerSheet(val: boolean) {
    if (val) {
      this.openSidePanelToolTab('files', null, 'user');
    } else {
      const kind = this.sidePanelStore.activeTab?.kind;
      if (kind === 'files' || kind === 'file') {
        this.sidePanelStore.dismissPanel();
      }
    }
  }

  public get showWorkspaceBoardSheet(): boolean {
    return this.sidePanelStore.isOpen && this.sidePanelStore.activeTab?.kind === 'board';
  }

  public set showWorkspaceBoardSheet(val: boolean) {
    if (val) {
      this.openSidePanelToolTab('board', null, 'user');
    } else if (this.sidePanelStore.activeTab?.kind === 'board') {
      this.sidePanelStore.dismissPanel();
    }
  }

  public get showSideQuestionSheet(): boolean {
    return this.sidePanelStore.isOpen && this.sidePanelStore.activeTab?.kind === 'side_question';
  }

  public set showSideQuestionSheet(val: boolean) {
    if (val) {
      this.openSidePanelToolTab('side_question', null, 'user');
    } else if (this.sidePanelStore.activeTab?.kind === 'side_question') {
      this.sidePanelStore.dismissPanel();
    }
  }

  public get showAutomationsSheet(): boolean {
    return this.sidePanelStore.isOpen && this.sidePanelStore.activeTab?.kind === 'automations';
  }

  public set showAutomationsSheet(val: boolean) {
    if (val) {
      this.openSidePanelToolTab('automations', null, 'user');
    } else if (this.sidePanelStore.activeTab?.kind === 'automations') {
      this.sidePanelStore.dismissPanel();
    }
  }

  public get showSubagentSheet(): boolean {
    return this.sidePanelStore.isOpen && this.sidePanelStore.activeTab?.kind === 'subagent';
  }

  public set showSubagentSheet(val: boolean) {
    if (!val && this.sidePanelStore.activeTab?.kind === 'subagent') {
      this.sidePanelStore.dismissPanel();
    }
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

  public initializeWithTestBridge(bridge: OrbitCoreBridge): void {
    if (this.bridge !== null) {
      if (this.pendingDeepLink.length > 0) {
        const uri = this.pendingDeepLink;
        const evKind = this.pendingDeepLinkEventKind;
        const linkNavSeq = this.pendingDeepLinkNavSeq;
        this.pendingDeepLink = '';
        this.pendingDeepLinkEventKind = '';
        this.pendingDeepLinkNavSeq = 0;
        if (linkNavSeq === this.navigationSeq) {
          this.handleDeepLinkUri(uri, evKind);
        }
      }
      return;
    }
    this.initializeWithBridgeInternal(bridge, false);
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
        const linkNavSeq = this.pendingDeepLinkNavSeq;
        const reqId = this.pendingDeepLinkRequestId;
        const seq = this.pendingDeepLinkSeq;
        this.pendingDeepLink = '';
        this.pendingDeepLinkEventKind = '';
        this.pendingDeepLinkNavSeq = 0;
        this.pendingDeepLinkRequestId = '';
        this.pendingDeepLinkSeq = 0;
        if (linkNavSeq === this.navigationSeq) {
          this.handleDeepLinkUri(uri, evKind, reqId, seq);
        }
      }
      return;
    }
    try {
      const opened = OrbitCoreBridge.openWithMasterKeyOrFallback(dbPath, masterKeyB64);
      this.initializeWithBridgeInternal(opened, true);
    } catch (e) {
      this.nativeAvailable = false;
      const msg = (e as Error).message;
      this.statusNote = msg !== undefined && msg.length > 0
        ? `Preview mode (${msg})`
        : 'Preview mode (liborbit_ohos.so not loaded)';
    }
  }

  private initializeWithBridgeInternal(bridge: OrbitCoreBridge, migrateLegacyPrefs: boolean): void {
    this.bridge = bridge;
    this.nativeAvailable = true;
    this.stageStore.bindBridge(this.bridge);
    this.stageStore.setTailLimit(this.stageTailLimit());
    this.fleetStore.bindBridge(this.bridge);
    this.personalAgentStore.bindBridge(this.bridge);
    this.composerStore.bindBridge(this.bridge);
    this.composerStore.hydrate();
    this.loadDraftPrefsFromDisk();

    if (migrateLegacyPrefs && !this.prefsStore.hasMigratedThreadPrefsV1()) {
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
      const linkNavSeq = this.pendingDeepLinkNavSeq;
      const reqId = this.pendingDeepLinkRequestId;
      const seq = this.pendingDeepLinkSeq;
      this.pendingDeepLink = '';
      this.pendingDeepLinkEventKind = '';
      this.pendingDeepLinkNavSeq = 0;
      this.pendingDeepLinkRequestId = '';
      this.pendingDeepLinkSeq = 0;
      if (linkNavSeq === this.navigationSeq) {
        this.handleDeepLinkUri(uri, evKind, reqId, seq);
      }
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
  }

  // Theme & Layout Management

  public bindPreferencesBackendForTest(backend: OrbitKeyValuePrefsBackend | null): void {
    this.prefsStore.bindTestBackend(backend);
    this.loadDraftPrefsFromDisk();
  }

  public bindContext(ctx: common.Context): void {
    this.abilityContext = ctx;
    this.prefsStore.init(ctx);
    this.appLanguage = normalizeOrbitAppLanguage(this.prefsStore.getAppLanguage());
    this.activeTurnSendBehavior = this.prefsStore.getActiveTurnSendBehavior();
    this.shellStore.bindWidthStorage(this.prefsStore);
    this.sidePanelStore.bindStorage(this.prefsStore);
    this.sidePanelStore.hydrate();
    this.sidePanelStore.setDefaultWidthVp(this.shellStore.sidePanelWidthVp);
    this.hydratePushPreferencesFromStorage();
    this.loadDraftPrefsFromDisk();
  }

  public bindApplicationContext(ctx: common.ApplicationContext): void {
    this.appContext = ctx;
    orbitBindThemeApplicationContext(ctx);
  }

  public applyPersistedThemeMode(): void {
    const saved = orbitNormalizeThemeMode(this.prefsStore.getThemeMode());
    this.themeMode = saved;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
    orbitSetThemeModePreference(saved);
    orbitApplySystemBarTheme(this.palette.isDark);
    if (this.appLanguage !== 'system') {
      applyNativeAppLanguage(this.appLanguage, this.appContext);
    }
  }

  public setThemeMode(mode: string): void {
    const normalized = orbitNormalizeThemeMode(mode);
    this.themeMode = normalized;
    this.palette = resolveOrbitPalette(this.themeMode, this.systemIsDark);
    this.prefsStore.setThemeMode(normalized);
    orbitSetThemeModePreference(normalized);
    orbitApplySystemBarTheme(this.palette.isDark);
    this.triggerHaptic('selection');
  }

  public setAppLanguage(lang: string): void {
    const normalized = normalizeOrbitAppLanguage(lang);
    this.appLanguage = normalized;
    this.prefsStore.setAppLanguage(normalized);
    applyNativeAppLanguage(normalized, this.appContext);
    this.triggerHaptic('selection');
  }

  public setActiveTurnSendBehavior(behavior: string): void {
    const normalized = (behavior === 'steer' || behavior === 'interrupt') ? behavior : 'queue';
    this.activeTurnSendBehavior = normalized;
    this.prefsStore.setActiveTurnSendBehavior(normalized);
    this.triggerHaptic('selection');
  }

  public setAccountDefaultPermissionMode(mode: string): OrbitScopedActionOutcome<string> {
    const normalized = normalizeModeSlug(mode);
    this.composerMode = normalized;
    if (this.bridge === null) {
      return { ok: false, data: null, errorDetail: 'Native core unavailable' };
    }
    try {
      this.bridge.setAccountDefaultMode(normalized, Date.now());
      return { ok: true, data: normalized, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to update default permission mode'
      };
    }
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
    this.toastResource = null;
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
      this.toastResource = null;
      this.toastTimerId = -1;
    }, 2600);
  }

  public showToastResource(resource: Resource, isError: boolean = false, errorSummary: string = ''): void {
    if (this.toastTimerId !== -1) {
      clearTimeout(this.toastTimerId);
      this.toastTimerId = -1;
    }
    const cleanSummary = errorSummary.trim();
    this.toastIsError = isError;
    this.toastResource = resource;
    this.toastMessage = cleanSummary;
    this.toastRevision += 1;
    if (isError && cleanSummary.length > 0) {
      this.lastError = cleanSummary;
    } else if (!isError && cleanSummary.length > 0) {
      this.statusNote = cleanSummary;
    }
    this.toastTimerId = setTimeout(() => {
      this.toastMessage = '';
      this.toastResource = null;
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
    this.gitDiffModeTrackers.forEach((tracker: OrbitGitDiffModeTracker): void => {
      if (tracker.settleTimerId !== -1) {
        clearTimeout(tracker.settleTimerId);
        tracker.settleTimerId = -1;
      }
    });
    this.voiceController.cancel();
    this.sidePanelStore.dispose();
    this.composerStore.dispose();
    this.personalAgentStore.dispose();
    this.eventScheduler.dispose();
  }

  public updateFoldPosture(isFoldable: boolean, posture: string): void {
    this.shellStore.updateFoldPosture(isFoldable, posture, this.sidePanelStore.isOpen);
    this.sidePanelStore.setCompactViewport(this.shellStore.breakpoint === 'sm');
  }

  public updateTitleButtonAvoidWidth(rightAvoidVp: number, decorHidden: boolean): void {
    this.shellStore.updateTitleButtonAvoidWidth(rightAvoidVp, decorHidden);
  }

  public effectiveSidebarWidth(): number {
    const layout = this.shellStore.resolveLayout(this.sidePanelStore.isOpen);
    return layout.sidebarVisible ? layout.sidebarWidthVp : 0;
  }

  public canShowInlineDiffInspector(): boolean {
    return this.shellStore.breakpoint !== 'sm';
  }

  public toggleSidebar(): void {
    this.shellStore.toggleSidebar(this.sidePanelStore.isOpen, (): void => {
      this.sidePanelStore.dismissPanel();
    });
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
  }

  public setSidebarThreadFilter(filter: string): void {
    this.sidebarThreadFilter = filter;
    this.saveDraftPrefsToDisk();
    this.refreshSidebar();
    this.triggerHapticPreset('selection');
  }

  private loadDraftPrefsFromDisk(): void {
    const perHostMap = this.prefsStore.getHostDefaultAgentPrefsMap();
    const hostKeys = Object.keys(perHostMap);
    for (let i = 0; i < hostKeys.length; i++) {
      const k = hostKeys[i];
      const entry = perHostMap[k];
      if (entry !== undefined && entry !== null) {
        this.hostDefaultPrefsByHost.set(k, {
          provider: typeof entry.provider === 'string' ? entry.provider.trim() : '',
          model: typeof entry.model === 'string' ? entry.model.trim() : '',
          effort: typeof entry.effort === 'string' ? entry.effort.trim() : ''
        });
      }
    }
    const loaded = this.prefsStore.loadDraftPrefs(this.draftPrefsFilePath);
    if (loaded !== null) {
      if (loaded.sidebarProjectSort === 'recent' || loaded.sidebarProjectSort === 'alphabetical') {
        this.sidebarProjectSort = loaded.sidebarProjectSort;
      }
      if (loaded.sidebarThreadFilter === 'all' || loaded.sidebarThreadFilter === 'active_unread') {
        this.sidebarThreadFilter = loaded.sidebarThreadFilter;
      }
      if (loaded.permissionMode === 'worktree') {
        this.draftUseWorktree = true;
      }
      if (loaded.provider.trim().length > 0) {
        this.composerProvider = loaded.provider.trim();
      }
      if (loaded.model.trim().length > 0 || loaded.effort.trim().length > 0) {
        const cleanModel = loaded.model.trim();
        const cleanEffort = loaded.effort.trim();
        this.composerModel = cleanModel;
        this.composerEffort = cleanEffort;
        this.hasUserCustomizedDraftModel = true;
        this.draftSelectionByProvider.set(this.composerProvider, {
          model: cleanModel,
          effort: cleanEffort
        });
      }
    }
    this.applyPersistedHostDefaultsToDraft(this.selectedHostId);
    this.hostDefaultPrefsRevision += 1;
  }

  private applyPersistedHostDefaultsToDraft(hostId: string): void {
    if (!this.isNewSessionDraft && this.selectedAgentId.length > 0) {
      return;
    }
    const cleanHost = hostId.trim();
    const saved = (cleanHost.length > 0 ? this.hostDefaultPrefsByHost.get(cleanHost) : undefined) ??
      this.hostDefaultPrefsByHost.get('__default__');
    if (saved === undefined) {
      return;
    }
    if (saved.provider.length > 0) {
      this.composerProvider = saved.provider;
    }
    if (saved.model.length > 0 || saved.effort.length > 0) {
      this.composerModel = saved.model;
      this.composerEffort = saved.effort;
      this.hasUserCustomizedDraftModel = true;
      this.draftSelectionByProvider.set(this.composerProvider, {
        model: saved.model,
        effort: saved.effort
      });
    }
  }

  private saveDraftPrefsToDisk(overrideDefaults?: OrbitHostDefaultAgentPrefs): void {
    this.prefsStore.saveDraftPrefs({
      provider: overrideDefaults !== undefined ? overrideDefaults.provider : this.composerProvider,
      model: overrideDefaults !== undefined ? overrideDefaults.model : this.composerModel,
      effort: overrideDefaults !== undefined ? overrideDefaults.effort : this.composerEffort,
      permissionMode: this.draftUseWorktree ? 'worktree' : 'local',
      sidebarProjectSort: this.sidebarProjectSort,
      sidebarThreadFilter: this.sidebarThreadFilter
    });
  }

  public setDraftUseWorktree(useWorktree: boolean): void {
    this.draftUseWorktree = useWorktree;
    this.saveDraftPrefsToDisk();
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
  }

  public toggleProjectMoreExpanded(projectId: string): void {
    this.fleetStore.toggleProjectMoreExpanded(projectId);
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
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
    const prevWidthVp = this.windowWidthVp;
    this.shellStore.updateWindowSize(widthVp, heightVp, this.sidePanelStore.isOpen);
    this.sidePanelStore.setCompactViewport(this.shellStore.breakpoint === 'sm');
    if (widthVp > 0) {
      this.stageStore.setTailLimit(resolveStageTailLimit(widthVp));
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
    if (reconciledBriefing.showBriefingSheet) {
      if (this.shellStore.modalRoute?.kind !== 'pa_briefing') {
        this.shellStore.openModal({ kind: 'pa_briefing' });
      }
    } else if (this.shellStore.modalRoute?.kind === 'pa_briefing') {
      this.shellStore.closeModal();
    }
  }

  public updateWindowWidth(widthVp: number): void {
    this.updateWindowSize(widthVp, this.shellStore.windowHeightVp);
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
    this.shellStore.updateHorizontalInsets(this.safeLeftVp, this.safeRightVp);
  }

  public setReducedMotion(enabled: boolean): void {
    this.shellStore.setReducedMotion(enabled);
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
    this.sidePanelStore.flushNow();
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
      this.navigationSeq += 1;
      this.pendingDeepLink = clean;
      this.pendingDeepLinkEventKind = eventKind;
      this.pendingDeepLinkNavSeq = this.navigationSeq;
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
    this.navigationSeq += 1;
    const navSeq = this.navigationSeq;
    if (this.bridge === null) {
      this.pendingOpenHostId = cleanHost;
      this.pendingOpenAgentId = cleanAgent;
      this.pendingOpenEventKind = cleanEvent;
      this.pendingOpenNavSeq = navSeq;
      this.pendingOpenRequestId = cleanReqId;
      this.pendingOpenSeq = cleanSeq;
      return;
    }
    const hostExists = this.hosts.some((h: HostCardDto): boolean => h.host_id === cleanHost);
    if (!hostExists) {
      this.pendingOpenHostId = cleanHost;
      this.pendingOpenAgentId = cleanAgent;
      this.pendingOpenEventKind = cleanEvent;
      this.pendingOpenNavSeq = navSeq;
      this.pendingOpenRequestId = cleanReqId;
      this.pendingOpenSeq = cleanSeq;
      this.reloadFleet();
      return;
    }
    this.pendingOpenHostId = '';
    this.pendingOpenAgentId = '';
    this.pendingOpenEventKind = '';
    this.pendingOpenNavSeq = 0;
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
        this.pendingAgentValidationNavSeq = navSeq;
        if (this.selectedHostId !== cleanHost) {
          this.selectHostInternal(cleanHost, false, false);
        } else {
          this.reloadHostAsync(cleanHost, false);
        }
        return;
      }
    }

    this.pendingAgentValidationHostId = '';
    this.pendingAgentValidationAgentId = '';
    this.pendingAgentValidationEventKind = '';
    this.pendingAgentValidationRequestId = '';
    this.pendingAgentValidationSeq = 0;
    this.pendingAgentValidationNavSeq = 0;
    this.showGlobalOverview = false;
    this.showPersonalAgentSetup = false;
    this.showBriefingSheet = false;
    if (this.selectedHostId !== cleanHost) {
      // `openSessionFromDeepLink` already incremented `navigationSeq` above; suppress
      // automatic `agents[0]` selection when an explicit target `cleanAgent` follows.
      this.selectHostInternal(cleanHost, false, cleanAgent.length === 0);
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

  public triggerHaptic(presetOrMs: OrbitHapticPreset | number = 'selection'): void {
    triggerNativeHaptic(presetOrMs);
  }

  public triggerHapticPreset(preset: OrbitHapticPreset): void {
    triggerNativeHaptic(preset);
  }

  public copyToClipboard(content: string, label?: ResourceStr): void {
    if (content.length === 0) {
      return;
    }
    if (writeClipboardText(content)) {
      if (label === undefined || label === 'Copied to clipboard') {
        this.showToastResource($r('app.string.toast_copied_clipboard'), false);
      } else if (typeof label === 'string') {
        this.showToast(label, false);
      } else {
        this.showToastResource(label, false);
      }
      this.triggerHapticPreset('selection');
    } else {
      this.showToastResource($r('app.string.toast_copy_failed'), true);
      this.triggerHapticPreset('warning');
    }
  }

  public pasteAuthorizedPairingUri(onFilled?: (uri: string) => void): void {
    readAuthorizedClipboardText().then((raw: string) => {
      if (raw.length === 0) {
        this.showToastResource($r('app.string.toast_clipboard_empty'), true);
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
        this.showToastResource($r('app.string.toast_no_pair_uri_clipboard'), true);
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'pasteAuthorizedPairingUri failed', err);
      this.showToastResource($r('app.string.toast_clipboard_read_failed'), true);
    });
  }

  public openHostSettings(initialPath?: string, hostId?: string): void {
    const targetHost = hostId !== undefined && hostId.trim().length > 0 ? hostId.trim() : this.selectedHostId;
    const cleanPath = initialPath !== undefined ? initialPath.trim() : '';
    this.shellStore.openModal({
      kind: 'host_settings',
      hostId: targetHost,
      initialPath: cleanPath.length > 0 ? cleanPath : undefined
    });
    this.triggerHaptic('selection');
  }

  public openImageViewerModal(imageUri: string, imageLabel: string = ''): void {
    const cleanUri = imageUri.trim();
    if (cleanUri.length === 0) {
      return;
    }
    this.shellStore.openModal({
      kind: 'image_viewer',
      imageUri: cleanUri,
      imageLabel: imageLabel
    });
    this.triggerHapticPreset('selection');
  }

  public openDiagramViewerModal(
    kind: string,
    language: string,
    source: string,
    svg: string,
    title: string = ''
  ): void {
    this.shellStore.openModal({
      kind: 'diagram_viewer',
      title: title,
      diagramKind: kind,
      diagramLanguage: language,
      diagramSource: source,
      diagramSvg: svg
    });
    this.triggerHapticPreset('selection');
  }

  public scanQrAndPair(context: common.Context | undefined): void {
    scanPairingQrCode(context).then((outcome) => {
      if (outcome.cancelled) {
        return;
      }
      if (outcome.uri.length > 0) {
        this.triggerHapticPreset('warning');
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
    // Resync strictly against the currently visible side-panel tab's own target (host/agent/cwd/slot).
    if (this.isSidePanelActuallyVisible()) {
      const tab = this.sidePanelStore.activeTab;
      if (tab !== null && tab.hostId.length > 0) {
        if (tab.hostId !== this.selectedHostId) {
          this.reloadHostAsync(tab.hostId);
        }
        const tabCwd = tab.cwd !== null ? tab.cwd : '';
        if (tab.kind === 'review' && tabCwd.length > 0 && this.bridge !== null) {
          const staged = tab.gitStaged === true;
          this.requestScopedGitReviewFetch(tab.hostId, tabCwd, staged, tab.path);
          this.gitPanelRevision += 1;
        } else if (tab.kind === 'files' && tabCwd.length > 0 && this.bridge !== null) {
          try {
            this.bridge.fetchWorkspaceFiles(tab.hostId, tabCwd);
          } catch (_err) {
            // Best-effort resync trigger
          }
          this.gitPanelRevision += 1;
        } else if (tab.kind === 'terminal') {
          this.terminalPanelRevision += 1;
        } else if (tab.kind === 'board') {
          const hostDetail = this.getScopedHostDetail(tab.hostId);
          const verifiedWs = hostDetail !== null
            ? this.verifyScopedWorkspace(hostDetail, tabCwd, tab.workspaceId ?? '', tab.agentId)
            : null;
          const wsId = verifiedWs !== null ? verifiedWs.workspace_id : '';
          if (wsId.length > 0 && this.bridge !== null) {
            try {
              this.bridge.fetchWorkspaceBoard(tab.hostId, wsId);
              this.bridge.fetchWorkspaceEvents(tab.hostId, wsId, 30);
            } catch (_err) {
              // Best-effort resync trigger
            }
          }
          this.panelDataRevision += 1;
        } else if (tab.kind === 'subagent' || tab.kind === 'peer') {
          const managedId = (tab.childAgentId !== undefined && tab.childAgentId !== null && tab.childAgentId.length > 0)
            ? tab.childAgentId
            : (tab.kind === 'peer' ? (tab.agentId ?? '') : '');
          if (managedId.length > 0) {
            const subSess = this.stageStore.getOrCreateSession(tab.hostId, managedId);
            this.stageStore.reloadFull(subSess, { limit: 24 }, 'resync').catch((err: Object) => {
              orbitLogWarn('OrbitStore', 'Resync panel agent reload failed', err);
            });
          } else if (tab.providerSubagentId !== undefined && tab.providerSubagentId !== null && tab.providerSubagentId.length > 0) {
            this.providerSubagentRevision += 1;
          }
        } else {
          this.panelDataRevision += 1;
        }
      }
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
    const hasValidPendingDeepLink =
      this.pendingDeepLink.length > 0 && this.pendingDeepLinkNavSeq === this.navigationSeq;
    if (this.pendingOpenHostId.length > 0) {
      const targetHost = this.pendingOpenHostId;
      const targetAgent = this.pendingOpenAgentId;
      const targetEvent = this.pendingOpenEventKind;
      const targetNavSeq = this.pendingOpenNavSeq;
      const targetReq = this.pendingOpenRequestId;
      const targetSeq = this.pendingOpenSeq;
      this.pendingOpenHostId = '';
      this.pendingOpenAgentId = '';
      this.pendingOpenEventKind = '';
      this.pendingOpenNavSeq = 0;
      this.pendingOpenRequestId = '';
      this.pendingOpenSeq = 0;
      // Only replay a queued deep-link if NO newer user navigation superseded it (R3-P1-2).
      if (targetNavSeq === this.navigationSeq) {
        if (snap.hosts.some((h: HostCardDto): boolean => h.host_id === targetHost)) {
          this.openSessionFromDeepLink(targetHost, targetAgent, targetEvent, targetReq, targetSeq);
        } else {
          this.showToast('Host not found', true);
          if (this.selectedHostId.length === 0 && snap.hosts.length > 0 && !hasValidPendingDeepLink) {
            // Default first-host selection is initialization, NOT user navigation (never bumps navigationSeq).
            this.selectHostInternal(snap.hosts[0].host_id, false);
          } else {
            this.refreshSidebarAsync();
          }
        }
      } else {
        this.refreshSidebarAsync();
      }
    } else if (this.selectedHostId.length === 0 && snap.hosts.length > 0) {
      if (!hasValidPendingDeepLink) {
        // Default first-host selection during cold start MUST NOT bump `navigationSeq` or cancel
        // a valid queued launch URI (R3-P1-2).
        this.selectHostInternal(snap.hosts[0].host_id, false);
      } else {
        this.refreshSidebarAsync();
      }
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
    this.selectHostInternal(hostId, true, true);
  }

  private selectHostInternal(
    hostId: string,
    bumpNavigation: boolean,
    autoSelectFirstAgent: boolean = true
  ): void {
    if (hostId.length === 0) {
      return;
    }
    if (bumpNavigation) {
      this.navigationSeq += 1;
    }
    const prevWasPa = this.isCurrentSessionPersonalAgent();
    const prevHostId = this.selectedHostId;
    const prevAgentId = this.selectedAgentId;
    const changed = this.fleetStore.setSelectedHostId(hostId);
    if (changed || bumpNavigation) {
      this.hostSelectionEpoch += 1;
    }
    this.pushPrefs = clonePushPreferences(this.getPushPreferencesForHost(hostId));
    if (changed) {
      const prevHostKey = prevHostId.trim();
      if (
        prevHostKey.length > 0 &&
        this.activeCwdHostId === prevHostKey &&
        this.activeCwd.length > 0
      ) {
        this.hostActiveCwdMap.set(prevHostKey, this.activeCwd);
      }
      const nextHostKey = hostId.trim();
      this.activeCwd = this.hostActiveCwdMap.get(nextHostKey) ?? '~';
      this.activeCwdHostId = nextHostKey;
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
      this.sidePanelStore.switchScope(hostId, null);
      this.schedules = [];
      this.conditionalTimers = [];
      this.goalRuns = [];
      this.monitorWatches = [];
      this.workflowRuns = [];
      this.automationsLoading = false;
      this.resetWorkspaceBoardState();
      this.syncActiveQueuedPrompts();
    }
    this.reloadHostAsync(hostId, autoSelectFirstAgent);
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
      this.onHostDetailLoaded(hostId, detail, true);
    }
  }

  public reloadHostAsync(hostId: string, autoSelectFirstAgent: boolean = true): void {
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    this.fleetStore.reloadHostAsync(hostId).then((detail: HostDetailDto | null): void => {
      if (detail === null) {
        return;
      }
      this.applyReplacedOptimisticMappings(hostId, detail);
      if (hostId === this.selectedHostId) {
        this.onHostDetailLoaded(hostId, detail, autoSelectFirstAgent);
      }
      if (this.isHostVisibleInSidePanel(hostId)) {
        this.panelDataRevision += 1;
      }
    }).catch((err: Object) => {
      orbitLogWarn('OrbitStore', 'reloadHostAsync failed', err);
    });
  }

  private applyReplacedOptimisticMappings(hostId: string, detail: HostDetailDto): void {
    const replacedMap = detail.replaced_optimistic_agents;
    if (replacedMap === undefined || replacedMap === null) {
      return;
    }
    const optIds = Object.keys(replacedMap);
    for (let i = 0; i < optIds.length; i++) {
      const oldOptId = optIds[i];
      const newAgentId = replacedMap[oldOptId];
      if (
        typeof newAgentId !== 'string' ||
        newAgentId.trim().length === 0 ||
        oldOptId === newAgentId
      ) {
        continue;
      }
      const stillHasOpt = detail.agents.some((a: AgentCardDto) => a.agent_id === oldOptId);
      if (stillHasOpt) {
        continue;
      }
      const oldKey = this.agentQueueKey(hostId, oldOptId);
      const newKey = this.agentQueueKey(hostId, newAgentId);
      this.createdOptimisticAgentKeys.delete(this.optimisticAgentKey(hostId, oldOptId));
      const pendingQueue = this.queuedPromptsByAgent.get(oldKey);
      if (pendingQueue !== undefined && pendingQueue.length > 0) {
        const existing = this.queuedPromptsByAgent.get(newKey) ?? [];
        this.queuedPromptsByAgent.delete(oldKey);
        this.queuedPromptsByAgent.set(newKey, existing.concat(pendingQueue));
      }
      this.composerStore.remapScope(hostId, oldOptId, hostId, newAgentId);
      this.sidePanelStore.remapOptimisticScopes(hostId, oldOptId, newAgentId);
      this.remapScopedSideQuestion(
        this.sidePanelStore.scopeKeyFor(hostId, oldOptId),
        this.sidePanelStore.scopeKeyFor(hostId, newAgentId),
        newAgentId
      );
      this.stageStore.dropSession(hostId, oldOptId);

      if (hostId === this.selectedHostId && this.selectedAgentId === oldOptId) {
        this.selectedAgentId = newAgentId;
        this.isNewSessionDraft = false;
        this.stageStore.activateSession(hostId, newAgentId);
        this.syncActiveQueuedPrompts();
        if (this.bridge !== null) {
          this.bridge.openAgentTimeline(hostId, this.selectedAgentId);
        }
        this.reloadStage();
      }
    }
  }

  private onHostDetailLoaded(
    hostId: string,
    detail: HostDetailDto,
    autoSelectFirstAgent: boolean = true
  ): void {
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

      // Consume every explicit optimistic -> real replacement map entry for this host
      // (even when the user already switched away from the optimistic scope).
      this.applyReplacedOptimisticMappings(hostId, detail);

      if (hostId === this.selectedHostId && (this.activeCwd === '~' || this.activeCwd.length === 0)) {
        if (detail.workspaces.length > 0) {
          this.setActiveCwd(detail.workspaces[0].cwd);
        } else if (detail.discovered_projects.length > 0) {
          this.setActiveCwd(detail.discovered_projects[0].path);
        } else if (detail.agents.length > 0) {
          this.setActiveCwd(detail.agents[0].cwd);
        }
      }

      const hasPendingDeepLinkOrOpen =
        (this.pendingDeepLink.length > 0 && this.pendingDeepLinkNavSeq === this.navigationSeq) ||
        (this.pendingOpenHostId.length > 0 && this.pendingOpenNavSeq === this.navigationSeq) ||
        (this.pendingAgentValidationHostId.length > 0 &&
          this.pendingAgentValidationAgentId.length > 0 &&
          this.pendingAgentValidationNavSeq === this.navigationSeq);
      const hasInFlightAgentHydration =
        this.inFlightAgentHydrationNavSeq > 0 &&
        this.inFlightAgentHydrationNavSeq === this.navigationSeq;

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
        autoSelectFirstAgent &&
        !hasPendingDeepLinkOrOpen &&
        !hasInFlightAgentHydration &&
        !this.isNewSessionDraft &&
        detail.agents.length > 0 &&
        this.stageTurns.length === 0 &&
        this.composerText.length === 0
      ) {
        // Automatic default first-agent selection MUST NOT bump `navigationSeq` (R3-P1-2).
        this.selectAgentInternal(detail.agents[0].agent_id, false);
      }

      if (this.isNewSessionDraft || this.selectedAgentId.length === 0) {
        this.applyPersistedHostDefaultsToDraft(hostId);
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
          const pNavSeq = this.pendingAgentValidationNavSeq;
          this.pendingAgentValidationHostId = '';
          this.pendingAgentValidationAgentId = '';
          this.pendingAgentValidationEventKind = '';
          this.pendingAgentValidationRequestId = '';
          this.pendingAgentValidationSeq = 0;
          this.pendingAgentValidationNavSeq = 0;
          if (pNavSeq === this.navigationSeq) {
            if (valResult === 'selected') {
              this.openSessionFromDeepLink(pHost, pAgent, pEvent, pReq, pSeq);
            } else {
              this.showLocalizedToast($r('app.string.briefing_sessionNotFound'), 'Session not found', true);
            }
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

  /**
   * Returns the exact (untrimmed) active workspace directory owned strictly by `hostId`,
   * or `''` when `hostId` has no recorded non-default (`~`) active directory.
   * NEVER returns another host's `activeCwd`.
   */
  public getActiveCwdForHost(hostId: string): string {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return '';
    }
    const mapped = this.hostActiveCwdMap.get(cleanHost);
    if (mapped !== undefined) {
      const exactMapped = nonBlankPath(mapped);
      if (exactMapped.length > 0 && exactMapped !== '~') {
        return exactMapped;
      }
      return '';
    }
    if (cleanHost === this.selectedHostId.trim() && cleanHost === this.activeCwdHostId.trim()) {
      const exactLive = nonBlankPath(this.activeCwd);
      if (exactLive.length > 0 && exactLive !== '~') {
        return exactLive;
      }
    }
    return '';
  }

  public setActiveCwd(cwd: string): void {
    if (cwd.length === 0) {
      return;
    }
    const targetHost = this.selectedHostId.trim();
    const changed = this.activeCwd !== cwd || this.activeCwdHostId !== targetHost;
    this.activeCwd = cwd;
    if (targetHost.length > 0) {
      this.hostActiveCwdMap.set(targetHost, cwd);
      this.activeCwdHostId = targetHost;
    }
    if (changed && (this.isNewSessionDraft || this.selectedAgentId.length === 0) && this.selectedHostId.length > 0) {
      const nextWsId = this.resolveScopedWorkspaceId(this.selectedHostId, cwd, null);
      this.sidePanelStore.rebindDraftCwd(this.selectedHostId, cwd, nextWsId);
    }
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
        }
        this.bridge.fetchWorkspaceFiles(this.selectedHostId, cwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'setActiveCwd fetch failed', err as Object);
      }
      if (canUpdateGitDiff) {
        this.gitDirty = true;
      }
      this.reloadWorkspaceFiles();
    }
  }

  public startNewSessionDraft(cwd?: string): void {
    this.navigationSeq += 1;
    if (this.isCurrentSessionPersonalAgent() && this.selectedHostId.length > 0 && this.selectedAgentId.length > 0) {
      this.markPersonalAgentVisitedNow(this.selectedHostId, this.selectedAgentId);
      this.personalAgentStore.clearActiveVisitKey();
    }
    this.briefingViewingOpen = false;
    this.showBriefingSheet = false;
    if (this.shellStore.modalRoute?.kind === 'pa_briefing') {
      this.shellStore.closeModal();
    }
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
    if (this.selectedHostId.length > 0) {
      this.sidePanelStore.switchScope(this.selectedHostId, null);
    }
    this.activeSessionUsage = null;
    this.applyPersistedHostDefaultsToDraft(this.selectedHostId);
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
    this.triggerHapticPreset('action');
  }

  public buildManagedSubagentTab(
    childHostId: string,
    childAgentId: string,
    rootHostId: string,
    rootAgentId: string,
    directParentHostId: string,
    directParentAgentId: string,
    titleOverride: string = '',
    cwdOverride: string | null = null
  ): OrbitSidePanelTab {
    const cleanChildHost = childHostId.trim();
    const cleanChildAgent = childAgentId.trim();
    const cleanRootHost = rootHostId.trim();
    const cleanRootAgent = rootAgentId.trim();
    const cleanDirectHost = directParentHostId.trim().length > 0 ? directParentHostId.trim() : cleanRootHost;
    const cleanDirectAgent = directParentAgentId.trim().length > 0 ? directParentAgentId.trim() : cleanRootAgent;
    const childCard = this.findAgentCardOnHost(cleanChildHost, cleanChildAgent);
    const title = titleOverride.trim().length > 0
      ? titleOverride.trim()
      : (childCard !== null && childCard.title.trim().length > 0 ? childCard.title.trim() : 'Subagent');
    const cwd = (cwdOverride !== null && cwdOverride.trim().length > 0)
      ? cwdOverride.trim()
      : (childCard !== null && childCard.cwd.length > 0 ? childCard.cwd : null);
    return {
      tabId: canonicalManagedSubagentTabId(cleanChildHost, cleanChildAgent),
      kind: 'subagent',
      hostId: cleanChildHost,
      agentId: cleanChildAgent,
      cwd: cwd,
      path: null,
      parentHostId: cleanRootHost,
      title: title,
      closable: true,
      parentAgentId: cleanRootAgent,
      directParentHostId: cleanDirectHost,
      directParentAgentId: cleanDirectAgent,
      childAgentId: cleanChildAgent
    };
  }

  public async resolveRootScopeHydratingChainAsync(
    startHostId: string,
    startAgentId: string,
    requireStartCardExists: boolean = false,
    isStillCurrent?: () => boolean
  ): Promise<OrbitHydratedRootResult> {
    const cleanStartHost = startHostId.trim();
    const cleanStartAgent = startAgentId.trim();
    if (cleanStartHost.length === 0 || cleanStartAgent.length === 0) {
      return {
        status: 'failed',
        hostId: '',
        rootAgentId: '',
        unresolvedHostId: null
      };
    }
    let resolved = this.sidePanelStore.resolveRootScope(cleanStartHost, cleanStartAgent, requireStartCardExists);
    const attemptedHosts: Set<string> = new Set<string>();
    for (let hops = 0; hops < 8; hops++) {
      if (isStillCurrent !== undefined && !isStillCurrent()) {
        return {
          status: 'failed',
          hostId: '',
          rootAgentId: '',
          unresolvedHostId: null
        };
      }
      const missingHost = resolved.unresolvedHostId !== undefined && resolved.unresolvedHostId !== null
        ? resolved.unresolvedHostId.trim()
        : '';
      const isMissingCard = resolved.missingAgentCard === true;
      if (missingHost.length === 0 && !isMissingCard) {
        break;
      }
      const targetHostToFetch = missingHost.length > 0 ? missingHost : resolved.hostId.trim();
      if (targetHostToFetch.length === 0 || attemptedHosts.has(targetHostToFetch)) {
        return {
          status: 'failed',
          hostId: '',
          rootAgentId: '',
          unresolvedHostId: targetHostToFetch.length > 0 ? targetHostToFetch : null
        };
      }
      attemptedHosts.add(targetHostToFetch);
      // Follow superseding same-host detail refreshes (e.g. concurrent `host_changed(TARGET)`)
      // instead of treating a superseded `null` as a terminal host failure (R3-P1-2).
      const loaded = await this.fleetStore.awaitLatestHostDetailAsync(
        targetHostToFetch,
        isMissingCard
      );
      if (isStillCurrent !== undefined && !isStillCurrent()) {
        return {
          status: 'failed',
          hostId: '',
          rootAgentId: '',
          unresolvedHostId: null
        };
      }
      if (loaded === null) {
        return {
          status: 'failed',
          hostId: '',
          rootAgentId: '',
          unresolvedHostId: targetHostToFetch
        };
      }
      resolved = this.sidePanelStore.resolveRootScope(cleanStartHost, cleanStartAgent, requireStartCardExists);
    }
    const remainingUnresolved = resolved.unresolvedHostId !== undefined && resolved.unresolvedHostId !== null
      ? resolved.unresolvedHostId.trim()
      : '';
    if (remainingUnresolved.length > 0 || resolved.missingAgentCard === true) {
      return {
        status: 'failed',
        hostId: '',
        rootAgentId: '',
        unresolvedHostId: remainingUnresolved.length > 0 ? remainingUnresolved : resolved.hostId.trim()
      };
    }
    const finalHost = resolved.hostId.trim();
    const finalAgent = resolved.rootAgentId !== null ? resolved.rootAgentId.trim() : '';
    if (finalHost.length === 0 || finalAgent.length === 0) {
      return {
        status: 'failed',
        hostId: '',
        rootAgentId: '',
        unresolvedHostId: null
      };
    }
    return {
      status: 'resolved',
      hostId: finalHost,
      rootAgentId: finalAgent,
      unresolvedHostId: null
    };
  }

  private commitManagedChildSelection(
    childHostId: string,
    childAgentId: string,
    rootHostId: string,
    rootAgentId: string,
    directParentHostId: string,
    directParentAgentId: string,
    switchPrimaryStageToRoot: boolean,
    titleOverride: string = '',
    cwdOverride: string | null = null
  ): void {
    if (switchPrimaryStageToRoot) {
      if (this.selectedHostId !== rootHostId) {
        this.selectHostInternal(rootHostId, false, false);
      }
      if (this.selectedAgentId !== rootAgentId || this.isNewSessionDraft || this.showGlobalOverview) {
        this.selectRootAgentDirectly(rootHostId, rootAgentId);
      }
    }
    const subTab = this.buildManagedSubagentTab(
      childHostId,
      childAgentId,
      rootHostId,
      rootAgentId,
      directParentHostId,
      directParentAgentId,
      titleOverride,
      cwdOverride
    );
    this.sidePanelStore.openSubagentTab(
      rootHostId,
      rootAgentId,
      subTab,
      { source: 'user', activate: true }
    );
    if (this.bridge !== null) {
      try {
        this.bridge.openAgentTimeline(childHostId, childAgentId);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'openAgentTimeline for subagent failed', err as Object);
      }
    }
    this.triggerHapticPreset('selection');
  }

  private optimisticAgentKey(hostId: string, agentId: string): string {
    return encodeSidePanelKeyTuple(['opt_agent', hostId.trim(), agentId.trim()]);
  }

  private clearOptimisticAgentKeysForHost(hostId: string): void {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0 || this.createdOptimisticAgentKeys.size === 0) {
      return;
    }
    const prefix = encodeSidePanelKeyTuple(['opt_agent', cleanHost, '']).slice(0, -3);
    const toRemove: string[] = [];
    this.createdOptimisticAgentKeys.forEach((k: string): void => {
      if (k.startsWith(prefix)) {
        toRemove.push(k);
      }
    });
    for (let i = 0; i < toRemove.length; i++) {
      this.createdOptimisticAgentKeys.delete(toRemove[i]);
    }
  }

  public selectAgent(agentId: string): void {
    this.selectAgentInternal(agentId, true);
  }

  private selectAgentInternal(agentId: string, bumpNavigation: boolean): void {
    if (agentId.length === 0) {
      return;
    }
    if (bumpNavigation) {
      this.navigationSeq += 1;
    }
    const navSeq = this.navigationSeq;

    // Subagent Force-Routing & Start-Card Invariant (P0-4 + R3-P1-2):
    // Require authoritative start-card verification whenever selecting an agent unless this store
    // holds explicit local creation credentials for that optimistic session (`createdOptimisticAgentKeys`).
    // If cached host detail omits the card, refresh once asynchronously under `navigationSeq` before
    // deciding whether it is a verified root, a managed child, or missing (fail closed).
    if (this.selectedHostId.length > 0) {
      const childHostId = this.selectedHostId;
      const isLocalOptimistic = this.createdOptimisticAgentKeys.has(
        this.optimisticAgentKey(childHostId, agentId)
      );
      const requireStartCard = !isLocalOptimistic;
      const childCard = this.findAgentCardOnHost(childHostId, agentId);
      const hasDirectParent = childCard !== null &&
        childCard.parent_agent_id !== null &&
        childCard.parent_agent_id.trim().length > 0;
      const directParentHostId = hasDirectParent && childCard !== null
        ? ((childCard.parent_host_id !== undefined && childCard.parent_host_id !== null && childCard.parent_host_id.trim().length > 0)
          ? childCard.parent_host_id.trim()
          : childHostId)
        : childHostId;
      const directParentAgentId = hasDirectParent && childCard !== null && childCard.parent_agent_id !== null
        ? childCard.parent_agent_id.trim()
        : '';

      const root = this.sidePanelStore.resolveRootScope(childHostId, agentId, requireStartCard);
      const hasUnresolvedHost = root.unresolvedHostId !== undefined &&
        root.unresolvedHostId !== null &&
        root.unresolvedHostId.trim().length > 0;
      const needsHydration = hasUnresolvedHost || root.missingAgentCard === true;

      if (needsHydration) {
        this.inFlightAgentHydrationNavSeq = navSeq;
        this.resolveRootScopeHydratingChainAsync(
          childHostId,
          agentId,
          requireStartCard,
          (): boolean => navSeq === this.navigationSeq
        ).then((hydrated: OrbitHydratedRootResult): void => {
          if (navSeq !== this.navigationSeq) {
            if (this.inFlightAgentHydrationNavSeq === navSeq) {
              this.inFlightAgentHydrationNavSeq = 0;
            }
            return;
          }
          if (hydrated.status !== 'resolved') {
            if (this.inFlightAgentHydrationNavSeq === navSeq) {
              this.inFlightAgentHydrationNavSeq = 0;
            }
            this.showToast('Unable to load parent session host — tap to retry', true);
            return;
          }
          const finalRootHost = hydrated.hostId;
          const finalRootAgent = hydrated.rootAgentId;
          if (finalRootHost === childHostId && finalRootAgent === agentId) {
            this.selectRootAgentDirectly(childHostId, agentId);
            if (this.inFlightAgentHydrationNavSeq === navSeq) {
              this.inFlightAgentHydrationNavSeq = 0;
            }
            return;
          }
          const hydratedCard = this.findAgentCardOnHost(childHostId, agentId);
          const hydratedDirectHost = (
            hydratedCard !== null &&
            hydratedCard.parent_host_id !== undefined &&
            hydratedCard.parent_host_id !== null &&
            hydratedCard.parent_host_id.trim().length > 0
          )
            ? hydratedCard.parent_host_id.trim()
            : directParentHostId;
          const hydratedDirectAgent = (
            hydratedCard !== null &&
            hydratedCard.parent_agent_id !== null &&
            hydratedCard.parent_agent_id.trim().length > 0
          )
            ? hydratedCard.parent_agent_id.trim()
            : (directParentAgentId.length > 0 ? directParentAgentId : finalRootAgent);
          const hydratedTitle = hydratedCard !== null ? hydratedCard.title : '';
          const hydratedCwd = hydratedCard !== null && hydratedCard.cwd.length > 0
            ? hydratedCard.cwd
            : this.activeCwd;
          this.commitManagedChildSelection(
            childHostId,
            agentId,
            finalRootHost,
            finalRootAgent,
            hydratedDirectHost,
            hydratedDirectAgent,
            true,
            hydratedTitle,
            hydratedCwd
          );
          if (this.inFlightAgentHydrationNavSeq === navSeq) {
            this.inFlightAgentHydrationNavSeq = 0;
          }
        }).catch((err: Object) => {
          if (this.inFlightAgentHydrationNavSeq === navSeq) {
            this.inFlightAgentHydrationNavSeq = 0;
          }
          orbitLogWarn('OrbitStore', 'Async parent chain resolution failed', err);
        });
        return;
      }

      if (
        root.rootAgentId !== null &&
        root.rootAgentId.length > 0 &&
        (root.rootAgentId !== agentId || root.hostId !== childHostId)
      ) {
        this.commitManagedChildSelection(
          childHostId,
          agentId,
          root.hostId,
          root.rootAgentId,
          directParentHostId,
          directParentAgentId.length > 0 ? directParentAgentId : root.rootAgentId,
          true,
          childCard !== null ? childCard.title : '',
          childCard !== null && childCard.cwd.length > 0 ? childCard.cwd : this.activeCwd
        );
        return;
      }
    }

    this.selectRootAgentDirectly(this.selectedHostId, agentId);
  }

  private selectRootAgentDirectly(hostId: string, agentId: string): void {
    const prevWasPa = this.isCurrentSessionPersonalAgent();
    const prevHostId = hostId;
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
      this.pendingFocusDecisionHostId !== hostId ||
      this.pendingFocusDecisionAgentId !== agentId
    ) {
      this.clearPendingDecisionFocus();
    }
    this.showGlobalOverview = false;
    this.showPersonalAgentSetup = false;
    this.isNewSessionDraft = false;
    this.selectedAgentId = agentId;
    this.activeSessionUsage = null;
    if (hostId.length > 0) {
      this.fleetStore.clearConversationDirty(hostId, agentId);
      this.composerStore.setActiveScope(hostId, agentId);
      this.stageStore.activateSession(hostId, agentId);
      this.sidePanelStore.switchScope(hostId, agentId);
    }
    // Unified PA visit & Briefing presentation lifecycle across ALL session navigation entries
    // (including cold-start deep links, sidebar taps, and cross-host switches).
    if (this.isCurrentSessionPersonalAgent()) {
      const paKey = `${hostId}::${agentId}`;
      const userDismissedCurrentPa = this.briefingDismissedByUserForPaKey === paKey;
      const userOpenedDiff = this.showInlineDiffInspector &&
        (this.diffInspectorOpenedByUser || this.isDiffTargetPinned);
      if (
        !prevWasPa ||
        !sameSession ||
        !this.personalAgentStore.activeVisitKeyMatches(hostId, agentId)
      ) {
        this.personalAgentStore.enterPersonalAgentVisit(hostId, agentId);
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
      if (this.shellStore.modalRoute?.kind === 'pa_briefing') {
        this.shellStore.closeModal();
      }
      this.briefingDismissedByUserForPaKey = '';
    }
    if (this.bridge !== null && hostId.length > 0) {
      const agents = this.currentHostDetail?.agents ?? [];
      let matchedSeq = 0;
      for (let i = 0; i < agents.length; i++) {
        if (agents[i].agent_id === agentId) {
          matchedSeq = agents[i].latest_seq;
          break;
        }
      }
      this.bridge.markAgentSeen(hostId, agentId, matchedSeq);
    }
    this.syncActiveQueuedPrompts();
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    if (this.bridge !== null && hostId.length > 0) {
      this.bridge.openAgentTimeline(hostId, agentId);
      this.bridge.listProviderSubagents(hostId, agentId);
      this.reloadStage();
      this.refreshSessionUsage();
      if (this.currentHostDetail !== null) {
        this.syncActiveWorkspaceBoardFromDetail(this.currentHostDetail);
      } else {
        this.resetWorkspaceBoardState();
      }
      this.refreshSidebarAsync();
    }
    this.triggerHapticPreset('selection');
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
      if (this.activeCwd !== delta.agent.cwd || this.activeCwdHostId !== this.selectedHostId.trim()) {
        this.activeCwd = delta.agent.cwd;
        const activeHost = this.selectedHostId.trim();
        if (activeHost.length > 0) {
          this.hostActiveCwdMap.set(activeHost, delta.agent.cwd);
          this.activeCwdHostId = activeHost;
        }
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('success');
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
      this.triggerHaptic('action');
    } catch (e) {
      this.showToast((e as Error).message, true);
    }
  }

  public refreshQuotasForHostSilent(hostId: string): void {
    const clean = hostId.trim();
    if (this.bridge === null || clean.length === 0) {
      return;
    }
    try {
      this.bridge.refreshQuotas(clean);
      this.reloadHost(clean);
    } catch (_e) {
      // Best-effort silent refresh for settings scope.
    }
  }

  public refreshActiveQuotas(): void {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    this.bridge.refreshQuotas(this.selectedHostId);
    this.reloadHost(this.selectedHostId);
    this.showToast('Refreshing account quotas…', false);
    this.triggerHaptic('selection');
  }

  public refreshModelsAndQuotas(forceModels: boolean = true): void {
    if (this.bridge === null || this.selectedHostId.length === 0) {
      return;
    }
    this.bridge.refreshQuotas(this.selectedHostId);
    this.bridge.refreshProviderModels(this.selectedHostId, forceModels);
    this.reloadHost(this.selectedHostId);
    this.showToast('Refreshing models…', false);
    this.triggerHaptic('selection');
  }

  public removeHost(hostId: string): void {
    if (this.bridge === null || hostId.length === 0) {
      return;
    }
    try {
      this.bridge.removeHost(hostId);
      this.revokeSideQuestionsForHost(hostId);
      this.releaseGitDiffTrackersForHost(hostId);
      this.clearOptimisticAgentKeysForHost(hostId);
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
      this.triggerHapticPreset('success');
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
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
  }

  public attachWorkingTreeDiffBadge(): void {
    this.addUniqueComposerBadge('diff', '@diff');
    this.triggerHapticPreset('selection');
  }

  public attachStagedDiffBadge(): void {
    this.addUniqueComposerBadge('staged', '@staged');
    this.triggerHapticPreset('selection');
  }

  public attachTerminalOutputBadge(): void {
    if (this.terminalRows.length === 0 && this.bridge !== null && this.selectedHostId.length > 0) {
      this.reloadTerminal();
    }
    this.addUniqueComposerBadge('terminal', '@terminal');
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
  }

  public setSlashDirective(slash: string): void {
    this.activeSlashDirective = slash.trim();
    this.triggerHapticPreset('selection');
  }

  public clearSlashDirective(): void {
    if (this.activeSlashDirective.length > 0) {
      this.activeSlashDirective = '';
      this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('action');
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
    this.triggerHapticPreset('success');
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
    this.triggerHapticPreset('success');
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
    this.triggerHapticPreset('selection');
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
    return this.fallbackCatalogForProvider(provider);
  }

  private fallbackCatalogForProvider(provider: string): ProviderModelCatalogDto | null {
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

  /**
   * Returns the ProviderModelCatalog strictly scoped to `hostId`.
   * When `hostId` differs from `selectedHostId`, NEVER reads `this.currentHostDetail`.
   */
  public catalogForHostProvider(hostId: string, provider: string): ProviderModelCatalogDto | null {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return this.catalogForProvider(provider);
    }
    const detail = this.hostDetailFor(cleanHost);
    if (detail !== null) {
      for (let i = 0; i < detail.provider_catalogs.length; i++) {
        if (detail.provider_catalogs[i].provider === provider) {
          return detail.provider_catalogs[i];
        }
      }
      return null;
    }
    if (cleanHost === this.selectedHostId.trim()) {
      return this.catalogForProvider(provider);
    }
    return this.fallbackCatalogForProvider(provider);
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
    desiredEffort: string,
    hostId?: string
  ): DraftModelSelection {
    const cleanHost = hostId !== undefined ? hostId.trim() : '';
    const useHostOverride = cleanHost.length > 0 && cleanHost !== this.selectedHostId.trim();
    if (!useHostOverride) {
      const fromCore = this.resolveModelSelectionFromCore(provider, desiredModel, desiredEffort);
      if (fromCore !== null) {
        return {
          model: fromCore.selected_model_id ?? '',
          effort: fromCore.selected_effort ?? ''
        };
      }
    }

    const models = cleanHost.length > 0
      ? this.modelsForHostProvider(cleanHost, provider)
      : this.modelsForProvider(provider);
    const catalog = cleanHost.length > 0
      ? this.catalogForHostProvider(cleanHost, provider)
      : this.catalogForProvider(provider);

    let resolvedModel = '';
    if (desiredModel.trim().length > 0) {
      const cleanDesired = desiredModel.trim();
      const existsInCatalog = models.length === 0 || models.some((m: ProviderModelSpecDto) => m.id === cleanDesired);
      if (existsInCatalog) {
        resolvedModel = cleanDesired;
      }
    }
    if (
      resolvedModel.length === 0 &&
      catalog !== null &&
      catalog.configured_default_model !== null &&
      catalog.configured_default_model.trim().length > 0
    ) {
      const configuredDefault = catalog.configured_default_model.trim();
      const validOnTarget = models.length === 0 || models.some((m: ProviderModelSpecDto) => m.id === configuredDefault);
      if (validOnTarget) {
        resolvedModel = configuredDefault;
      }
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

    const cleanDesiredEffort = desiredEffort.trim();
    const options = spec !== null ? spec.thinking_options : [];
    if (options.length === 0) {
      if (cleanDesiredEffort.length > 0) {
        return { model: resolvedModel, effort: cleanDesiredEffort };
      }
      const fallbackEff = (spec !== null && spec.default_effort !== undefined && spec.default_effort !== null)
        ? spec.default_effort
        : '';
      return { model: resolvedModel, effort: fallbackEff };
    }
    if (cleanDesiredEffort.length > 0) {
      for (let o = 0; o < options.length; o++) {
        if (options[o].effort === cleanDesiredEffort) {
          return { model: resolvedModel, effort: cleanDesiredEffort };
        }
      }
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

  public getHostDefaultProvider(hostId: string): string {
    const cleanHost = hostId.trim();
    const saved = cleanHost.length > 0
      ? this.hostDefaultPrefsByHost.get(cleanHost)
      : this.hostDefaultPrefsByHost.get('__default__');
    if (saved !== undefined && saved.provider.length > 0) {
      return saved.provider;
    }
    if (cleanHost.length === 0 || cleanHost === this.selectedHostId.trim()) {
      return this.composerProvider;
    }
    const detail = this.hostDetailFor(cleanHost);
    if (detail !== null && detail.provider_catalogs.length > 0) {
      return detail.provider_catalogs[0].provider;
    }
    return this.composerProvider;
  }

  public getHostDefaultModel(hostId: string): string {
    const cleanHost = hostId.trim();
    const saved = cleanHost.length > 0
      ? this.hostDefaultPrefsByHost.get(cleanHost)
      : this.hostDefaultPrefsByHost.get('__default__');
    if (saved !== undefined && saved.model.length > 0) {
      return saved.model;
    }
    if (cleanHost.length === 0 || cleanHost === this.selectedHostId.trim()) {
      return this.composerModel;
    }
    const prov = this.getHostDefaultProvider(cleanHost);
    const resolved = this.resolveDefaultModelAndEffort(prov, '', '', cleanHost);
    return resolved.model;
  }

  public getHostDefaultEffort(hostId: string): string {
    const cleanHost = hostId.trim();
    const saved = cleanHost.length > 0
      ? this.hostDefaultPrefsByHost.get(cleanHost)
      : this.hostDefaultPrefsByHost.get('__default__');
    if (saved !== undefined && saved.effort.length > 0) {
      return saved.effort;
    }
    if (cleanHost.length === 0 || cleanHost === this.selectedHostId.trim()) {
      return this.composerEffort;
    }
    const prov = this.getHostDefaultProvider(cleanHost);
    const model = this.getHostDefaultModel(cleanHost);
    const resolved = this.resolveDefaultModelAndEffort(prov, model, '', cleanHost);
    return resolved.effort;
  }

  private persistHostDefaultAgentPrefs(
    hostId: string,
    provider: string,
    model: string,
    effort: string
  ): void {
    const cleanHost = hostId.trim();
    const scopeKey = cleanHost.length > 0 ? cleanHost : '__default__';
    const nextPrefs: OrbitHostDefaultAgentPrefs = {
      provider: provider.trim(),
      model: model.trim(),
      effort: effort.trim()
    };
    this.hostDefaultPrefsByHost.set(scopeKey, nextPrefs);
    this.prefsStore.saveHostDefaultAgentPrefs(scopeKey, nextPrefs);
    this.hostDefaultPrefsRevision += 1;

    const matchesSelectedHost = cleanHost.length === 0 || cleanHost === this.selectedHostId.trim();
    if (matchesSelectedHost) {
      this.saveDraftPrefsToDisk(nextPrefs);
      if (this.isNewSessionDraft || this.selectedAgentId.length === 0) {
        this.composerProvider = nextPrefs.provider;
        this.composerModel = nextPrefs.model;
        this.composerEffort = nextPrefs.effort;
        this.hasUserCustomizedDraftModel = true;
        this.draftSelectionByProvider.set(nextPrefs.provider, {
          model: nextPrefs.model,
          effort: nextPrefs.effort
        });
      }
    }
  }

  /**
   * Settings-only host-scoped default provider selection.
   * Persists to Preferences for `hostId` without EVER invoking `switchAgentProvider` on a live agent.
   */
  public setHostDefaultProvider(hostId: string, provider: string): void {
    const cleanProvider = provider.trim();
    if (cleanProvider.length === 0) {
      return;
    }
    const cleanHost = hostId.trim();
    const resolved = this.resolveDefaultModelAndEffort(cleanProvider, '', '', cleanHost);
    const hostModels = this.modelsForHostProvider(cleanHost, cleanProvider);
    let safeModel = resolved.model;
    if (hostModels.length > 0) {
      const inTarget = safeModel.length > 0 && hostModels.some((m: ProviderModelSpecDto): boolean => m.id === safeModel);
      if (!inTarget) {
        safeModel = hostModels[0].id;
      }
    }
    const finalSelection = safeModel === resolved.model
      ? resolved
      : this.resolveDefaultModelAndEffort(cleanProvider, safeModel, resolved.effort, cleanHost);
    this.persistHostDefaultAgentPrefs(cleanHost, cleanProvider, finalSelection.model, finalSelection.effort);
    this.triggerHapticPreset('selection');
  }

  /**
   * Settings-only host-scoped default model selection.
   * Persists to Preferences for `hostId` without mutating any active session on another host.
   */
  public setHostDefaultModel(hostId: string, modelId: string): void {
    const cleanHost = hostId.trim();
    const curProvider = this.getHostDefaultProvider(cleanHost);
    const curEffort = this.getHostDefaultEffort(cleanHost);
    const resolved = this.resolveDefaultModelAndEffort(curProvider, modelId, curEffort, cleanHost);
    this.persistHostDefaultAgentPrefs(cleanHost, curProvider, resolved.model, resolved.effort);
    this.triggerHapticPreset('selection');
  }

  /**
   * Settings-only host-scoped default effort selection.
   * Persists to Preferences for `hostId` without mutating any active session on another host.
   */
  public setHostDefaultEffort(hostId: string, effort: string): void {
    const cleanHost = hostId.trim();
    const curProvider = this.getHostDefaultProvider(cleanHost);
    const curModel = this.getHostDefaultModel(cleanHost);
    const resolved = this.resolveDefaultModelAndEffort(curProvider, curModel, effort, cleanHost);
    this.persistHostDefaultAgentPrefs(cleanHost, curProvider, resolved.model, resolved.effort);
    this.triggerHapticPreset('selection');
  }

  public setComposerProvider(provider: string): void {
    this.switchDraftProvider(provider);
  }

  public setComposerModel(modelId: string, hostId?: string): void {
    if (hostId !== undefined && hostId.trim().length > 0 && hostId.trim() !== this.selectedHostId.trim()) {
      this.setHostDefaultModel(hostId, modelId);
      return;
    }
    this.applyProviderModelEffort(this.composerProvider, modelId, this.composerEffort, hostId);
  }

  public setComposerEffort(effort: string, hostId?: string): void {
    if (hostId !== undefined && hostId.trim().length > 0 && hostId.trim() !== this.selectedHostId.trim()) {
      this.setHostDefaultEffort(hostId, effort);
      return;
    }
    this.applyProviderModelEffort(this.composerProvider, this.composerModel, effort, hostId);
  }

  public modelsForHostProvider(hostId: string, provider: string): ProviderModelSpecDto[] {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return this.modelsForProvider(provider);
    }
    const detail = this.hostDetailFor(cleanHost);
    if (detail !== null) {
      for (let i = 0; i < detail.provider_catalogs.length; i++) {
        if (detail.provider_catalogs[i].provider === provider) {
          return detail.provider_catalogs[i].models;
        }
      }
      return [];
    }
    if (cleanHost === this.selectedHostId.trim()) {
      return this.modelsForProvider(provider);
    }
    const fallback = this.fallbackCatalogForProvider(provider);
    return fallback !== null ? fallback.models : [];
  }

  public refreshModelsForHostSilent(hostId: string, forceRefresh: boolean = true): void {
    const clean = hostId.trim();
    if (this.bridge === null || clean.length === 0) {
      return;
    }
    try {
      this.bridge.refreshProviderModels(clean, forceRefresh);
      this.reloadHost(clean);
    } catch (_e) {
      // Best-effort silent refresh scoped to settings target host.
    }
  }

  public refreshActiveModels(forceRefresh: boolean = true): void {
    this.refreshModelsAndQuotas(forceRefresh);
  }

  public hostDetailFor(hostId: string): HostDetailDto | null {
    const clean = hostId.trim();
    if (clean.length === 0) {
      return null;
    }
    if (clean === this.selectedHostId && this.currentHostDetail !== null) {
      return this.currentHostDetail;
    }
    return this.fleetStore.getCachedHostDetail(clean);
  }

  public selectEffort(effort: string): void {
    this.applyProviderModelEffort(this.composerProvider, this.composerModel, effort);
  }

  public applyModelSelection(modelId: string, effort: string): void {
    this.applyProviderModelEffort(this.composerProvider, modelId, effort);
  }

  public applyProviderModelEffort(
    provider: string,
    model: string,
    effort: string,
    hostId?: string
  ): void {
    const prevProvider = this.composerProvider;
    const cleanHost = hostId !== undefined ? hostId.trim() : '';
    const resolved = this.resolveDefaultModelAndEffort(provider, model, effort, hostId);
    this.composerProvider = provider;
    this.composerModel = resolved.model;
    this.composerEffort = resolved.effort;
    if (this.isNewSessionDraft || this.selectedAgentId.length === 0) {
      this.hasUserCustomizedDraftModel = true;
      this.draftSelectionByProvider.set(provider, resolved);
      const scopeHost = cleanHost.length > 0 ? cleanHost : this.selectedHostId.trim();
      const prefsKey = scopeHost.length > 0 ? scopeHost : '__default__';
      const nextPrefs: OrbitHostDefaultAgentPrefs = {
        provider: provider.trim(),
        model: resolved.model.trim(),
        effort: resolved.effort.trim()
      };
      this.hostDefaultPrefsByHost.set(prefsKey, nextPrefs);
      this.prefsStore.saveHostDefaultAgentPrefs(prefsKey, nextPrefs);
      this.saveDraftPrefsToDisk(nextPrefs);
      this.hostDefaultPrefsRevision += 1;
    }
    const matchesSelectedHost = cleanHost.length === 0 || cleanHost === this.selectedHostId.trim();
    if (
      matchesSelectedHost &&
      this.bridge !== null &&
      this.selectedHostId.length > 0 &&
      this.selectedAgentId.length > 0 &&
      !this.isNewSessionDraft
    ) {
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
    this.triggerHapticPreset('action');
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
      } else if (snip.kind === 'file_quote') {
        // Explicit cross-project/cross-host file excerpt carrying its own source path & read body (P1-6).
        quotes.push({
          path: snip.path.length > 0 ? snip.path : snip.lineLabel,
          line_number: null,
          code: snip.content
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
      this.triggerHapticPreset('success');
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
    this.triggerHapticPreset('success');
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
    this.triggerHapticPreset('selection');
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
    this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('warning');
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
      this.triggerHapticPreset('success');
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
      this.submitBtwFromComposer(q);
      return;
    }
    if (overridePrompt === undefined && composed.is_compact) {
      this.composerText = '';
      this.activeSlashDirective = '';
      this.submitSlashCompact(composed.display_prompt);
      return;
    }

    if (this.isActiveAgentRunning()) {
      const hasPendingDecision =
        this.pendingPermissions.length > 0 || this.pendingQuestions.length > 0;
      const effectiveBehavior = hasPendingDecision ? 'queue' : this.activeTurnSendBehavior;
      if (effectiveBehavior === 'queue') {
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
          this.triggerHaptic('action');
          return;
        }
      } else if (
        effectiveBehavior === 'interrupt' &&
        this.selectedAgentId.length > 0 &&
        !this.isNewSessionDraft
      ) {
        try {
          this.bridge.interruptAgent(this.selectedHostId, this.selectedAgentId);
        } catch (err) {
          orbitLogWarn('OrbitStore', 'interrupt before send failed', err as Object);
        }
      }
      // For 'steer' or post-'interrupt', fall through to sendPromptWithMeta below.
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
        if (optId.trim().length > 0) {
          this.createdOptimisticAgentKeys.add(this.optimisticAgentKey(this.selectedHostId, optId));
        }
        const promotedWsId = this.resolveScopedWorkspaceId(this.selectedHostId, this.activeCwd, optId);
        this.composerStore.clearActiveDraft(true);
        const draftScopeKey = this.sidePanelStore.scopeKeyFor(this.selectedHostId, null);
        const targetScopeKey = this.sidePanelStore.scopeKeyFor(this.selectedHostId, optId);
        this.sidePanelStore.promoteDraftScope(this.selectedHostId, optId, this.activeCwd, promotedWsId);
        this.remapScopedSideQuestion(draftScopeKey, targetScopeKey, optId, this.activeCwd);
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
      this.triggerHaptic('action');
    } catch (e) {
      this.showToast((e as Error).message, true);
      this.triggerHaptic('warning');
    }
  }

  // Native Pull-to-Refresh Actions

  public async pullRefreshFleetAsync(): Promise<void> {
    this.triggerHaptic('selection');
    if (this.bridge === null) {
      return;
    }
    try {
      this.bridge.connectAllHosts();
      for (let i = 0; i < this.hosts.length; i++) {
        try {
          this.bridge.refreshHost(this.hosts[i].host_id);
        } catch (err) {
          orbitLogWarn('OrbitStore', 'pullRefreshFleetAsync refreshHost failed', err as Object);
        }
      }
      await this.reloadFleetAsync();
      if (this.selectedHostId.length > 0) {
        await this.reloadHostAsync(this.selectedHostId);
      }
    } catch (err) {
      orbitLogWarn('OrbitStore', 'pullRefreshFleetAsync failed', err as Object);
    }
  }

  public async pullRefreshStageAsync(): Promise<void> {
    this.pullRefreshStage();
    if (this.bridge === null) {
      return;
    }
    try {
      await this.reloadFleetAsync();
      if (this.selectedHostId.length > 0) {
        await this.reloadHostAsync(this.selectedHostId);
      }
    } catch (err) {
      orbitLogWarn('OrbitStore', 'pullRefreshStageAsync failed', err as Object);
    }
  }

  public pullRefreshStage(): void {
    this.triggerHaptic('selection');
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
    this.triggerHapticPreset('action');
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
      this.triggerHaptic('warning');
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
      const scopeKey = this.sidePanelStore.scopeKeyFor(this.selectedHostId, agentId);
      this.dropScopeAndRevokeSideQuestion(scopeKey);
      this.sidePanelStore.removeSubagentTabs(this.selectedHostId, agentId);
      if (this.selectedAgentId === agentId) {
        this.startNewSessionDraft();
      }
      this.reloadHost(this.selectedHostId);
      this.triggerHaptic('warning');
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
      const lower = decision.trim().toLowerCase();
      this.triggerHaptic(lower.includes('deny') || lower.includes('reject') ? 'warning' : 'success');
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
      this.triggerHapticPreset('success');
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
    if (this.sidePanelStore.isOpen && this.sidePanelStore.activeTab?.kind === 'review') {
      this.sidePanelStore.dismissPanel();
      this.triggerHapticPreset('selection');
      return;
    }
    // Reuse `openGitInspector()` so wide-screen right-rail mutual exclusion (yielding docked
    // Briefing panel and marking `diffInspectorOpenedByUser = true`) stays strictly uniform.
    this.openGitInspector();
  }

  public openGitInspector(focusPath?: string, cwdOverride?: string, hostIdOverride?: string): void {
    this.diffInspectorOpenedByUser = true;
    if (this.isCurrentSessionPersonalAgent()) {
      this.briefingDismissedByUserForPaKey = `${this.selectedHostId}::${this.selectedAgentId}`;
    }
    this.briefingViewingOpen = false;
    this.showBriefingPanel = false;
    this.showBriefingSheet = false;
    if (this.shellStore.modalRoute?.kind === 'pa_briefing') {
      this.shellStore.closeModal();
    }
    if (focusPath !== undefined) {
      this.gitSelectedPath = focusPath;
    }
    const cleanCwd = cwdOverride !== undefined && cwdOverride.trim().length > 0 ? cwdOverride : undefined;
    const cleanHost = hostIdOverride !== undefined && hostIdOverride.trim().length > 0 ? hostIdOverride.trim() : undefined;
    const normPath = focusPath !== undefined ? this.normalizeGitFocusPath(focusPath) : null;
    const prevTab = this.sidePanelStore.activeTab;
    const wasOpen = this.sidePanelStore.isOpen;
    this.openSidePanelToolTab(
      'review',
      normPath,
      'user',
      cleanCwd,
      cleanHost
    );
    // Avoid issuing a duplicate global `refreshGitReview` when mounting/switching to a Review tab
    // (which triggers `OrbitScopedGitTabView`'s own tracked fetch). Only refresh directly if the
    // exact same Review tab was already mounted & active.
    const nextTab = this.sidePanelStore.activeTab;
    if (
      wasOpen &&
      prevTab !== null &&
      nextTab !== null &&
      prevTab.kind === 'review' &&
      nextTab.kind === 'review' &&
      prevTab.tabId === nextTab.tabId &&
      nextTab.cwd !== null &&
      nextTab.cwd.trim().length > 0
    ) {
      this.requestScopedGitReviewFetch(
        nextTab.hostId,
        nextTab.cwd,
        nextTab.gitStaged === true,
        nextTab.path,
        true
      );
    }
    this.triggerHapticPreset('selection');
  }

  public setGitStagedMode(staged: boolean): void {
    this.gitStagedMode = staged;
    this.refreshGitReview(staged, this.gitSelectedPath.length > 0 ? this.gitSelectedPath : null);
    this.triggerHapticPreset('selection');
  }

  public selectGitFileFilter(path: string): void {
    this.gitSelectedPath = path;
    this.refreshGitReview(this.gitStagedMode, path.length > 0 ? path : null);
    this.triggerHapticPreset('selection');
  }

  public refreshGitReview(staged: boolean, path: string | null): void {
    const targetHost = this.resolvedDiffHostId();
    const targetCwd = this.resolvedDiffCwd();
    if (this.bridge === null || targetHost.length === 0) {
      return;
    }
    try {
      this.bridge.fetchGitStatus(targetHost, targetCwd);
      this.bridge.fetchGitDiff(
        targetHost,
        targetCwd,
        this.normalizeGitFocusPath(path),
        staged
      );
    } catch (err) {
      orbitLogWarn('OrbitStore', 'refreshGitReview failed', err as Object);
    }
    if (this.isReviewScopeVisible(targetHost, targetCwd)) {
      this.reloadGitDiff(targetCwd, targetHost);
    }
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
      const pathOpt = this.gitSelectedPath.length > 0 ? this.normalizeGitFocusPath(this.gitSelectedPath) : null;
      const gr = JSON.parse(
        this.bridge.gitReviewJson(targetHost, cwd, pathOpt, this.gitStagedMode)
      ) as GitReviewDto;
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
    this.openGitInspector();
  }

  // Detached PTY Multi-Terminal Operations

  public inspectGitDiffForCwd(cwd: string, hostIdOverride?: string): void {
    const cleanCwd = nonBlankPath(cwd);
    const cleanHost = hostIdOverride !== undefined && hostIdOverride.trim().length > 0
      ? hostIdOverride.trim()
      : undefined;
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    this.openGitInspector('', cleanCwd.length > 0 ? cleanCwd : undefined, cleanHost);
  }

  /**
   * Opens Git Diff for a specific `(hostId, cwd)` targeted from a Briefing decision card.
   * Routes into PR-4's scoped Review tab for `(hostId, cwd)` while yielding the docked Briefing rail.
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
    this.openGitInspector('', cwd, cleanHost);
  }

  public openPtyTerminalInCwd(cwd: string, hostIdOverride?: string): void {
    const clean = nonBlankPath(cwd);
    const cleanHost = hostIdOverride !== undefined && hostIdOverride.trim().length > 0
      ? hostIdOverride.trim()
      : undefined;
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    this.openSidePanelToolTab('terminal', null, 'user', clean.length > 0 ? clean : undefined, cleanHost);
    const targetHost = cleanHost ?? this.selectedHostId;
    if (this.bridge !== null && targetHost.length > 0) {
      const detail = this.getScopedHostDetail(targetHost);
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
        this.attachScopedTerminal(targetHost, matchedTermId, 1);
      } else {
        this.startScopedTerminal(targetHost, clean.length > 0 ? clean : this.activeCwd);
      }
    }
    this.triggerHapticPreset('action');
  }

  public openPtyTerminal(): void {
    this.openSidePanelToolTab('terminal', null, 'user');
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
    this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('selection');
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

  public askSideQuestion(question: string, _contextSummary?: string): string {
    return this.submitBtwFromComposer(question);
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
    this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('success');
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
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    this.openSidePanelToolTab('side_question', null, 'user');
    this.triggerHapticPreset('selection');
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
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    if (this.bridge !== null && this.selectedHostId.length > 0) {
      this.reloadHost(this.selectedHostId);
    }
    this.shellStore.openModal({
      kind: 'archived',
      hostId: this.selectedHostId
    });
    this.triggerHapticPreset('selection');
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

  public unarchiveSession(agentId: string, hostIdOverride?: string): void {
    const targetHost = (hostIdOverride !== undefined && hostIdOverride.trim().length > 0)
      ? hostIdOverride.trim()
      : this.selectedHostId;
    if (this.bridge === null || targetHost.length === 0 || agentId.length === 0) {
      return;
    }
    try {
      this.bridge.unarchiveAgent(targetHost, agentId);
      this.clearArchivedSubagentLocalMark(targetHost, agentId);
      this.reloadHost(targetHost);
      this.showToast('Session restored', false);
      this.triggerHapticPreset('success');
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
      const scopeKey = this.sidePanelStore.scopeKeyFor(this.selectedHostId, agentId);
      this.dropScopeAndRevokeSideQuestion(scopeKey);
      this.sidePanelStore.removeSubagentTabs(this.selectedHostId, agentId);
      if (this.selectedAgentId === agentId) {
        this.startNewSessionDraft();
      }
      this.reloadHost(this.selectedHostId);
      this.showToast('Session deleted', false);
      this.triggerHapticPreset('warning');
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
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    this.openSidePanelToolTab('board', null, 'user');
    this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('success');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'spawnWorkspacePeer failed', err as Object);
      this.showToast((err as Error).message || 'Failed to spawn peer', true);
      return false;
    }
  }

  // 5. Automations: Schedules, Conditional Timers, Goals, Monitors & Workflows

  public openAutomationsSheet(): void {
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    this.openSidePanelToolTab('automations', null, 'user');
    this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('warning');
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
      this.triggerHapticPreset('warning');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('warning');
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
      this.triggerHapticPreset('warning');
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
      this.triggerHapticPreset('warning');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('warning');
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
      this.triggerHapticPreset('selection');
    } catch (err) {
      orbitLogWarn('OrbitStore', 'updatePushPreferencesJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to update push preferences', true);
    }
  }

  public async sendTestPushNotificationScoped(
    hostId: string
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Connect a host first' };
    }
    try {
      const deviceId = this.prefsStore.getOrCreateDeviceId();
      await this.bridge.testPushNotificationJson(cleanHost, deviceId);
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'testPushNotificationJson failed', err as Object);
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Test push failed'
      };
    }
  }

  public async sendTestPushNotification(): Promise<void> {
    const targetHost = this.selectedHostId.trim();
    const originEpoch = this.hostSelectionEpoch;
    if (this.bridge === null || targetHost.length === 0) {
      this.showToastResource($r('app.string.toast_connect_host_first'), true);
      return;
    }
    const res = await this.sendTestPushNotificationScoped(targetHost);
    if (this.selectedHostId.trim() !== targetHost || this.hostSelectionEpoch !== originEpoch) {
      return;
    }
    if (res.ok) {
      this.showToastResource($r('app.string.toast_test_push_sent'), false);
      this.triggerHapticPreset('action');
    } else {
      this.showToastResource($r('app.string.toast_test_push_failed'), true, res.errorDetail);
      this.triggerHapticPreset('warning');
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
    this.triggerHapticPreset('success');
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
    this.navigationSeq += 1;
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
      this.selectHostInternal(cleanHost, true, false);
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
    this.navigationSeq += 1;
    // Reuse fresh briefing when available; fetch once only when missing/stale/invalidated.
    this.personalAgentStore.ensureFreshBriefing();
    const pa = this.personalAgentStore.briefing?.personal_agent ?? null;
    if (pa !== null && pa.host_id.length > 0 && pa.agent_id.length > 0) {
      this.showPersonalAgentSetup = false;
      this.showGlobalOverview = false;
      this.selectAgentOnHost(pa.host_id, pa.agent_id, true);
      if (this.shellStore.breakpoint === 'sm') {
        this.sidebarOpen = false;
      }
      this.triggerHapticPreset('selection');
      return;
    }
    this.showPersonalAgentSetup = true;
    this.showGlobalOverview = false;
    this.briefingViewingOpen = false;
    this.showBriefingSheet = false;
    if (this.shellStore.modalRoute?.kind === 'pa_briefing') {
      this.shellStore.closeModal();
    }
    // Discovery is owned by PersonalAgentSetupCard.aboutToAppear() via coalesced refreshRouterAndMesh().
    if (this.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN) {
      this.sidebarOpen = false;
    }
    this.triggerHapticPreset('selection');
  }

  public toggleBriefingPanel(): void {
    const nextOpen = !this.showBriefingPanel;
    this.briefingViewingOpen = nextOpen;
    this.showBriefingPanel = nextOpen;
    if (nextOpen) {
      this.briefingDismissedByUserForPaKey = '';
      this.closeGitInspector();
      if (this.sidePanelStore.isOpen) {
        this.sidePanelStore.dismissPanel();
      }
      this.showBriefingSheet = false;
      if (this.shellStore.modalRoute?.kind === 'pa_briefing') {
        this.shellStore.closeModal();
      }
      this.personalAgentStore.ensureFreshBriefing();
    } else if (this.isCurrentSessionPersonalAgent()) {
      this.briefingDismissedByUserForPaKey = `${this.selectedHostId}::${this.selectedAgentId}`;
    }
    this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('selection');
      return;
    }
    this.showBriefingPanel = false;
    this.showBriefingSheet = true;
    this.shellStore.openModal({ kind: 'pa_briefing' });
    this.personalAgentStore.ensureFreshBriefing();
    const pa = this.personalAgentStore.briefing?.personal_agent ?? null;
    if (pa !== null && pa.host_id.length > 0) {
      this.refreshPersonalAgentActivity(pa.host_id, 50, null, false);
    }
    this.triggerHapticPreset('selection');
  }

  public closeBriefingSheet(): void {
    this.showBriefingSheet = false;
    if (this.shellStore.modalRoute?.kind === 'pa_briefing') {
      this.shellStore.closeModal();
    }
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('success');
    } else if (feedback.kind === 'approved_with_skipped') {
      this.showLocalizedToast(
        $r('app.string.briefing_batchApprovedWithSkipped', feedback.resolvedCount, feedback.skippedCount),
        `Approved ${feedback.resolvedCount}, skipped ${feedback.skippedCount}`,
        false,
        [feedback.resolvedCount, feedback.skippedCount]
      );
      this.triggerHapticPreset('success');
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
        this.selectHostInternal(targetRouter.host_id, true, false);
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
    this.triggerHapticPreset('action');
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
          this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('action');
    }
  }

  public cancelVoiceDictation(): void {
    this.voiceController.cancel();
    this.stopVoiceTimer();
    this.isVoiceRecording = false;
    this.voiceInterimTranscript = '';
    this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('success');
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
    if (this.selectedHostId.length === 0 || this.selectedAgentId.length === 0) {
      return;
    }
    const root = this.sidePanelStore.resolveRootScope(this.selectedHostId, this.selectedAgentId);
    const rootHost = root.hostId.length > 0 ? root.hostId : this.selectedHostId;
    const rootAgent = root.rootAgentId !== null ? root.rootAgentId : this.selectedAgentId;
    this.openSubagentEntryInRootScope(rootHost, rootAgent, sub, this.activeCwd);
  }

  public openSubagentEntryInRootScope(
    rootHostId: string,
    rootAgentId: string,
    sub: SessionSubagentEntryDto,
    fallbackCwd: string | null = null
  ): void {
    const cleanRootHost = rootHostId.trim();
    const cleanRootAgent = rootAgentId.trim();
    if (cleanRootHost.length === 0 || cleanRootAgent.length === 0) {
      return;
    }
    this.navigationSeq += 1;
    const navSeq = this.navigationSeq;
    const root = this.sidePanelStore.resolveRootScope(cleanRootHost, cleanRootAgent, true);
    const resolvedRootHost = root.hostId.length > 0 ? root.hostId : cleanRootHost;
    const resolvedRootAgent = root.rootAgentId !== null ? root.rootAgentId : cleanRootAgent;

    const managedChildId = (sub.child_agent_id !== null && sub.child_agent_id.trim().length > 0)
      ? sub.child_agent_id.trim()
      : null;
    const providerSubId = (
      sub.provider_subagent_id !== undefined &&
      sub.provider_subagent_id !== null &&
      sub.provider_subagent_id.trim().length > 0
    )
      ? sub.provider_subagent_id.trim()
      : (managedChildId === null ? sub.id : null);

    // Preserve actual child host and direct parent separately from root scope (P0-4).
    const directParentHost = resolveSubagentDirectParentHostId(sub, resolvedRootHost);
    const directParentAgent = resolveSubagentDirectParentAgentId(sub, resolvedRootAgent);
    const actualChildHost = managedChildId !== null
      ? resolveSubagentActualHostId(sub, resolvedRootHost)
      : directParentHost;
    const childCard = managedChildId !== null
      ? this.findAgentCardOnHost(actualChildHost, managedChildId)
      : null;
    const effectiveCwd = (childCard !== null && childCard.cwd.length > 0)
      ? childCard.cwd
      : fallbackCwd;

    const title = sub.role.trim().length > 0 ? sub.role.trim() : 'Subagent';
    const hasUnresolvedHost = root.unresolvedHostId !== undefined &&
      root.unresolvedHostId !== null &&
      root.unresolvedHostId.trim().length > 0;
    if (hasUnresolvedHost || root.missingAgentCard === true) {
      this.resolveRootScopeHydratingChainAsync(cleanRootHost, cleanRootAgent, true).then((hydrated: OrbitHydratedRootResult): void => {
        if (navSeq !== this.navigationSeq) {
          return;
        }
        if (hydrated.status !== 'resolved') {
          this.showToast('Unable to load parent session host — tap to retry', true);
          return;
        }
        const finalRootHost = hydrated.hostId;
        const finalRootAgent = hydrated.rootAgentId;
        const tabId = managedChildId !== null
          ? canonicalManagedSubagentTabId(actualChildHost, managedChildId)
          : canonicalProviderSubagentTabId(directParentHost, directParentAgent, providerSubId ?? sub.id);
        const asyncTab: OrbitSidePanelTab = {
          tabId: tabId,
          kind: 'subagent',
          hostId: actualChildHost,
          agentId: managedChildId !== null ? managedChildId : (providerSubId ?? sub.id),
          cwd: effectiveCwd,
          path: null,
          parentHostId: finalRootHost,
          title: title,
          closable: true,
          parentAgentId: finalRootAgent,
          directParentHostId: directParentHost,
          directParentAgentId: directParentAgent,
          childAgentId: managedChildId,
          providerSubagentId: providerSubId,
          subagentCallId: sub.id
        };
        this.sidePanelStore.openSubagentTab(finalRootHost, finalRootAgent, asyncTab, { source: 'user', activate: true });
        this.triggerHapticPreset('selection');
      }).catch((err: Object) => {
        orbitLogWarn('OrbitStore', 'openSubagentEntryInRootScope chain resolution failed', err);
      });
      return;
    }

    const tabId = managedChildId !== null
      ? canonicalManagedSubagentTabId(actualChildHost, managedChildId)
      : canonicalProviderSubagentTabId(directParentHost, directParentAgent, providerSubId ?? sub.id);

    const subTab: OrbitSidePanelTab = {
      tabId: tabId,
      kind: 'subagent',
      hostId: actualChildHost,
      agentId: managedChildId !== null ? managedChildId : (providerSubId ?? sub.id),
      cwd: effectiveCwd,
      path: null,
      parentHostId: resolvedRootHost,
      title: title,
      closable: true,
      parentAgentId: resolvedRootAgent,
      directParentHostId: directParentHost,
      directParentAgentId: directParentAgent,
      childAgentId: managedChildId,
      providerSubagentId: providerSubId,
      subagentCallId: sub.id
    };

    // Subagent Force-Routing Invariant: NEVER switch main Stage away from root agent.
    this.sidePanelStore.openSubagentTab(
      resolvedRootHost,
      resolvedRootAgent,
      subTab,
      { source: 'user', activate: true }
    );
    this.triggerHapticPreset('selection');
  }

  // 11. File Explorer & Code Viewer (Read / Edit / Create / Rename / Delete)

  public openFileExplorer(initialPath?: string, cwdOverride?: string, hostIdOverride?: string): void {
    const cleanCwd = cwdOverride !== undefined && cwdOverride.trim().length > 0 ? cwdOverride.trim() : undefined;
    const cleanHost = hostIdOverride !== undefined && hostIdOverride.trim().length > 0 ? hostIdOverride.trim() : undefined;
    if (this.shellStore.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    if (initialPath !== undefined && initialPath.trim().length > 0) {
      const cleanPath = initialPath.trim();
      this.openSidePanelFileTab(cleanHost ?? this.selectedHostId, cleanCwd ?? this.activeCwd, cleanPath);
    } else {
      this.openSidePanelToolTab('files', null, 'user', cleanCwd, cleanHost);
    }
    this.triggerHapticPreset('selection');
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
    if (this.inspectedFileView !== null && !isWorkspaceFileEditable(this.inspectedFileView)) {
      this.showToast('This file is read-only, binary, or truncated', true);
      return false;
    }
    this.fileExplorerBusy = true;
    try {
      const expectedEtag = this.inspectedFileView?.etag ?? null;
      const outcome = await this.saveScopedWorkspaceFile(
        this.selectedHostId,
        this.activeCwd,
        path,
        content,
        expectedEtag,
        false
      );
      if (outcome.ok) {
        await this.openWorkspaceFileInViewer(path);
        return true;
      }
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
      this.triggerHapticPreset('selection');
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
      if (this.isDiffTargetPinned || (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd))) {
        this.pullRefreshGitReview();
      }
      this.showToast(all ? 'Staged all changes' : 'Staged file', false);
      this.triggerHapticPreset('selection');
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
      if (this.isDiffTargetPinned || (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd))) {
        this.pullRefreshGitReview();
      }
      this.showToast(all ? 'Unstaged all changes' : 'Unstaged file', false);
      this.triggerHapticPreset('selection');
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
      if (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd)) {
        this.pullRefreshGitReview();
        this.refreshGitCollabStatus();
      }
      this.showToast(`Committed ${res.short_sha}`, false);
      this.triggerHapticPreset('success');
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
      if (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd)) {
        this.refreshGitCollabStatus();
      }
      this.showToast(`Created PR: ${res.title}`, false);
      this.triggerHapticPreset('success');
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
      if (this.isDiffTargetPinned || (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd))) {
        this.pullRefreshGitReview();
        this.refreshGitCollabStatus();
      }
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
      if (this.isDiffTargetPinned || (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd))) {
        this.pullRefreshGitReview();
        this.refreshGitCollabStatus();
      }
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
      if (this.isDiffTargetPinned || (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd))) {
        this.pullRefreshGitReview();
        this.refreshGitCollabStatus();
      }
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
      if (this.isDiffTargetPinned || (this.selectedHostId === targetHost && cwdPathsEquivalent(this.activeCwd, targetCwd))) {
        this.pullRefreshGitReview();
        this.refreshGitCollabStatus();
      }
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
    this.shellStore.openModal({
      kind: 'global_search',
      hostId: this.selectedHostId
    });
    this.triggerHapticPreset('selection');
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
    this.shellStore.openModal({
      kind: 'host_settings',
      hostId: this.selectedHostId,
      initialPath: 'personal_agent'
    });
    this.refreshRouterAndMesh();
    this.triggerHaptic('selection');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('selection');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('selection');
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
        this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('success');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('action');
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
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'startWorkflowJson failed', err as Object);
      this.showToast((err as Error).message || 'Failed to start workflow', true);
      return false;
    }
  }

  // 17. Transcript Export (Markdown / JSON / HTML)

  public openExportTranscriptSheet(): void {
    this.shellStore.openModal({
      kind: 'export_transcript',
      hostId: this.selectedHostId,
      agentId: this.selectedAgentId
    });
    this.triggerHapticPreset('selection');
  }

  // 18. Side Panel Tab Openers & Strictly Scoped Data-Isolation Helpers (PR-4)
  // Every scoped helper below reads ONLY its explicit `hostId`/`agentId`/`cwd`/`path` arguments
  // and NEVER falls back to global `this.selectedHostId` or `this.activeCwd`.

  /**
   * Pure observable cache read (ZERO synchronous N-API calls, safe inside `@Computed` and `build()`).
   * Call `ensureScopedHostDetailAsync(hostId)` from lifecycle hooks to populate cache misses.
   */
  public getScopedHostDetail(hostId: string): HostDetailDto | null {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return null;
    }
    const _rev = this.fleetStore.hostDetailRevision;
    if (_rev < 0) {
      return null;
    }
    return this.fleetStore.getCachedHostDetail(cleanHost);
  }

  public ensureScopedHostDetailAsync(hostId: string): void {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return;
    }
    this.fleetStore.ensureHostDetailAsync(cleanHost);
  }

  public findAgentCardOnHost(hostId: string, agentId: string): AgentCardDto | null {
    const detail = this.getScopedHostDetail(hostId);
    if (detail === null || agentId.trim().length === 0) {
      return null;
    }
    for (let i = 0; i < detail.agents.length; i++) {
      if (detail.agents[i].agent_id === agentId) {
        return detail.agents[i];
      }
    }
    return null;
  }

  private resolveCurrentScopeTuple(): [string, string | null, string] {
    const hostId = this.selectedHostId.trim();
    if (hostId.length === 0) {
      return ['', null, ''];
    }
    if (this.isNewSessionDraft || this.selectedAgentId.trim().length === 0) {
      return [hostId, null, this.activeCwd];
    }
    const root = this.sidePanelStore.resolveRootScope(hostId, this.selectedAgentId);
    const resolvedHost = root.hostId.length > 0 ? root.hostId : hostId;
    const resolvedAgent = root.rootAgentId;
    const card = resolvedAgent !== null ? this.findAgentCardOnHost(resolvedHost, resolvedAgent) : null;
    const cwd = card !== null && card.cwd.length > 0 ? card.cwd : this.activeCwd;
    return [resolvedHost, resolvedAgent, cwd];
  }

  public openSidePanelToolTab(
    kind: OrbitSidePanelKind,
    path: string | null = null,
    source: 'user' | 'auto' = 'user',
    cwdOverride?: string,
    hostIdOverride?: string
  ): void {
    const tuple = this.resolveCurrentScopeTuple();
    const scopeHostId = tuple[0];
    const rootAgentId = tuple[1];
    const hasExplicitHost = hostIdOverride !== undefined && hostIdOverride.trim().length > 0;
    const tabHostId = hasExplicitHost ? hostIdOverride!.trim() : scopeHostId;
    const hasExplicitCwd = cwdOverride !== undefined && cwdOverride.trim().length > 0;
    const resolvedCwd = hasExplicitCwd ? cwdOverride! : tuple[2];
    if (tabHostId.length === 0) {
      return;
    }
    const effectiveScopeHost = scopeHostId.length > 0 ? scopeHostId : tabHostId;
    const scopeKey = this.sidePanelStore.scopeKeyFor(effectiveScopeHost, rootAgentId);
    if (this.sidePanelStore.activeScopeKey !== scopeKey) {
      this.sidePanelStore.switchScopeByKey(scopeKey);
    }

    const explicitTargetDifferent =
      (hasExplicitCwd && !cwdPathsEquivalent(resolvedCwd, tuple[2])) ||
      (hasExplicitHost && tabHostId !== scopeHostId);

    let title = 'Panel';
    let tabId = `${kind}:${scopeKey}`;
    if (kind === 'review') {
      title = explicitTargetDifferent && resolvedCwd.length > 0
        ? `Git · ${basenameOfPath(resolvedCwd)}`
        : 'Git Review';
      if (explicitTargetDifferent && resolvedCwd.length > 0) {
        tabId = canonicalCwdToolTabId('review', tabHostId, resolvedCwd);
      }
    } else if (kind === 'files') {
      title = explicitTargetDifferent && resolvedCwd.length > 0
        ? `Files · ${basenameOfPath(resolvedCwd)}`
        : 'Files';
      if (explicitTargetDifferent && resolvedCwd.length > 0) {
        tabId = canonicalCwdToolTabId('files', tabHostId, resolvedCwd);
      }
    } else if (kind === 'file') {
      const cleanPath = nonBlankPath(path);
      title = cleanPath.length > 0 ? basenameOfPath(cleanPath) : 'File';
      tabId = canonicalFileTabId(tabHostId, resolvedCwd, cleanPath);
    } else if (kind === 'terminal') {
      title = explicitTargetDifferent && resolvedCwd.length > 0
        ? `Terminal · ${basenameOfPath(resolvedCwd)}`
        : 'Terminal';
      if (explicitTargetDifferent && resolvedCwd.length > 0) {
        tabId = canonicalCwdToolTabId('terminal', tabHostId, resolvedCwd);
      }
    } else if (kind === 'board') {
      title = 'Board';
    } else if (kind === 'side_question') {
      title = 'Side Q&A';
      tabId = canonicalSideQuestionTabId(scopeKey);
    } else if (kind === 'briefing') {
      title = 'Briefing';
    } else if (kind === 'automations') {
      title = 'Automations';
    } else if (kind === 'subagent') {
      title = 'Subagent Crew';
      tabId = `crew:${scopeKey}`;
    }

    const boardAgentForResolve = explicitTargetDifferent ? null : rootAgentId;
    const wsId = kind === 'board'
      ? this.resolveScopedWorkspaceId(tabHostId, resolvedCwd, boardAgentForResolve)
      : null;

    const tab: OrbitSidePanelTab = {
      tabId: tabId,
      kind: kind,
      hostId: tabHostId,
      agentId: explicitTargetDifferent ? null : rootAgentId,
      cwd: resolvedCwd.length > 0 ? resolvedCwd : null,
      path: path,
      parentHostId: effectiveScopeHost,
      title: title,
      closable: true,
      parentAgentId: rootAgentId,
      workspaceId: wsId !== null && wsId.length > 0 ? wsId : null,
      terminalSlot: kind === 'terminal' ? 1 : null
    };

    const opts: OrbitOpenSidePanelTabOptions = {
      source: source,
      activate: true
    };
    this.sidePanelStore.openTab(tab, opts);
  }

  public openSidePanelFileTab(hostId: string, cwd: string, filePath: string): void {
    const cleanHost = hostId.trim();
    const cleanPath = nonBlankPath(filePath);
    if (cleanHost.length === 0 || cleanPath.length === 0) {
      return;
    }
    const cleanCwd = nonBlankPath(cwd);
    const tab: OrbitSidePanelTab = {
      tabId: canonicalFileTabId(cleanHost, cleanCwd, cleanPath),
      kind: 'file',
      hostId: cleanHost,
      agentId: this.isNewSessionDraft ? null : (this.selectedAgentId.length > 0 ? this.selectedAgentId : null),
      cwd: cleanCwd.length > 0 ? cleanCwd : null,
      path: cleanPath,
      parentHostId: cleanHost,
      title: basenameOfPath(cleanPath),
      closable: true
    };
    this.sidePanelStore.openTab(tab, { source: 'user', activate: true });
    this.triggerHapticPreset('selection');
  }

  public openSubagentCrewPanel(hostId?: string, rootAgentId?: string): void {
    const targetHost = hostId !== undefined && hostId.trim().length > 0
      ? hostId.trim()
      : this.selectedHostId.trim();
    const targetAgent = rootAgentId !== undefined && rootAgentId.trim().length > 0
      ? rootAgentId.trim()
      : this.selectedAgentId.trim();
    if (targetHost.length === 0 || targetAgent.length === 0) {
      return;
    }
    this.navigationSeq += 1;
    const navSeq = this.navigationSeq;
    const root = this.sidePanelStore.resolveRootScope(targetHost, targetAgent, true);
    const hasUnresolvedHost = root.unresolvedHostId !== undefined &&
      root.unresolvedHostId !== null &&
      root.unresolvedHostId.trim().length > 0;
    if (hasUnresolvedHost || root.missingAgentCard === true) {
      this.resolveRootScopeHydratingChainAsync(targetHost, targetAgent, true).then((hydrated: OrbitHydratedRootResult): void => {
        if (navSeq !== this.navigationSeq) {
          return;
        }
        if (hydrated.status !== 'resolved') {
          this.showToast('Unable to load parent session host — tap to retry', true);
          return;
        }
        const resolvedHost = hydrated.hostId;
        const resolvedAgent = hydrated.rootAgentId;
        const card = this.findAgentCardOnHost(resolvedHost, resolvedAgent);
        const scopeKey = this.sidePanelStore.scopeKeyFor(resolvedHost, resolvedAgent);
        const crewTab: OrbitSidePanelTab = {
          tabId: `crew:${scopeKey}`,
          kind: 'subagent',
          hostId: resolvedHost,
          agentId: resolvedAgent,
          cwd: card !== null && card.cwd.length > 0 ? card.cwd : null,
          path: null,
          parentHostId: resolvedHost,
          title: 'Subagent Crew',
          closable: true,
          parentAgentId: resolvedAgent
        };
        this.sidePanelStore.openSubagentTab(
          resolvedHost,
          resolvedAgent,
          crewTab,
          { source: 'user', activate: true }
        );
        this.triggerHapticPreset('selection');
      }).catch((err: Object) => {
        orbitLogWarn('OrbitStore', 'openSubagentCrewPanel async parent resolution failed', err);
      });
      return;
    }
    const resolvedHost = root.hostId.length > 0 ? root.hostId : targetHost;
    const resolvedAgent = root.rootAgentId !== null ? root.rootAgentId : targetAgent;
    const card = this.findAgentCardOnHost(resolvedHost, resolvedAgent);
    const scopeKey = this.sidePanelStore.scopeKeyFor(resolvedHost, resolvedAgent);
    const crewTab: OrbitSidePanelTab = {
      tabId: `crew:${scopeKey}`,
      kind: 'subagent',
      hostId: resolvedHost,
      agentId: resolvedAgent,
      cwd: card !== null && card.cwd.length > 0 ? card.cwd : null,
      path: null,
      parentHostId: resolvedHost,
      title: 'Subagent Crew',
      closable: true,
      parentAgentId: resolvedAgent
    };
    this.sidePanelStore.openSubagentTab(
      resolvedHost,
      resolvedAgent,
      crewTab,
      { source: 'user', activate: true }
    );
    this.triggerHapticPreset('selection');
  }

  /**
   * Opens a peer card in the side panel. If the card is itself a child subagent, enforces the
   * mandatory "child click -> root agent scope + subagent tab" invariant (P1-6 / P0-4 / R3-P1-2).
   * Always bumps unified `navigationSeq` even for synchronous peer opens so any older in-flight
   * cross-host hydration cannot commit afterward and steal the Stage.
   */
  public openPeerPanelTab(hostId: string, peer: AgentCardDto, originParentHostId?: string): void {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0 || peer.agent_id.trim().length === 0) {
      return;
    }
    this.navigationSeq += 1;
    const navSeq = this.navigationSeq;
    const label = (peer.handle !== undefined && peer.handle !== null && peer.handle.trim().length > 0)
      ? `@${peer.handle.trim()}`
      : (peer.title.trim().length > 0 ? peer.title.trim() : peer.provider_label);

    if (peer.parent_agent_id !== null && peer.parent_agent_id.trim().length > 0) {
      const directParentHost = (
        peer.parent_host_id !== undefined &&
        peer.parent_host_id !== null &&
        peer.parent_host_id.trim().length > 0
      )
        ? peer.parent_host_id.trim()
        : cleanHost;
      const directParentAgent = peer.parent_agent_id.trim();
      const root = this.sidePanelStore.resolveRootScope(directParentHost, directParentAgent, true);
      const hasUnresolvedHost = root.unresolvedHostId !== undefined &&
        root.unresolvedHostId !== null &&
        root.unresolvedHostId.trim().length > 0;
      if (hasUnresolvedHost || root.missingAgentCard === true) {
        this.resolveRootScopeHydratingChainAsync(directParentHost, directParentAgent, true).then((hydrated: OrbitHydratedRootResult): void => {
          if (navSeq !== this.navigationSeq) {
            return;
          }
          if (hydrated.status !== 'resolved') {
            this.showToast('Unable to load parent session host — tap to retry', true);
            return;
          }
          this.commitManagedChildSelection(
            cleanHost,
            peer.agent_id,
            hydrated.hostId,
            hydrated.rootAgentId,
            directParentHost,
            directParentAgent,
            false,
            label,
            peer.cwd.length > 0 ? peer.cwd : null
          );
        }).catch((err: Object) => {
          orbitLogWarn('OrbitStore', 'openPeerPanelTab async parent resolution failed', err);
        });
        return;
      }
      const resolvedRootHost = root.hostId.length > 0 ? root.hostId : directParentHost;
      const resolvedRootAgent = root.rootAgentId !== null ? root.rootAgentId : directParentAgent;
      this.commitManagedChildSelection(
        cleanHost,
        peer.agent_id,
        resolvedRootHost,
        resolvedRootAgent,
        directParentHost,
        directParentAgent,
        false,
        label,
        peer.cwd.length > 0 ? peer.cwd : null
      );
      return;
    }

    const parentHost = (originParentHostId !== undefined && originParentHostId.trim().length > 0)
      ? originParentHostId.trim()
      : (this.selectedHostId.length > 0 ? this.selectedHostId : cleanHost);
    const tab: OrbitSidePanelTab = {
      tabId: canonicalPeerTabId(cleanHost, peer.agent_id),
      kind: 'peer',
      hostId: cleanHost,
      agentId: peer.agent_id,
      cwd: peer.cwd.length > 0 ? peer.cwd : null,
      path: null,
      parentHostId: parentHost,
      title: label,
      closable: true
    };
    this.sidePanelStore.openTab(tab, { source: 'user', activate: true });
    this.triggerHapticPreset('selection');
  }

  public openBriefingPanelTab(): void {
    this.openSidePanelToolTab('briefing', null, 'user');
    this.triggerHapticPreset('selection');
  }

  public getScopedBriefing(hostId: string, agentId: string | null = null): PersonalAgentBriefingDto | null {
    return this.personalAgentStore.fetchScopedBriefing(hostId, agentId);
  }

  /**
   * Resolves the EXACT, UNAMBIGUOUS workspace ID matching `(hostId, cwd, agentId)`.
   * Returns `''` on mismatch, ambiguity, or missing host detail — NEVER falls back to
   * `detail.workspaces[0]` and NEVER strips `/home/<user>` prefixes (P0-3 + R3-P0-1).
   */
  public resolveScopedWorkspaceId(hostId: string, cwd: string, agentId: string | null): string {
    const detail = this.getScopedHostDetail(hostId);
    if (detail === null) {
      return '';
    }
    const verified = this.verifyScopedWorkspace(detail, cwd, '', agentId);
    return verified !== null ? verified.workspace_id : '';
  }

  // --- Scoped Git Review, Collab & Write Helpers ---

  /**
   * Normalizes a cwd-relative Git path by stripping redundant `./` and empty segments while
   * preserving every literal character including leading/trailing spaces (e.g. `report.txt `).
   */
  private normalizeGitRelPath(raw: string): string {
    if (raw.length === 0) {
      return '';
    }
    if (raw.startsWith('/')) {
      return raw;
    }
    const segs = raw.split('/');
    const out: string[] = [];
    for (let i = 0; i < segs.length; i++) {
      const s = segs[i];
      if (s.length === 0 || s === '.') {
        continue;
      }
      out.push(s);
    }
    return out.join('/');
  }

  private normalizeGitFocusPath(focusPath: string | null): string | null {
    if (focusPath === null) {
      return null;
    }
    const norm = this.normalizeGitRelPath(focusPath);
    return norm.length > 0 ? norm : null;
  }

  private safeReadGitReviewSnapshot(
    hostId: string,
    cwd: string,
    focusPath: string | null,
    staged: boolean
  ): GitReviewDto | null {
    if (this.bridge === null) {
      return null;
    }
    try {
      const normPath = this.normalizeGitFocusPath(focusPath);
      const raw = this.bridge.gitReviewJson(hostId, cwd, normPath, staged);
      if (raw === undefined || raw === null || raw.trim().length === 0) {
        return null;
      }
      return JSON.parse(raw) as GitReviewDto | null;
    } catch (_err) {
      return null;
    }
  }

  private clearGitDiffSettleTimer(tracker: OrbitGitDiffModeTracker): void {
    if (tracker.settleTimerId !== -1) {
      clearTimeout(tracker.settleTimerId);
      tracker.settleTimerId = -1;
    }
  }

  private gitDiffTrackerKey(
    hostId: string,
    cwd: string,
    focusPath: string | null,
    staged: boolean
  ): string {
    const normPath = this.normalizeGitFocusPath(focusPath);
    return encodeSidePanelKeyTuple([
      'git_diff_mode',
      hostId.trim(),
      normalizeAbsoluteCwd(cwd),
      normPath === null ? 'all:' : `file:${normPath}`,
      staged ? 'S' : 'U'
    ]);
  }

  private isTrackerCurrentlyVisibleReviewTab(tracker: OrbitGitDiffModeTracker): boolean {
    if (!this.isSidePanelActuallyVisible()) {
      return false;
    }
    const activeTab = this.sidePanelStore.activeTab;
    if (activeTab === null || activeTab.kind !== 'review') {
      return false;
    }
    if (activeTab.hostId.trim() !== tracker.hostId) {
      return false;
    }
    const tabCwd = nonBlankPath(activeTab.cwd);
    if (tabCwd.length === 0 || normalizeAbsoluteCwd(tabCwd) !== tracker.normCwd) {
      return false;
    }
    if ((activeTab.gitStaged === true) !== tracker.staged) {
      return false;
    }
    return this.normalizeGitFocusPath(activeTab.path) === tracker.focusPath;
  }

  private evictInactiveGitDiffTrackersIfNeeded(keepKey: string): void {
    if (this.gitDiffModeTrackers.size <= ORBIT_MAX_GIT_DIFF_TRACKERS) {
      return;
    }
    while (this.gitDiffModeTrackers.size > ORBIT_MAX_GIT_DIFF_TRACKERS) {
      let oldestIdleKey = '';
      let oldestIdleClock = Number.MAX_SAFE_INTEGER;
      let oldestAnyInactiveKey = '';
      let oldestAnyInactiveClock = Number.MAX_SAFE_INTEGER;
      this.gitDiffModeTrackers.forEach((tr: OrbitGitDiffModeTracker, key: string): void => {
        if (key === keepKey || this.isTrackerCurrentlyVisibleReviewTab(tr)) {
          return;
        }
        if (tr.modeState !== 'loading' && tr.lastTouchedAt < oldestIdleClock) {
          oldestIdleClock = tr.lastTouchedAt;
          oldestIdleKey = key;
        }
        if (tr.lastTouchedAt < oldestAnyInactiveClock) {
          oldestAnyInactiveClock = tr.lastTouchedAt;
          oldestAnyInactiveKey = key;
        }
      });
      const victimKey = oldestIdleKey.length > 0 ? oldestIdleKey : oldestAnyInactiveKey;
      if (victimKey.length === 0) {
        break;
      }
      const victim = this.gitDiffModeTrackers.get(victimKey);
      if (victim !== undefined) {
        this.clearGitDiffSettleTimer(victim);
        victim.cachedSnapshot = null;
        victim.lastStatus = null;
      }
      this.gitDiffModeTrackers.delete(victimKey);
    }
  }

  public releaseGitDiffTrackerIfUnused(
    hostId: string,
    cwd: string | null,
    focusPath: string | null
  ): void {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (cleanHost.length === 0 || cleanCwd.length === 0) {
      return;
    }
    const modes: boolean[] = [false, true];
    for (let i = 0; i < modes.length; i++) {
      const key = this.gitDiffTrackerKey(cleanHost, cleanCwd, focusPath, modes[i]);
      const tracker = this.gitDiffModeTrackers.get(key);
      if (tracker === undefined || this.isTrackerCurrentlyVisibleReviewTab(tracker)) {
        continue;
      }
      // R5-P1-1: Immediately drop large diff body cache & remove inactive tracker on close,
      // unmount, or same-kind Review tab takeover.
      this.clearGitDiffSettleTimer(tracker);
      tracker.cachedSnapshot = null;
      tracker.lastStatus = null;
      this.gitDiffModeTrackers.delete(key);
    }
  }

  private releaseGitDiffTrackersForHost(hostId: string): void {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return;
    }
    const toDelete: string[] = [];
    this.gitDiffModeTrackers.forEach((tracker: OrbitGitDiffModeTracker, key: string): void => {
      if (tracker.hostId === cleanHost) {
        this.clearGitDiffSettleTimer(tracker);
        tracker.cachedSnapshot = null;
        tracker.lastStatus = null;
        toDelete.push(key);
      }
    });
    for (let i = 0; i < toDelete.length; i++) {
      this.gitDiffModeTrackers.delete(toDelete[i]);
    }
  }

  public get gitDiffTrackerCount(): number {
    return this.gitDiffModeTrackers.size;
  }

  public hasCachedGitDiffPayload(
    hostId: string,
    cwd: string,
    focusPath: string | null,
    staged?: boolean
  ): boolean {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    if (staged !== undefined) {
      const tracker = this.gitDiffModeTrackers.get(
        this.gitDiffTrackerKey(cleanHost, cleanCwd, focusPath, staged)
      );
      return tracker !== undefined && tracker.cachedSnapshot !== null;
    }
    const uTracker = this.gitDiffModeTrackers.get(
      this.gitDiffTrackerKey(cleanHost, cleanCwd, focusPath, false)
    );
    const sTracker = this.gitDiffModeTrackers.get(
      this.gitDiffTrackerKey(cleanHost, cleanCwd, focusPath, true)
    );
    return (
      (uTracker !== undefined && uTracker.cachedSnapshot !== null) ||
      (sTracker !== undefined && sTracker.cachedSnapshot !== null)
    );
  }

  private markHiddenGitTrackersDirty(hostId: string, cwd: string): void {
    const cleanHost = hostId.trim();
    const normCwd = normalizeAbsoluteCwd(cwd);
    if (cleanHost.length === 0 || normCwd.length === 0) {
      return;
    }
    this.gitDiffModeTrackers.forEach((tracker: OrbitGitDiffModeTracker): void => {
      if (tracker.hostId !== cleanHost || tracker.normCwd !== normCwd) {
        return;
      }
      tracker.dirtyWhileHidden = true;
      tracker.cachedSnapshot = null;
    });
  }

  private getOrCreateGitDiffTracker(
    hostId: string,
    cwd: string,
    focusPath: string | null,
    staged: boolean
  ): OrbitGitDiffModeTracker {
    const key = this.gitDiffTrackerKey(hostId, cwd, focusPath, staged);
    this.gitDiffTrackerClock += 1;
    const existing = this.gitDiffModeTrackers.get(key);
    if (existing !== undefined) {
      existing.rawCwd = nonBlankPath(cwd);
      existing.lastTouchedAt = this.gitDiffTrackerClock;
      return existing;
    }
    const created: OrbitGitDiffModeTracker = {
      hostId: hostId.trim(),
      normCwd: normalizeAbsoluteCwd(cwd),
      rawCwd: nonBlankPath(cwd),
      focusPath: this.normalizeGitFocusPath(focusPath),
      staged: staged,
      requestSeq: 0,
      modeState: 'idle',
      baselineDiffRevision: 0,
      confirmedDiffRevision: 0,
      dirtyWhileHidden: false,
      lastStatus: null,
      cachedSnapshot: null,
      settleTimerId: -1,
      lastTouchedAt: this.gitDiffTrackerClock,
      nonErrorLoadingRecoveryBudget: 1,
      lastObservedIdentityEpoch: 0,
      lastObservedStatusRevision: 0,
      lastHandledErrorRevision: 0,
      pendingErrorRecovery: false,
      errorObservedStatusRevision: 0
    };
    this.gitDiffModeTrackers.set(key, created);
    this.evictInactiveGitDiffTrackersIfNeeded(key);
    return created;
  }

  private readSnapStatusRevision(snap: GitReviewDto | null): number {
    return snap !== null && typeof snap.status_revision === 'number' && snap.status_revision > 0
      ? snap.status_revision
      : 0;
  }

  private readSnapIdentityEpoch(snap: GitReviewDto | null): number {
    return snap !== null && typeof snap.cwd_identity_epoch === 'number' && snap.cwd_identity_epoch > 0
      ? snap.cwd_identity_epoch
      : 0;
  }

  private readSnapStatusIdentityEpoch(snap: GitReviewDto | null): number {
    if (snap === null || snap.status === null || snap.status === undefined) {
      return -1;
    }
    if (typeof snap.status_identity_epoch === 'number') {
      return snap.status_identity_epoch;
    }
    return this.readSnapIdentityEpoch(snap);
  }

  private readSnapDiffErrorRevision(snap: GitReviewDto | null): number {
    return snap !== null &&
      typeof snap.last_diff_error_revision === 'number' &&
      snap.last_diff_error_revision > 0
      ? snap.last_diff_error_revision
      : 0;
  }

  private readSnapDiffErrorStatusRevision(snap: GitReviewDto | null, fallbackStatusRev: number): number {
    if (snap !== null && typeof snap.last_diff_error_status_revision === 'number') {
      return snap.last_diff_error_status_revision;
    }
    return fallbackStatusRev;
  }

  private requestScopedGitReviewFetch(
    hostId: string,
    cwd: string,
    staged: boolean,
    focusPath: string | null,
    includeStatus: boolean = true,
    preloadedSnap: GitReviewDto | null = null,
    resetRetryBudget: boolean = true
  ): void {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return;
    }
    const tracker = this.getOrCreateGitDiffTracker(cleanHost, cleanCwd, focusPath, staged);
    this.clearGitDiffSettleTimer(tracker);

    const preSnap =
      preloadedSnap !== null
        ? preloadedSnap
        : this.safeReadGitReviewSnapshot(cleanHost, cleanCwd, tracker.focusPath, staged);
    const preDiffRev =
      preSnap !== null && typeof preSnap.diff_revision === 'number' && preSnap.diff_revision > 0
        ? preSnap.diff_revision
        : 0;
    if (preSnap !== null) {
      if (preSnap.status !== undefined) {
        tracker.lastStatus = preSnap.status;
      }
      const preEpoch = this.readSnapIdentityEpoch(preSnap);
      const preStatusEpoch = this.readSnapStatusIdentityEpoch(preSnap);
      if (preEpoch > tracker.lastObservedIdentityEpoch && preStatusEpoch === preEpoch) {
        tracker.lastObservedIdentityEpoch = preEpoch;
      }
      const preStatusRev = this.readSnapStatusRevision(preSnap);
      if (preStatusEpoch === preEpoch && preStatusRev > tracker.lastObservedStatusRevision) {
        tracker.lastObservedStatusRevision = preStatusRev;
      }
      const preErrRev = this.readSnapDiffErrorRevision(preSnap);
      if (preErrRev > tracker.lastHandledErrorRevision) {
        tracker.lastHandledErrorRevision = preErrRev;
      }
    }

    tracker.dirtyWhileHidden = false;
    tracker.cachedSnapshot = null;
    tracker.pendingErrorRecovery = false;
    if (resetRetryBudget) {
      tracker.nonErrorLoadingRecoveryBudget = 1;
    }
    tracker.requestSeq += 1;
    const reqSeq = tracker.requestSeq;
    tracker.modeState = 'loading';
    tracker.baselineDiffRevision = preDiffRev;

    tracker.settleTimerId = setTimeout((): void => {
      if (tracker.requestSeq === reqSeq && tracker.modeState === 'loading') {
        tracker.settleTimerId = -1;
        tracker.modeState = 'unverified';
        tracker.cachedSnapshot = null;
        this.gitPanelRevision += 1;
        this.panelDataRevision += 1;
      }
    }, 1500);

    try {
      if (includeStatus) {
        this.bridge.fetchGitStatus(cleanHost, cleanCwd);
      }
      this.bridge.fetchGitDiff(cleanHost, cleanCwd, tracker.focusPath, staged);
    } catch (err) {
      this.clearGitDiffSettleTimer(tracker);
      tracker.modeState = 'unverified';
      tracker.cachedSnapshot = null;
      orbitLogWarn('OrbitStore', 'fetchScopedGitReview failed', err as Object);
    }
  }

  private adoptInFlightGitReviewFetch(
    tracker: OrbitGitDiffModeTracker,
    snap: GitReviewDto
  ): void {
    this.clearGitDiffSettleTimer(tracker);
    if (snap.status !== undefined) {
      tracker.lastStatus = snap.status;
    }
    const snapEpoch = this.readSnapIdentityEpoch(snap);
    const snapStatusEpoch = this.readSnapStatusIdentityEpoch(snap);
    if (snapEpoch > tracker.lastObservedIdentityEpoch && snapStatusEpoch === snapEpoch) {
      tracker.lastObservedIdentityEpoch = snapEpoch;
    }
    const snapStatusRev = this.readSnapStatusRevision(snap);
    if (snapStatusEpoch === snapEpoch && snapStatusRev > tracker.lastObservedStatusRevision) {
      tracker.lastObservedStatusRevision = snapStatusRev;
    }
    const snapErrRev = this.readSnapDiffErrorRevision(snap);
    if (snapErrRev > tracker.lastHandledErrorRevision) {
      tracker.lastHandledErrorRevision = snapErrRev;
    }
    tracker.dirtyWhileHidden = false;
    tracker.cachedSnapshot = null;
    tracker.confirmedDiffRevision = 0;
    tracker.baselineDiffRevision = 0;
    tracker.pendingErrorRecovery = false;
    tracker.requestSeq += 1;
    const reqSeq = tracker.requestSeq;
    tracker.modeState = 'loading';
    tracker.settleTimerId = setTimeout((): void => {
      if (tracker.requestSeq === reqSeq && tracker.modeState === 'loading') {
        tracker.settleTimerId = -1;
        tracker.modeState = 'unverified';
        tracker.cachedSnapshot = null;
        this.gitPanelRevision += 1;
        this.panelDataRevision += 1;
      }
    }, 1500);
  }

  /**
   * Reconciles a post-mutation scoped Git Review refresh (`R19-N2`, `R24-P1-1`):
   * - Prioritizes returning an already-confirmed positive-revision snapshot (`diff_revision > 0`)
   *   whose `(identity, path, staged)` matches the slot — even when a concurrent same-slot
   *   `diff_in_flight === true` coexists with `status_reverify_pending === true` — so confirmed
   *   diff lines and revision stay stably rendered without clearing cache or arming a wipe timer
   *   while reusing the in-flight request.
   * - Otherwise reuses the single live native in-flight request when `preSnap.diff_in_flight === true`
   *   (zero duplicate UUIDs across core routine refresh, Store callback, and component callback).
   * - Stays silent during `status_reverify_pending === true` when no positive diff is cached yet.
   * - Settles to `unverified` with 0 fetches if status already proves the slot clean.
   * - Otherwise dispatches a single targeted fetch.
   */
  private syncPostMutationGitReview(
    hostId: string,
    cwd: string,
    staged: boolean,
    focusPath: string | null,
    includeStatusIfFetching: boolean = false
  ): GitReviewDto | null {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return null;
    }
    const tracker = this.getOrCreateGitDiffTracker(cleanHost, cleanCwd, focusPath, staged);
    const preSnap = this.safeReadGitReviewSnapshot(cleanHost, cleanCwd, tracker.focusPath, staged);
    if (preSnap !== null) {
      if (preSnap.status !== undefined) {
        tracker.lastStatus = preSnap.status;
      }
      const preEpoch = this.readSnapIdentityEpoch(preSnap);
      const preStatusEpoch = this.readSnapStatusIdentityEpoch(preSnap);
      if (preEpoch > tracker.lastObservedIdentityEpoch && preStatusEpoch === preEpoch) {
        tracker.lastObservedIdentityEpoch = preEpoch;
      }
      const preStatusRev = this.readSnapStatusRevision(preSnap);
      if (preStatusEpoch === preEpoch && preStatusRev > tracker.lastObservedStatusRevision) {
        tracker.lastObservedStatusRevision = preStatusRev;
      }
      const preErrRev = this.readSnapDiffErrorRevision(preSnap);
      if (preErrRev > tracker.lastHandledErrorRevision) {
        tracker.lastHandledErrorRevision = preErrRev;
      }
      const preDiffRev =
        typeof preSnap.diff_revision === 'number' && preSnap.diff_revision > 0
          ? preSnap.diff_revision
          : 0;
      const preFocusPath = this.normalizeGitFocusPath(preSnap.selected_path ?? null);
      const slotMatched =
        preSnap.staged === staged &&
        preFocusPath === tracker.focusPath &&
        preEpoch >= tracker.lastObservedIdentityEpoch;
      if (slotMatched && preDiffRev > 0) {
        if (preEpoch > tracker.lastObservedIdentityEpoch) {
          tracker.lastObservedIdentityEpoch = preEpoch;
        }
        this.clearGitDiffSettleTimer(tracker);
        tracker.dirtyWhileHidden = false;
        tracker.modeState = 'confirmed';
        tracker.baselineDiffRevision = preDiffRev;
        tracker.confirmedDiffRevision = preDiffRev;
        tracker.cachedSnapshot = preSnap;
        tracker.nonErrorLoadingRecoveryBudget = 1;
        tracker.pendingErrorRecovery = false;
        return preSnap;
      }
      if (preSnap.diff_in_flight === true) {
        if (tracker.modeState !== 'loading' || tracker.baselineDiffRevision !== 0) {
          this.adoptInFlightGitReviewFetch(tracker, preSnap);
        }
        return null;
      }
      if (preSnap.status_reverify_pending === true) {
        return null;
      }
      if (
        preSnap.status !== null &&
        preSnap.status !== undefined &&
        !gitStatusMayHaveDiffForSlot(preSnap.status, tracker.focusPath, staged)
      ) {
        this.clearGitDiffSettleTimer(tracker);
        tracker.dirtyWhileHidden = false;
        tracker.modeState = 'unverified';
        tracker.confirmedDiffRevision = 0;
        tracker.baselineDiffRevision = 0;
        tracker.cachedSnapshot = null;
        tracker.pendingErrorRecovery = false;
        return null;
      }
    }
    this.requestScopedGitReviewFetch(
      cleanHost,
      cleanCwd,
      staged,
      focusPath,
      includeStatusIfFetching,
      preSnap
    );
    return null;
  }

  private onScopedGitDirtyArrived(hostId: string, cwd: string): void {
    const cleanHost = hostId.trim();
    const normCwd = normalizeAbsoluteCwd(cwd);
    if (cleanHost.length === 0 || normCwd.length === 0) {
      return;
    }
    // Visibility-scoped evaluation (R5-P1-1): ONLY read `gitReviewJson` for the currently active,
    // visible Review tab's exact `(hostId, normCwd, focusPath, staged)` slot; mark all other
    // trackers dirty with zero synchronous FFI calls.
    this.gitDiffModeTrackers.forEach((tracker: OrbitGitDiffModeTracker): void => {
      if (tracker.hostId !== cleanHost || tracker.normCwd !== normCwd) {
        return;
      }
      if (!this.isTrackerCurrentlyVisibleReviewTab(tracker)) {
        tracker.dirtyWhileHidden = true;
        tracker.cachedSnapshot = null;
        return;
      }
      tracker.dirtyWhileHidden = false;
      const snap = this.safeReadGitReviewSnapshot(
        cleanHost,
        tracker.rawCwd,
        tracker.focusPath,
        tracker.staged
      );
      if (snap === null) {
        return;
      }
      if (snap.status !== undefined) {
        tracker.lastStatus = snap.status;
        if (cleanHost === this.selectedHostId && cwdPathsEquivalent(tracker.rawCwd, this.activeCwd)) {
          this.gitStatus = snap.status;
        }
      }
      const snapDiffRev =
        typeof snap.diff_revision === 'number' && snap.diff_revision > 0 ? snap.diff_revision : 0;
      const snapStatusRev = this.readSnapStatusRevision(snap);
      const snapIdentityEpoch = this.readSnapIdentityEpoch(snap);
      const snapStatusIdentityEpoch = this.readSnapStatusIdentityEpoch(snap);
      const snapErrorRev = this.readSnapDiffErrorRevision(snap);
      const prevObservedStatusRev = tracker.lastObservedStatusRevision;
      if (
        snapStatusIdentityEpoch === snapIdentityEpoch &&
        snapStatusRev > tracker.lastObservedStatusRevision
      ) {
        tracker.lastObservedStatusRevision = snapStatusRev;
      }

      // Rule A (R20-P1-1 / R20-P1-2 / R21-P1-1): Directory identity `(repo_root, repo_prefix)`
      // switched! Immediately drop stale cached payload. Only advance `lastObservedIdentityEpoch`
      // and judge clean-vs-dirty once a matching `GitStatusSnapshot` for `snapIdentityEpoch` has
      // arrived (`snapStatusIdentityEpoch === snapIdentityEpoch`)—so if a whole-repo diff revealed
      // the switch before the new directory's status arrived, this visible slot stays pending
      // until the real new-identity status lands (and still confirms immediately if THIS slot
      // already received a valid post-switch diff).
      if (snapIdentityEpoch > tracker.lastObservedIdentityEpoch) {
        if (snapErrorRev > tracker.lastHandledErrorRevision) {
          tracker.lastHandledErrorRevision = snapErrorRev;
        }
        this.clearGitDiffSettleTimer(tracker);
        tracker.confirmedDiffRevision = 0;
        tracker.baselineDiffRevision = 0;
        tracker.cachedSnapshot = null;
        tracker.pendingErrorRecovery = false;
        tracker.nonErrorLoadingRecoveryBudget = 1;
        if (snap.staged === tracker.staged && snapDiffRev > 0) {
          tracker.lastObservedIdentityEpoch = snapIdentityEpoch;
          tracker.modeState = 'confirmed';
          tracker.baselineDiffRevision = snapDiffRev;
          tracker.confirmedDiffRevision = snapDiffRev;
          tracker.cachedSnapshot = snap;
          return;
        }
        if (snap.diff_in_flight === true) {
          if (snapStatusIdentityEpoch === snapIdentityEpoch) {
            tracker.lastObservedIdentityEpoch = snapIdentityEpoch;
          }
          this.adoptInFlightGitReviewFetch(tracker, snap);
          return;
        }
        if (snapStatusIdentityEpoch !== snapIdentityEpoch) {
          tracker.modeState = 'unverified';
          return;
        }
        tracker.lastObservedIdentityEpoch = snapIdentityEpoch;
        if (gitStatusMayHaveDiffForSlot(snap.status, tracker.focusPath, tracker.staged)) {
          this.requestScopedGitReviewFetch(
            cleanHost,
            tracker.rawCwd,
            tracker.staged,
            tracker.focusPath,
            false,
            snap
          );
        } else {
          tracker.modeState = 'unverified';
        }
        return;
      }

      // Rule B (R20-P1-5 / R21-P1-3): Native recorded a new terminal `RpcError` for this exact
      // slot. Register pending recovery unconditionally—decoupled from 1.5s UI timeout AND
      // non-zero prior `diff_revision`—anchoring `errorObservedStatusRevision` to native
      // `last_diff_error_status_revision` (recorded at the exact moment `RpcError` settled in
      // Rust). Fall through directly into Rule C in the SAME coalesced frame so an error +
      // post-error status flushed together immediately trigger the single recovery fetch, while a
      // pre-error status + error flushed together (`status_revision === errorObservedStatusRevision`)
      // stay unverified and wait for the next status!
      let errorJustRegistered = false;
      if (snapErrorRev > tracker.lastHandledErrorRevision) {
        tracker.lastHandledErrorRevision = snapErrorRev;
        tracker.errorObservedStatusRevision = this.readSnapDiffErrorStatusRevision(
          snap,
          snapStatusRev
        );
        tracker.pendingErrorRecovery = true;
        errorJustRegistered = true;
        if (tracker.modeState === 'loading' && snap.diff_in_flight !== true) {
          this.clearGitDiffSettleTimer(tracker);
          tracker.modeState = 'unverified';
          tracker.cachedSnapshot = null;
        }
      }

      // Fresh diff confirmation strictly newer than baseline.
      if (snap.staged === tracker.staged && snapDiffRev > tracker.baselineDiffRevision) {
        this.clearGitDiffSettleTimer(tracker);
        tracker.modeState = 'confirmed';
        tracker.baselineDiffRevision = snapDiffRev;
        tracker.confirmedDiffRevision = snapDiffRev;
        tracker.cachedSnapshot = snap;
        tracker.nonErrorLoadingRecoveryBudget = 1;
        tracker.pendingErrorRecovery = false;
        return;
      }

      // Rule C (R20-P1-5 / R20-P1-6 / R21-P1-3): Consume the single post-error recovery opportunity
      // IF AND ONLY IF a strictly newer valid `GitStatusSnapshot` arrived AFTER the error settled
      // (`snap.status_revision > tracker.errorObservedStatusRevision`). Works identically across
      // separate frames, same-frame coalesced flushes, post-1.5s timeout, and non-zero prior diff.
      if (tracker.pendingErrorRecovery && snapStatusRev > tracker.errorObservedStatusRevision) {
        tracker.pendingErrorRecovery = false;
        if (gitStatusMayHaveDiffForSlot(snap.status, tracker.focusPath, tracker.staged)) {
          if (snap.diff_in_flight === true) {
            this.adoptInFlightGitReviewFetch(tracker, snap);
          } else {
            this.requestScopedGitReviewFetch(
              cleanHost,
              tracker.rawCwd,
              tracker.staged,
              tracker.focusPath,
              false,
              snap,
              false
            );
          }
        } else {
          this.clearGitDiffSettleTimer(tracker);
          tracker.modeState = 'unverified';
          tracker.cachedSnapshot = null;
        }
        return;
      }

      if (errorJustRegistered) {
        return;
      }

      if (
        tracker.modeState === 'confirmed' &&
        snap.staged === tracker.staged &&
        snapDiffRev > 0 &&
        snapDiffRev >= tracker.confirmedDiffRevision
      ) {
        tracker.cachedSnapshot = snap;
        return;
      }

      if (tracker.modeState === 'confirmed' && snapDiffRev === 0) {
        // R17-P1-3 / R17-P1-4 / R17-P1-5 / R18-N1 / R19-P1-1: Native core invalidated this
        // visible slot's cached diff. Drop stale body immediately; reuse native re-dispatch when
        // `diff_in_flight === true`, or dispatch once if status may have a diff; else `unverified`.
        this.clearGitDiffSettleTimer(tracker);
        tracker.confirmedDiffRevision = 0;
        tracker.baselineDiffRevision = 0;
        tracker.cachedSnapshot = null;
        tracker.pendingErrorRecovery = false;
        if (gitStatusMayHaveDiffForSlot(snap.status, tracker.focusPath, tracker.staged)) {
          if (snap.diff_in_flight === true) {
            this.adoptInFlightGitReviewFetch(tracker, snap);
          } else {
            this.requestScopedGitReviewFetch(
              cleanHost,
              tracker.rawCwd,
              tracker.staged,
              tracker.focusPath,
              false,
              snap
            );
          }
        } else {
          tracker.modeState = 'unverified';
        }
        return;
      }

      if (tracker.modeState === 'loading' && snapDiffRev === 0 && snap.diff_in_flight === false) {
        // R18-P1-3: Non-error invalidation of a visible `loading` slot with no live native
        // re-dispatch: allow at most 1 bounded recovery fetch when status still has changes.
        this.clearGitDiffSettleTimer(tracker);
        tracker.confirmedDiffRevision = 0;
        tracker.baselineDiffRevision = 0;
        tracker.cachedSnapshot = null;
        if (
          gitStatusMayHaveDiffForSlot(snap.status, tracker.focusPath, tracker.staged) &&
          tracker.nonErrorLoadingRecoveryBudget > 0
        ) {
          tracker.nonErrorLoadingRecoveryBudget -= 1;
          this.requestScopedGitReviewFetch(
            cleanHost,
            tracker.rawCwd,
            tracker.staged,
            tracker.focusPath,
            false,
            snap,
            false
          );
        } else {
          tracker.modeState = 'unverified';
        }
        return;
      }

      // Rule D (R22-P1-2): A previously cleared/unverified visible slot (`diff_revision === 0`,
      // no pending error recovery) receives a strictly newer matching-identity `GitStatusSnapshot`
      // (`snapStatusRev > prevObservedStatusRev`, e.g. post-completion `GetGitStatus` recheck
      // after a delayed Stage ack temporarily overwrote `[true, false]` with `[true]`): adopt any
      // live in-flight request or dispatch strictly ONE targeted fetch if this slot now has a diff
      // (while clean statuses stay `unverified` with 0 fetches and non-advancing `status_revision`
      // events stay silent).
      if (
        (tracker.modeState === 'unverified' || tracker.modeState === 'idle') &&
        !tracker.pendingErrorRecovery &&
        snapDiffRev === 0 &&
        snapStatusIdentityEpoch === snapIdentityEpoch &&
        snapStatusRev > prevObservedStatusRev &&
        gitStatusMayHaveDiffForSlot(snap.status, tracker.focusPath, tracker.staged)
      ) {
        if (snap.diff_in_flight === true) {
          this.adoptInFlightGitReviewFetch(tracker, snap);
        } else {
          this.requestScopedGitReviewFetch(
            cleanHost,
            tracker.rawCwd,
            tracker.staged,
            tracker.focusPath,
            false,
            snap,
            false
          );
        }
      }
    });
  }

  public getScopedGitDiffModeState(
    hostId: string,
    cwd: string,
    staged: boolean,
    focusPath: string | null
  ): OrbitGitDiffModeState {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (cleanHost.length === 0 || cleanCwd.length === 0) {
      return 'idle';
    }
    const tracker = this.gitDiffModeTrackers.get(
      this.gitDiffTrackerKey(cleanHost, cleanCwd, focusPath, staged)
    );
    return tracker !== undefined ? tracker.modeState : 'idle';
  }

  public isScopedGitDiffConfirmed(
    hostId: string,
    cwd: string,
    staged: boolean,
    focusPath: string | null
  ): boolean {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    const tracker = this.gitDiffModeTrackers.get(
      this.gitDiffTrackerKey(cleanHost, cleanCwd, focusPath, staged)
    );
    return tracker !== undefined && !tracker.dirtyWhileHidden && tracker.modeState === 'confirmed';
  }

  public loadScopedGitReview(
    hostId: string,
    cwd: string,
    staged: boolean,
    focusPath: string | null,
    triggerRemoteFetch: boolean = false,
    includeStatus: boolean = true,
    reuseInFlight: boolean = false
  ): GitReviewDto | null {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return null;
    }
    const tracker = this.getOrCreateGitDiffTracker(cleanHost, cleanCwd, focusPath, staged);
    const emptySnapshot: GitReviewDto = {
      revision: 0,
      host_id: cleanHost,
      cwd: cleanCwd,
      status: tracker.lastStatus,
      selected_path: tracker.focusPath,
      staged: staged,
      diff_revision: 0,
      raw_diff: '',
      diff_files: []
    };
    if (triggerRemoteFetch || tracker.dirtyWhileHidden) {
      if (reuseInFlight && !tracker.dirtyWhileHidden) {
        const adoptedOrConfirmed = this.syncPostMutationGitReview(
          cleanHost,
          cleanCwd,
          staged,
          focusPath,
          includeStatus
        );
        return adoptedOrConfirmed !== null ? adoptedOrConfirmed : emptySnapshot;
      }
      this.requestScopedGitReviewFetch(
        cleanHost,
        cleanCwd,
        staged,
        focusPath,
        includeStatus || tracker.dirtyWhileHidden
      );
      return emptySnapshot;
    }
    if (tracker.modeState !== 'confirmed') {
      return emptySnapshot;
    }
    if (tracker.cachedSnapshot !== null) {
      return tracker.cachedSnapshot;
    }
    const parsed = this.safeReadGitReviewSnapshot(cleanHost, cleanCwd, tracker.focusPath, staged);
    if (parsed !== null) {
      tracker.lastStatus = parsed.status;
      if (parsed.staged === staged && parsed.diff_revision > 0) {
        tracker.cachedSnapshot = parsed;
        return parsed;
      }
    }
    return emptySnapshot;
  }

  public async loadScopedGitCollabStatus(
    hostId: string,
    cwd: string
  ): Promise<OrbitScopedGitCollabData> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const empty: OrbitScopedGitCollabData = {
      prStatus: null,
      stashes: [],
      conflicts: [],
      rebaseStatus: null
    };
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return empty;
    }
    let prStatus: GitPrStatusDto | null = null;
    let stashes: GitStashEntryDto[] = [];
    let conflicts: GitConflictEntryDto[] = [];
    let rebaseStatus: GitRebaseStatusDto | null = null;
    try {
      prStatus = JSON.parse(await this.bridge.gitPrStatusJson(cleanHost, cleanCwd)) as GitPrStatusDto;
    } catch (_e) {
      prStatus = null;
    }
    try {
      stashes = JSON.parse(await this.bridge.gitStashListJson(cleanHost, cleanCwd)) as GitStashEntryDto[];
    } catch (_e) {
      stashes = [];
    }
    try {
      conflicts = JSON.parse(await this.bridge.gitConflictsJson(cleanHost, cleanCwd)) as GitConflictEntryDto[];
    } catch (_e) {
      conflicts = [];
    }
    try {
      rebaseStatus = JSON.parse(await this.bridge.gitRebaseStatusJson(cleanHost, cleanCwd)) as GitRebaseStatusDto;
    } catch (_e) {
      rebaseStatus = null;
    }
    return {
      prStatus: prStatus,
      stashes: stashes,
      conflicts: conflicts,
      rebaseStatus: rebaseStatus
    };
  }

  private resolveActiveReviewStagedMode(hostId: string, cwd: string, fallback: boolean): boolean {
    const activeTab = this.sidePanelStore.activeTab;
    if (
      activeTab !== null &&
      activeTab.kind === 'review' &&
      activeTab.hostId === hostId &&
      activeTab.cwd !== null &&
      cwdPathsEquivalent(activeTab.cwd, cwd)
    ) {
      return activeTab.gitStaged === true;
    }
    return fallback;
  }

  private resolveActiveReviewFocusPath(hostId: string, cwd: string): string | null {
    const activeTab = this.sidePanelStore.activeTab;
    if (
      activeTab !== null &&
      activeTab.kind === 'review' &&
      activeTab.hostId === hostId &&
      activeTab.cwd !== null &&
      cwdPathsEquivalent(activeTab.cwd, cwd)
    ) {
      return activeTab.path;
    }
    return null;
  }

  public async gitStageScopedPaths(
    hostId: string,
    cwd: string,
    paths: string[],
    all: boolean
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    try {
      await this.bridge.gitStagePathsJson(cleanHost, cleanCwd, paths, all);
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        false
      );
      this.gitPanelRevision += 1;
      this.showToast(all ? 'Staged all changes' : 'Staged file', false);
      this.triggerHapticPreset('selection');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitStageScopedPaths failed', err as Object);
      this.showToast((err as Error).message || 'Stage failed', true);
      return false;
    }
  }

  public async gitUnstageScopedPaths(
    hostId: string,
    cwd: string,
    paths: string[],
    all: boolean
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    try {
      await this.bridge.gitUnstagePathsJson(cleanHost, cleanCwd, paths, all);
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, true),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        false
      );
      this.gitPanelRevision += 1;
      this.showToast(all ? 'Unstaged all changes' : 'Unstaged file', false);
      this.triggerHapticPreset('selection');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitUnstageScopedPaths failed', err as Object);
      this.showToast((err as Error).message || 'Unstage failed', true);
      return false;
    }
  }

  public async gitGenerateScopedCommitMessage(hostId: string, cwd: string): Promise<string> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return '';
    }
    try {
      const json = await this.bridge.gitGenerateCommitMessageJson(cleanHost, cleanCwd);
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
      orbitLogWarn('OrbitStore', 'gitGenerateScopedCommitMessage failed', err as Object);
      this.showToast((err as Error).message || 'Unable to generate commit message', true);
      return '';
    }
  }

  public async gitCommitScoped(
    hostId: string,
    cwd: string,
    message: string,
    amend: boolean
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanMsg = message.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || cleanMsg.length === 0) {
      return false;
    }
    try {
      const json = await this.bridge.gitCommitJson(cleanHost, cleanCwd, cleanMsg, amend);
      const res = JSON.parse(json) as GitCommitResultDto;
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        false
      );
      this.gitPanelRevision += 1;
      this.showToast(`Committed ${res.short_sha}`, false);
      this.triggerHapticPreset('success');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitCommitScoped failed', err as Object);
      this.showToast((err as Error).message || 'Commit failed', true);
      return false;
    }
  }

  public async gitCreateScopedPr(
    hostId: string,
    cwd: string,
    title: string,
    body: string,
    base: string | null,
    draft: boolean
  ): Promise<GitCreatePrResultDto | null> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanTitle = title.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || cleanTitle.length === 0) {
      return null;
    }
    try {
      const json = await this.bridge.gitCreatePrJson(
        cleanHost,
        cleanCwd,
        cleanTitle,
        body.trim(),
        base !== null && base.trim().length > 0 ? base.trim() : null,
        draft
      );
      const res = JSON.parse(json) as GitCreatePrResultDto;
      this.gitPanelRevision += 1;
      this.showToast(`Created PR: ${res.title}`, false);
      this.triggerHapticPreset('success');
      return res;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitCreateScopedPr failed', err as Object);
      this.showToast((err as Error).message || 'Create PR failed', true);
      return null;
    }
  }

  public async gitSaveScopedStash(
    hostId: string,
    cwd: string,
    message: string | null,
    includeUntracked: boolean
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    try {
      await this.bridge.gitStashSaveJson(cleanHost, cleanCwd, message, includeUntracked);
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        false
      );
      this.gitPanelRevision += 1;
      this.showToast('Saved working tree stash', false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Stash failed', true);
      return false;
    }
  }

  public async gitPopScopedStash(
    hostId: string,
    cwd: string,
    stashRef: string,
    expectedOid: string | null
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || stashRef.trim().length === 0) {
      return false;
    }
    try {
      await this.bridge.gitStashPopJson(cleanHost, cleanCwd, stashRef, expectedOid);
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        false
      );
      this.gitPanelRevision += 1;
      this.showToast(`Popped ${stashRef}`, false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Pop stash failed', true);
      return false;
    }
  }

  public async gitRebaseContinueScoped(hostId: string, cwd: string): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    try {
      await this.bridge.gitRebaseContinueJson(cleanHost, cleanCwd);
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        false
      );
      this.gitPanelRevision += 1;
      this.showToastResource($r('app.string.toast_rebase_continued'), false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Rebase continue failed', true);
      return false;
    }
  }

  public async gitRebaseAbortScoped(hostId: string, cwd: string): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    try {
      await this.bridge.gitRebaseAbortJson(cleanHost, cleanCwd);
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        false
      );
      this.gitPanelRevision += 1;
      this.showToastResource($r('app.string.toast_rebase_aborted'), false);
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Rebase abort failed', true);
      return false;
    }
  }

  public async gitPushScoped(
    hostId: string,
    cwd: string,
    remote: string | null,
    branch: string | null,
    setUpstream: boolean
  ): Promise<OrbitScopedActionOutcome<GitPushResultDto>> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      const cleanRemote = remote !== null && remote.trim().length > 0 ? remote.trim() : null;
      const cleanBranch = branch !== null && branch.trim().length > 0 ? branch.trim() : null;
      const req: GitPushRequestDto = {
        host_id: cleanHost,
        cwd: cleanCwd,
        remote: cleanRemote,
        branch: cleanBranch,
        set_upstream: setUpstream
      };
      const raw = await this.bridge.gitPushJson(JSON.stringify(req));
      const res = JSON.parse(raw) as GitPushResultDto;
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        true
      );
      this.gitPanelRevision += 1;
      return { ok: true, data: res, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitPushScoped failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  public async gitDiscardScopedFiles(
    hostId: string,
    cwd: string,
    paths: string[],
    includeUntracked: boolean
  ): Promise<OrbitScopedActionOutcome<number>> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || paths.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    const validPaths: string[] = [];
    for (let i = 0; i < paths.length; i++) {
      // Preserve literal trailing spaces on paths (never call .trim() on non-empty file paths).
      if (paths[i].length > 0 && paths[i].trim().length > 0) {
        validPaths.push(paths[i]);
      }
    }
    if (validPaths.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      const req: GitDiscardFilesRequestDto = {
        host_id: cleanHost,
        cwd: cleanCwd,
        paths: validPaths,
        include_untracked: includeUntracked
      };
      await this.bridge.gitDiscardFilesJson(JSON.stringify(req));
      this.syncPostMutationGitReview(
        cleanHost,
        cleanCwd,
        this.resolveActiveReviewStagedMode(cleanHost, cleanCwd, false),
        this.resolveActiveReviewFocusPath(cleanHost, cleanCwd),
        true
      );
      this.gitPanelRevision += 1;
      return { ok: true, data: validPaths.length, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'gitDiscardScopedFiles failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  public highlightScopedWorkspaceCode(
    language: string,
    code: string,
    maxLines: number = 250
  ): HighlightedCodeDto | null {
    if (this.bridge === null) {
      return null;
    }
    try {
      const capped = Math.max(1, Math.min(250, Math.floor(maxLines)));
      // R2-P1-3: Pre-clip oversized inputs on safe UTF-16 surrogate boundaries BEFORE
      // JSON.stringify so 16 MiB single-line bundles complete in < 5 ms without copying 16 MiB JSON.
      const clipped = clipCodeForSyncHighlightPreview(code, capped);
      const req: HighlightWorkspaceCodeRequestDto = {
        language: language,
        code: clipped.clippedCode,
        max_lines: capped,
        plain_line_numbers: clipped.plainLineNumbers.length > 0 ? clipped.plainLineNumbers : undefined
      };
      const raw = this.bridge.highlightWorkspaceCodeJson(JSON.stringify(req));
      const parsed = JSON.parse(raw) as HighlightedCodeDto;
      if (clipped.preTruncated) {
        return {
          language: parsed.language,
          lines: parsed.lines,
          total_lines: clipped.totalLines > 0
            ? Math.max(parsed.total_lines, clipped.totalLines)
            : parsed.total_lines,
          total_lines_exact: clipped.totalLinesExact && parsed.total_lines_exact !== false,
          truncated: true
        };
      }
      return {
        language: parsed.language,
        lines: parsed.lines,
        total_lines: parsed.total_lines,
        total_lines_exact: parsed.total_lines_exact !== false,
        truncated: parsed.truncated
      };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'highlightScopedWorkspaceCode failed', err as Object);
      return null;
    }
  }

  // --- Scoped Workspace Files & Protected File Save Helpers ---

  public loadScopedWorkspaceFiles(
    hostId: string,
    cwd: string,
    triggerRemoteFetch: boolean = false
  ): string[] {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return [];
    }
    if (triggerRemoteFetch) {
      try {
        this.bridge.fetchWorkspaceFiles(cleanHost, cleanCwd);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'fetchScopedWorkspaceFiles failed', err as Object);
      }
    }
    try {
      return JSON.parse(this.bridge.workspaceFilesJson(cleanHost, cleanCwd)) as string[];
    } catch (err) {
      orbitLogWarn('OrbitStore', 'loadScopedWorkspaceFiles failed', err as Object);
      return [];
    }
  }

  public async readScopedWorkspaceFile(
    hostId: string,
    cwd: string,
    path: string
  ): Promise<WorkspaceFileViewDto | null> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanPath = nonBlankPath(path);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || cleanPath.length === 0) {
      return null;
    }
    try {
      const json = await this.bridge.readWorkspaceFileJson(cleanHost, cleanCwd, cleanPath);
      return JSON.parse(json) as WorkspaceFileViewDto;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'readScopedWorkspaceFile failed', err as Object);
      return null;
    }
  }

  public async readScopedWorkspaceFileChunk(
    req: ReadWorkspaceFileRequestDto
  ): Promise<OrbitScopedActionOutcome<WorkspaceFileViewDto>> {
    const cleanHost = req.host_id.trim();
    const cleanCwd = nonBlankPath(req.cwd);
    const cleanPath = nonBlankPath(req.path);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || cleanPath.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      const payload: ReadWorkspaceFileRequestDto = {
        host_id: cleanHost,
        cwd: cleanCwd,
        path: cleanPath,
        offset_bytes: req.offset_bytes !== undefined && req.offset_bytes !== null && req.offset_bytes >= 0
          ? Math.floor(req.offset_bytes)
          : null,
        max_bytes: req.max_bytes !== undefined && req.max_bytes !== null && req.max_bytes > 0
          ? Math.floor(req.max_bytes)
          : null,
        expected_etag: req.expected_etag !== undefined && req.expected_etag !== null && req.expected_etag.trim().length > 0
          ? req.expected_etag.trim()
          : null
      };
      const json = await this.bridge.readWorkspaceFileRequestJson(JSON.stringify(payload));
      const view = JSON.parse(json) as WorkspaceFileViewDto;
      return { ok: true, data: view, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'readScopedWorkspaceFileChunk failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  /**
   * Concurrency-safe workspace file write (P0-2):
   * Sends `expected_etag` with `force: false` by default and inspects `WorkspaceFileWriteResultDto.conflict`.
   * Only passes `force: true` when the user explicitly confirms overwriting a detected conflict.
   */
  public async saveScopedWorkspaceFile(
    hostId: string,
    cwd: string,
    path: string,
    content: string,
    expectedEtag: string | null = null,
    force: boolean = false
  ): Promise<OrbitWorkspaceFileWriteOutcome> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanPath = nonBlankPath(path);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || cleanPath.length === 0) {
      return {
        ok: false,
        conflict: false,
        etag: '',
        currentContent: null,
        errorMessage: 'Target file is unavailable'
      };
    }
    try {
      const req: WriteWorkspaceFileRequestDto = {
        host_id: cleanHost,
        cwd: cleanCwd,
        path: cleanPath,
        content: content,
        expected_etag: expectedEtag,
        force: force
      };
      const rawResult = await this.bridge.writeWorkspaceFileJson(JSON.stringify(req));
      if (rawResult === undefined || rawResult === null || rawResult.trim().length === 0) {
        this.showToast('Save failed: empty write receipt from host', true);
        return {
          ok: false,
          conflict: false,
          etag: '',
          currentContent: null,
          errorMessage: 'Empty write receipt from host.'
        };
      }
      let parsed: WorkspaceFileWriteResultDto | null = null;
      try {
        const candidate: Object | null = JSON.parse(rawResult) as Object | null;
        if (candidate !== null && typeof candidate === 'object' && !Array.isArray(candidate)) {
          parsed = candidate as WorkspaceFileWriteResultDto;
        }
      } catch (_parseErr) {
        parsed = null;
      }
      if (parsed === null) {
        this.showToast('Save failed: invalid write receipt from host', true);
        return {
          ok: false,
          conflict: false,
          etag: '',
          currentContent: null,
          errorMessage: 'Invalid write receipt from host.'
        };
      }
      // Validate common receipt target `(host_id, cwd, path)` AND u64-safe `size_bytes` FIRST
      // BEFORE branching on `conflict` vs success so wrong-target or malformed conflict receipts
      // NEVER surface a conflict banner or unlock force-overwrite (R3-P1-1).
      const receiptHost = typeof parsed.host_id === 'string' ? parsed.host_id.trim() : '';
      const receiptCwd = typeof parsed.cwd === 'string' ? nonBlankPath(parsed.cwd) : '';
      const receiptPath = typeof parsed.path === 'string' ? nonBlankPath(parsed.path) : '';
      const receiptSizeValid = typeof parsed.size_bytes === 'number' &&
        Number.isSafeInteger(parsed.size_bytes) &&
        parsed.size_bytes >= 0;
      const targetMatches =
        receiptHost.length > 0 &&
        receiptCwd.length > 0 &&
        receiptPath.length > 0 &&
        receiptHost === cleanHost &&
        cwdPathsEquivalent(receiptCwd, cleanCwd) &&
        normalizeWorkspaceRelPath(receiptPath, cleanCwd) === normalizeWorkspaceRelPath(cleanPath, cleanCwd);
      if (!targetMatches || !receiptSizeValid) {
        const reason = !targetMatches && receiptHost.length > 0
          ? 'Write receipt target mismatch.'
          : 'Invalid write receipt from host.';
        this.showToast(`Save failed: ${reason}`, true);
        return {
          ok: false,
          conflict: false,
          etag: '',
          currentContent: null,
          errorMessage: reason
        };
      }
      if (parsed.conflict !== undefined && parsed.conflict !== null) {
        const conflictObj: Object = parsed.conflict as Object;
        const conflictIsObj = typeof conflictObj === 'object' && !Array.isArray(conflictObj);
        const conflictEtag = conflictIsObj && typeof parsed.conflict.current_etag === 'string'
          ? parsed.conflict.current_etag.trim()
          : '';
        const conflictSizeValid = conflictIsObj &&
          typeof parsed.conflict.current_size_bytes === 'number' &&
          Number.isSafeInteger(parsed.conflict.current_size_bytes) &&
          parsed.conflict.current_size_bytes >= 0;
        if (!conflictIsObj || conflictEtag.length === 0 || !conflictSizeValid) {
          this.showToast('Save failed: malformed conflict receipt from host', true);
          return {
            ok: false,
            conflict: false,
            etag: '',
            currentContent: null,
            errorMessage: 'Malformed conflict receipt from host.'
          };
        }
        this.showToast('Save conflict: file modified on host', true);
        return {
          ok: false,
          conflict: true,
          etag: conflictEtag,
          currentContent: parsed.conflict.current_content ?? null,
          errorMessage: 'File changed on host since it was opened.'
        };
      }
      const receiptEtag = typeof parsed.etag === 'string' ? parsed.etag.trim() : '';
      if (receiptEtag.length === 0) {
        const reason = 'Incomplete write receipt from host.';
        this.showToast(`Save failed: ${reason}`, true);
        return {
          ok: false,
          conflict: false,
          etag: '',
          currentContent: null,
          errorMessage: reason
        };
      }
      this.showToast(`Saved ${basenameOfPath(cleanPath)}`, false);
      this.triggerHapticPreset('success');
      return {
        ok: true,
        conflict: false,
        etag: receiptEtag,
        currentContent: null,
        errorMessage: ''
      };
    } catch (err) {
      const msg = (err as Error).message || 'Save failed';
      orbitLogWarn('OrbitStore', 'saveScopedWorkspaceFile failed', err as Object);
      this.showToast(msg, true);
      return {
        ok: false,
        conflict: false,
        etag: '',
        currentContent: null,
        errorMessage: msg
      };
    }
  }

  public async createScopedWorkspaceEntry(
    hostId: string,
    cwd: string,
    path: string,
    isDir: boolean,
    initialContent: string | null
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanPath = nonBlankPath(path);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || cleanPath.length === 0) {
      return false;
    }
    try {
      const req: CreateWorkspaceEntryRequestDto = {
        host_id: cleanHost,
        cwd: cleanCwd,
        path: cleanPath,
        is_dir: isDir,
        initial_content: initialContent
      };
      await this.bridge.createWorkspaceEntryJson(JSON.stringify(req));
      this.bridge.fetchWorkspaceFiles(cleanHost, cleanCwd);
      this.gitPanelRevision += 1;
      this.showToast(`Created ${basenameOfPath(cleanPath)}`, false);
      this.triggerHapticPreset('selection');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createScopedWorkspaceEntry failed', err as Object);
      this.showToast((err as Error).message || 'Create failed', true);
      return false;
    }
  }

  public async renameScopedWorkspaceEntry(
    hostId: string,
    cwd: string,
    oldPath: string,
    newPath: string
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanOld = nonBlankPath(oldPath);
    const cleanNew = nonBlankPath(newPath);
    if (
      this.bridge === null ||
      cleanHost.length === 0 ||
      cleanCwd.length === 0 ||
      cleanOld.length === 0 ||
      cleanNew.length === 0
    ) {
      return false;
    }
    try {
      const req: RenameWorkspaceEntryRequestDto = {
        host_id: cleanHost,
        cwd: cleanCwd,
        old_path: cleanOld,
        new_path: cleanNew
      };
      await this.bridge.renameWorkspaceEntryJson(JSON.stringify(req));
      this.bridge.fetchWorkspaceFiles(cleanHost, cleanCwd);
      this.gitPanelRevision += 1;
      this.showToast(`Renamed to ${basenameOfPath(cleanNew)}`, false);
      this.triggerHapticPreset('selection');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'renameScopedWorkspaceEntry failed', err as Object);
      this.showToast((err as Error).message || 'Rename failed', true);
      return false;
    }
  }

  public async deleteScopedWorkspaceEntry(
    hostId: string,
    cwd: string,
    path: string
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanPath = nonBlankPath(path);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0 || cleanPath.length === 0) {
      return false;
    }
    try {
      const req: DeleteWorkspaceEntryRequestDto = {
        host_id: cleanHost,
        cwd: cleanCwd,
        path: cleanPath
      };
      await this.bridge.deleteWorkspaceEntryJson(JSON.stringify(req));
      this.bridge.fetchWorkspaceFiles(cleanHost, cleanCwd);
      this.gitPanelRevision += 1;
      this.showToast(`Deleted ${basenameOfPath(cleanPath)}`, false);
      this.triggerHapticPreset('selection');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'deleteScopedWorkspaceEntry failed', err as Object);
      this.showToast((err as Error).message || 'Delete failed', true);
      return false;
    }
  }

  /**
   * Quotes a file viewed inside a scoped side-panel tab into the composer (P1-6):
   * - When the file belongs to the active session's exact `(selectedHostId, activeCwd)`, attaches it as a
   *   regular workspace file badge.
   * - When the file comes from a different host or project directory, inserts an explicit `file_quote`
   *   carrying the already-loaded file content and `(hostId, cwd, path)` origin so `composePromptViaCore`
   *   never resolves a same-named file from the current project!
   */
  public quoteScopedFileIntoComposer(
    hostId: string,
    cwd: string,
    filePath: string,
    loadedContent: string
  ): void {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const cleanPath = nonBlankPath(filePath);
    if (cleanHost.length === 0 || cleanPath.length === 0) {
      return;
    }
    const relPath = cleanCwd.length > 0 && cleanPath.startsWith(`${cleanCwd}/`)
      ? cleanPath.slice(cleanCwd.length + 1)
      : cleanPath;
    if (
      cleanHost === this.selectedHostId &&
      cleanCwd.length > 0 &&
      this.activeCwd.trim().length > 0 &&
      cwdPathsEquivalent(cleanCwd, this.activeCwd)
    ) {
      this.attachFileBadge(relPath);
      return;
    }
    const originPath = cleanCwd.length > 0
      ? `${cleanHost}:${cleanCwd.replace(/\/+$/, '')}/${relPath.replace(/^\/+/, '')}`
      : `${cleanHost}:${relPath}`;
    const trimmedContent = loadedContent.length > 6000
      ? `${loadedContent.slice(0, 6000)}\n/* … truncated … */`
      : loadedContent;
    const next: QuotedDiffSnippet = {
      id: `file_quote_${Date.now()}_${Math.floor(Math.random() * 1000)}`,
      path: originPath,
      lineLabel: basenameOfPath(relPath),
      content: trimmedContent.length > 0 ? trimmedContent : `(empty file: ${originPath})`,
      kind: 'file_quote'
    };
    this.quotedSnippets = this.quotedSnippets.concat([next]);
    this.showToast(`Quoted ${basenameOfPath(relPath)} into composer`, false);
    this.triggerHapticPreset('selection');
  }

  // --- Scoped PTY Terminal Helpers ---

  public loadScopedTerminalScreen(hostId: string, slot: number): TerminalScreenDto | null {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return null;
    }
    try {
      const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
      const raw = this.bridge.terminalScreenSlotJson(cleanHost, normalizedSlot);
      return JSON.parse(raw) as TerminalScreenDto | null;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'loadScopedTerminalScreen failed', err as Object);
      return null;
    }
  }

  public loadScopedTerminalDelta(
    hostId: string,
    slot: number,
    sinceRevision: number
  ): TerminalScreenDeltaDto | null {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return null;
    }
    try {
      const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
      const safeSince = sinceRevision > 0 ? Math.floor(sinceRevision) : 0;
      const raw = this.bridge.terminalScreenDeltaSlotJson(cleanHost, normalizedSlot, safeSince);
      return JSON.parse(raw) as TerminalScreenDeltaDto | null;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'loadScopedTerminalDelta failed', err as Object);
      return null;
    }
  }

  public sendScopedTerminalBytes(hostId: string, slot: number, bytes: Uint8Array): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || bytes.length === 0) {
      return;
    }
    try {
      const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
      this.bridge.sendTerminalBytes(cleanHost, normalizedSlot, bytes);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'sendScopedTerminalBytes failed', err as Object);
    }
  }

  public sendScopedTerminalUtf8(hostId: string, slot: number, raw: string): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || raw.length === 0) {
      return;
    }
    try {
      const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
      const encoder = new util.TextEncoder();
      const bytes = encoder.encodeInto(raw);
      this.bridge.sendTerminalBytes(cleanHost, normalizedSlot, bytes);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'sendScopedTerminalUtf8 failed', err as Object);
    }
  }

  private clearScopedTerminalGridCache(hostId: string, slot?: number): void {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return;
    }
    if (slot !== undefined) {
      const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
      this.scopedTerminalGridByKey.delete(encodeSidePanelKeyTuple(['pty_grid', cleanHost, `${normalizedSlot}`]));
      return;
    }
    const prefix = encodeSidePanelKeyTuple(['pty_grid', cleanHost, '']).slice(0, -3);
    const toDelete: string[] = [];
    this.scopedTerminalGridByKey.forEach((_v: string, k: string): void => {
      if (k.startsWith(prefix)) {
        toDelete.push(k);
      }
    });
    for (let i = 0; i < toDelete.length; i++) {
      this.scopedTerminalGridByKey.delete(toDelete[i]);
    }
  }

  public resizeScopedTerminalIfChanged(
    hostId: string,
    slot: number,
    cols: number,
    rows: number
  ): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return;
    }
    const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
    const clampedCols = Math.max(24, Math.min(240, Math.floor(cols)));
    const clampedRows = Math.max(8, Math.min(120, Math.floor(rows)));
    const key = encodeSidePanelKeyTuple(['pty_grid', cleanHost, `${normalizedSlot}`]);
    const sig = `${clampedCols}x${clampedRows}`;
    if (this.scopedTerminalGridByKey.get(key) === sig) {
      return;
    }
    try {
      this.bridge.resizeTerminalSlot(cleanHost, normalizedSlot, clampedCols, clampedRows);
      // Cache dimensions ONLY after native resize succeeds so a failed attempt can be retried (P1-8).
      this.scopedTerminalGridByKey.set(key, sig);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'resizeScopedTerminalIfChanged failed', err as Object);
    }
  }

  public startScopedTerminal(
    hostId: string,
    cwd: string,
    cols?: number,
    rows?: number,
    slot: number = 1
  ): void {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (this.bridge === null || cleanHost.length === 0) {
      return;
    }
    try {
      const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
      this.bridge.startTerminalSlot(
        cleanHost,
        cleanCwd.length > 0 ? cleanCwd : '~',
        cols !== undefined && cols >= 24 ? Math.floor(cols) : this.terminalCols,
        rows !== undefined && rows >= 8 ? Math.floor(rows) : this.terminalGridRows,
        normalizedSlot
      );
      this.clearScopedTerminalGridCache(cleanHost, normalizedSlot);
      this.triggerHapticPreset('action');
    } catch (err) {
      orbitLogWarn('OrbitStore', 'startScopedTerminal failed', err as Object);
      this.showToastResource($r('app.string.terminal_start_failed'), true);
    }
  }

  public attachScopedTerminal(hostId: string, terminalId: string, slot: number): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || terminalId.trim().length === 0) {
      return;
    }
    try {
      const normalizedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
      this.bridge.attachTerminal(cleanHost, terminalId, normalizedSlot);
      this.clearScopedTerminalGridCache(cleanHost, normalizedSlot);
      this.terminalPanelRevision += 1;
      this.triggerHapticPreset('selection');
    } catch (err) {
      orbitLogWarn('OrbitStore', 'attachScopedTerminal failed', err as Object);
    }
  }

  public killScopedTerminal(hostId: string, terminalId: string): void {
    const cleanHost = hostId.trim();
    const cleanTermId = terminalId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanTermId.length === 0) {
      return;
    }
    try {
      this.bridge.killTerminal(cleanHost, cleanTermId);
      this.clearScopedTerminalGridCache(cleanHost);
      this.reloadHostAsync(cleanHost);
      this.terminalPanelRevision += 1;
      this.showToastResource($r('app.string.terminal_closed_toast'), false);
      this.triggerHapticPreset('action');
    } catch (err) {
      orbitLogWarn('OrbitStore', 'killScopedTerminal failed', err as Object);
      this.showToastResource($r('app.string.terminal_close_failed'), true);
    }
  }

  public extractTerminalScreenPlainText(screen: TerminalScreenDto | null): string {
    if (screen === null || screen.rows_data.length === 0) {
      return '';
    }
    const lines: string[] = [];
    for (let i = 0; i < screen.rows_data.length; i++) {
      let line = '';
      const spans = screen.rows_data[i].spans;
      for (let j = 0; j < spans.length; j++) {
        line += spans[j].text;
      }
      lines.push(line.replace(/\s+$/, ''));
    }
    while (lines.length > 0 && lines[lines.length - 1].length === 0) {
      lines.pop();
    }
    return lines.join('\n');
  }

  public quoteScopedTerminalIntoComposer(screen: TerminalScreenDto | null, cwd: string): void {
    const snapshot = this.extractTerminalScreenPlainText(screen);
    if (snapshot.length === 0) {
      this.showToast('Terminal buffer is empty', true);
      return;
    }
    const cleanCwd = nonBlankPath(cwd);
    const label = cleanCwd.length > 0 ? `terminal:${basenameOfPath(cleanCwd)}` : 'terminal';
    const next: QuotedDiffSnippet = {
      id: `term_${Date.now()}`,
      path: label,
      lineLabel: label,
      content: snapshot,
      kind: 'terminal'
    };
    this.quotedSnippets = this.quotedSnippets.concat([next]);
    this.showToast('Quoted terminal output into composer', false);
    this.triggerHapticPreset('selection');
  }

  // --- Scoped Mission Workspace Board & Peers Helpers (P0-3 + R2-P0-2 + P1-8) ---

  private verifyScopedWorkspace(
    detail: HostDetailDto,
    cwd: string,
    explicitWorkspaceId: string,
    agentId: string | null
  ): WorkspaceDto | null {
    const cleanCwd = nonBlankPath(cwd);
    const cleanAgent = agentId !== null ? agentId.trim() : '';
    const explicitWs = explicitWorkspaceId.trim();

    let agentFound = false;
    let agentWorkspaceId = '';
    let agentWorkspace: WorkspaceDto | null = null;
    if (cleanAgent.length > 0) {
      for (let i = 0; i < detail.agents.length; i++) {
        const ag = detail.agents[i];
        if (ag.agent_id === cleanAgent) {
          agentFound = true;
          if (ag.workspace_id !== null && ag.workspace_id.trim().length > 0) {
            agentWorkspaceId = ag.workspace_id.trim();
          }
          break;
        }
      }
      // If a tab declares an agentId, that agent card MUST be present in authoritative host detail
      // so ownership can be verified — NEVER let a persisted explicitWorkspaceId bypass missing agent cards (R3-P0-1).
      if (!agentFound) {
        return null;
      }
      if (agentWorkspaceId.length > 0) {
        for (let i = 0; i < detail.workspaces.length; i++) {
          if (detail.workspaces[i].workspace_id === agentWorkspaceId) {
            agentWorkspace = detail.workspaces[i];
            break;
          }
        }
        // Fail closed if agent references a workspace ID missing from authoritative host detail.
        if (agentWorkspace === null) {
          return null;
        }
      }
    }

    // Collect ALL exact full-normalized-cwd candidates (NEVER stripping /home/<user> prefixes, R3-P0-1).
    const cwdMatches: WorkspaceDto[] = [];
    if (cleanCwd.length > 0) {
      for (let i = 0; i < detail.workspaces.length; i++) {
        if (cwdPathsEquivalent(detail.workspaces[i].cwd, cleanCwd, null)) {
          cwdMatches.push(detail.workspaces[i]);
        }
      }
    }

    // Independent agent ownership proof: verify cwd & explicitWorkspaceId agree without conflict.
    if (agentWorkspace !== null) {
      if (explicitWs.length > 0 && explicitWs !== agentWorkspace.workspace_id) {
        return null;
      }
      if (cleanCwd.length > 0 && !cwdPathsEquivalent(agentWorkspace.cwd, cleanCwd, null)) {
        return null;
      }
      return agentWorkspace;
    }

    // Without independent agent ownership proof (draft tab or standalone non-workspace agent):
    // persisted `explicitWorkspaceId` MUST NOT break ties among multiple same-cwd workspaces (R3-P0-1).
    if (cleanCwd.length > 0) {
      if (cwdMatches.length !== 1) {
        return null;
      }
      const uniqueMatch = cwdMatches[0];
      if (explicitWs.length > 0 && uniqueMatch.workspace_id !== explicitWs) {
        return null;
      }
      return uniqueMatch;
    }

    // No agent ownership AND no cwd: allow explicitWs ONLY when the host has a single unambiguous workspace
    // sharing that workspace's cwd.
    if (explicitWs.length > 0) {
      let explicitMatch: WorkspaceDto | null = null;
      for (let i = 0; i < detail.workspaces.length; i++) {
        if (detail.workspaces[i].workspace_id === explicitWs) {
          explicitMatch = detail.workspaces[i];
          break;
        }
      }
      if (explicitMatch === null) {
        return null;
      }
      let sameCwdCount = 0;
      for (let i = 0; i < detail.workspaces.length; i++) {
        if (cwdPathsEquivalent(detail.workspaces[i].cwd, explicitMatch.cwd, null)) {
          sameCwdCount += 1;
        }
      }
      return sameCwdCount === 1 ? explicitMatch : null;
    }

    return null;
  }

  public loadScopedWorkspaceBoard(
    hostId: string,
    cwd: string,
    workspaceId: string,
    agentId: string | null,
    triggerRemoteFetch: boolean = false
  ): OrbitScopedBoardData {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    const detail = this.getScopedHostDetail(cleanHost);

    // R2-P0-2: Never trust a persisted/stale explicit workspaceId while host detail is missing (cold cache).
    if (detail === null) {
      if (triggerRemoteFetch && cleanHost.length > 0) {
        this.ensureScopedHostDetailAsync(cleanHost);
      }
      return {
        workspace: null,
        workspaceId: '',
        workspaceVerified: false,
        snapshotLoaded: false,
        content: '',
        revision: 0,
        updatedBy: '',
        boardPath: '',
        peers: [],
        events: []
      };
    }

    const workspace = this.verifyScopedWorkspace(detail, cleanCwd, workspaceId, agentId);
    const resolvedWsId = workspace !== null ? workspace.workspace_id : '';
    const workspaceVerified = workspace !== null && resolvedWsId.length > 0;

    const peers: AgentCardDto[] = [];
    if (workspaceVerified) {
      for (let i = 0; i < detail.agents.length; i++) {
        const ag = detail.agents[i];
        const archived = ag.archived_at_rfc3339 !== undefined &&
          ag.archived_at_rfc3339 !== null &&
          ag.archived_at_rfc3339.length > 0;
        if (ag.workspace_id === resolvedWsId && !archived && ag.status.state !== 'closed') {
          peers.push(ag);
        }
      }
    }

    let snapshotLoaded = false;
    let content = '';
    let revision = 0;
    let updatedBy = '';
    let boardPath = '';
    let events: WorkspaceEventRecordDto[] = [];

    if (this.bridge !== null && cleanHost.length > 0 && workspaceVerified) {
      if (triggerRemoteFetch) {
        try {
          this.bridge.fetchWorkspaceBoard(cleanHost, resolvedWsId);
          this.bridge.fetchWorkspaceEvents(cleanHost, resolvedWsId, 30);
        } catch (err) {
          orbitLogWarn('OrbitStore', 'fetchScopedWorkspaceBoard failed', err as Object);
        }
      }
      try {
        const snapJson = this.bridge.workspaceBoardJson(cleanHost, resolvedWsId);
        if (snapJson !== undefined && snapJson !== null && snapJson.trim().length > 0) {
          const parsedSnap: Object | null = JSON.parse(snapJson) as Object | null;
          if (parsedSnap !== null && typeof parsedSnap === 'object') {
            const snap = parsedSnap as WorkspaceBoardSnapshotDto;
            if (typeof snap.content === 'string') {
              snapshotLoaded = true;
              content = snap.content;
              revision = typeof snap.revision === 'number' ? snap.revision : 0;
              updatedBy = snap.updated_by ?? '';
              boardPath = snap.board_path ?? '';
            }
          }
        }
      } catch (err) {
        snapshotLoaded = false;
        orbitLogWarn('OrbitStore', 'loadScopedWorkspaceBoard snapshot failed', err as Object);
      }
      try {
        const eventsJson = this.bridge.workspaceEventsJson(cleanHost, resolvedWsId);
        if (eventsJson !== undefined && eventsJson !== null && eventsJson.trim().length > 0) {
          const parsedEvents: Object | null = JSON.parse(eventsJson) as Object | null;
          if (Array.isArray(parsedEvents)) {
            events = parsedEvents as WorkspaceEventRecordDto[];
          }
        }
      } catch (err) {
        orbitLogWarn('OrbitStore', 'loadScopedWorkspaceBoard events failed', err as Object);
      }
    }

    return {
      workspace: workspace,
      workspaceId: resolvedWsId,
      workspaceVerified: workspaceVerified,
      snapshotLoaded: snapshotLoaded,
      content: content,
      revision: revision,
      updatedBy: updatedBy,
      boardPath: boardPath,
      peers: peers,
      events: events
    };
  }

  public getWorkspaceBoardDraft(targetKey: string): OrbitWorkspaceBoardDraft | null {
    const clean = targetKey.trim();
    if (clean.length === 0) {
      return null;
    }
    const found = this.workspaceBoardDrafts.get(clean);
    if (found === undefined) {
      return null;
    }
    return {
      content: found.content,
      baseRevision: found.baseRevision
    };
  }

  public setWorkspaceBoardDraft(targetKey: string, content: string, baseRevision: number): void {
    const clean = targetKey.trim();
    if (clean.length === 0) {
      return;
    }
    this.workspaceBoardDrafts.set(clean, {
      content: content,
      baseRevision: Math.max(0, Math.floor(baseRevision))
    });
  }

  public clearWorkspaceBoardDraft(targetKey: string): void {
    const clean = targetKey.trim();
    if (clean.length === 0) {
      return;
    }
    this.workspaceBoardDrafts.delete(clean);
  }

  public isHostBoardCasSupported(hostId: string): boolean {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return false;
    }
    const detail = this.getScopedHostDetail(cleanHost);
    if (detail !== null && detail.host !== undefined && detail.host !== null) {
      return detail.host.board_cas_supported === true;
    }
    for (let i = 0; i < this.hosts.length; i++) {
      if (this.hosts[i].host_id === cleanHost) {
        return this.hosts[i].board_cas_supported === true;
      }
    }
    return false;
  }

  public async saveScopedWorkspaceBoardWithCas(
    hostId: string,
    workspaceId: string,
    content: string,
    expectedRevision: number,
    force: boolean = false,
    expectedCwd: string = '',
    expectedAgentId: string | null = null
  ): Promise<OrbitScopedBoardSaveOutcome> {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0) {
      return {
        ok: false,
        conflict: false,
        currentRevision: 0,
        currentContent: '',
        errorMessage: '',
        errorCode: 'unbound'
      };
    }
    const detail = this.getScopedHostDetail(cleanHost);
    if (detail === null) {
      return {
        ok: false,
        conflict: false,
        currentRevision: 0,
        currentContent: '',
        errorMessage: '',
        errorCode: 'loading'
      };
    }
    const verified = this.verifyScopedWorkspace(detail, expectedCwd, cleanWs, expectedAgentId);
    if (verified === null || verified.workspace_id !== cleanWs) {
      return {
        ok: false,
        conflict: false,
        currentRevision: 0,
        currentContent: '',
        errorMessage: '',
        errorCode: 'changed'
      };
    }
    let hasLoadedSnapshot = false;
    let liveRevision = 0;
    let liveContent = '';
    try {
      const snapJson = this.bridge.workspaceBoardJson(cleanHost, cleanWs);
      if (snapJson !== undefined && snapJson !== null && snapJson.trim().length > 0) {
        const parsedSnap: Object | null = JSON.parse(snapJson) as Object | null;
        if (parsedSnap !== null && typeof parsedSnap === 'object') {
          const snap = parsedSnap as WorkspaceBoardSnapshotDto;
          if (typeof snap.content === 'string') {
            hasLoadedSnapshot = true;
            liveContent = snap.content;
            liveRevision = typeof snap.revision === 'number' ? snap.revision : 0;
          }
        }
      }
    } catch (_err) {
      hasLoadedSnapshot = false;
    }
    if (!hasLoadedSnapshot) {
      return {
        ok: false,
        conflict: false,
        currentRevision: 0,
        currentContent: '',
        errorMessage: '',
        errorCode: 'snapshot_not_loaded'
      };
    }
    const detailBoard = detail.workspace_boards !== undefined && detail.workspace_boards !== null
      ? detail.workspace_boards[cleanWs]
      : undefined;
    if (detailBoard !== undefined && typeof detailBoard.revision === 'number' && detailBoard.revision > liveRevision) {
      liveRevision = detailBoard.revision;
      if (typeof detailBoard.content === 'string') {
        liveContent = detailBoard.content;
      }
    }
    // R2-P0-A: Refuse guarded CAS writes locally BEFORE calling native RPC when the connected
    // daemon did not advertise `board_cas_supported === true`, preserving both daemon & draft.
    if (!force && detail.host.board_cas_supported !== true) {
      return {
        ok: false,
        conflict: false,
        currentRevision: liveRevision,
        currentContent: liveContent,
        errorMessage: '',
        errorCode: 'cas_unsupported'
      };
    }
    if (!force && hasBoardCasConflict(expectedRevision, liveRevision)) {
      return {
        ok: false,
        conflict: true,
        currentRevision: liveRevision,
        currentContent: liveContent,
        errorMessage: '',
        errorCode: 'cas_conflict'
      };
    }
    try {
      const req: UpdateWorkspaceBoardCasRequestDto = {
        host_id: cleanHost,
        workspace_id: cleanWs,
        content: content,
        section: null,
        updated_by: 'harmony-user',
        expected_revision: force ? null : Math.max(0, Math.floor(expectedRevision))
      };
      const raw = await this.bridge.updateWorkspaceBoardCasJson(JSON.stringify(req));
      const parsed = JSON.parse(raw) as WorkspaceBoardCasResultDto;
      const returnedBoard = parsed.board;
      const returnedRev = returnedBoard !== undefined && returnedBoard !== null && typeof returnedBoard.revision === 'number'
        ? returnedBoard.revision
        : liveRevision;
      const returnedContent = returnedBoard !== undefined && returnedBoard !== null && typeof returnedBoard.content === 'string'
        ? returnedBoard.content
        : liveContent;
      this.panelDataRevision += 1;
      if (parsed.conflict) {
        return {
          ok: false,
          conflict: true,
          currentRevision: returnedRev,
          currentContent: returnedContent,
          errorMessage: '',
          errorCode: 'cas_conflict'
        };
      }
      if (parsed.ok) {
        return {
          ok: true,
          conflict: false,
          currentRevision: returnedRev,
          currentContent: returnedContent,
          errorMessage: '',
          errorCode: ''
        };
      }
      return {
        ok: false,
        conflict: false,
        currentRevision: returnedRev,
        currentContent: returnedContent,
        errorMessage: '',
        errorCode: 'rpc_error'
      };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'saveScopedWorkspaceBoardWithCas failed', err as Object);
      const detailMsg = err instanceof Error ? err.message.trim() : String(err).trim();
      const isCasUnsupported = detailMsg.toLowerCase().indexOf('board_cas_unsupported') >= 0;
      return {
        ok: false,
        conflict: false,
        currentRevision: liveRevision,
        currentContent: liveContent,
        errorMessage: detailMsg,
        errorCode: isCasUnsupported ? 'cas_unsupported' : 'rpc_error'
      };
    }
  }

  public async saveScopedWorkspaceBoard(
    hostId: string,
    workspaceId: string,
    content: string,
    expectedCwd: string = '',
    expectedAgentId: string | null = null
  ): Promise<boolean> {
    const res = await this.saveScopedWorkspaceBoardWithCas(
      hostId,
      workspaceId,
      content,
      0,
      true,
      expectedCwd,
      expectedAgentId
    );
    return res.ok;
  }

  // --- Worktree Handoff, Workspace v2 & Dev Scripts Helpers (PR-5) ---

  public async previewScopedWorktreeHandoff(
    req: PreviewWorktreeHandoffRequestDto
  ): Promise<OrbitScopedActionOutcome<WorktreeHandoffPreviewDto>> {
    const cleanHost = req.host_id.trim();
    const cleanWs = req.workspace_id.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      const payload: PreviewWorktreeHandoffRequestDto = {
        host_id: cleanHost,
        workspace_id: cleanWs,
        source_agent_id: req.source_agent_id !== undefined && req.source_agent_id !== null && req.source_agent_id.trim().length > 0
          ? req.source_agent_id.trim()
          : null,
        direction: req.direction,
        target_workspace_id: req.target_workspace_id !== undefined && req.target_workspace_id !== null && req.target_workspace_id.trim().length > 0
          ? req.target_workspace_id.trim()
          : null,
        include_committed: req.include_committed
      };
      const raw = await this.bridge.previewWorktreeHandoffJson(JSON.stringify(payload));
      const preview = JSON.parse(raw) as WorktreeHandoffPreviewDto;
      return { ok: true, data: preview, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'previewScopedWorktreeHandoff failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  public async executeScopedWorktreeHandoff(
    req: ExecuteWorktreeHandoffRequestDto
  ): Promise<OrbitScopedActionOutcome<WorktreeHandoffOutcomeDto>> {
    const cleanHost = req.host_id.trim();
    const cleanWs = req.workspace_id.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      const payload: ExecuteWorktreeHandoffRequestDto = {
        host_id: cleanHost,
        workspace_id: cleanWs,
        source_agent_id: req.source_agent_id !== undefined && req.source_agent_id !== null && req.source_agent_id.trim().length > 0
          ? req.source_agent_id.trim()
          : null,
        direction: req.direction,
        mode: req.mode,
        target_workspace_id: req.target_workspace_id !== undefined && req.target_workspace_id !== null && req.target_workspace_id.trim().length > 0
          ? req.target_workspace_id.trim()
          : null,
        include_committed: req.include_committed,
        branch: req.branch !== undefined && req.branch !== null && req.branch.trim().length > 0
          ? req.branch.trim()
          : null,
        allow_three_way_conflicts: req.allow_three_way_conflicts
      };
      const raw = await this.bridge.executeWorktreeHandoffJson(JSON.stringify(payload));
      const outcome = JSON.parse(raw) as WorktreeHandoffOutcomeDto;
      this.reloadHostAsync(cleanHost);
      this.gitPanelRevision += 1;
      this.panelDataRevision += 1;
      return { ok: true, data: outcome, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'executeScopedWorktreeHandoff failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  public createWorkspaceV2(req: CreateWorkspaceV2RequestDto): boolean {
    const cleanHost = req.host_id.trim();
    const cleanCwd = nonBlankPath(req.source_cwd);
    if (this.bridge === null || cleanHost.length === 0 || cleanCwd.length === 0) {
      return false;
    }
    try {
      const payload: CreateWorkspaceV2RequestDto = {
        host_id: cleanHost,
        source_cwd: cleanCwd,
        title: req.title !== undefined && req.title !== null && req.title.trim().length > 0 ? req.title.trim() : null,
        isolation: req.isolation ?? 'local',
        worktree_mode: req.worktree_mode ?? 'branch_off',
        worktree_branch: req.worktree_branch !== undefined && req.worktree_branch !== null && req.worktree_branch.trim().length > 0
          ? req.worktree_branch.trim()
          : null,
        base_ref: req.base_ref !== undefined && req.base_ref !== null && req.base_ref.trim().length > 0
          ? req.base_ref.trim()
          : null,
        pr_number: req.pr_number !== undefined && req.pr_number !== null && req.pr_number > 0
          ? Math.floor(req.pr_number)
          : null
      };
      this.bridge.createWorkspaceV2Json(JSON.stringify(payload));
      this.reloadHostAsync(cleanHost);
      this.panelDataRevision += 1;
      this.showToastResource($r('app.string.workspace_create_dispatched'), false);
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createWorkspaceV2 failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      this.showToastResource(
        detail.length > 0
          ? $r('app.string.workspace_create_failed_detail', detail)
          : $r('app.string.workspace_create_failed'),
        true,
        detail
      );
      return false;
    }
  }

  public async archiveWorkspaceV2(
    hostId: string,
    workspaceId: string,
    keepAgentsUnclassified: boolean
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      await this.bridge.archiveWorkspaceV2Json(cleanHost, cleanWs, keepAgentsUnclassified);
      this.reloadHostAsync(cleanHost);
      this.panelDataRevision += 1;
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'archiveWorkspaceV2 failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  public async unarchiveWorkspace(
    hostId: string,
    workspaceId: string
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      await this.bridge.unarchiveWorkspaceJson(cleanHost, cleanWs);
      this.reloadHostAsync(cleanHost);
      this.panelDataRevision += 1;
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'unarchiveWorkspace failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  public async renameWorkspace(
    hostId: string,
    workspaceId: string,
    title: string
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    const cleanTitle = title.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0 || cleanTitle.length === 0) {
      return { ok: false, data: null, errorDetail: '' };
    }
    try {
      await this.bridge.renameWorkspaceConfirmedJson(cleanHost, cleanWs, cleanTitle);
      this.reloadHostAsync(cleanHost);
      this.panelDataRevision += 1;
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'renameWorkspace failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      return { ok: false, data: null, errorDetail: detail };
    }
  }

  public requestWorkspaceScripts(hostId: string, workspaceId: string): boolean {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0) {
      return false;
    }
    try {
      this.bridge.requestWorkspaceScripts(cleanHost, cleanWs);
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'requestWorkspaceScripts failed', err as Object);
      return false;
    }
  }

  public getWorkspaceScripts(hostId: string, workspaceId: string): WorkspaceScriptInfoDto[] {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (cleanHost.length === 0 || cleanWs.length === 0) {
      return [];
    }
    const detail = this.getScopedHostDetail(cleanHost);
    if (detail === null || detail.workspace_scripts === undefined || detail.workspace_scripts === null) {
      return [];
    }
    const list = detail.workspace_scripts[cleanWs];
    return Array.isArray(list) ? list : [];
  }

  public startWorkspaceScript(hostId: string, workspaceId: string, scriptName: string): boolean {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    const cleanName = scriptName.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0 || cleanName.length === 0) {
      return false;
    }
    try {
      this.bridge.startWorkspaceScriptJson(cleanHost, cleanWs, cleanName);
      this.triggerHapticPreset('selection');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'startWorkspaceScript failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      this.showToastResource(
        detail.length > 0
          ? $r('app.string.scripts_start_failed_detail', detail)
          : $r('app.string.scripts_start_failed'),
        true,
        detail
      );
      return false;
    }
  }

  public stopWorkspaceScript(hostId: string, workspaceId: string, scriptName: string): boolean {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    const cleanName = scriptName.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0 || cleanName.length === 0) {
      return false;
    }
    try {
      this.bridge.stopWorkspaceScriptJson(cleanHost, cleanWs, cleanName);
      this.triggerHapticPreset('selection');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'stopWorkspaceScript failed', err as Object);
      const detail = err instanceof Error ? err.message.trim() : String(err).trim();
      this.showToastResource(
        detail.length > 0
          ? $r('app.string.scripts_stop_failed_detail', detail)
          : $r('app.string.scripts_stop_failed'),
        true,
        detail
      );
      return false;
    }
  }

  public resolveProjectWorkspaces(
    hostId: string,
    projectCwd: string,
    projectDirectories?: string[],
    targetGroupId?: string
  ): WorkspaceDto[] {
    const cleanHost = hostId.trim();
    const detail = this.getScopedHostDetail(cleanHost);
    if (detail === null) {
      return [];
    }
    const primary = nonBlankPath(projectCwd);
    const fallbackDirs: string[] = [];
    if (primary.length > 0) {
      fallbackDirs.push(primary);
    }
    if (projectDirectories !== undefined && projectDirectories !== null) {
      for (let i = 0; i < projectDirectories.length; i++) {
        const d = nonBlankPath(projectDirectories[i]);
        if (d.length > 0 && fallbackDirs.indexOf(d) < 0) {
          fallbackDirs.push(d);
        }
      }
    }
    const hostGroups: OrbitProjectDirectoryGroupInput[] = [];
    let resolvedTargetId = (targetGroupId ?? '').trim();
    const groups = this.sidebarView !== null && Array.isArray(this.sidebarView.projects)
      ? this.sidebarView.projects
      : [];
    let matchedSidebarGroupWorkspaceIds: string[] | null = null;
    for (let i = 0; i < groups.length; i++) {
      const g = groups[i];
      if (g.host_id !== cleanHost) {
        continue;
      }
      hostGroups.push({
        id: g.id,
        primaryCwd: g.primary_cwd,
        directories: g.directories
      });
      const isTargetMatch = resolvedTargetId.length > 0
        ? g.id === resolvedTargetId
        : (primary.length > 0 && cwdPathsEquivalent(g.primary_cwd, primary, null));
      if (isTargetMatch) {
        if (resolvedTargetId.length === 0) {
          resolvedTargetId = g.id;
        }
        if (Array.isArray(g.workspace_ids)) {
          matchedSidebarGroupWorkspaceIds = g.workspace_ids;
        }
      }
    }
    // R3-P1-2: Prefer authoritative Core `SidebarProjectGroupView.workspace_ids` projection
    // directly when available so HarmonyOS project menus NEVER drift from Rust `sidebar.rs`.
    if (matchedSidebarGroupWorkspaceIds !== null) {
      const idSet = new Set<string>(matchedSidebarGroupWorkspaceIds);
      const projected: WorkspaceDto[] = [];
      for (let i = 0; i < detail.workspaces.length; i++) {
        const ws = detail.workspaces[i];
        if (idSet.has(ws.workspace_id)) {
          projected.push(ws);
        }
      }
      return projected;
    }
    if (resolvedTargetId.length === 0) {
      if (fallbackDirs.length === 0) {
        return [];
      }
      resolvedTargetId = `__fallback__::${cleanHost}::${fallbackDirs[0]}`;
      hostGroups.push({
        id: resolvedTargetId,
        primaryCwd: fallbackDirs[0],
        directories: fallbackDirs
      });
    } else {
      let targetExists = false;
      for (let i = 0; i < hostGroups.length; i++) {
        if (hostGroups[i].id === resolvedTargetId) {
          targetExists = true;
          break;
        }
      }
      if (!targetExists && fallbackDirs.length > 0) {
        hostGroups.push({
          id: resolvedTargetId,
          primaryCwd: fallbackDirs[0],
          directories: fallbackDirs
        });
      }
    }
    return resolveAuthoritativeProjectWorkspaces(detail.workspaces, hostGroups, resolvedTargetId);
  }

  public resolveActiveWorktreeWorkspace(): OrbitWorktreeHandoffContext | null {
    const hostId = this.selectedHostId.trim();
    if (hostId.length === 0) {
      return null;
    }
    const detail = this.getScopedHostDetail(hostId);
    if (detail === null) {
      return null;
    }
    const agent = this.currentAgent;
    const explicitWs = agent !== null && agent.workspace_id !== null ? agent.workspace_id.trim() : '';
    const cwd = agent !== null && nonBlankPath(agent.cwd).length > 0 ? agent.cwd : this.activeCwd;
    const ws = this.verifyScopedWorkspace(detail, cwd, explicitWs, agent !== null ? agent.agent_id : null);
    if (ws === null) {
      return null;
    }
    return {
      hostId: hostId,
      workspaceId: ws.workspace_id,
      sourceAgentId: agent !== null ? agent.agent_id : null,
      cwd: ws.cwd,
      branch: ws.git_branch ?? '',
      isWorktree: ws.is_worktree,
      title: ws.title
    };
  }

  public openWorktreeHandoffSheet(
    hostId: string,
    workspaceId: string,
    sourceAgentId: string | null,
    cwd: string,
    title: string = ''
  ): void {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (cleanHost.length === 0 || cleanWs.length === 0) {
      return;
    }
    this.shellStore.openModal({
      kind: 'worktree_handoff',
      hostId: cleanHost,
      workspaceId: cleanWs,
      agentId: sourceAgentId !== null ? sourceAgentId : undefined,
      cwd: cwd,
      title: title
    });
  }

  public openWorkspaceScriptsSheet(
    hostId: string,
    workspaceId: string,
    title: string = ''
  ): void {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (cleanHost.length === 0 || cleanWs.length === 0) {
      return;
    }
    // WorkspaceScriptsSheet owns automatic ListWorkspaceScripts dispatch on appear / target change (P1-6).
    this.shellStore.openModal({
      kind: 'workspace_scripts',
      hostId: cleanHost,
      workspaceId: cleanWs,
      title: title
    });
  }

  public openCreateWorkspaceV2Sheet(
    hostId: string,
    sourceCwd: string,
    title: string = ''
  ): void {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(sourceCwd);
    if (cleanHost.length === 0 || cleanCwd.length === 0) {
      return;
    }
    this.shellStore.openModal({
      kind: 'create_workspace_v2',
      hostId: cleanHost,
      cwd: cleanCwd,
      title: title
    });
  }

  public openRenameWorkspaceSheet(
    hostId: string,
    workspaceId: string,
    currentTitle: string = ''
  ): void {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    if (cleanHost.length === 0 || cleanWs.length === 0) {
      return;
    }
    this.shellStore.openModal({
      kind: 'rename_workspace',
      hostId: cleanHost,
      workspaceId: cleanWs,
      title: currentTitle
    });
  }

  public spawnScopedWorkspacePeer(
    hostId: string,
    workspaceId: string,
    cwd: string,
    handle: string,
    prompt: string,
    provider: string = ''
  ): boolean {
    const cleanHost = hostId.trim();
    const cleanWs = workspaceId.trim();
    const cleanPrompt = prompt.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanWs.length === 0 || cleanPrompt.length === 0) {
      this.showToast(cleanWs.length === 0 ? 'No workspace bound to this tab' : 'Enter instructions for the peer', true);
      return false;
    }
    const cleanHandle = handle.trim();
    const detail = this.getScopedHostDetail(cleanHost);
    let resolvedProvider = provider.trim();
    if (resolvedProvider.length === 0 && detail !== null && detail.host.available_providers.length > 0) {
      resolvedProvider = detail.host.available_providers[0];
    }
    if (resolvedProvider.length === 0) {
      resolvedProvider = this.composerProvider;
    }
    const req: SpawnWorkspacePeerRequestDto = {
      host_id: cleanHost,
      workspace_id: cleanWs,
      cwd: nonBlankPath(cwd).length > 0 ? nonBlankPath(cwd) : '~',
      provider: resolvedProvider,
      model: null,
      effort: null,
      mode: normalizeModeSlug(this.composerMode),
      initial_prompt: cleanPrompt,
      handle: cleanHandle.length > 0 ? cleanHandle : null
    };
    try {
      const optId = this.bridge.spawnWorkspacePeerJson(JSON.stringify(req));
      this.reloadHostAsync(cleanHost);
      if (optId.length > 0) {
        this.showToast(
          cleanHandle.length > 0 ? `Spawned @${cleanHandle}` : 'Spawned workspace peer',
          false
        );
      }
      this.panelDataRevision += 1;
      this.triggerHapticPreset('success');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'spawnScopedWorkspacePeer failed', err as Object);
      this.showToast((err as Error).message || 'Failed to spawn peer', true);
      return false;
    }
  }

  // --- Unified Scoped Side Question (/btw) State Machine (P1-2) ---

  private allocateSideQuestionToken(): number {
    this.sideQuestionTokenSeq += 1;
    return this.sideQuestionTokenSeq;
  }

  private buildSideQuestionTab(
    scopeKey: string,
    hostId: string,
    agentId: string | null,
    cwd: string
  ): OrbitSidePanelTab {
    return {
      tabId: canonicalSideQuestionTabId(scopeKey),
      kind: 'side_question',
      hostId: hostId,
      agentId: agentId,
      cwd: nonBlankPath(cwd).length > 0 ? nonBlankPath(cwd) : null,
      path: null,
      parentHostId: hostId,
      title: 'Side Q&A',
      closable: true,
      parentAgentId: agentId
    };
  }

  private resolveModelSnapshotForScope(hostId: string, agentId: string | null): [string, string | null] {
    if (agentId !== null && agentId.trim().length > 0) {
      const card = this.findAgentCardOnHost(hostId, agentId.trim());
      if (card !== null && card.provider.trim().length > 0) {
        const modelVal = (card.model !== null && card.model.trim().length > 0) ? card.model.trim() : null;
        return [card.provider.trim(), modelVal];
      }
    }
    const detail = this.getScopedHostDetail(hostId);
    const defaultProvider = (
      detail !== null &&
      detail.host.available_providers.length > 0 &&
      hostId !== this.selectedHostId
    )
      ? detail.host.available_providers[0]
      : this.composerProvider;
    const defaultModel = hostId === this.selectedHostId && this.composerModel.trim().length > 0
      ? this.composerModel.trim()
      : null;
    return [defaultProvider, defaultModel];
  }

  public getScopedSideQuestionState(
    scopeKey: string,
    hostId: string,
    agentId: string | null,
    cwd: string
  ): OrbitScopedSideQuestionState {
    const _rev = this.sideQuestionRevision;
    if (_rev < 0) {
      // keep reactive subscription
    }
    const existing = this.sideQuestionByScope.get(scopeKey);
    if (existing !== undefined) {
      return existing;
    }
    const modelSnap = this.resolveModelSnapshotForScope(hostId, agentId);
    return {
      scopeKey: scopeKey,
      hostId: hostId,
      agentId: agentId,
      cwd: cwd,
      provider: modelSnap[0],
      model: modelSnap[1],
      draft: '',
      prompt: '',
      answer: '',
      error: '',
      streaming: false,
      requestId: '',
      requestToken: 0,
      generation: 0
    };
  }

  public setScopedSideQuestionDraft(
    scopeKey: string,
    hostId: string,
    agentId: string | null,
    cwd: string,
    draft: string
  ): void {
    const cur = this.getScopedSideQuestionState(scopeKey, hostId, agentId, cwd);
    if (cur.draft === draft && this.sideQuestionByScope.has(scopeKey)) {
      return;
    }
    const next: OrbitScopedSideQuestionState = {
      scopeKey: scopeKey,
      hostId: hostId,
      agentId: agentId,
      cwd: cwd,
      provider: cur.provider,
      model: cur.model,
      draft: draft,
      prompt: cur.prompt,
      answer: cur.answer,
      error: cur.error,
      streaming: cur.streaming,
      requestId: cur.requestId,
      requestToken: cur.requestToken,
      generation: cur.generation
    };
    this.sideQuestionByScope.set(scopeKey, next);
    this.sideQuestionRevision += 1;
  }

  public clearScopedSideQuestion(scopeKey: string): void {
    const cur = this.sideQuestionByScope.get(scopeKey);
    if (cur === undefined) {
      return;
    }
    if (cur.requestToken > 0) {
      this.activeSideQuestionScopeByToken.delete(cur.requestToken);
    }
    const cleared: OrbitScopedSideQuestionState = {
      scopeKey: cur.scopeKey,
      hostId: cur.hostId,
      agentId: cur.agentId,
      cwd: cur.cwd,
      provider: cur.provider,
      model: cur.model,
      draft: '',
      prompt: '',
      answer: '',
      error: '',
      streaming: false,
      requestId: '',
      requestToken: 0,
      generation: cur.generation + 1
    };
    this.sideQuestionByScope.set(scopeKey, cleared);
    this.sideQuestionRevision += 1;
  }

  public revokeScopedSideQuestion(scopeKey: string): void {
    const cleanScope = scopeKey.trim();
    if (cleanScope.length === 0) {
      return;
    }
    const cur = this.sideQuestionByScope.get(cleanScope);
    if (cur !== undefined && cur.requestToken > 0) {
      this.activeSideQuestionScopeByToken.delete(cur.requestToken);
    }
    const staleTokens: number[] = [];
    this.activeSideQuestionScopeByToken.forEach((ownerKey: string, tok: number): void => {
      if (ownerKey === cleanScope) {
        staleTokens.push(tok);
      }
    });
    for (let i = 0; i < staleTokens.length; i++) {
      this.activeSideQuestionScopeByToken.delete(staleTokens[i]);
    }
    if (this.sideQuestionByScope.delete(cleanScope)) {
      this.sideQuestionRevision += 1;
    }
  }

  public dropScopeAndRevokeSideQuestion(scopeKey: string): void {
    this.revokeScopedSideQuestion(scopeKey);
    this.sidePanelStore.dropScope(scopeKey);
  }

  private revokeSideQuestionsForHost(hostId: string): void {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return;
    }
    const targetScopes: string[] = [];
    this.sideQuestionByScope.forEach((state: OrbitScopedSideQuestionState, sk: string): void => {
      if (state.hostId === cleanHost || sk === `draft::${cleanHost}` || sk.startsWith(`${cleanHost}::`)) {
        targetScopes.push(sk);
      }
    });
    for (let i = 0; i < targetScopes.length; i++) {
      this.revokeScopedSideQuestion(targetScopes[i]);
    }
  }

  private remapScopedSideQuestion(
    oldScopeKey: string,
    newScopeKey: string,
    newAgentId: string,
    newCwd?: string
  ): void {
    if (oldScopeKey === newScopeKey) {
      return;
    }
    const cur = this.sideQuestionByScope.get(oldScopeKey);
    if (cur === undefined) {
      return;
    }
    this.sideQuestionByScope.delete(oldScopeKey);
    if (!this.sideQuestionByScope.has(newScopeKey)) {
      const moved: OrbitScopedSideQuestionState = {
        scopeKey: newScopeKey,
        hostId: cur.hostId,
        agentId: newAgentId,
        cwd: newCwd !== undefined && newCwd.length > 0 ? newCwd : cur.cwd,
        provider: cur.provider,
        model: cur.model,
        draft: cur.draft,
        prompt: cur.prompt,
        answer: cur.answer,
        error: cur.error,
        streaming: cur.streaming,
        requestId: cur.requestId,
        requestToken: cur.requestToken,
        generation: cur.generation
      };
      this.sideQuestionByScope.set(newScopeKey, moved);
      // Migrate active request token ownership to the promoted/remapped scope (P1-2).
      if (cur.requestToken > 0 && this.activeSideQuestionScopeByToken.get(cur.requestToken) === oldScopeKey) {
        this.activeSideQuestionScopeByToken.set(cur.requestToken, newScopeKey);
      }
      this.sideQuestionRevision += 1;
    } else if (cur.requestToken > 0) {
      // Target scope already owns its own Q&A state; revoke the superseded request token so it cannot leak back.
      this.activeSideQuestionScopeByToken.delete(cur.requestToken);
    }
  }

  /**
   * Entry point for Composer `/btw <question>` submission (P1-2):
   * - Opens `side_question` tab in the originating scope with `source: 'auto'` (desktop parity:
   *   on `sm` adds tab + unread dot without hijacking screen; on `md`/`lg` opens when not dismissed).
   * - Routes request & streaming updates into the unified per-scope side-question state.
   */
  public submitBtwFromComposer(question: string): string {
    const tuple = this.resolveCurrentScopeTuple();
    const hostId = tuple[0];
    const rootAgentId = tuple[1];
    const cwd = tuple[2];
    if (hostId.length === 0) {
      return '';
    }
    const scopeKey = this.sidePanelStore.scopeKeyFor(hostId, rootAgentId);
    const sideTab = this.buildSideQuestionTab(scopeKey, hostId, rootAgentId, cwd);
    this.sidePanelStore.openTabInScope(scopeKey, sideTab, { source: 'auto', activate: true });
    const cleanQ = question.trim();
    if (cleanQ.length === 0) {
      return '';
    }
    const modelSnap: [string, string | null] = [
      this.composerProvider,
      this.composerModel.trim().length > 0 ? this.composerModel.trim() : null
    ];
    return this.submitScopedSideQuestion(scopeKey, hostId, rootAgentId, cwd, cleanQ, 'auto', modelSnap);
  }

  public submitScopedSideQuestion(
    scopeKey: string,
    hostId: string,
    agentId: string | null,
    cwd: string,
    question: string,
    source: 'user' | 'auto' = 'user',
    modelSnapshot?: [string, string | null]
  ): string {
    const cleanHost = hostId.trim();
    const cleanQ = question.trim();
    if (cleanHost.length === 0 || cleanQ.length === 0) {
      return '';
    }
    const cleanAgent = agentId !== null && agentId.trim().length > 0 ? agentId.trim() : null;
    const effectiveScopeKey = scopeKey.trim().length > 0
      ? scopeKey.trim()
      : this.sidePanelStore.scopeKeyFor(cleanHost, cleanAgent);
    const prev = this.getScopedSideQuestionState(effectiveScopeKey, cleanHost, cleanAgent, cwd);
    if (prev.requestToken > 0) {
      this.activeSideQuestionScopeByToken.delete(prev.requestToken);
    }
    const nextGen = prev.generation + 1;
    const requestToken = this.allocateSideQuestionToken();
    const snapModel = modelSnapshot !== undefined
      ? modelSnapshot
      : this.resolveModelSnapshotForScope(cleanHost, cleanAgent);

    const sideTab = this.buildSideQuestionTab(effectiveScopeKey, cleanHost, cleanAgent, cwd);

    if (this.bridge === null) {
      this.sideQuestionByScope.set(effectiveScopeKey, {
        scopeKey: effectiveScopeKey,
        hostId: cleanHost,
        agentId: cleanAgent,
        cwd: cwd,
        provider: snapModel[0],
        model: snapModel[1],
        draft: '',
        prompt: cleanQ,
        answer: '',
        error: 'Native core unavailable',
        streaming: false,
        requestId: '',
        requestToken: 0,
        generation: nextGen
      });
      this.sideQuestionRevision += 1;
      return '';
    }

    const activeState: OrbitScopedSideQuestionState = {
      scopeKey: effectiveScopeKey,
      hostId: cleanHost,
      agentId: cleanAgent,
      cwd: cwd,
      provider: snapModel[0],
      model: snapModel[1],
      draft: '',
      prompt: cleanQ,
      answer: '',
      error: '',
      streaming: true,
      requestId: '',
      requestToken: requestToken,
      generation: nextGen
    };
    this.sideQuestionByScope.set(effectiveScopeKey, activeState);
    this.activeSideQuestionScopeByToken.set(requestToken, effectiveScopeKey);
    this.sideQuestionRevision += 1;

    // Keep legacy mirrors synced for any callers still observing them on the active scope.
    if (effectiveScopeKey === this.sidePanelStore.activeScopeKey) {
      this.sideQuestionActive = true;
      this.sideQuestionStreaming = true;
      this.sideQuestionPrompt = cleanQ;
      this.sideQuestionAnswer = '';
      this.sideQuestionError = '';
    }

    let ctxSummary = '';
    if (cleanAgent !== null) {
      try {
        ctxSummary = this.bridge.buildSideQuestionContext(cleanHost, cleanAgent, 4000);
      } catch (_err) {
        ctxSummary = '';
      }
    }
    const req: AskSideQuestionRequestDto = {
      host_id: cleanHost,
      agent_id: cleanAgent,
      cwd: nonBlankPath(cwd).length > 0 ? nonBlankPath(cwd) : '~',
      provider: snapModel[0],
      model: snapModel[1],
      question: cleanQ,
      context_summary: ctxSummary.length > 0 ? ctxSummary : null
    };

    const applyUpdate = (deltaText: string, fullText: string | null, errMsg: string | null, finished: boolean): void => {
      // Strictly verify immutable requestToken & its current scope owner (migrated on promoteDraftScope,
      // revoked on clear/dropScope/removeHost) so late callbacks NEVER leak into reused drafts or deleted scopes.
      const ownerScopeKey = this.activeSideQuestionScopeByToken.get(requestToken);
      if (ownerScopeKey === undefined) {
        return;
      }
      const current = this.sideQuestionByScope.get(ownerScopeKey);
      if (current === undefined || current.requestToken !== requestToken) {
        this.activeSideQuestionScopeByToken.delete(requestToken);
        return;
      }
      if (finished) {
        this.activeSideQuestionScopeByToken.delete(requestToken);
      }
      const nextAnswer = fullText !== null && fullText.length > 0
        ? fullText
        : (deltaText.length > 0 ? `${current.answer}${deltaText}` : current.answer);
      const nextErr = errMsg !== null && errMsg.length > 0 ? errMsg : current.error;
      const updated: OrbitScopedSideQuestionState = {
        scopeKey: current.scopeKey,
        hostId: current.hostId,
        agentId: current.agentId,
        cwd: current.cwd,
        provider: current.provider,
        model: current.model,
        draft: current.draft,
        prompt: current.prompt,
        answer: nextAnswer,
        error: nextErr,
        streaming: finished ? false : current.streaming,
        requestId: current.requestId,
        requestToken: finished ? 0 : current.requestToken,
        generation: current.generation
      };
      this.sideQuestionByScope.set(ownerScopeKey, updated);
      this.sideQuestionRevision += 1;

      if (ownerScopeKey === this.sidePanelStore.activeScopeKey) {
        this.sideQuestionAnswer = nextAnswer;
        this.sideQuestionError = nextErr;
        if (finished) {
          this.sideQuestionStreaming = false;
        }
      }
      if (finished) {
        // Surface unread indicator in the CURRENT owning scope without stealing focus or compact screen (P1-2).
        const ownerTab = this.buildSideQuestionTab(
          ownerScopeKey,
          current.hostId,
          current.agentId,
          current.cwd
        );
        this.sidePanelStore.openTabInScope(ownerScopeKey, ownerTab, { source: 'auto', activate: false });
      }
    };

    try {
      const reqId = this.bridge.askSideQuestion(JSON.stringify(req), (err: Error | null, eventJson: string) => {
        if (err !== null && err !== undefined) {
          applyUpdate('', null, err.message || 'Side question failed', true);
          return;
        }
        if (eventJson === undefined || eventJson === null || eventJson.length === 0) {
          return;
        }
        try {
          const ev = JSON.parse(eventJson) as SideQuestionEnvelopeDto;
          if (ev.Delta !== undefined) {
            applyUpdate(ev.Delta.text_delta ?? '', null, null, false);
          } else if (ev.Done !== undefined) {
            applyUpdate('', ev.Done.full_text ?? '', ev.Done.error ?? '', true);
          } else {
            const delta = ev.text_delta ?? '';
            const hasFull = ev.full_text !== undefined && ev.full_text !== null;
            const hasErr = ev.error !== undefined && ev.error !== null && ev.error.length > 0;
            const isDone = hasFull || hasErr || ev.kind === 'done';
            applyUpdate(delta, hasFull ? (ev.full_text ?? '') : null, hasErr ? (ev.error ?? '') : null, isDone);
          }
        } catch (parseErr) {
          orbitLogWarn('OrbitStore', 'submitScopedSideQuestion event parse failed', parseErr as Object);
        }
      });

      if (reqId === undefined || reqId === null || reqId.trim().length === 0) {
        applyUpdate('', null, 'Unable to start side question on this host', true);
        return '';
      }
      const ownerScopeKey = this.activeSideQuestionScopeByToken.get(requestToken);
      if (ownerScopeKey !== undefined) {
        const latest = this.sideQuestionByScope.get(ownerScopeKey);
        if (latest !== undefined && latest.requestToken === requestToken) {
          latest.requestId = reqId;
          this.sideQuestionRevision += 1;
        }
      }
      if (source === 'user') {
        const currentOwner = this.activeSideQuestionScopeByToken.get(requestToken) ?? effectiveScopeKey;
        const currentState = this.sideQuestionByScope.get(currentOwner);
        const tabToOpen = currentState !== undefined
          ? this.buildSideQuestionTab(currentOwner, currentState.hostId, currentState.agentId, currentState.cwd)
          : sideTab;
        this.sidePanelStore.openTabInScope(currentOwner, tabToOpen, { source: 'user', activate: true });
      }
      return reqId;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'submitScopedSideQuestion failed', err as Object);
      applyUpdate('', null, (err as Error).message || 'Side question failed', true);
      return '';
    }
  }

  // --- Cross-Host Subagent Crew, Timelines, Steer & Fail-Closed Archive (P0-4, P1-4, P1-13) ---

  public clearArchivedSubagentLocalMark(hostId: string, agentId: string): void {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      return;
    }
    const managedKey = encodeSidePanelKeyTuple(['managed_archive', cleanHost, cleanAgent]);
    let changed = this.archivedSubagentIndex.delete(managedKey);
    if (this.archivedSubagentIndex.delete(`managed::${cleanHost}::${cleanAgent}`)) {
      changed = true;
    }
    this.archivedManagedMarkRevByKey.delete(managedKey);
    const toRemove: string[] = [];
    const legacySuffix = `::${cleanHost}::${cleanAgent}`;
    this.archivedSubagentIndex.forEach((k: string): void => {
      if (k.endsWith(legacySuffix) && k.indexOf('::provider::') < 0) {
        toRemove.push(k);
      }
    });
    for (let i = 0; i < toRemove.length; i++) {
      this.archivedSubagentIndex.delete(toRemove[i]);
      changed = true;
    }
    if (changed) {
      this.syncArchivedSubagentKeys();
      this.panelDataRevision += 1;
    }
  }

  public isSubagentArchivedLocally(
    rootHostId: string,
    rootAgentId: string,
    sub: SessionSubagentEntryDto
  ): boolean {
    const actualHost = resolveSubagentActualHostId(sub, rootHostId);
    const directParentHost = resolveSubagentDirectParentHostId(sub, rootHostId);
    const directParentAgent = resolveSubagentDirectParentAgentId(sub, rootAgentId);

    if (sub.child_agent_id !== null && sub.child_agent_id.trim().length > 0) {
      const cid = sub.child_agent_id.trim();
      const managedKey = encodeSidePanelKeyTuple(['managed_archive', actualHost, cid]);
      const card = this.findAgentCardOnHost(actualHost, cid);
      if (card !== null) {
        const archived = card.archived_at_rfc3339 !== undefined &&
          card.archived_at_rfc3339 !== null &&
          card.archived_at_rfc3339.length > 0;
        if (archived || card.status.state === 'closed') {
          return true;
        }
        // Authoritative fresher HostDetail for THIS child's actual host shows the session is open & unarchived (R3-P1-9).
        const markedRev = this.archivedManagedMarkRevByKey.get(managedKey);
        if (markedRev !== undefined && this.fleetStore.getHostDetailRevision(actualHost) > markedRev) {
          return false;
        }
      }
      if (
        this.archivedSubagentIndex.has(managedKey) ||
        this.archivedSubagentIndex.has(`managed::${actualHost}::${cid}`) ||
        this.archivedSubagentIndex.has(`${rootHostId}::${rootAgentId}::${actualHost}::${cid}`)
      ) {
        return true;
      }
      return false;
    }

    const provKey = (
      sub.provider_subagent_id !== undefined &&
      sub.provider_subagent_id !== null &&
      sub.provider_subagent_id.trim().length > 0
    )
      ? sub.provider_subagent_id.trim()
      : sub.id.trim();
    if (provKey.length > 0) {
      const providerKey = encodeSidePanelKeyTuple([
        'provider_archive',
        rootHostId,
        rootAgentId,
        directParentHost,
        directParentAgent,
        provKey
      ]);
      if (
        this.archivedSubagentIndex.has(providerKey) ||
        this.archivedSubagentIndex.has(
          `${rootHostId}::${rootAgentId}::provider::${directParentHost}::${directParentAgent}::${provKey}`
        )
      ) {
        return true;
      }
    }
    return false;
  }

  /**
   * Pure projection of Subagent Crew for `(rootHostId, rootAgentId)` across ALL cached hosts (P0-4, P1-12, P1-13):
   * - Preserves actual `host_id`, `parent_host_id`, `parent_agent_id` for cross-host & nested descendants.
   * - Propagates live `awaiting_permission` from managed AgentCards even on existing insights entries.
   * - Applies local archive filtering uniformly across both insights entries and scanned managed cards.
   */
  public getScopedSubagentsForSession(hostId: string, agentId: string): SessionSubagentEntryDto[] {
    const cleanRootHost = hostId.trim();
    const cleanRootAgent = agentId.trim();
    if (cleanRootHost.length === 0 || cleanRootAgent.length === 0) {
      return EMPTY_SUBAGENTS;
    }
    const _rev = this.fleetStore.hostDetailRevision + this.archivedSubagentKeys.length;
    if (_rev < 0) {
      return EMPTY_SUBAGENTS;
    }

    const sess = this.stageStore.getSession(cleanRootHost, cleanRootAgent);
    const rawList: SessionSubagentEntryDto[] = (
      sess !== null &&
      sess.insights !== null &&
      sess.insights.subagents !== undefined
    )
      ? sess.insights.subagents
      : EMPTY_SUBAGENTS;

    const result: SessionSubagentEntryDto[] = [];
    const seenChildKeys: Set<string> = new Set<string>();

    for (let i = 0; i < rawList.length; i++) {
      const sub = rawList[i];
      if (this.isSubagentArchivedLocally(cleanRootHost, cleanRootAgent, sub)) {
        continue;
      }
      if (sub.child_agent_id !== null && sub.child_agent_id.trim().length > 0) {
        const actualHost = resolveSubagentActualHostId(sub, cleanRootHost);
        const childId = sub.child_agent_id.trim();
        const childKey = `${actualHost}::${childId}`;
        seenChildKeys.add(childKey);
        const card = this.findAgentCardOnHost(actualHost, childId);
        if (card !== null) {
          const derivedStatus = card.is_awaiting_permission
            ? 'awaiting_permission'
            : (card.is_running ? 'running' : sub.status);
          const enriched: SessionSubagentEntryDto = {
            id: sub.id,
            role: sub.role,
            prompt_preview: sub.prompt_preview,
            status: derivedStatus,
            child_agent_id: childId,
            provider_subagent_id: sub.provider_subagent_id ?? null,
            last_seq: Math.max(sub.last_seq, card.latest_seq),
            host_id: actualHost,
            host_name: sub.host_name ?? null,
            parent_agent_id: resolveSubagentDirectParentAgentId(sub, cleanRootAgent),
            parent_host_id: resolveSubagentDirectParentHostId(sub, cleanRootHost),
            depth: sub.depth,
            lineage: sub.lineage
          };
          result.push(enriched);
          continue;
        }
      }
      result.push(sub);
    }

    // Scan ALL cached host details so cross-host and nested managed descendants are discovered
    // without touching synchronous N-API.
    const hostIds = this.fleetStore.getAllCachedHostIds();
    for (let hIdx = 0; hIdx < hostIds.length; hIdx++) {
      const candidateHostId = hostIds[hIdx];
      const detail = this.fleetStore.getCachedHostDetail(candidateHostId);
      if (detail === null) {
        continue;
      }
      for (let i = 0; i < detail.agents.length; i++) {
        const ag = detail.agents[i];
        if (ag.parent_agent_id === null || ag.parent_agent_id.trim().length === 0) {
          continue;
        }
        const archived = ag.archived_at_rfc3339 !== undefined &&
          ag.archived_at_rfc3339 !== null &&
          ag.archived_at_rfc3339.length > 0;
        if (archived || ag.status.state === 'closed') {
          continue;
        }
        const childKey = `${candidateHostId}::${ag.agent_id}`;
        if (seenChildKeys.has(childKey)) {
          continue;
        }
        const resolvedRoot = this.sidePanelStore.resolveRootScope(candidateHostId, ag.agent_id);
        if (resolvedRoot.hostId !== cleanRootHost || resolvedRoot.rootAgentId !== cleanRootAgent) {
          continue;
        }
        const directParentHost = (
          ag.parent_host_id !== undefined &&
          ag.parent_host_id !== null &&
          ag.parent_host_id.trim().length > 0
        )
          ? ag.parent_host_id.trim()
          : candidateHostId;
        const statusSlug = ag.is_awaiting_permission
          ? 'awaiting_permission'
          : (ag.is_running ? 'running' : (ag.status.state || 'idle'));
        const entry: SessionSubagentEntryDto = {
          id: ag.agent_id,
          role: ag.title.trim().length > 0 ? ag.title.trim() : ag.provider_label,
          prompt_preview: ag.status_label,
          status: statusSlug,
          child_agent_id: ag.agent_id,
          provider_subagent_id: null,
          last_seq: ag.latest_seq,
          host_id: candidateHostId,
          host_name: detail.host.display_name,
          parent_agent_id: ag.parent_agent_id,
          parent_host_id: directParentHost
        };
        if (this.isSubagentArchivedLocally(cleanRootHost, cleanRootAgent, entry)) {
          continue;
        }
        seenChildKeys.add(childKey);
        result.push(entry);
      }
    }

    return result;
  }

  /**
   * Subscribes once to a managed subagent or peer session and triggers an incremental stage fetch.
   * Returns the `@ObservedV2 OrbitStageSession` so views observe `session.commitRevision` directly
   * instead of re-opening timelines or copying all turns on every global revision (P1-4).
   */
  public bindScopedManagedSession(hostId: string, agentId: string): OrbitStageSession | null {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      return null;
    }
    const sess = this.stageStore.getOrCreateSession(cleanHost, cleanAgent);
    if (this.bridge !== null) {
      try {
        this.bridge.openAgentTimeline(cleanHost, cleanAgent);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'openAgentTimeline failed for scoped session', err as Object);
      }
      this.stageStore.fetchIncremental(sess, 24).catch((err: Object) => {
        orbitLogWarn('OrbitStore', 'fetchIncremental failed for scoped session', err);
      });
    }
    return sess;
  }

  /**
   * Fetches provider-native subagent events using the direct parent `(directParentHostId, directParentAgentId)`
   * and parses the real `Vec<ProviderSubagentEvent>` JSON array serialized by Rust N-API (P1-4).
   */
  public loadScopedProviderSubagentTimeline(
    directParentHostId: string,
    directParentAgentId: string,
    providerSubagentId: string,
    subscribe: boolean = false
  ): TimelineItemDto[] {
    const cleanHost = directParentHostId.trim();
    const cleanParent = directParentAgentId.trim();
    const cleanSub = providerSubagentId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanParent.length === 0 || cleanSub.length === 0) {
      return [];
    }
    try {
      if (subscribe) {
        this.bridge.openProviderSubagentTimeline(cleanHost, cleanParent, cleanSub);
      }
      const raw = this.bridge.providerSubagentTimelineJson(cleanHost, cleanParent, cleanSub);
      return parseProviderSubagentTimelineJson(raw);
    } catch (err) {
      orbitLogWarn('OrbitStore', 'loadScopedProviderSubagentTimeline failed', err as Object);
      return [];
    }
  }

  private timelineModelToDto(model: OrbitTimelineItemModel): TimelineItemDto {
    return {
      item_type: model.itemType,
      seq: model.seq,
      item_id: model.itemKey.length > 0 ? model.itemKey : undefined,
      text: model.rawText,
      summary_title: model.title,
      status: model.status,
      blocks: model.rawBlocks.length > 0 ? model.rawBlocks : undefined,
      detail: model.detail ?? undefined
    };
  }

  public projectStageSessionTimelineItems(sess: OrbitStageSession | null): TimelineItemDto[] {
    if (sess === null) {
      return [];
    }
    const items: TimelineItemDto[] = [];
    for (let i = 0; i < sess.turns.length; i++) {
      const turn = sess.turns[i];
      const seenKeys: Set<string> = new Set<string>();
      const pushUnique = (model: OrbitTimelineItemModel | null): void => {
        if (model === null) {
          return;
        }
        const key = model.itemKey.length > 0 ? model.itemKey : `${model.itemType}:${model.seq}`;
        if (seenKeys.has(key)) {
          return;
        }
        seenKeys.add(key);
        items.push(this.timelineModelToDto(model));
      };

      pushUnique(turn.userPrompt);
      pushUnique(turn.agentMessage);
      for (let f = 0; f < turn.foldedSteps.length; f++) {
        pushUnique(turn.foldedSteps[f]);
      }
      for (let j = 0; j < turn.liveSegments.length; j++) {
        const seg = turn.liveSegments[j];
        // Handle BOTH single-item segments (`seg.item`, e.g. active assistant_message / tool_call)
        // AND array segments (`seg.items`, e.g. step_group) in chronological segment order (P1-4).
        if (seg.item !== null) {
          pushUnique(seg.item);
        }
        for (let k = 0; k < seg.items.length; k++) {
          pushUnique(seg.items[k]);
        }
      }
      pushUnique(turn.terminalReply);
      for (let n = 0; n < turn.notices.length; n++) {
        pushUnique(turn.notices[n]);
      }
    }
    return items;
  }

  public steerScopedAgent(hostId: string, agentId: string, prompt: string): boolean {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    const cleanPrompt = prompt.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanAgent.length === 0 || cleanPrompt.length === 0) {
      return false;
    }
    try {
      this.bridge.sendPromptWithMeta(cleanHost, cleanAgent, cleanPrompt, cleanPrompt, null);
      this.showToast('Sent steer message', false);
      const sess = this.stageStore.getSession(cleanHost, cleanAgent);
      if (sess !== null) {
        this.stageStore.fetchIncremental(sess, 24).catch((_e: Object) => {
          // Best-effort refresh
        });
      }
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'steerScopedAgent failed', err as Object);
      this.showToast((err as Error).message || 'Steer failed', true);
      return false;
    }
  }

  /**
   * Archives a single subagent on its ACTUAL host (`sub.host_id ?? rootHostId`).
   * Fail-closed (P0-4 + P1-7 + P1-13):
   * - For managed children, queues `archiveAgent` on the child's actual host and records the current
   *   `hostDetailRevision` so subsequent authoritative unarchive updates clear the optimistic local hide mark.
   * - For provider-native subagents, scopes tab removal strictly by `(directParentHost, directParentAgent, provId)`
   *   across both open and closed tab stacks so sibling parents sharing a provider ID never collide.
   */
  public archiveScopedSubagent(
    rootHostId: string,
    rootAgentId: string,
    sub: SessionSubagentEntryDto
  ): boolean {
    const cleanRootHost = rootHostId.trim();
    const cleanRootAgent = rootAgentId.trim();
    if (cleanRootHost.length === 0 || cleanRootAgent.length === 0) {
      return false;
    }
    const rootScopeKey = this.sidePanelStore.scopeKeyFor(cleanRootHost, cleanRootAgent);
    const actualChildHost = resolveSubagentActualHostId(sub, cleanRootHost);
    const directParentHost = resolveSubagentDirectParentHostId(sub, cleanRootHost);
    const directParentAgent = resolveSubagentDirectParentAgentId(sub, cleanRootAgent);

    if (sub.child_agent_id !== null && sub.child_agent_id.trim().length > 0) {
      const childId = sub.child_agent_id.trim();
      if (this.bridge === null) {
        this.showToast('Native core unavailable; archive request failed', true);
        return false;
      }
      try {
        this.bridge.archiveAgent(actualChildHost, childId);
      } catch (err) {
        orbitLogWarn('OrbitStore', 'archiveScopedSubagent bridge call failed', err as Object);
        this.showToast((err as Error).message || 'Failed to queue subagent archive', true);
        return false;
      }
      // Only mutate local state & tabs AFTER native RPC accepts/queues the request.
      this.sidePanelStore.removeSubagentTabs(actualChildHost, childId, undefined, false);
      const managedKey = encodeSidePanelKeyTuple(['managed_archive', actualChildHost, childId]);
      this.archivedSubagentIndex.add(managedKey);
      this.archivedManagedMarkRevByKey.set(managedKey, this.fleetStore.getHostDetailRevision(actualChildHost));
      this.syncArchivedSubagentKeys();
      this.panelDataRevision += 1;
      this.reloadHostAsync(actualChildHost);
      this.triggerHapticPreset('success');
      return true;
    }

    // Provider-native subagent (local dismissal scoped strictly to its direct parent).
    const provId = (
      sub.provider_subagent_id !== undefined &&
      sub.provider_subagent_id !== null &&
      sub.provider_subagent_id.trim().length > 0
    )
      ? sub.provider_subagent_id.trim()
      : sub.id.trim();
    if (provId.length === 0) {
      return false;
    }
    this.sidePanelStore.removeSubagentTabs(
      directParentHost,
      provId,
      rootScopeKey,
      true,
      directParentHost,
      directParentAgent
    );
    this.archivedSubagentIndex.add(
      encodeSidePanelKeyTuple([
        'provider_archive',
        cleanRootHost,
        cleanRootAgent,
        directParentHost,
        directParentAgent,
        provId
      ])
    );
    this.syncArchivedSubagentKeys();
    this.panelDataRevision += 1;
    this.triggerHapticPreset('success');
    return true;
  }

  private syncArchivedSubagentKeys(): void {
    const nextKeys: string[] = [];
    this.archivedSubagentIndex.forEach((k: string): void => {
      nextKeys.push(k);
    });
    this.archivedSubagentKeys = nextKeys;
  }

  public archiveSettledSubagentsForSession(
    rootHostId: string,
    rootAgentId: string,
    subagents: SessionSubagentEntryDto[]
  ): OrbitSubagentBatchArchiveOutcome {
    let queuedCount = 0;
    let dismissedCount = 0;
    let failedCount = 0;
    for (let i = 0; i < subagents.length; i++) {
      const sub = subagents[i];
      if (!isSubagentSettledStatus(sub.status)) {
        continue;
      }
      const isManaged = sub.child_agent_id !== null && sub.child_agent_id.trim().length > 0;
      if (this.archiveScopedSubagent(rootHostId, rootAgentId, sub)) {
        if (isManaged) {
          queuedCount += 1;
        } else {
          dismissedCount += 1;
        }
      } else {
        failedCount += 1;
      }
    }
    const acceptedTotal = queuedCount + dismissedCount;
    if (failedCount > 0 && acceptedTotal > 0) {
      this.showToast(`Accepted ${acceptedTotal}, ${failedCount} failed — tap to retry`, true);
    } else if (failedCount > 0) {
      this.showToast(`Failed to archive ${failedCount} subagent${failedCount === 1 ? '' : 's'}`, true);
    } else if (queuedCount > 0 && dismissedCount > 0) {
      this.showToast(`Archive requested (${queuedCount}) · dismissed (${dismissedCount})`, false);
    } else if (queuedCount > 0) {
      this.showToast(`Archive requested for ${queuedCount} subagent${queuedCount === 1 ? '' : 's'}`, false);
    } else if (dismissedCount > 0) {
      this.showToast(`Dismissed ${dismissedCount} finished subagent${dismissedCount === 1 ? '' : 's'}`, false);
    }
    return {
      archivedCount: acceptedTotal,
      queuedCount: queuedCount,
      dismissedCount: dismissedCount,
      failedCount: failedCount
    };
  }

  // --- Scoped Automations Helpers (Full Parity for All 5 Automation Kinds — P1-8) ---

  public async loadScopedAutomations(hostId: string): Promise<OrbitScopedAutomationsData> {
    const cleanHost = hostId.trim();
    const empty: OrbitScopedAutomationsData = {
      schedules: [],
      timers: [],
      goals: [],
      monitors: [],
      workflows: []
    };
    if (this.bridge === null || cleanHost.length === 0) {
      return empty;
    }
    try {
      this.ensureScopedHostDetailAsync(cleanHost);
      const detail = this.getScopedHostDetail(cleanHost);
      const schedules: ScheduleSpecDto[] = detail !== null && detail.schedules !== undefined
        ? detail.schedules
        : [];
      const timerJson = await this.settleAutomationJson(
        this.bridge.listConditionalTimersJson(cleanHost, null, true),
        'scoped.listConditionalTimersJson'
      );
      const goalJson = await this.settleAutomationJson(
        this.bridge.listGoalRunsJson(cleanHost, null),
        'scoped.listGoalRunsJson'
      );
      const monJson = await this.settleAutomationJson(
        this.bridge.listMonitorWatchesJson(cleanHost, null),
        'scoped.listMonitorWatchesJson'
      );
      const wfJson = await this.settleAutomationJson(
        this.bridge.listWorkflowRunsJson(cleanHost, true),
        'scoped.listWorkflowRunsJson'
      );
      return {
        schedules: schedules,
        timers: timerJson !== null ? (JSON.parse(timerJson) as ConditionalTimerRecordDto[]) : [],
        goals: goalJson !== null ? (JSON.parse(goalJson) as GoalRunRecordDto[]) : [],
        monitors: monJson !== null ? (JSON.parse(monJson) as MonitorWatchRecordDto[]) : [],
        workflows: wfJson !== null ? (JSON.parse(wfJson) as WorkflowRunRecordDto[]) : []
      };
    } catch (err) {
      orbitLogWarn('OrbitStore', 'loadScopedAutomations failed', err as Object);
      return empty;
    }
  }

  public createScopedSchedule(
    hostId: string,
    cwd: string,
    name: string,
    cronExpr: string,
    prompt: string,
    useWorktree: boolean
  ): boolean {
    const cleanHost = hostId.trim();
    const cleanName = name.trim();
    const cleanCron = cronExpr.trim();
    const cleanPrompt = prompt.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanName.length === 0 || cleanCron.length === 0 || cleanPrompt.length === 0) {
      this.showToast('Name, cron expression, and prompt are required', true);
      return false;
    }
    const modelSnap = this.resolveModelSnapshotForScope(cleanHost, null);
    const payload: CreateSchedulePayloadDto = {
      host_id: cleanHost,
      name: cleanName,
      cron_expr: cleanCron,
      prompt: cleanPrompt,
      cwd: nonBlankPath(cwd).length > 0 ? nonBlankPath(cwd) : '~',
      provider: modelSnap[0],
      model: modelSnap[1],
      effort: null,
      mode: normalizeModeSlug(this.composerMode),
      worktree: useWorktree,
      archive_on_finish: true,
      max_runs: null
    };
    try {
      this.bridge.createScheduleJson(JSON.stringify(payload));
      this.bridge.refreshHost(cleanHost);
      this.reloadHostAsync(cleanHost);
      this.showToast(`Created schedule "${cleanName}"`, false);
      this.triggerHapticPreset('success');
      return true;
    } catch (err) {
      orbitLogWarn('OrbitStore', 'createScopedSchedule failed', err as Object);
      this.showToast((err as Error).message || 'Failed to create schedule', true);
      return false;
    }
  }

  public pauseScopedSchedule(hostId: string, scheduleId: string): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.pauseSchedule(cleanHost, scheduleId);
      this.reloadHostAsync(cleanHost);
      this.showToast('Schedule paused', false);
      this.triggerHapticPreset('selection');
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to pause schedule', true);
    }
  }

  public resumeScopedSchedule(hostId: string, scheduleId: string): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.resumeSchedule(cleanHost, scheduleId);
      this.reloadHostAsync(cleanHost);
      this.showToast('Schedule resumed', false);
      this.triggerHapticPreset('selection');
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to resume schedule', true);
    }
  }

  public runScopedScheduleOnce(hostId: string, scheduleId: string): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.runScheduleOnce(cleanHost, scheduleId);
      this.showToast('Triggered schedule run', false);
      this.triggerHapticPreset('action');
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to trigger schedule', true);
    }
  }

  public deleteScopedSchedule(hostId: string, scheduleId: string): void {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || scheduleId.length === 0) {
      return;
    }
    try {
      this.bridge.deleteSchedule(cleanHost, scheduleId);
      this.reloadHostAsync(cleanHost);
      this.showToast('Schedule deleted', false);
      this.triggerHapticPreset('warning');
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to delete schedule', true);
    }
  }

  public loadScopedScheduleRuns(hostId: string, scheduleId: string): ScheduleRunRecordDto[] {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || scheduleId.length === 0) {
      return [];
    }
    try {
      this.bridge.fetchScheduleRuns(cleanHost, scheduleId, 10);
      const raw = this.bridge.scheduleRunsJson(cleanHost, scheduleId);
      return JSON.parse(raw) as ScheduleRunRecordDto[];
    } catch (err) {
      orbitLogWarn('OrbitStore', 'loadScopedScheduleRuns failed', err as Object);
      return [];
    }
  }

  public async createScopedConditionalTimer(
    hostId: string,
    agentId: string | null,
    durationSeconds: number,
    prompt: string,
    conditionKind: string
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId !== null ? agentId.trim() : '';
    const cleanPrompt = prompt.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanAgent.length === 0 || cleanPrompt.length === 0) {
      this.showToast('Requires an active session and prompt to arm a timer', true);
      return false;
    }
    try {
      const condObj: TimerConditionDto = { kind: conditionKind };
      await this.bridge.createTimerJson(
        cleanHost,
        cleanAgent,
        Math.max(10, Math.round(durationSeconds)),
        cleanPrompt,
        JSON.stringify(condObj)
      );
      this.showToast('Conditional timer armed', false);
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to arm timer', true);
      return false;
    }
  }

  public async cancelScopedConditionalTimer(hostId: string, timerId: string): Promise<boolean> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || timerId.length === 0) {
      return false;
    }
    try {
      await this.bridge.cancelConditionalTimerJson(cleanHost, timerId);
      this.showToast('Timer canceled', false);
      this.triggerHapticPreset('warning');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to cancel timer', true);
      return false;
    }
  }

  public async fireScopedConditionalTimerNow(hostId: string, timerId: string): Promise<boolean> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || timerId.length === 0) {
      return false;
    }
    try {
      await this.bridge.fireConditionalTimerNowJson(cleanHost, timerId);
      this.showToast('Timer fired', false);
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to fire timer', true);
      return false;
    }
  }

  public async startScopedGoalRun(
    hostId: string,
    agentId: string | null,
    objective: string,
    verifyCmd: string | null,
    maxIterations: number
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId !== null ? agentId.trim() : '';
    const cleanObj = objective.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanAgent.length === 0 || cleanObj.length === 0) {
      this.showToast('Requires an active session and objective to start a goal', true);
      return false;
    }
    try {
      const req: StartGoalRunRequestDto = {
        host_id: cleanHost,
        agent_id: cleanAgent,
        objective: cleanObj,
        criteria: [],
        verify_command: verifyCmd !== null && verifyCmd.trim().length > 0 ? verifyCmd.trim() : null,
        max_iterations: Math.max(1, maxIterations),
        max_duration_secs: 7200
      };
      await this.bridge.startGoalJson(JSON.stringify(req));
      this.showToast('Goal loop started', false);
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to start goal', true);
      return false;
    }
  }

  public async controlScopedGoalRun(hostId: string, goalId: string, action: string): Promise<boolean> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || goalId.length === 0) {
      return false;
    }
    try {
      const req: ControlGoalRunRequestDto = {
        host_id: cleanHost,
        goal_id: goalId,
        action: action
      };
      await this.bridge.controlGoalRunJson(JSON.stringify(req));
      const label = action === 'pause'
        ? 'Goal paused'
        : (action === 'resume' ? 'Goal resumed' : (action === 'cancel' ? 'Goal stopped' : 'Goal verifying'));
      this.showToast(label, false);
      this.triggerHapticPreset('warning');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to update goal', true);
      return false;
    }
  }

  public async createScopedMonitorWatch(
    hostId: string,
    agentId: string | null,
    name: string,
    kind: string,
    target: string,
    pattern: string | null
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId !== null ? agentId.trim() : '';
    const cleanName = name.trim();
    const cleanTarget = target.trim();
    if (
      this.bridge === null ||
      cleanHost.length === 0 ||
      cleanAgent.length === 0 ||
      cleanName.length === 0 ||
      cleanTarget.length === 0
    ) {
      this.showToast('Requires an active session, name, and target', true);
      return false;
    }
    try {
      const req: StartMonitorWatchRequestDto = {
        host_id: cleanHost,
        agent_id: cleanAgent,
        name: cleanName,
        kind: kind === 'http_health'
          ? { kind: 'http_health', url: cleanTarget }
          : (kind === 'log_file_tail'
            ? { kind: 'log_file_tail', path: cleanTarget, pattern: pattern }
            : { kind: 'command_stream', command: cleanTarget, pattern: pattern }),
        expires_in_secs: 3600
      };
      await this.bridge.createMonitorJson(JSON.stringify(req));
      this.showToast('Monitor watch started', false);
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to start monitor', true);
      return false;
    }
  }

  public async stopScopedMonitorWatch(hostId: string, monitorId: string): Promise<boolean> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || monitorId.length === 0) {
      return false;
    }
    try {
      const req: StopMonitorWatchRequestDto = {
        host_id: cleanHost,
        monitor_id: monitorId
      };
      await this.bridge.stopMonitorWatchJson(JSON.stringify(req));
      this.showToast('Monitor stopped', false);
      this.triggerHapticPreset('warning');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to stop monitor', true);
      return false;
    }
  }

  public async startScopedDagWorkflow(
    hostId: string,
    cwd: string,
    name: string,
    stepPrompt: string
  ): Promise<boolean> {
    const cleanHost = hostId.trim();
    const cleanName = name.trim();
    const cleanPrompt = stepPrompt.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanName.length === 0 || cleanPrompt.length === 0) {
      return false;
    }
    try {
      const modelSnap = this.resolveModelSnapshotForScope(cleanHost, null);
      const step: DagStepSpecDto = {
        step_id: 'step_1',
        name: cleanName,
        prompt: cleanPrompt,
        depends_on: []
      };
      const req: StartDagWorkflowRequestDto = {
        host_id: cleanHost,
        name: cleanName,
        cwd: nonBlankPath(cwd).length > 0 ? nonBlankPath(cwd) : '~',
        steps: [step],
        default_provider: modelSnap[0],
        default_mode: normalizeModeSlug(this.composerMode),
        max_concurrency: 2
      };
      await this.bridge.startWorkflowJson(JSON.stringify(req));
      this.showToast('Workflow started', false);
      this.triggerHapticPreset('action');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to start workflow', true);
      return false;
    }
  }

  public async cancelScopedWorkflowRun(hostId: string, runId: string): Promise<boolean> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || runId.length === 0) {
      return false;
    }
    try {
      const req: CancelWorkflowRunRequestDto = {
        host_id: cleanHost,
        run_id: runId
      };
      await this.bridge.cancelWorkflowRunJson(JSON.stringify(req));
      this.showToast('Workflow canceled', false);
      this.triggerHapticPreset('warning');
      return true;
    } catch (err) {
      this.showToast((err as Error).message || 'Failed to cancel workflow', true);
      return false;
    }
  }

  // --- Store-backed Automations In-Flight Submission Locks (R3-P1-10) ---

  public automationSubmitLockKey(
    scopeKey: string,
    tabId: string,
    hostId: string,
    agentId: string | null,
    cwd: string | null,
    formKind: string
  ): string {
    return encodeSidePanelKeyTuple([
      'auto_submit',
      scopeKey.trim(),
      tabId.trim(),
      hostId.trim(),
      agentId !== null ? agentId.trim() : '',
      nonBlankPath(cwd),
      formKind.trim()
    ]);
  }

  public isAutomationFormSubmitting(lockKey: string): boolean {
    return lockKey.length > 0 && this.automationSubmitTokenByKey.has(lockKey);
  }

  public acquireAutomationSubmitToken(lockKey: string): number {
    if (lockKey.length === 0 || this.automationSubmitTokenByKey.has(lockKey)) {
      return 0;
    }
    this.automationSubmitSeq += 1;
    const token = this.automationSubmitSeq;
    this.automationSubmitTokenByKey.set(lockKey, token);
    this.automationSubmitRevision += 1;
    return token;
  }

  public releaseAutomationSubmitToken(lockKey: string, token: number): boolean {
    if (lockKey.length === 0 || token <= 0) {
      return false;
    }
    if (this.automationSubmitTokenByKey.get(lockKey) !== token) {
      return false;
    }
    this.automationSubmitTokenByKey.delete(lockKey);
    this.automationSubmitRevision += 1;
    return true;
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

  // --- PR-6 Settings & Tier-2 Extensions Scoped Actions (Zero Global Toast Side-Effects) ---

  public exportHostPairingUriScoped(hostId: string): OrbitScopedActionOutcome<string> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const uri = this.bridge.exportHostPairingUri(cleanHost).trim();
      if (uri.length === 0) {
        return { ok: false, data: null, errorDetail: 'Pairing URI is empty' };
      }
      return { ok: true, data: uri, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to export pairing URI'
      };
    }
  }

  public applyRelayToFleetScoped(rawRelayUrl: string): OrbitScopedActionOutcome<number> {
    if (this.bridge === null) {
      return { ok: false, data: null, errorDetail: 'Native core unavailable' };
    }
    try {
      const count = this.bridge.applyRelayToFleet(rawRelayUrl.trim());
      this.reloadFleet();
      return { ok: true, data: count, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to apply relay across fleet'
      };
    }
  }

  public async getUsageInsightsScoped(
    hostId: string,
    days: number
  ): Promise<OrbitScopedActionOutcome<UsageInsightsSnapshotDto>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.getUsageInsightsJson(cleanHost, days);
      const parsed = JSON.parse(raw) as UsageInsightsSnapshotDto;
      return { ok: true, data: parsed, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load usage insights'
      };
    }
  }

  public async listExternalMcpServersScoped(
    hostId: string,
    cwd?: string | null
  ): Promise<OrbitScopedActionOutcome<ExternalMcpServerRecordDto[]>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const exactCwd = cwd !== undefined && cwd !== null && cwd.trim().length > 0 ? cwd : null;
      const raw = await this.bridge.listExternalMcpServersJson(cleanHost, exactCwd);
      const parsed = JSON.parse(raw) as ExternalMcpServerRecordDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load MCP servers'
      };
    }
  }

  public async toggleExternalMcpServerScoped(
    hostId: string,
    serverId: string,
    enabled: boolean,
    cwd?: string | null
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanServer = serverId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanServer.length === 0) {
      return { ok: false, data: null, errorDetail: 'Invalid MCP server parameters' };
    }
    try {
      const exactCwd = cwd !== undefined && cwd !== null && cwd.trim().length > 0 ? cwd : null;
      await this.bridge.toggleExternalMcpServerJson(cleanHost, cleanServer, enabled, exactCwd);
      return { ok: true, data: enabled, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to toggle MCP server'
      };
    }
  }

  public async listManagedSkillsScoped(
    hostId: string,
    cwd?: string | null
  ): Promise<OrbitScopedActionOutcome<ManagedSkillRecordDto[]>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const exactCwd = cwd !== undefined && cwd !== null && cwd.trim().length > 0 ? cwd : null;
      const raw = await this.bridge.listManagedSkillsJson(cleanHost, exactCwd);
      const parsed = JSON.parse(raw) as ManagedSkillRecordDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load skills'
      };
    }
  }

  public async listProjectMemoriesScoped(
    hostId: string,
    cwd: string,
    limit: number = 100
  ): Promise<OrbitScopedActionOutcome<ProjectMemoryItemDto[]>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cwd.trim().length === 0) {
      return { ok: false, data: null, errorDetail: 'Host or project directory unavailable' };
    }
    try {
      const raw = await this.bridge.listProjectMemoriesJson(cleanHost, cwd, limit);
      const parsed = JSON.parse(raw) as ProjectMemoryItemDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load project memories'
      };
    }
  }

  public async confirmProjectMemoryScoped(
    hostId: string,
    memoryId: string,
    cwd?: string | null
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanId = memoryId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanId.length === 0) {
      return { ok: false, data: null, errorDetail: 'Invalid memory parameters' };
    }
    try {
      const exactCwd = cwd !== undefined && cwd !== null && cwd.trim().length > 0 ? cwd : null;
      await this.bridge.confirmProjectMemoryJson(cleanHost, cleanId, exactCwd);
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to confirm project memory'
      };
    }
  }

  public async deleteProjectMemoryScoped(
    hostId: string,
    memoryId: string,
    cwd?: string | null
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanId = memoryId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanId.length === 0) {
      return { ok: false, data: null, errorDetail: 'Invalid memory parameters' };
    }
    try {
      const exactCwd = cwd !== undefined && cwd !== null && cwd.trim().length > 0 ? cwd : null;
      await this.bridge.deleteProjectMemoryJson(cleanHost, cleanId, exactCwd);
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to delete project memory'
      };
    }
  }

  public async listPluginsScoped(
    hostId: string
  ): Promise<OrbitScopedActionOutcome<InstalledPluginDto[]>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.listPluginsJson(cleanHost);
      const parsed = JSON.parse(raw) as InstalledPluginDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load plugins'
      };
    }
  }

  public async setPluginEnabledScoped(
    hostId: string,
    pluginId: string,
    enabled: boolean
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanPlugin = pluginId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanPlugin.length === 0) {
      return { ok: false, data: null, errorDetail: 'Invalid plugin parameters' };
    }
    try {
      await this.bridge.setPluginEnabledJson(cleanHost, cleanPlugin, enabled);
      return { ok: true, data: enabled, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to update plugin state'
      };
    }
  }

  public async getSandboxConfigScoped(
    hostId: string
  ): Promise<OrbitScopedActionOutcome<SandboxPolicyDto | null>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.getSandboxConfigJson(cleanHost);
      const parsed = raw.trim() === 'null' ? null : (JSON.parse(raw) as SandboxPolicyDto | null);
      return { ok: true, data: parsed, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load sandbox config'
      };
    }
  }

  public async getGuardianConfigScoped(
    hostId: string
  ): Promise<OrbitScopedActionOutcome<GuardianPolicyConfigDto>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.getGuardianConfigJson(cleanHost);
      const parsed = JSON.parse(raw) as GuardianPolicyConfigDto;
      return { ok: true, data: parsed, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load Guardian policy'
      };
    }
  }

  public async updateGuardianConfigScoped(
    hostId: string,
    config: GuardianPolicyConfigDto
  ): Promise<OrbitScopedActionOutcome<GuardianPolicyConfigDto>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.updateGuardianConfigJson(cleanHost, JSON.stringify(config));
      const parsed = JSON.parse(raw) as GuardianPolicyConfigDto;
      return { ok: true, data: parsed, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to save Guardian policy'
      };
    }
  }

  public async listPermissionRulesScoped(
    hostId: string,
    projectCwd?: string | null
  ): Promise<OrbitScopedActionOutcome<PermissionRuleRecordDto[]>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const cleanCwd =
        projectCwd !== undefined && projectCwd !== null && projectCwd.trim().length > 0
          ? projectCwd.trim()
          : null;
      const raw = await this.bridge.listPermissionRulesJson(cleanHost, cleanCwd);
      const parsed = JSON.parse(raw) as PermissionRuleRecordDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to load permission rules'
      };
    }
  }

  public async upsertPermissionRuleScoped(
    hostId: string,
    input: UpsertPermissionRuleInputDto
  ): Promise<OrbitScopedActionOutcome<PermissionRuleRecordDto>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.upsertPermissionRuleJson(cleanHost, JSON.stringify(input));
      const parsed = JSON.parse(raw) as PermissionRuleRecordDto;
      return { ok: true, data: parsed, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to save permission rule'
      };
    }
  }

  public async deletePermissionRuleScoped(
    hostId: string,
    ruleId: string
  ): Promise<OrbitScopedActionOutcome<boolean>> {
    const cleanHost = hostId.trim();
    const cleanRule = ruleId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanRule.length === 0) {
      return { ok: false, data: null, errorDetail: 'Invalid permission rule parameters' };
    }
    try {
      await this.bridge.deletePermissionRuleJson(cleanHost, cleanRule);
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to delete permission rule'
      };
    }
  }

  public listAcpAgentsScoped(hostId: string): OrbitScopedActionOutcome<AcpAgentAvailabilityDto[]> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = this.bridge.listAcpAgentsJson(cleanHost);
      const parsed = JSON.parse(raw) as AcpAgentAvailabilityDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to list ACP agents'
      };
    }
  }

  public refreshAcpAgentsScoped(hostId: string): OrbitScopedActionOutcome<boolean> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      this.bridge.refreshAcpAgents(cleanHost);
      return { ok: true, data: true, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to refresh ACP agents'
      };
    }
  }

  public async refreshAcpRegistryScoped(
    hostId: string,
    registryUrl?: string | null
  ): Promise<OrbitScopedActionOutcome<AcpAgentAvailabilityDto[]>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.refreshAcpRegistryJson(cleanHost, registryUrl ?? null);
      const parsed = JSON.parse(raw) as AcpAgentAvailabilityDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to refresh ACP registry'
      };
    }
  }

  public async testAcpAgentScoped(
    hostId: string,
    agentId: string,
    cwd?: string | null
  ): Promise<OrbitScopedActionOutcome<AcpAgentTestResultDto>> {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanAgent.length === 0) {
      return { ok: false, data: null, errorDetail: 'Invalid ACP agent parameters' };
    }
    try {
      const raw = await this.bridge.testAcpAgentJson(cleanHost, cleanAgent, cwd ?? null);
      const parsed = JSON.parse(raw) as AcpAgentTestResultDto;
      return { ok: true, data: parsed, errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'ACP agent test failed'
      };
    }
  }

  public async upsertCustomAcpAgentScoped(
    hostId: string,
    spec: AcpAgentSpecDto
  ): Promise<OrbitScopedActionOutcome<AcpAgentAvailabilityDto[]>> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return { ok: false, data: null, errorDetail: 'Host unavailable' };
    }
    try {
      const raw = await this.bridge.upsertCustomAcpAgentJson(cleanHost, JSON.stringify(spec));
      const parsed = JSON.parse(raw) as AcpAgentAvailabilityDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to save custom ACP agent'
      };
    }
  }

  public async deleteCustomAcpAgentScoped(
    hostId: string,
    agentId: string
  ): Promise<OrbitScopedActionOutcome<AcpAgentAvailabilityDto[]>> {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (this.bridge === null || cleanHost.length === 0 || cleanAgent.length === 0) {
      return { ok: false, data: null, errorDetail: 'Invalid ACP agent parameters' };
    }
    try {
      const raw = await this.bridge.deleteCustomAcpAgentJson(cleanHost, cleanAgent);
      const parsed = JSON.parse(raw) as AcpAgentAvailabilityDto[];
      return { ok: true, data: Array.isArray(parsed) ? parsed : [], errorDetail: '' };
    } catch (err) {
      return {
        ok: false,
        data: null,
        errorDetail: (err as Error).message || 'Failed to delete custom ACP agent'
      };
    }
  }
}
