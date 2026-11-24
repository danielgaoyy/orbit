import type { ComposerInlineDirective } from "@/lib/slash-commands";
import type { DraftPersistenceController } from "@/lib/draft-persistence";
import type { NotificationSoundTone } from "@/lib/notification-sound";
import type {
  BrowserActionRequest,
  BrowserActionResponseView,
  FleetOverviewView,
  GitReposInspectionView,
  GoalRunRecord,
  HostDetailView,
  ImChannelsSnapshotView,
  LspDiagnostic,
  ProviderKind,
  PushDevicesSnapshotView,
  ReasoningEffort,
  ReleaseUpdateInfo,
  SessionMode,
  SlashDirective,
} from "@/lib/orbit-ipc";
import type {
  ComposerFocusOptions,
  ContextMentionItem,
  HostUpgradeOutcome,
  OrbitUiState,
  PendingDraftCreateState,
  PersistedPrefs,
  ReviewScope,
  SettingsSectionTarget,
  SimpleSyncGate,
  StagePullGate,
  TerminalSplitDirection,
  TerminalStreamChannel,
  ThemeMode,
  ToastAction,
  UndoArchiveEntry,
  WindowedAgentStageView,
} from "./store-types";

/**
 * Compile-time checked subset of `orbitActions` invoked across slice boundaries.
 * Decouples individual domain slices from `orbit-store.ts` with zero circular imports.
 */
