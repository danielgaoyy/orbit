import {
  type SettingsSection,
  type SettingsSectionTarget,
  resolveSettingsSectionTarget as resolveConsolidatedSettingsTarget,
} from "@/components/settings/settings-nav";
import { getLocale, resolveLocale } from "@/lib/i18n";
import { detectRichMediaKind } from "@/lib/media-utils";
import {
  type CloudSyncResultView,
  type HostDetailView,
  PROVIDER_LIST,
  type ProviderKind,
  type ReleaseUpdateInfo,
  compareSemver,
  isAgentForegroundRunning,
  normalizeSemver,
} from "@/lib/orbit-ipc";
import { isBareHomeDir } from "@/lib/projects";
import { SIDE_PANEL_DEFAULT_WIDTH_PX } from "@/lib/workspace-layout";
import type {
  FileViewCacheEntry,
  OrbitUiState,
  PersistedPrefs,
  ResolvedTheme,
  SidePanelTab,
  SimpleSyncGate,
  TerminalTabSession,
  ThemeMode,
  WorkspaceLineEnding,
} from "./store-types";

export const PREFS_STORAGE_KEY = "orbit.desktop.prefs.v3";
export const THREAD_PREF_MIGRATION_KEY = "orbit.desktop.prefs.migrated_v2";
export const ONBOARDING_COMPLETED_STORAGE_KEY = "orbit:onboarding_completed_v1";

export const COMPACT_VIEWPORT_COLLAPSE_PX = 800;
export const COMPACT_VIEWPORT_EXPAND_PX = 824;
export const DUAL_SIDEBAR_YIELD_PX = 1040;
export const DUAL_SIDEBAR_RESTORE_PX = 1080;

export const BOOTSTRAP_TIMEOUT_MS = 6000;
export const SEEN_REPORT_DEBOUNCE_MS = 1500;
export const MAX_NOTIFIED_TERMINAL_GOALS = 512;
export const PENDING_DRAFT_CREATE_TTL_MS = 30_000;
export const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;
export const MAX_FILE_CACHE_ENTRIES = 20;
export const MAX_FILE_CACHE_BYTES = 64 * 1024 * 1024;

export function resolveFleetTargetVersion(
  appVersion: string | null | undefined,
  releaseInfo: ReleaseUpdateInfo | null | undefined,
): string | null {
  const normApp = normalizeSemver(appVersion);
  const normLatest = normalizeSemver(releaseInfo?.latest_version);
  if (normApp && normLatest) {
    return compareSemver(normLatest, normApp) >= 0 ? normLatest : normApp;
  }
  return normLatest ?? normApp ?? null;
}

export function normalizeLineEndingKind(raw?: string | null): WorkspaceLineEnding {
  if (raw === "crlf" || raw === "mixed") return raw;
  return "lf";
}

export function normalizeEditorText(raw: string): string {
  return raw.replace(/\r\n/g, "\n");
}

export function threadScopeKey(hostId: string, agentId: string): string {
  return `${hostId}::${agentId}`;
}

export function subagentArchiveKey(
  hostId: string,
  parentAgentId: string,
  subagentId: string,
): string {
  return `${hostId}::${parentAgentId}::${subagentId}`;
}

export function isSubagentArchived(
  archivedSubagents: Record<string, true>,
  hostId: string,
  parentAgentId: string,
  target: {
    id?: string | null;
    providerSubagentId?: string | null;
    childAgentId?: string | null;
  },
): boolean {
  if (target.id && archivedSubagents[subagentArchiveKey(hostId, parentAgentId, target.id)]) {
    return true;
  }
  if (
    target.providerSubagentId &&
    archivedSubagents[subagentArchiveKey(hostId, parentAgentId, target.providerSubagentId)]
  ) {
    return true;
  }
  if (
    target.childAgentId &&
    archivedSubagents[subagentArchiveKey(hostId, parentAgentId, target.childAgentId)]
  ) {
    return true;
  }
  return false;
}

export function fileCacheKey(hostId: string, cwd: string, path: string): string {
  return `${hostId}::${cwd}::${path}`;
}

export function sideQuestionScopeKey(
  s: Pick<OrbitUiState, "selectedHostId" | "selectedAgentId" | "isDraftNewSession">,
): string {
  if (s.isDraftNewSession || !s.selectedAgentId) {
    return `draft::${s.selectedHostId ?? "none"}`;
  }
  return `${s.selectedHostId ?? "none"}::${s.selectedAgentId}`;
}

