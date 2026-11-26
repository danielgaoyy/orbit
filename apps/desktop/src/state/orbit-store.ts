import { preferenceDefaults } from "./preference-defaults";
import { useCallback, useRef, useSyncExternalStore } from "react";
import {
  DRAFTS_STORAGE_KEY,
  createDraftPersistenceController,
  hasNonEmptyScopeDraft,
} from "@/lib/draft-persistence";
import { registerOrbitLinkOpener } from "@/lib/file-links";
import {
  type NotificationSoundTone,
} from "@/lib/notification-sound";
import { dispatchDesktopNotificationWithNativeFallback } from "@/lib/notifications";
import {
  type CoreNotification,
  type FleetOverviewView,
  type HostDetailView,
  compareSemver,
  isAgentForegroundRunning,
  isLinuxDesktop,
  isMacDesktop,
  isWindowsDesktop,
  normalizeSemver,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import {
  MAX_CACHED_STAGE_VIEWS,
  collectPinnedStageKeys,
  computeGitReviewSignature,
  evictInactiveStageEntries,
  normalizeCwdForCompare,
  recordStageAccess,
  removeStageAccessKeys,
} from "@/lib/stage-cache";
import { createAutomationsSlice } from "./slices/automations-slice";
import { createFleetSlice } from "./slices/fleet-slice";
import { createPreferencesSlice } from "./slices/preferences-slice";
import { createStageStreamSlice } from "./slices/stage-stream-slice";
import { createTerminalSlice } from "./slices/terminal-slice";
import { createUpdaterSlice } from "./slices/updater-slice";
import { createWorkspaceSlice } from "./slices/workspace-slice";
import {
  SIDE_PANEL_STORAGE_KEY,
  loadPersistedSidePanelSnapshots,
  resolveSidePanelScopeKey,
  savePersistedSidePanelSnapshots,
  sidePanelTabScopeKey,
} from "./side-panel-scope";
import {
  MAX_NOTIFIED_TERMINAL_GOALS,
  PREFS_STORAGE_KEY,
  SEEN_REPORT_DEBOUNCE_MS,
  applyDocumentTheme,
  buildPersistedPrefsPayload,
  collectChangedRecordKeys,
  composerScopeKey,
  createDefaultInitialState,
  detectSystemTheme,
  fileCacheKey,
  isAutoRefreshableCwd,
  isHourInQuietWindow,
  isSubagentArchived,
  loadPrefs,
  resolveFleetTargetVersion,
  resolveTheme,
  scheduleCoalescedGate,
  sideQuestionScopeKey,
  subagentArchiveKey,
  threadScopeKey,
} from "./store-helpers";
import type { OrbitCrossSliceActions, OrbitStoreContext } from "./store-runtime";
import {
  ALL_TOPICS_MASK,
  STORE_TOPICS,
  type StoreTopic,
  TOPIC_BIT,
  type TopicSubscriptionHandle,
  computeChangedTopicMask,
  createTopicStoreKernel,
  evaluateSelectorWithTopicTracking,
  topicsToMask,
} from "./store-topics";
import type {
  AutoGitRefreshGate,
  ComposerFocusOptions,
  OrbitStoreHmrData,
  OrbitUiState,
  PendingDraftCreateState,
  ResolvedTheme,
  SimpleSyncGate,
  StagePullGate,
  TerminalStreamChannel,
  ThemeMode,
  ToastAction,
  UndoArchiveEntry,
  WindowedAgentStageView,
} from "./store-types";

export {
  SIDE_PANEL_STORAGE_KEY,
  STORE_TOPICS,
  TOPIC_BIT,
  composerScopeKey,
  fileCacheKey,
  isSubagentArchived,
  resolveFleetTargetVersion,
  resolveSidePanelScopeKey,
  sidePanelTabScopeKey,
  sideQuestionScopeKey,
  subagentArchiveKey,
  threadScopeKey,
};
export type { StoreTopic };
export type {
  ActiveTurnSendBehavior,
  ActiveViewMode,
  ArchivedThreadEntry,
  ComposerFocusOptions,
  ContextMentionItem,
  DiffLayoutMode,
  DiffQuoteSnippet,
  FileEditorDraftState,
  FileViewCacheEntry,
  FleetUpdatePhase,
  FleetUpdateState,
  HostDaemonUpgradeState,
  HostSettingsTab,
  HostUpgradeOutcome,
  InlineMentionTarget,
  InstructionsTarget,
  MediaPreviewTarget,
  OrbitUiState,
  OverviewSectionId,
  PastedAttachment,
  PendingJumpTarget,
  PendingUnsavedFileAction,
  QueuedPromptItem,
  ResolvedTheme,
  ReviewScope,
  SettingsCoreSectionId,
  SettingsSection,
  SettingsSectionTarget,
  SettingsSubTabId,
  SidePanelOpenSource,
  SidePanelScopeKey,
  SidePanelSnapshot,
  SidePanelTab,
  SidePanelTabKind,
  SideQuestionEntry,
  SidebarProjectSort,
  SidebarThreadFilter,
  SlashDirective,
  TerminalCursorStyle,
  TerminalSplitDirection,
  TerminalSplitGroup,
  TerminalTabSession,
  TerminalThemeMode,
  ThemeMode,
  ToastAction,
  UndoArchiveEntry,
  WorkspaceLineEnding,
  WorkspacePanelMode,
  WorktreeHandoffDialogTarget,
} from "./store-types";

const viteHot = (
  import.meta as {
    hot?: {
      data: OrbitStoreHmrData;
      dispose: (cb: (data: OrbitStoreHmrData) => void) => void;
    };
  }
).hot;
const hmrData = viteHot?.data;

const initialPrefs = loadPrefs();
const initialTheme: ThemeMode = initialPrefs.theme ?? preferenceDefaults.theme;
const initialResolvedTheme: ResolvedTheme = resolveTheme(initialTheme);
applyDocumentTheme(initialResolvedTheme);
if (typeof document !== "undefined") {
  if (isMacDesktop()) {
    document.documentElement.classList.add("orbit-mac");
  } else if (isWindowsDesktop()) {
    document.documentElement.classList.add("orbit-windows");
  } else if (isLinuxDesktop()) {
    document.documentElement.classList.add("orbit-linux");
  }
}

let disposed = false;
let initialized = false;
let autoInitScheduled = false;

const draftPersistence = createDraftPersistenceController(() => kernel.getState(), {
  onPersistPayload: (json) => {
    if (json === null) {
      void orbitIpc.deleteComposerDraft(DRAFTS_STORAGE_KEY);
    } else {
      void orbitIpc.setComposerDraft(DRAFTS_STORAGE_KEY, json);
    }
  },
});
const initialDrafts = draftPersistence.hydrate(initialPrefs.archivedThreads);
if (hmrData?.draftUpdatedAtByScope) {
  draftPersistence.restoreUpdatedAtByScope(hmrData.draftUpdatedAtByScope);
}

const initialSidePanelByScope = loadPersistedSidePanelSnapshots();

const defaultInitialState = createDefaultInitialState(
  initialPrefs,
  initialTheme,
  initialResolvedTheme,
  initialDrafts,
  initialSidePanelByScope,
);

const initialState: OrbitUiState = hmrData?.state
  ? { ...defaultInitialState, ...hmrData.state }
  : defaultInitialState;

const kernel = createTopicStoreKernel<OrbitUiState>(initialState, () => disposed);

const composerDraftListeners = new Set<() => void>();
const draftPresenceListeners = new Set<() => void>();
const composerFocusListeners = new Set<(opts?: ComposerFocusOptions) => void>();

const autoUpgradeAttempted = new Map<string, string>();
const stagePullGates = new Map<string, StagePullGate>();
const TIMELINE_STREAM_COALESCE_MS = 32;
const timelineStreamCoalescers = new Map<
  string,
  { lastDispatchedAt: number; timerId: ReturnType<typeof setTimeout> | null }
>();

function scheduleCoalescedTimelinePull(hostId: string, agentId: string): void {
  if (typeof window === "undefined" || typeof window.setTimeout !== "function") {
    void orbitActions.pullStageDelta(hostId, agentId);
    return;
  }
  const key = `${hostId}::${agentId}`;
  const now = typeof performance !== "undefined" ? performance.now() : Date.now();
  let state = timelineStreamCoalescers.get(key);
  if (!state) {
    state = { lastDispatchedAt: 0, timerId: null };
    timelineStreamCoalescers.set(key, state);
  }
  const elapsed = now - state.lastDispatchedAt;
  if (state.timerId === null && elapsed >= TIMELINE_STREAM_COALESCE_MS) {
    state.lastDispatchedAt = now;
    void orbitActions.pullStageDelta(hostId, agentId);
    return;
  }
  if (state.timerId !== null) return;
  const delay = Math.max(1, Math.ceil(TIMELINE_STREAM_COALESCE_MS - elapsed));
  state.timerId = setTimeout(() => {
    const cur = timelineStreamCoalescers.get(key);
    if (cur) {
      cur.timerId = null;
      cur.lastDispatchedAt = typeof performance !== "undefined" ? performance.now() : Date.now();
    }
    if (disposed) return;
    void orbitActions.pullStageDelta(hostId, agentId);
  }, delay);
}
const hostSyncGates = new Map<string, SimpleSyncGate>();
const fleetSyncGate: SimpleSyncGate = { inFlight: false, queued: false, rafId: null };
const sidebarSyncGate: SimpleSyncGate = { inFlight: false, queued: false, rafId: null };
const lastReportedSeenSeq = new Map<string, number>();
const pendingSeenTimers = new Map<
  string,
  { hostId: string; agentId: string; seq: number; timerId: number }
>();
const autoGitRefreshGates = new Map<string, AutoGitRefreshGate>();
const terminalChannels = hmrData?.terminalChannels ?? new Map<string, TerminalStreamChannel>();
const detachedTerminalIds = new Set<string>();
const notifiedApprovalAgents = new Set<string>();
const notifiedTerminalGoals = new Set<string>();
const fileLoadSeqByKey = new Map<string, number>();

let stageAccessOrder: string[] = [];
let gitChangedDebounceTimer: number | null = null;
let toastTimer: ReturnType<typeof setTimeout> | null = null;
let savePrefsTimer: ReturnType<typeof setTimeout> | null = null;
let savePrefsPending = false;
let saveSidePanelTimer: ReturnType<typeof setTimeout> | null = null;
let saveSidePanelPending = false;
let lastTrayStatusKey: string | null = null;
let trayStatusSyncTimer: ReturnType<typeof setTimeout> | null = null;
let pendingDraftCreate: PendingDraftCreateState | null = null;

function writePrefsNow(snap: OrbitUiState): void {
  if (typeof window === "undefined") return;
  savePrefsPending = false;
  if (savePrefsTimer !== null) {
    clearTimeout(savePrefsTimer);
    savePrefsTimer = null;
  }
  try {
    window.localStorage.setItem(
      PREFS_STORAGE_KEY,
      JSON.stringify(buildPersistedPrefsPayload(snap)),
    );
  } catch {
    // Ignore storage quota errors
  }
}

function scheduleSavePrefs(): void {
  if (typeof window === "undefined") return;
  savePrefsPending = true;
  if (savePrefsTimer !== null) return;
  savePrefsTimer = setTimeout(() => {
    savePrefsTimer = null;
    if (disposed) return;
    if (savePrefsPending) {
      writePrefsNow(kernel.getState());
    }
  }, 180);
}

function writeSidePanelNow(snap: OrbitUiState): void {
  if (typeof window === "undefined") return;
  saveSidePanelPending = false;
  if (saveSidePanelTimer !== null) {
    clearTimeout(saveSidePanelTimer);
    saveSidePanelTimer = null;
  }
  savePersistedSidePanelSnapshots(snap);
}

function scheduleSaveSidePanel(): void {
  if (typeof window === "undefined") return;
  saveSidePanelPending = true;
  if (saveSidePanelTimer !== null) return;
  saveSidePanelTimer = setTimeout(() => {
    saveSidePanelTimer = null;
    if (disposed) return;
    if (saveSidePanelPending) {
      writeSidePanelNow(kernel.getState());
    }
  }, 220);
}

function hasSidePanelStateChanged(prev: OrbitUiState, next: OrbitUiState): boolean {
  return (
    prev.sidePanelOpen !== next.sidePanelOpen ||
    prev.sidePanelTabs !== next.sidePanelTabs ||
    prev.activeSidePanelTabId !== next.activeSidePanelTabId ||
    prev.sidePanelSplit !== next.sidePanelSplit ||
    prev.sidePanelByScope !== next.sidePanelByScope ||
    prev.sidePanelScopeKey !== next.sidePanelScopeKey ||
    prev.sidePanelUserDismissed !== next.sidePanelUserDismissed ||
    prev.sidePanelClosedTabs !== next.sidePanelClosedTabs ||
    prev.sidePanelUnseenTabIds !== next.sidePanelUnseenTabIds ||
    prev.sidePanelCollapsedByResize !== next.sidePanelCollapsedByResize
  );
}

function emitComposerDraftChange(): void {
  if (disposed) return;
  for (const l of composerDraftListeners) {
    l();
  }
}

function emitDraftPresenceChange(): void {
  if (disposed) return;
  for (const l of draftPresenceListeners) {
    l();
  }
}

function syncDraftMutations(prev: OrbitUiState, next: OrbitUiState): void {
  if (
    prev.composerDrafts === next.composerDrafts &&
    prev.pastedAttachments === next.pastedAttachments &&
    prev.diffQuotes === next.diffQuotes &&
    prev.contextItems === next.contextItems &&
    prev.inlineMentions === next.inlineMentions &&
    prev.slashDirectives === next.slashDirectives &&
    prev.sideQuestionDrafts === next.sideQuestionDrafts
  ) {
    return;
  }

  const changedKeys = new Set<string>();
  collectChangedRecordKeys(prev.composerDrafts, next.composerDrafts, changedKeys);
  collectChangedRecordKeys(prev.pastedAttachments, next.pastedAttachments, changedKeys);
  collectChangedRecordKeys(prev.diffQuotes, next.diffQuotes, changedKeys);
  collectChangedRecordKeys(prev.contextItems, next.contextItems, changedKeys);
  collectChangedRecordKeys(prev.inlineMentions, next.inlineMentions, changedKeys);
  collectChangedRecordKeys(prev.slashDirectives, next.slashDirectives, changedKeys);
  collectChangedRecordKeys(prev.sideQuestionDrafts, next.sideQuestionDrafts, changedKeys);

  if (changedKeys.size === 0) return;

  const dirtyKeys: string[] = [];
  const clearedKeys: string[] = [];
  let presenceChanged = false;

  for (const key of changedKeys) {
    const hadDraft = hasNonEmptyScopeDraft(prev, key);
    const hasDraft = hasNonEmptyScopeDraft(next, key);
    if (hadDraft !== hasDraft) {
      presenceChanged = true;
    }
    if (hasDraft) {
      dirtyKeys.push(key);
    } else {
      clearedKeys.push(key);
    }
  }

  if (dirtyKeys.length > 0) {
    draftPersistence.markDirty(dirtyKeys);
  }
  if (clearedKeys.length > 0) {
    draftPersistence.removeScopes(clearedKeys, false);
  }
  if (presenceChanged) {
    emitDraftPresenceChange();
  }
}

function setState(updater: (prev: OrbitUiState) => OrbitUiState, persist = false): void {
  if (disposed) return;
  const prev = kernel.getState();
  const next = updater(prev);
  if (next === prev) return;
  const dirtyMask = computeChangedTopicMask(prev, next);
  kernel.replaceStateSilent(next);
  if (persist) {
    scheduleSavePrefs();
  }
  if (hasSidePanelStateChanged(prev, next)) {
    scheduleSaveSidePanel();
  }
  syncDraftMutations(prev, next);
  if (next.composerDrafts !== prev.composerDrafts) {
    emitComposerDraftChange();
  }
  if (dirtyMask !== 0) {
    kernel.emitMask(dirtyMask);
  }
}

function bumpFileLoadSeq(cKey: string): number {
  const next = (fileLoadSeqByKey.get(cKey) ?? 0) + 1;
  fileLoadSeqByKey.set(cKey, next);
  return next;
}

function applyStageCacheUpdate(
  s: OrbitUiState,
  tlKey: string,
  nextStage: WindowedAgentStageView,
): Record<string, WindowedAgentStageView> {
  const merged = {
    ...s.stages,
    [tlKey]: nextStage,
  };
  stageAccessOrder = recordStageAccess(stageAccessOrder, tlKey);
  const pinnedKeys = collectPinnedStageKeys({
    selectedHostId: s.selectedHostId,
    selectedAgentId: s.selectedAgentId,
    isDraftNewSession: s.isDraftNewSession,
    sidePanelTabs: s.sidePanelTabs,
  });
  for (const [k, gate] of stagePullGates.entries()) {
    if (gate.inFlight) {
      pinnedKeys.add(k);
    }
  }
  const { stages, accessOrder, evictedKeys } = evictInactiveStageEntries(
    merged,
    stageAccessOrder,
    pinnedKeys,
    MAX_CACHED_STAGE_VIEWS,
  );
  stageAccessOrder = accessOrder;
  for (const evictedKey of evictedKeys) {
    const gate = stagePullGates.get(evictedKey);
    if (gate && !gate.inFlight) {
      stagePullGates.delete(evictedKey);
    }
  }
  return stages;
}

function runAutoRefreshGit(hostId: string, gateKey: string): void {
  const gate = autoGitRefreshGates.get(gateKey);
  if (!gate || disposed) return;
  if (gate.timerId !== null && typeof window !== "undefined") {
    window.clearTimeout(gate.timerId);
    gate.timerId = null;
    gate.timerDueAt = 0;
  }
  if (typeof document !== "undefined" && document.visibilityState === "hidden") {
    return;
  }
  if (gate.inFlight) {
    gate.queued = true;
    return;
  }
  const targetCwd = gate.cwd;
  if (!isAutoRefreshableCwd(targetCwd)) return;
  gate.inFlight = true;
  gate.queued = false;
  gate.lastStartedAt = Date.now();
  void orbitActions
    .refreshGit(hostId, targetCwd, null, kernel.getState().diffStaged)
    .catch(() => undefined)
    .finally(() => {
      gate.inFlight = false;
      if (gate.queued && !disposed) {
        gate.queued = false;
        runAutoRefreshGit(hostId, gateKey);
      }
    });
}

function scheduleAutoRefreshGit(
  hostId: string | null | undefined,
  cwd: string | null | undefined,
  opts?: {
    delayMs?: number;
    minIntervalMs?: number;
    reason?: "stream" | "settled" | "focus";
  },
): void {
  if (disposed || typeof window === "undefined") return;
  if (!hostId || !isAutoRefreshableCwd(cwd)) return;
  if (typeof document !== "undefined" && document.visibilityState === "hidden") return;

  const normCwd = normalizeCwdForCompare(cwd);
  if (!normCwd) return;
  const gateKey = `${hostId}::${normCwd}`;
  let gate = autoGitRefreshGates.get(gateKey);
  if (!gate) {
    gate = {
      timerId: null,
      timerDueAt: 0,
      trailingTimerId: null,
      inFlight: false,
      queued: false,
      lastStartedAt: 0,
      lastFocusTriggerAt: 0,
      cwd,
    };
    autoGitRefreshGates.set(gateKey, gate);
  } else {
    gate.cwd = cwd;
  }

  const now = Date.now();
  const reason = opts?.reason ?? "settled";

  if (reason === "focus") {
    if (now - gate.lastFocusTriggerAt < 3000) return;
    gate.lastFocusTriggerAt = now;
  }

  const baseDelay = opts?.delayMs ?? 800;
  const minInterval = opts?.minIntervalMs ?? 0;
  const cooldownRemaining =
    minInterval > 0 && gate.lastStartedAt > 0
      ? Math.max(0, minInterval - (now - gate.lastStartedAt))
      : 0;
  const effectiveDelay = Math.max(baseDelay, cooldownRemaining);
  const targetDueAt = now + effectiveDelay;

  if (gate.timerId !== null) {
    if (targetDueAt < gate.timerDueAt) {
      window.clearTimeout(gate.timerId);
      gate.timerId = null;
      gate.timerDueAt = 0;
    } else {
      return;
    }
  }

  gate.timerDueAt = targetDueAt;
  gate.timerId = window.setTimeout(() => {
    gate!.timerId = null;
    gate!.timerDueAt = 0;
    runAutoRefreshGit(hostId, gateKey);
  }, effectiveDelay);
}

function scheduleStreamAutoRefreshGit(
  hostId: string | null | undefined,
  cwd: string | null | undefined,
): void {
  if (disposed || typeof window === "undefined") return;
  if (!hostId || !isAutoRefreshableCwd(cwd)) return;
  if (typeof document !== "undefined" && document.visibilityState === "hidden") return;

  scheduleAutoRefreshGit(hostId, cwd, {
    delayMs: 1800,
    minIntervalMs: 5000,
    reason: "stream",
  });

  const normCwd = normalizeCwdForCompare(cwd);
  const gateKey = `${hostId}::${normCwd}`;
  const gate = autoGitRefreshGates.get(gateKey);
  if (!gate) return;
  if (gate.trailingTimerId !== null) {
    window.clearTimeout(gate.trailingTimerId);
  }
  gate.trailingTimerId = window.setTimeout(() => {
    gate.trailingTimerId = null;
    if (disposed) return;
    if (Date.now() - gate.lastStartedAt >= 1200) {
      runAutoRefreshGit(hostId, gateKey);
    }
  }, 1200);
}

function flushPendingSeenMarks(onlyKey?: string): void {
  const keys = onlyKey ? [onlyKey] : Array.from(pendingSeenTimers.keys());
  for (const key of keys) {
    const entry = pendingSeenTimers.get(key);
    if (!entry) continue;
    pendingSeenTimers.delete(key);
    if (typeof window !== "undefined") {
      window.clearTimeout(entry.timerId);
    }
    void orbitIpc.markAgentSeen(entry.hostId, entry.agentId, entry.seq);
  }
}

function markThreadSeenInCore(
  hostId: string,
  agentId: string,
  seq: number,
  force = false,
): void {
  const key = threadScopeKey(hostId, agentId);
  const prev = lastReportedSeenSeq.get(key) ?? -1;
  if (!force && seq <= prev) return;
  const nextSeq = Math.max(seq, prev);
  lastReportedSeenSeq.set(key, nextSeq);

  if (force || typeof window === "undefined") {
    const existing = pendingSeenTimers.get(key);
    if (existing) {
      pendingSeenTimers.delete(key);
      if (typeof window !== "undefined") {
        window.clearTimeout(existing.timerId);
      }
    }
    void orbitIpc.markAgentSeen(hostId, agentId, nextSeq);
    return;
  }

  const existing = pendingSeenTimers.get(key);
  if (existing) {
    existing.seq = Math.max(existing.seq, nextSeq);
    return;
  }
  const timerId = window.setTimeout(() => {
    const current = pendingSeenTimers.get(key);
    if (!current) return;
    pendingSeenTimers.delete(key);
    void orbitIpc.markAgentSeen(current.hostId, current.agentId, current.seq);
  }, SEEN_REPORT_DEBOUNCE_MS);
  pendingSeenTimers.set(key, { hostId, agentId, seq: nextSeq, timerId });
}

function trackNotifiedTerminalGoal(key: string): void {
  if (notifiedTerminalGoals.size >= MAX_NOTIFIED_TERMINAL_GOALS) {
    const oldest = notifiedTerminalGoals.values().next().value;
    if (oldest !== undefined) {
      notifiedTerminalGoals.delete(oldest);
    }
  }
  notifiedTerminalGoals.add(key);
}

function isInDesktopQuietHours(): boolean {
  const snap = kernel.getState();
  if (!snap.notifyQuietHoursEnabled) return false;
  return isHourInQuietWindow(
    new Date().getHours(),
    snap.notifyQuietStartHour,
    snap.notifyQuietEndHour,
  );
}

function scheduleTrayStatusSync(runningCount: number, pendingApprovals: number): void {
  const nextKey = `${runningCount}:${pendingApprovals}`;
  if (nextKey === lastTrayStatusKey) return;
  if (trayStatusSyncTimer !== null) {
    clearTimeout(trayStatusSyncTimer);
  }
  trayStatusSyncTimer = setTimeout(() => {
    trayStatusSyncTimer = null;
    if (disposed) return;
    lastTrayStatusKey = nextKey;
    void orbitIpc.updateTrayStatus(runningCount, pendingApprovals).catch(() => undefined);
  }, 140);
}

function dispatchDesktopNotification(opts: {
  title: string;
  body: string;
  hostId: string;
  agentId?: string | null;
  tag?: string;
  tone?: NotificationSoundTone;
  forceEvenIfVisible?: boolean;
  permissionRequestId?: string | null;
  isPlanApproval?: boolean;
  humanOnly?: boolean;
  riskLevel?: string | null;
  allowDecision?: string | null;
  denyDecision?: string | null;
}): boolean {
  const snap = kernel.getState();
  return dispatchDesktopNotificationWithNativeFallback(opts, {
    soundEnabled: snap.notifySoundEnabled,
    inQuietHours: isInDesktopQuietHours(),
    onFocusTarget: (hostId, agentId) => {
      orbitActions.closeSettings();
      orbitActions.closeOnboardingGuide(false);
      if (hostId && agentId) {
        orbitActions.selectAgent(hostId, agentId);
      } else if (hostId) {
        orbitActions.selectHost(hostId);
      }
    },
  });
}

function notifyToast(
  msg: string,
  undo?: UndoArchiveEntry | null,
  action?: ToastAction | null,
): void {
  if (toastTimer) clearTimeout(toastTimer);
  setState((s) => ({
    ...s,
    toastNotice: msg,
    lastArchivedUndo: undo ?? null,
    toastAction: action ?? null,
  }));
  toastTimer = setTimeout(() => {
    setState((s) => ({
      ...s,
      toastNotice: null,
      lastArchivedUndo: null,
      toastAction: null,
    }));
  }, undo ? 6000 : action ? 8000 : 3000);
}

function maybeAutoSyncOutdatedHosts(): void {
  if ((import.meta as { env?: { DEV?: boolean } }).env?.DEV) return;
  const snap = kernel.getState();
  const normApp = normalizeSemver(snap.appVersion);
  if (!normApp || snap.isBrowserOffline) return;

  for (const host of snap.fleet.hosts) {
    const isOnline =
      host.connection_state === "online_direct" || host.connection_state === "online_relay";
    if (!isOnline) continue;

    const normDaemon = normalizeSemver(host.daemon_version);
    if (!normDaemon) continue;

    if (compareSemver(normDaemon, normApp) >= 0) {
      if (kernel.getState().hostUpgradeStates[host.host_id]?.status === "failed") {
        setState((s) => {
          const next = { ...s.hostUpgradeStates };
          delete next[host.host_id];
          return { ...s, hostUpgradeStates: next };
        });
      }
      continue;
    }

    const attemptKey = `${normDaemon}->${normApp}`;
    if (autoUpgradeAttempted.get(host.host_id) === attemptKey) continue;
    if (kernel.getState().hostUpgradeStates[host.host_id]?.status === "upgrading") continue;

    const isLocal = isLocalHostCard(host);
    const supportsNativeWireUpgrade = (host.daemon_protocol_version ?? 0) >= 4;
    if (isLocal || host.ssh_alias || supportsNativeWireUpgrade) {
      autoUpgradeAttempted.set(host.host_id, attemptKey);
      void orbitActions.upgradeHostDaemon(host.host_id, {
        silent: true,
        targetVersion: normApp,
      });
    }
  }
}

function syncAttentionIndicators(
  overview: FleetOverviewView,
  details: Record<string, HostDetailView>,
): void {
  if (typeof document !== "undefined") {
    document.title =
      overview.total_pending_approvals > 0
        ? `(${overview.total_pending_approvals}) Orbit`
        : "Orbit";
  }

  let runningCount = 0;
  for (const detail of Object.values(details)) {
    for (const ag of detail.agents) {
      if (!ag.archived_at_rfc3339 && isAgentForegroundRunning(ag)) {
        runningCount += 1;
      }
    }
  }
  scheduleTrayStatusSync(runningCount, overview.total_pending_approvals);

  if (!kernel.getState().notifyOnApproval) return;

  for (const detail of Object.values(details)) {
    for (const ag of detail.agents) {
      const key = `${detail.host.host_id}::${ag.agent_id}`;
      if (ag.is_awaiting_permission) {
        if (!notifiedApprovalAgents.has(key)) {
          notifiedApprovalAgents.add(key);
          const pendingPerm = (detail.pending_permissions ?? []).find(
            (p) => p.agent_id === ag.agent_id,
          );
          const decisions = pendingPerm?.allowed_decisions ?? [];
          const allowDecision =
            decisions.find((d) => !/deny|reject|always/i.test(d)) ?? "allow";
          const denyDecision = decisions.find((d) => /deny|reject/i.test(d)) ?? "deny";
          dispatchDesktopNotification({
            title: `Orbit · ${detail.host.display_name}`,
            body: pendingPerm?.title
              ? `${ag.title}: ${pendingPerm.title}`
              : `${ag.title} requires your approval`,
            hostId: detail.host.host_id,
            agentId: ag.agent_id,
            tag: `orbit-approval-${detail.host.host_id}-${ag.agent_id}`,
            tone: "approval",
            permissionRequestId: pendingPerm?.request_id ?? null,
            isPlanApproval: Boolean(pendingPerm?.plan_approval),
            humanOnly: Boolean(pendingPerm?.human_only),
            riskLevel: pendingPerm?.guardian_assessment?.risk_level ?? null,
            allowDecision,
            denyDecision,
          });
        }
      } else {
        notifiedApprovalAgents.delete(key);
      }
    }
  }
}

async function drainPromptQueueIfIdle(hostId: string, detail: HostDetailView): Promise<void> {
  for (const ag of detail.agents) {
    const key = `${hostId}::${ag.agent_id}`;
    const queue = kernel.getState().queuedPrompts[key];
    if (
      queue &&
      queue.length > 0 &&
      !isAgentForegroundRunning(ag) &&
      !ag.is_awaiting_permission
    ) {
      const [nextItem, ...rest] = queue;
      setState((s) => ({
        ...s,
        queuedPrompts: {
          ...s.queuedPrompts,
          [key]: rest,
        },
      }));
      await orbitIpc.sendPrompt(hostId, ag.agent_id, nextItem.wirePrompt, {
        displayPrompt: nextItem.displayText,
        badges: nextItem.badges,
      });
    }
  }
}

function subscribeCoreEvents(): Promise<void> {
  return orbitIpc
    .subscribe((ev: CoreNotification) => {
      if (disposed) return;
      const snap = kernel.getState();
      if (ev.kind === "fleet_changed") {
        scheduleCoalescedGate(fleetSyncGate, () => orbitActions.reloadFleetAndHosts(false));
      } else if (ev.kind === "host_changed") {
        let gate = hostSyncGates.get(ev.host_id);
        if (!gate) {
          gate = { inFlight: false, queued: false, rafId: null };
          hostSyncGates.set(ev.host_id, gate);
        }
        const hostId = ev.host_id;
        scheduleCoalescedGate(gate, () => orbitActions.reloadHostDetail(hostId, true));
      } else if (ev.kind === "timeline_changed") {
        const isMainStage =
          !snap.isDraftNewSession &&
          ev.host_id === snap.selectedHostId &&
          ev.agent_id === snap.selectedAgentId;
        const isOpenTimelineTab =
          snap.sidePanelOpen &&
          snap.sidePanelTabs.some(
            (t) =>
              (t.hostId ?? snap.selectedHostId) === ev.host_id &&
              ((t.kind === "peer" && t.peerAgentId === ev.agent_id) ||
                (t.kind === "subagent" && t.childAgentId === ev.agent_id)),
          );
        if (isMainStage || isOpenTimelineTab) {
          scheduleCoalescedTimelinePull(ev.host_id, ev.agent_id);
        }
        if (isMainStage && snap.selectedWorkspaceCwd) {
          scheduleStreamAutoRefreshGit(ev.host_id, snap.selectedWorkspaceCwd);
        }
      } else if (ev.kind === "git_changed") {
        if (
          ev.host_id === snap.selectedHostId &&
          normalizeCwdForCompare(ev.cwd) === normalizeCwdForCompare(snap.selectedWorkspaceCwd)
        ) {
          const hostId = ev.host_id;
          const cwd = ev.cwd;
          if (gitChangedDebounceTimer !== null) {
            window.clearTimeout(gitChangedDebounceTimer);
          }
          gitChangedDebounceTimer = window.setTimeout(() => {
            gitChangedDebounceTimer = null;
            if (disposed) return;
            void (async () => {
              const rKey = `${hostId}::${cwd}`;
              const prevSig = computeGitReviewSignature(kernel.getState().gitReviews[rKey]);
              await orbitActions.loadGitReviewSnapshot(hostId, cwd, null);
              if (disposed) return;

              const curSnap = kernel.getState();
              const activeDiffPath = curSnap.selectedDiffPath;
              if (activeDiffPath) {
                const loadedFiles = curSnap.gitReviews[rKey]?.diff_files ?? [];
                const normTarget = activeDiffPath.replace(/^\.\//, "");
                const hasSelected = loadedFiles.some(
                  (f) => f.path === activeDiffPath || f.path.replace(/^\.\//, "") === normTarget,
                );
                if (!hasSelected) {
                  await orbitActions.loadGitReviewSnapshot(hostId, cwd, activeDiffPath);
                  if (disposed) return;
                }
              }

              const latestSnap = kernel.getState();
              const nextSig = computeGitReviewSignature(latestSnap.gitReviews[rKey]);
              if (nextSig !== prevSig) {
                orbitActions.markOpenFilesStaleForWorkspace(hostId, cwd);
                void orbitActions.ensureWorkspaceFiles(hostId, cwd, true);
              } else if (!latestSnap.workspaceFilesCache[rKey]) {
                void orbitActions.ensureWorkspaceFiles(hostId, cwd, false);
              }
            })().catch(() => undefined);
          }, 150);
        }
      } else if (ev.kind === "terminal_changed") {
        if (!ev.raw_bytes || ev.raw_bytes.length === 0) return;
        const chKey = `${ev.host_id}::${ev.slot}`;
        let ch = terminalChannels.get(chKey);
        if (!ch) {
          ch = { chunks: [], subs: new Set() };
          terminalChannels.set(chKey, ch);
        }
        const bytes = Uint8Array.from(ev.raw_bytes);
        ch.chunks.push(bytes);
        if (ch.chunks.length > 96) {
          ch.chunks.splice(0, ch.chunks.length - 96);
        }
        for (const sub of ch.subs) {
          sub(bytes);
        }
      }
    })
    .catch(() => undefined);
}

const storeContext: OrbitStoreContext = {
  getState: () => kernel.getState(),
  setState,
  replaceStateSilent: (next) => kernel.replaceStateSilent(next),
  get actions(): OrbitCrossSliceActions {
    return orbitActions;
  },
  isInitialized: () => initialized,
  setInitialized: (value) => {
    initialized = value;
  },
  isDisposed: () => disposed,
  initialPrefs,
  scheduleSavePrefs,
  notifyToast,
  dispatchDesktopNotification,
  syncAttentionIndicators,
  maybeAutoSyncOutdatedHosts,
  subscribeCoreEvents,
  scheduleAutoRefreshGit,
  drainPromptQueueIfIdle,
  draftPersistence,
  emitComposerDraftChange,
  emitDraftPresenceChange,
  composerFocusListeners,
  terminalChannels,
  detachedTerminalIds,
  stagePullGates,
  hostSyncGates,
  fleetSyncGate,
  sidebarSyncGate,
  applyStageCacheUpdate,
  recordStageKeyAccess: (tlKey) => {
    stageAccessOrder = recordStageAccess(stageAccessOrder, tlKey);
  },
  pruneStageKeyAccess: (keysToRemove) => {
    stageAccessOrder = removeStageAccessKeys(stageAccessOrder, keysToRemove);
  },
  flushPendingSeenMarks,
  markThreadSeenInCore,
  lastReportedSeenSeq,
  getPendingDraftCreate: () => pendingDraftCreate,
  setPendingDraftCreate: (next) => {
    pendingDraftCreate = next;
  },
  notifiedTerminalGoals,
  trackNotifiedTerminalGoal,
  fileLoadSeqByKey,
  bumpFileLoadSeq,
};

const fleetSlice = createFleetSlice(storeContext);
const updaterSlice = createUpdaterSlice(storeContext);
const stageStreamSlice = createStageStreamSlice(storeContext);
const workspaceSlice = createWorkspaceSlice(storeContext);
const terminalSlice = createTerminalSlice(storeContext);
const automationsSlice = createAutomationsSlice(storeContext);
const preferencesSlice = createPreferencesSlice(storeContext);

export function getOrbitState(): OrbitUiState {
  return kernel.getState();
}

export const orbitActions = {
  ...fleetSlice,
  ...updaterSlice,
  ...stageStreamSlice,
  ...workspaceSlice,
  ...terminalSlice,
  ...automationsSlice,
  ...preferencesSlice,
  getSnapshot(): OrbitUiState {
    return kernel.getState();
  },
};

const beforeUnloadListener = (e: BeforeUnloadEvent) => {
  const snap = kernel.getState();
  if (savePrefsPending) {
    writePrefsNow(snap);
  }
  if (saveSidePanelPending) {
    writeSidePanelNow(snap);
  }
  if (Object.values(snap.fileEditorDrafts).some((d) => d.dirty)) {
    e.preventDefault();
    e.returnValue = "";
  }
};

const handleAppFocusGitRefresh = () => {
  if (!initialized || disposed) return;
  if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
  const snap = kernel.getState();
  if (!snap.selectedHostId || !snap.selectedWorkspaceCwd) return;
  scheduleAutoRefreshGit(snap.selectedHostId, snap.selectedWorkspaceCwd, {
    delayMs: 600,
    minIntervalMs: 3000,
    reason: "focus",
  });
};

const handleVisibilityGitRefresh = () => {
  if (typeof document !== "undefined" && document.visibilityState === "visible") {
    handleAppFocusGitRefresh();
  }
};

if (typeof window !== "undefined") {
  window.addEventListener("beforeunload", beforeUnloadListener);
  window.addEventListener("focus", handleAppFocusGitRefresh);
}
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", handleVisibilityGitRefresh);
}

let themeMediaQuery: MediaQueryList | null = null;
let themeMediaListener: (() => void) | null = null;
if (typeof window !== "undefined" && window.matchMedia) {
  themeMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
  themeMediaListener = () => {
    if (kernel.getState().theme === "system") {
      const nextResolved = detectSystemTheme();
      applyDocumentTheme(nextResolved);
      setState((s) => ({ ...s, resolvedTheme: nextResolved }));
    }
  };
  themeMediaQuery.addEventListener("change", themeMediaListener);
}

registerOrbitLinkOpener({
  openFile(rawPath, opts) {
    orbitActions.openFileInSidePanel(rawPath, opts);
  },
  prefetchFile(rawPath, opts) {
    orbitActions.prefetchWorkspaceFile(rawPath, opts);
  },
  openBrowser(url) {
    orbitActions.openBrowserTab(url);
  },
  openExternal(url) {
    void orbitIpc.openExternalBrowserUrl(url);
  },
});

function ensureStoreInitialized(): void {
  if (initialized || disposed || autoInitScheduled) return;
  autoInitScheduled = true;
  queueMicrotask(() => {
    autoInitScheduled = false;
    if (!initialized && !disposed) {
      void orbitActions.init();
    }
  });
}

if (hmrData?.wasInitialized) {
  initialized = true;
  queueMicrotask(() => {
    if (disposed) return;
    void subscribeCoreEvents();
    void orbitActions.reloadFleetAndHosts(!kernel.getState().fleetHydrated).catch(() => undefined);
    orbitActions.refreshSidebarView();
  });
}

if (viteHot) {
  viteHot.dispose((data) => {
    draftPersistence.flushNow();
    data.draftUpdatedAtByScope = draftPersistence.exportUpdatedAtByScope();
    draftPersistence.dispose();

    disposed = true;
    data.state = kernel.getState();
    data.terminalChannels = terminalChannels;
    data.wasInitialized = initialized;

    if (themeMediaQuery && themeMediaListener) {
      themeMediaQuery.removeEventListener("change", themeMediaListener);
      themeMediaQuery = null;
      themeMediaListener = null;
    }
    if (typeof window !== "undefined") {
      window.removeEventListener("beforeunload", beforeUnloadListener);
      window.removeEventListener("focus", handleAppFocusGitRefresh);
    }
    if (typeof document !== "undefined") {
      document.removeEventListener("visibilitychange", handleVisibilityGitRefresh);
    }
    if (savePrefsTimer !== null) {
      clearTimeout(savePrefsTimer);
      savePrefsTimer = null;
    }
    if (saveSidePanelPending) {
      writeSidePanelNow(kernel.getState());
    } else if (saveSidePanelTimer !== null) {
      clearTimeout(saveSidePanelTimer);
      saveSidePanelTimer = null;
    }
    if (toastTimer) {
      clearTimeout(toastTimer);
      toastTimer = null;
    }
    if (gitChangedDebounceTimer !== null && typeof window !== "undefined") {
      window.clearTimeout(gitChangedDebounceTimer);
      gitChangedDebounceTimer = null;
    }
    for (const gate of autoGitRefreshGates.values()) {
      if (typeof window !== "undefined") {
        if (gate.timerId !== null) window.clearTimeout(gate.timerId);
        if (gate.trailingTimerId !== null) window.clearTimeout(gate.trailingTimerId);
      }
    }
    autoGitRefreshGates.clear();
    for (const entry of pendingSeenTimers.values()) {
      if (typeof window !== "undefined") {
        window.clearTimeout(entry.timerId);
      }
    }
    pendingSeenTimers.clear();

    const cancelGate = (gate: SimpleSyncGate) => {
      if (gate.rafId !== null && typeof window !== "undefined") {
        window.cancelAnimationFrame(gate.rafId);
        gate.rafId = null;
      }
      gate.queued = false;
    };
    cancelGate(fleetSyncGate);
    cancelGate(sidebarSyncGate);
    for (const gate of hostSyncGates.values()) {
      cancelGate(gate);
    }
  });
}

export function subscribeOrbitTopics(
  topics: StoreTopic | readonly StoreTopic[],
  onStoreChange: () => void,
): () => void {
  const unsubscribe = kernel.subscribeTopics(topics, onStoreChange);
  ensureStoreInitialized();
  return unsubscribe;
}

function subscribeStoreMask(mask: number, onStoreChange: () => void): TopicSubscriptionHandle {
  const handle = kernel.subscribeMask(mask, onStoreChange);
  ensureStoreInitialized();
  return handle;
}

function subscribeComposerDraft(onDraftChange: () => void): () => void {
  composerDraftListeners.add(onDraftChange);
  return () => {
    composerDraftListeners.delete(onDraftChange);
  };
}

function subscribeDraftPresence(onPresenceChange: () => void): () => void {
  draftPresenceListeners.add(onPresenceChange);
  return () => {
    draftPresenceListeners.delete(onPresenceChange);
  };
}

function noopSubscribe(): () => void {
  return () => {};
}

const isDevSelectorGuard = Boolean(
  (import.meta as { env?: { DEV?: boolean } }).env?.DEV,
);
let didWarnUnstableSelector = false;

function warnUnstableSelectorOnce(): void {
  if (didWarnUnstableSelector) return;
  didWarnUnstableSelector = true;
  console.error(
    "[orbit-store] useOrbitSelector selector returned a non-stable reference for identical state. Use a module-level constant (e.g. EMPTY_*) for fallback arrays/objects.",
  );
}

/**
 * Topic-isolated external store selector hook.
 *
 * - Automatically tracks the exact `StoreTopic` bitmask accessed by `selector` (zero caller changes needed),
 *   so streaming `stage` deltas never wake unrelated subscribers (`sidebar`, `preferences`, `terminal`, etc.).
 * - Optionally accepts explicit `topics` to lock subscription scope directly.
 */
export function useOrbitSelector<T>(
  selector: (s: OrbitUiState) => T,
  topics?: StoreTopic | readonly StoreTopic[],
): T {
  const explicitMask = topics !== undefined ? topicsToMask(topics) : null;
  const selectorRef = useRef(selector);
  selectorRef.current = selector;
  const explicitMaskRef = useRef<number | null>(explicitMask);
  explicitMaskRef.current = explicitMask;

  const subRef = useRef<{
    handle: TopicSubscriptionHandle | null;
    desiredMask: number;
  }>({
    handle: null,
    desiredMask: explicitMask ?? ALL_TOPICS_MASK,
  });

  const cacheRef = useRef<{
    hasCache: boolean;
    state: OrbitUiState | undefined;
    selector: ((s: OrbitUiState) => T) | undefined;
    explicitMask: number | null;
    value: T;
  }>({
    hasCache: false,
    state: undefined,
    selector: undefined,
    explicitMask: null,
    value: undefined as T,
  });

  const subscribe = useCallback((onStoreChange: () => void) => {
    const handle = subscribeStoreMask(subRef.current.desiredMask, onStoreChange);
    subRef.current.handle = handle;
    return () => {
      handle.unsubscribe();
      if (subRef.current.handle === handle) {
        subRef.current.handle = null;
      }
    };
  }, []);

  const getSnapshot = useCallback((): T => {
    const currentState = kernel.getState();
    const currentSelector = selectorRef.current;
    const currentExplicitMask = explicitMaskRef.current;
    const cache = cacheRef.current;
    if (
      cache.hasCache &&
      Object.is(cache.state, currentState) &&
      cache.selector === currentSelector &&
      cache.explicitMask === currentExplicitMask
    ) {
      return cache.value;
    }

    let nextValue: T;
    if (currentExplicitMask !== null) {
      if (subRef.current.desiredMask !== currentExplicitMask) {
        subRef.current.desiredMask = currentExplicitMask;
        subRef.current.handle?.updateMask(currentExplicitMask);
      }
      nextValue = currentSelector(currentState);
    } else {
      const { value, mask } = evaluateSelectorWithTopicTracking(currentState, currentSelector);
      if (subRef.current.desiredMask !== mask) {
        subRef.current.desiredMask = mask;
        subRef.current.handle?.updateMask(mask);
      }
      nextValue = value;
    }

    if (isDevSelectorGuard && !didWarnUnstableSelector) {
      const checkValue = currentSelector(currentState);
      if (!Object.is(nextValue, checkValue)) {
        warnUnstableSelectorOnce();
      }
    }

    cache.state = currentState;
    cache.selector = currentSelector;
    cache.explicitMask = currentExplicitMask;
    cache.value = nextValue;
    cache.hasCache = true;
    return nextValue;
  }, []);

  return useSyncExternalStore(subscribe, getSnapshot);
}

/**
 * Isolated Composer draft text subscription:
 * Fires strictly when `composerDrafts` mutates, keeping keystrokes 100% isolated from the rest of the app.
 */
export function useComposerDraft(scopeKey: string): string {
  return useSyncExternalStore(
    subscribeComposerDraft,
    () => kernel.getState().composerDrafts[scopeKey] ?? "",
  );
}

/**
 * Ultra-lightweight boolean subscription for sidebar thread rows:
 * Fires ONLY when a scope transitions between empty and non-empty unsent draft (never per-keystroke),
 * and skips subscription completely while a row is `selected` (`enabled = false`).
 */
export function useHasThreadDraft(scopeKey: string, enabled = true): boolean {
  return useSyncExternalStore(
    enabled ? subscribeDraftPresence : noopSubscribe,
    () => (enabled ? hasNonEmptyScopeDraft(kernel.getState(), scopeKey) : false),
  );
}