export interface OrbitCrossSliceActions {
  init(): Promise<void>;
  refreshSidebarView(): void;
  reloadFleetOnly(): Promise<void>;
  reconnectHost(hostId: string): Promise<void>;
  checkFleetUpdates(opts?: { silent?: boolean }): Promise<ReleaseUpdateInfo | null>;
  installDesktopAppUpdate(explicitRelease?: ReleaseUpdateInfo | null): Promise<boolean>;
  upgradeHostDaemon(
    hostId: string,
    opts?: {
      force?: boolean;
      silent?: boolean;
      targetVersion?: string | null;
      skipReload?: boolean;
    },
  ): Promise<HostUpgradeOutcome>;
  upgradeAllConnectedHosts(opts?: {
    force?: boolean;
    includeDesktopApp?: boolean;
    onlyHostIds?: string[];
  }): Promise<void>;
  reloadFleetAndHosts(initialBootstrap?: boolean): Promise<void>;
  reloadHostDetail(hostId: string, includeFleetOverview?: boolean): Promise<void>;
  pullStageDelta(hostId: string, agentId: string, forceFull?: boolean): Promise<void>;
  loadEarlierStageTurns(
    hostId: string,
    agentId: string,
    targetTurnIndex?: number,
  ): Promise<number>;
  selectHost(hostId: string, opts?: { keepDraft?: boolean }): void;
  selectAgent(hostId: string, agentId: string, jumpToSeq?: number | null): void;
  startNewThreadDraft(
    hostId?: string,
    cwd?: string,
    prefillPrompt?: string,
    prefillSlash?: ComposerInlineDirective | null,
    workspaceId?: string | null,
  ): void;
  focusComposer(opts?: ComposerFocusOptions): void;
  setDraftConfig(patch: {
    provider?: ProviderKind;
    acpAgentId?: string | null;
    model?: string | null;
    effort?: ReasoningEffort;
    fastMode?: boolean;
    mode?: SessionMode;
    profileId?: string | null;
    useWorktree?: boolean;
    worktreeBranch?: string;
    worktreeRepoPath?: string | null;
    workspaceId?: string | null;
  }): void;
  toggleActiveFastMode(action?: "toggle" | "enable" | "disable" | boolean): void;
  setComposerDraft(text: string): void;
  ensureWorkspaceFiles(hostId: string, cwd: string, force?: boolean): Promise<void>;
  addContextMention(item: ContextMentionItem): void;
  captureActiveTerminalTail(): { label: string; lineCount: number; content: string } | null;
  _submitComposerUnlocked(
    hostId: string,
    key: string,
    steerImmediately: boolean,
    rawDraft: string,
    storedDirective: SlashDirective | null,
  ): Promise<void>;
  forkAgent(opts: {
    hostId: string;
    sourceAgentId: string;
    upToSeq?: number | null;
    worktree?: boolean;
    branch?: string;
    baseRef?: string;
    title?: string;
    initialPrompt?: string;
  }): Promise<boolean>;
  archiveThread(hostId: string, agentId: string): void;
  restoreArchivedThread(hostId: string, agentId: string, options?: { open?: boolean }): void;
  addProjectDirectory(hostId: string, dirPath: string, name?: string): Promise<void>;
  toggleSidePanel(openOverride?: boolean): void;
  openSidePanelNewTab(opts?: { skipUnsavedGuard?: boolean }): void;
  markOpenFilesStaleForWorkspace(hostId: string, cwd: string): void;
  focusSidePanelTab(tabId: string, opts?: { skipUnsavedGuard?: boolean }): void;
  closeSidePanelTab(tabId: string, opts?: { skipUnsavedGuard?: boolean }): void;
  openReviewTab(opts?: { path?: string | null; scope?: ReviewScope }): void;
  toggleReviewTab(openOverride?: boolean): void;
  openFilesExplorerTab(): void;
  openExportDialog(hostId: string, agentId: string): void;
  openCopyCodeBlocksModal(hostId: string, agentId: string): Promise<void>;
  openFileInSidePanel(
    rawPath: string,
    opts?: {
      focusLine?: number | null;
      focusEndLine?: number | null;
      hostId?: string;
      cwd?: string;
      fallbackPath?: string;
      initialMode?: "view" | "edit";
      skipUnsavedGuard?: boolean;
    },
  ): void;
  prefetchWorkspaceFile(
    rawPath: string,
    opts?: {
      hostId?: string;
      cwd?: string;
      fallbackPath?: string;
      intent?: "hover" | "press";
    },
  ): void;
  loadWorkspaceFile(
    hostId: string,
    cwd: string,
    path: string,
    forceReload?: boolean,
  ): Promise<void>;
  enterFileEditMode(hostId: string, cwd: string, path: string): void;
  discardFileEditorDraft(hostId: string, cwd: string, path: string): void;
  saveWorkspaceFile(
    hostId: string,
    cwd: string,
    path: string,
    force?: boolean,
  ): Promise<"saved" | "conflict" | "error">;
  loadImChannels(hostId: string): Promise<ImChannelsSnapshotView | null>;
  loadPushDevices(hostId: string): Promise<PushDevicesSnapshotView | null>;
  loadFileDiagnostics(
    hostId: string,
    cwd: string,
    path: string,
    silent?: boolean,
  ): Promise<LspDiagnostic[]>;
  startGoalRun(req: {
    hostId: string;
    agentId: string;
    objective: string;
    criteria?: string[];
    verifyCommand?: string | null;
    maxIterations?: number | null;
    maxDurationSecs?: number | null;
  }): Promise<GoalRunRecord | null>;
  executeBrowserAction(
    hostId: string,
    sessionId: string,
    action: BrowserActionRequest,
  ): Promise<BrowserActionResponseView | null>;
  openTerminalTab(opts?: { spawnIfEmpty?: boolean }): void;
  toggleTerminalTab(openOverride?: boolean): void;
  splitActiveTerminalPane(direction: TerminalSplitDirection): void;
  setTerminalSplitRatios(splitId: string, ratios: number[]): void;
  runCommandInTerminal(
    command: string,
    opts?: { execute?: boolean; hostId?: string; cwd?: string },
  ): void;
  onTerminalTabAttached(hostId: string, tabKey: string, terminalId: string): void;
  openBrowserTab(initialUrl?: string): void;
  openSideQuestionTab(opts?: { entryId?: string }): void;
  submitSideQuestion(
    rawQuestion?: string,
    targetOverride?: {
      hostId: string;
      scopeKey?: string;
      isDraftNewSession: boolean;
      agentId: string | null;
      workspaceCwd: string;
      draftProvider?: ProviderKind;
      draftAcpAgentId?: string | null;
      draftModels?: Partial<Record<ProviderKind, string | null>>;
    },
  ): Promise<void>;
  openProviderSubagentTimeline(
    hostId: string,
    parentAgentId: string,
    subagentId: string,
  ): Promise<void>;
  refreshGit(
    hostId: string,
    cwd: string,
    path: string | null,
    staged: boolean,
  ): Promise<void>;
  loadGitReviewSnapshot(hostId: string, cwd: string, path: string | null): Promise<void>;
  spawnNewTerminalTab(hostId: string, cwd: string): void;
  openSettings(section?: SettingsSectionTarget): void;
  closeSettings(): void;
  closeOnboardingGuide(markCompleted?: boolean): void;
  setTheme(theme: ThemeMode): void;
  inspectGitRepos(
    hostId: string,
    path: string,
    extraDirs?: string[],
  ): Promise<GitReposInspectionView | null>;
  openPeerSplitTab(hostId: string, peerAgentId: string, title?: string): void;
  openSubagentTab(opts: {
    callId: string;
    role: string;
    promptPreview?: string;
    childAgentId?: string | null;
    childHostId?: string | null;
    providerSubagentId?: string | null;
    parentAgentId?: string | null;
  }): void;
  fetchWorkspaceBoardAndEvents(hostId: string, workspaceId: string): Promise<void>;
  showToast(msg: string, action?: ToastAction | null): void;
  hydrateSyncedPreferencesFromCore(): Promise<void>;
}