export function composerScopeKey(s: OrbitUiState): string {
  if (s.isDraftNewSession || !s.selectedAgentId) {
    return `draft::${s.selectedHostId ?? "none"}`;
  }
  return `${s.selectedHostId ?? "none"}::${s.selectedAgentId}`;
}

export function loadPrefs(): PersistedPrefs {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(PREFS_STORAGE_KEY);
    if (!raw) return {};
    return JSON.parse(raw) as PersistedPrefs;
  } catch {
    return {};
  }
}

export function buildPersistedPrefsPayload(snap: OrbitUiState): PersistedPrefs {
  return {
    locale: snap.locale,
    theme: snap.theme,
    enterToSend: snap.enterToSend,
    notifyOnApproval: snap.notifyOnApproval,
    notifyOnTurnComplete: snap.notifyOnTurnComplete,
    notifyOnAgentError: snap.notifyOnAgentError,
    notifyOnGoalFinished: snap.notifyOnGoalFinished,
    notifySoundEnabled: snap.notifySoundEnabled,
    notifyQuietHoursEnabled: snap.notifyQuietHoursEnabled,
    notifyQuietStartHour: snap.notifyQuietStartHour,
    notifyQuietEndHour: snap.notifyQuietEndHour,
    coalesceExploration: snap.coalesceExploration,
    worktreeBranchPrefix: snap.worktreeBranchPrefix,
    preferredEditor: snap.preferredEditor,
    terminalFontSize: snap.terminalFontSize,
    terminalCursorStyle: snap.terminalCursorStyle,
    terminalScrollback: snap.terminalScrollback,
    sidebarOpen: snap.sidebarDesktopOpenPref,
    sidebarWidth: snap.sidebarWidth,
    hiddenProjectKeys: snap.hiddenProjectKeys,
    threadCustomTitles: snap.threadCustomTitles,
    archivedThreads: snap.archivedThreads,
    archivedSubagents: snap.archivedSubagents,
    collapsedProjectIds: snap.collapsedProjectIds,
    collapsedInboxProjectIds: snap.collapsedInboxProjectIds,
    threadManualOrder: snap.threadManualOrder,
    sidebarProjectSort: snap.sidebarProjectSort,
    sidebarThreadFilter: snap.sidebarThreadFilter,
    sidePanelWidth: snap.sidePanelWidth,
    overviewDesktopOpenPref: snap.overviewDesktopOpenPref,
    overviewCollapsedSections: snap.overviewCollapsedSections,
    diffLayout: snap.diffLayout,
    draftProvider: snap.draftProvider,
    draftAcpAgentId: snap.draftAcpAgentId,
    draftModels: snap.draftModels,
    draftEffort: snap.draftEffort,
    draftFastMode: snap.draftFastMode,
    draftMode: snap.draftMode,
    draftUseWorktree: snap.draftUseWorktree,
    promptHistory: snap.promptHistory.slice(-30),
  };
}

export function detectSystemTheme(): ResolvedTheme {
  if (typeof window === "undefined" || !window.matchMedia) return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

export function resolveTheme(mode: ThemeMode): ResolvedTheme {
  return mode === "system" ? detectSystemTheme() : mode;
}

export function applyDocumentTheme(resolved: ResolvedTheme): void {
  if (typeof document === "undefined") return;
  const root = document.documentElement;
  if (resolved === "dark") {
    root.classList.add("dark");
    root.style.colorScheme = "dark";
  } else {
    root.classList.remove("dark");
    root.style.colorScheme = "light";
  }
}

export function hasVisibleActiveTerminal(s: {
  sidePanelOpen: boolean;
  sidePanelTabs: readonly SidePanelTab[];
  activeSidePanelTabId: string | null;
  terminalTabs: readonly TerminalTabSession[];
  activeTerminalTabKey: string | null;
}): boolean {
  if (!s.sidePanelOpen || !s.activeSidePanelTabId || !s.activeTerminalTabKey) {
    return false;
  }
  const activeSideTab = s.sidePanelTabs.find((t) => t.id === s.activeSidePanelTabId);
  if (activeSideTab?.kind !== "terminal") return false;
  return s.terminalTabs.some((t) => t.key === s.activeTerminalTabKey);
}

export function isAutoRefreshableCwd(cwd: string | null | undefined): cwd is string {
  if (!cwd) return false;
  const trimmed = cwd.trim();
  if (!trimmed || trimmed === "~" || isBareHomeDir(trimmed)) return false;
  return true;
}

export function scheduleCoalescedGate(gate: SimpleSyncGate, run: () => Promise<void>): void {
  if (gate.inFlight) {
    gate.queued = true;
    return;
  }
  if (gate.rafId !== null) return;

  const exec = async () => {
    gate.rafId = null;
    if (gate.inFlight) {
      gate.queued = true;
      return;
    }
    gate.inFlight = true;
    try {
      while (true) {
        gate.queued = false;
        await run();
        if (!gate.queued) break;
      }
    } finally {
      gate.inFlight = false;
    }
  };

  if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
    gate.rafId = window.requestAnimationFrame(() => {
      void exec();
    });
  } else {
    void exec();
  }
}

export function collectChangedRecordKeys<T>(
  prev: Record<string, T>,
  next: Record<string, T>,
  out: Set<string>,
): void {
  if (prev === next) return;
  for (const k of Object.keys(next)) {
    if (prev[k] !== next[k]) out.add(k);
  }
  for (const k of Object.keys(prev)) {
    if (!(k in next)) out.add(k);
  }
}

export function withBootstrapTimeout<T>(promise: Promise<T>, label: string): Promise<T> {
  return new Promise<T>((resolve, reject) => {
    const timer = setTimeout(() => {
      reject(new Error(`${label} timed out`));
    }, BOOTSTRAP_TIMEOUT_MS);
    promise.then(
      (value) => {
        clearTimeout(timer);
        resolve(value);
      },
      (err) => {
        clearTimeout(timer);
        reject(err);
      },
    );
  });
}

export function isHourInQuietWindow(hour: number, startHour: number, endHour: number): boolean {
  const h = ((Math.floor(hour) % 24) + 24) % 24;
  const s = ((Math.floor(startHour) % 24) + 24) % 24;
  const e = ((Math.floor(endHour) % 24) + 24) % 24;
  if (s === e) return false;
  if (s < e) return h >= s && h < e;
  return h >= s || h < e;
}

export function isAgentErrorState(
  ag: { status_label?: string; attention?: string } | null | undefined,
): boolean {
  if (!ag) return false;
  return (
    ag.attention === "error" ||
    (typeof ag.status_label === "string" && ag.status_label.trim().toLowerCase() === "error")
  );
}

export function hasCompletedOnboarding(): boolean {
  if (typeof window === "undefined") return true;
  try {
    const val = window.localStorage.getItem(ONBOARDING_COMPLETED_STORAGE_KEY);
    return val === "true" || val === "1";
  } catch {
    return true;
  }
}

export function markOnboardingCompleted(): void {
  if (typeof window === "undefined") return;
  try {
    window.localStorage.setItem(ONBOARDING_COMPLETED_STORAGE_KEY, "true");
  } catch {
    // Ignore storage quota errors
  }
}

export function resolveAvailableDraftProvider(
  detail: HostDetailView | null | undefined,
  current: ProviderKind,
): ProviderKind {
  if (!detail) return current;
  const hasInstalledAcp = Boolean(
    detail.acp_agents?.some((a) => a.installed && a.spec.enabled),
  );
  if (current === "acp" && hasInstalledAcp) return "acp";
  const avail = detail.host.available_providers;
  if (avail.includes(current)) return current;
  if (avail.length > 0) return avail[0];
  if (hasInstalledAcp) return "acp";
  return current;
}

export function slugifyPrefixedBranch(prefix: string, prompt: string): string {
  const cleanPrefix = prefix.trim().replace(/\/+$/, "");
  const slug = prompt
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 28);
  const suffix = Math.random().toString(36).slice(2, 6);
  return `${cleanPrefix}/${slug || "task"}-${suffix}`;
}

export function basenameFromPath(rawPath: string): string {
  const clean = rawPath.replace(/\\/g, "/").replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) : clean;
}

export function omitRecordKeys<T>(
  record: Record<string, T>,
  keysToRemove: ReadonlySet<string>,
): Record<string, T> {
  if (keysToRemove.size === 0) return record;
  let mutated = false;
  const next: Record<string, T> = {};
  for (const [k, v] of Object.entries(record)) {
    if (keysToRemove.has(k)) {
      mutated = true;
    } else {
      next[k] = v;
    }
  }
  return mutated ? next : record;
}