export interface OrbitStoreContext {
  getState(): OrbitUiState;
  setState(updater: (prev: OrbitUiState) => OrbitUiState, persist?: boolean): void;
  replaceStateSilent(next: OrbitUiState): void;
  readonly actions: OrbitCrossSliceActions;

  isInitialized(): boolean;
  setInitialized(value: boolean): void;
  isDisposed(): boolean;
  readonly initialPrefs: PersistedPrefs;

  scheduleSavePrefs(): void;
  notifyToast(msg: string, undo?: UndoArchiveEntry | null, action?: ToastAction | null): void;
  dispatchDesktopNotification(opts: {
    title: string;
    body: string;
    hostId: string;
    agentId?: string | null;
    tag?: string;
    tone?: NotificationSoundTone;
    forceEvenIfVisible?: boolean;
  }): boolean;
  syncAttentionIndicators(
    overview: FleetOverviewView,
    details: Record<string, HostDetailView>,
  ): void;
  maybeAutoSyncOutdatedHosts(): void;
  subscribeCoreEvents(): Promise<void>;

  scheduleAutoRefreshGit(
    hostId: string | null | undefined,
    cwd: string | null | undefined,
    opts?: {
      delayMs?: number;
      minIntervalMs?: number;
      reason?: "stream" | "settled" | "focus";
    },
  ): void;
  drainPromptQueueIfIdle(hostId: string, detail: HostDetailView): Promise<void>;

  readonly draftPersistence: DraftPersistenceController;
  emitComposerDraftChange(): void;
  emitDraftPresenceChange(): void;
  readonly composerFocusListeners: Set<(opts?: ComposerFocusOptions) => void>;

  readonly terminalChannels: Map<string, TerminalStreamChannel>;
  readonly detachedTerminalIds: Set<string>;

  readonly stagePullGates: Map<string, StagePullGate>;
  readonly hostSyncGates: Map<string, SimpleSyncGate>;
  readonly fleetSyncGate: SimpleSyncGate;
  readonly sidebarSyncGate: SimpleSyncGate;

  applyStageCacheUpdate(
    s: OrbitUiState,
    tlKey: string,
    nextStage: WindowedAgentStageView,
  ): Record<string, WindowedAgentStageView>;
  recordStageKeyAccess(tlKey: string): void;
  pruneStageKeyAccess(keysToRemove: ReadonlySet<string>): void;

  flushPendingSeenMarks(onlyKey?: string): void;
  markThreadSeenInCore(hostId: string, agentId: string, seq: number, force?: boolean): void;
  readonly lastReportedSeenSeq: Map<string, number>;

  getPendingDraftCreate(): PendingDraftCreateState | null;
  setPendingDraftCreate(next: PendingDraftCreateState | null): void;

  readonly notifiedTerminalGoals: Set<string>;
  trackNotifiedTerminalGoal(key: string): void;

  readonly fileLoadSeqByKey: Map<string, number>;
  bumpFileLoadSeq(cKey: string): number;
}