export function collectRemotelyArchivedDraftScopes(
  s: OrbitUiState,
  details: Iterable<readonly [string, HostDetailView]>,
  activeScopeKey: string | null,
): Set<string> {
  const keysToPrune = new Set<string>();
  for (const [hostId, detail] of details) {
    for (const ag of detail.agents) {
      if (!ag.archived_at_rfc3339 || isAgentForegroundRunning(ag)) continue;
      const scopeKey = threadScopeKey(hostId, ag.agent_id);
      if (scopeKey === activeScopeKey) continue;
      if (
        scopeKey in s.composerDrafts ||
        scopeKey in s.slashDirectives ||
        scopeKey in s.pastedAttachments ||
        scopeKey in s.diffQuotes ||
        scopeKey in s.sideQuestionDrafts
      ) {
        keysToPrune.add(scopeKey);
      }
    }
  }
  return keysToPrune;
}

export function resolveSettingsSectionTarget(
  s: OrbitUiState,
  section?: SettingsSectionTarget,
): SettingsSection {
  const targetHostId = s.selectedHostId ?? s.fleet.hosts[0]?.host_id ?? null;
  return resolveConsolidatedSettingsTarget(section, targetHostId, s.settingsSection);
}

export function isLikelyTextFile(file: File, bytes: Uint8Array): boolean {
  if (file.size > 64 * 1024) return false;
  if (detectRichMediaKind(file.name, file.type) !== null) return false;
  const mime = (file.type || "").toLowerCase();
  if (
    mime === "application/pdf" ||
    mime === "application/zip" ||
    mime.startsWith("audio/") ||
    mime.startsWith("video/") ||
    mime.startsWith("image/")
  ) {
    return false;
  }
  const probeLen = Math.min(bytes.length, 4096);
  for (let i = 0; i < probeLen; i += 1) {
    if (bytes[i] === 0) return false;
  }
  return true;
}

export function isOfflineUploadError(errText: string): boolean {
  return /disconnected|reconnect|offline/i.test(errText);
}

export function estimateFileCacheEntryBytes(entry: FileViewCacheEntry): number {
  return (entry.data?.content.length ?? 0) + 4096;
}

export function formatCloudSyncToast(res: CloudSyncResultView): string {
  const parts: string[] = [];
  if (res.hosts_added > 0) parts.push(`+${res.hosts_added} host${res.hosts_added === 1 ? "" : "s"}`);
  if (res.hosts_updated > 0) parts.push(`${res.hosts_updated} updated`);
  if (res.hosts_removed > 0) parts.push(`-${res.hosts_removed} removed`);
  if (res.projects_added > 0)
    parts.push(`+${res.projects_added} project${res.projects_added === 1 ? "" : "s"}`);
  if (res.preferences_updated > 0) parts.push(`${res.preferences_updated} pref synced`);
  if (parts.length === 0) {
    return res.pushed_remote
      ? `Cloud Sync uploaded (rev #${res.vault_revision})`
      : `Cloud Sync up to date (rev #${res.vault_revision})`;
  }
  return `Cloud Synced: ${parts.join(", ")}`;
}

export function upsertLruRecord<T>(
  record: Record<string, T>,
  key: string,
  value: T,
  maxEntries: number,
): Record<string, T> {
  const next: Record<string, T> = {};
  const entries = Object.entries(record).filter(([k]) => k !== key);
  const start = Math.max(0, entries.length - (maxEntries - 1));
  for (let i = start; i < entries.length; i += 1) {
    next[entries[i][0]] = entries[i][1];
  }
  next[key] = value;
  return next;
}

export function createDefaultInitialState(
  initialPrefs: PersistedPrefs,
  initialTheme: ThemeMode,
  initialResolvedTheme: ResolvedTheme,
  initialDrafts: {
    composerDrafts: OrbitUiState["composerDrafts"];
    slashDirectives: OrbitUiState["slashDirectives"];
    contextItems: OrbitUiState["contextItems"];
    pastedAttachments: OrbitUiState["pastedAttachments"];
    diffQuotes: OrbitUiState["diffQuotes"];
    sideQuestionDrafts: OrbitUiState["sideQuestionDrafts"];
  },
): OrbitUiState {
  const initialDesktopSidebarOpenPref = initialPrefs.sidebarOpen ?? true;
  const initialIsCompactViewport =
    typeof window !== "undefined" ? window.innerWidth < COMPACT_VIEWPORT_COLLAPSE_PX : false;
  const initialIsTightForDualSidebars =
    typeof window !== "undefined" ? window.innerWidth < DUAL_SIDEBAR_YIELD_PX : false;

  return {
    fleet: { revision: 0, total_pending_approvals: 0, hosts: [], projects: [] },
    fleetHydrated: false,
    bootstrapError: null,
    hostDetails: {},
    selectedHostId: null,
    instructionsTarget: null,
    selectedAgentId: null,
    selectedWorkspaceCwd: "",
    isDraftNewSession: false,
    sessionScrollNonce: 0,
    pendingJumpTarget: null,

    draftProvider: initialPrefs.draftProvider ?? "codex",
    draftAcpAgentId: initialPrefs.draftAcpAgentId ?? null,
    draftModels: {
      ...Object.fromEntries(
        [...PROVIDER_LIST.map((p) => p.id), "acp" as const].map((id) => [id, null]),
      ),
      ...(initialPrefs.draftModels ?? {}),
    },
    draftEffort: initialPrefs.draftEffort ?? "medium",
    draftFastMode: initialPrefs.draftFastMode ?? false,
    draftMode:
      initialPrefs.draftMode && initialPrefs.draftMode !== "plan"
        ? initialPrefs.draftMode
        : "accept_edits",
    draftProfileId: null,
    draftUseWorktree: initialPrefs.draftUseWorktree ?? true,
    draftWorktreeBranch: "",
    draftWorktreeRepoPath: null,
    draftWorkspaceId: null,
    gitReposCache: {},

    composerDrafts: initialDrafts.composerDrafts,
    slashDirectives: initialDrafts.slashDirectives,
    inlineMentions: {},
    contextItems: initialDrafts.contextItems,
    pastedAttachments: initialDrafts.pastedAttachments,
    diffQuotes: initialDrafts.diffQuotes,
    queuedPrompts: {},
    workspaceFilesCache: {},
    workspaceEmptyDirsCache: {},
    promptHistory: initialPrefs.promptHistory ?? [],

    stages: {},
    sidebarView: null,
    slashCommands: [],
    rewoundSessions: {},

    sidePanelOpen: false,
    sidePanelWidth:
      initialPrefs.sidePanelWidth ?? initialPrefs.inspectorWidth ?? SIDE_PANEL_DEFAULT_WIDTH_PX,
    sidePanelTabs: [],
    activeSidePanelTabId: null,
    fileViewCache: {},
    fileEditorDrafts: {},
    pendingUnsavedFileAction: null,
    sideQuestionsByScope: {},
    sideQuestionDrafts: initialDrafts.sideQuestionDrafts,
    sideQuestionFocusNonce: 0,
    sidePanelLaunchpadFocusNonce: 0,

    reviewScope: "working_tree",
    diffStaged: false,
    diffLayout: initialPrefs.diffLayout ?? "unified",
    selectedDiffPath: null,
    gitReviews: {},
    lspServersByWorkspace: {},
    lspDiagnosticsByFile: {},
    lifecycleHooksByScope: {},
    browserByHost: {},
    guardianByHost: {},
    eventTriggersByHost: {},
    imChannelsByHost: {},
    pushByHost: {},

    overviewDesktopOpenPref: initialPrefs.overviewDesktopOpenPref ?? true,
    overviewPopoverOpen: false,
    stageCanDockOverview: false,
    overviewCollapsedSections: {
      review: initialPrefs.overviewCollapsedSections?.review ?? false,
      files: initialPrefs.overviewCollapsedSections?.files ?? false,
      terminal: initialPrefs.overviewCollapsedSections?.terminal ?? false,
      tasks: initialPrefs.overviewCollapsedSections?.tasks ?? false,
      subagents: initialPrefs.overviewCollapsedSections?.subagents ?? false,
    },
    quickOpenFilesOpen: false,

    sidebarOpen: initialIsCompactViewport ? false : initialDesktopSidebarOpenPref,
    sidebarDesktopOpenPref: initialDesktopSidebarOpenPref,
    sidebarAutoYieldedForSidePanel: false,
    isCompactViewport: initialIsCompactViewport,
    isTightForDualSidebars: initialIsTightForDualSidebars,
    sidebarWidth:
      initialPrefs.sidebarWidth &&
      initialPrefs.sidebarWidth !== 248 &&
      initialPrefs.sidebarWidth !== 260 &&
      initialPrefs.sidebarWidth !== 276
        ? initialPrefs.sidebarWidth
        : 256,
    collapsedHostIds: {},
    collapsedProjectIds: initialPrefs.collapsedProjectIds ?? {},
    collapsedWorkspaceIds: {},
    collapsedInboxProjectIds: initialPrefs.collapsedInboxProjectIds ?? {},
    hiddenProjectKeys: initialPrefs.hiddenProjectKeys ?? {},
    threadCustomTitles: initialPrefs.threadCustomTitles ?? {},
    archivedThreads: initialPrefs.archivedThreads ?? {},
    archivedSubagents: initialPrefs.archivedSubagents ?? {},
    threadManualOrder: initialPrefs.threadManualOrder ?? {},
    sidebarProjectSort: initialPrefs.sidebarProjectSort ?? "recent",
    sidebarThreadFilter: initialPrefs.sidebarThreadFilter ?? "all",

    terminalTabs: [],
    activeTerminalTabKey: null,
    terminalFocused: false,

    activeView: "workspace",
    workspacePanel: "conversation",
    settingsSection: "general",
    commandPaletteOpen: false,
    onboardingOpen: false,
    pairDialogOpen: false,
    relayDialogOpen: false,
    remoteFolderPickerHostId: null,
    exportDialogTarget: null,
    copyCodeBlocksTarget: null,
    handoffDialogTarget: null,
    mediaPreviewTarget: null,
    sshHosts: [],
    externalEditors: [],
    externalMcpServersByHost: {},
    toastNotice: null,
    toastAction: null,
    lastArchivedUndo: null,
    appVersion: null,
    isBrowserOffline: typeof navigator !== "undefined" ? !navigator.onLine : false,
    dismissedVersionMismatchHosts: {},
    hostUpgradeStates: {},
    releaseUpdateInfo: null,
    checkingReleaseUpdate: false,
    releaseCheckError: null,
    fleetUpdate: {
      phase: "idle",
      targetVersion: null,
      desktopStatus: null,
      desktopProgress: null,
      desktopError: null,
      startedAt: null,
      summaryMessage: null,
      pendingDesktopUpdate: false,
    },

    cloudSyncStatus: null,
    pathRulesByScope: {},
    cloudSyncBusy: false,
    cloudSyncError: null,
    cloudSyncDeviceFlow: null,
    cloudSyncLastResult: null,

    locale: initialPrefs.locale ? resolveLocale(initialPrefs.locale) : getLocale(),
    theme: initialTheme,
    resolvedTheme: initialResolvedTheme,
    enterToSend: initialPrefs.enterToSend ?? true,
    notifyOnApproval: initialPrefs.notifyOnApproval ?? true,
    notifyOnTurnComplete: initialPrefs.notifyOnTurnComplete ?? false,
    notifyOnAgentError: initialPrefs.notifyOnAgentError ?? true,
    notifyOnGoalFinished: initialPrefs.notifyOnGoalFinished ?? true,
    notifySoundEnabled: initialPrefs.notifySoundEnabled ?? true,
    notifyQuietHoursEnabled: initialPrefs.notifyQuietHoursEnabled ?? false,
    notifyQuietStartHour:
      typeof initialPrefs.notifyQuietStartHour === "number"
        ? Math.min(23, Math.max(0, Math.round(initialPrefs.notifyQuietStartHour)))
        : 23,
    notifyQuietEndHour:
      typeof initialPrefs.notifyQuietEndHour === "number"
        ? Math.min(23, Math.max(0, Math.round(initialPrefs.notifyQuietEndHour)))
        : 7,
    coalesceExploration: initialPrefs.coalesceExploration ?? true,
    worktreeBranchPrefix: initialPrefs.worktreeBranchPrefix ?? "auto",
    preferredEditor: initialPrefs.preferredEditor ?? null,
    terminalFontSize:
      initialPrefs.terminalFontSize !== undefined
        ? Math.min(18, Math.max(10, Math.round(initialPrefs.terminalFontSize)))
        : 13,
    terminalCursorStyle: initialPrefs.terminalCursorStyle ?? "bar",
    terminalScrollback: initialPrefs.terminalScrollback ?? 5000,
  };
}
