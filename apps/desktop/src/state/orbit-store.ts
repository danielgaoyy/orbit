import { useSyncExternalStore } from "react";
import {
  normalizeLineRange,
  normalizeWorkspacePathCandidate,
  registerOrbitLinkOpener,
  resolveUniqueWorkspaceFileSuffix,
} from "@/lib/file-links";
import {
  recordRecentSentTurnImages,
  seedPromptImagePreview,
  type SentImagePreviewMeta,
} from "@/lib/prompt-image-cache";
import {
  getCatalogForProvider,
  resolveActiveModelSpec,
  resolveSupportedEffortForModel,
  seedFallbackProviderCatalogs,
} from "@/lib/model-catalog";
import {
  isLocalHostCard,
  matchesProjectDirectory,
  projectPathScopeKey,
} from "@/lib/projects";
import {
  type AgentStageView,
  type ClientThreadPrefSeed,
  type ComposerContextInput,
  type CoreNotification,
  type ExternalEditorInfo,
  type ExternalEditorKind,
  type FleetOverviewView,
  type FleetSidebarView,
  type GitReviewView,
  type HostDetailView,
  type PromptBadge,
  type ProviderKind,
  type ReasoningEffort,
  type RichBlockView,
  type SessionMode,
  type SidebarProjectGroupView,
  type SlashCommandView,
  type SlashDirective,
  type SshHostItem,
  type WorkspaceFileView,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  isMacDesktop,
  compareSemver,
  normalizeSemver,
  orbitIpc,
} from "@/lib/orbit-ipc";

export interface HostDaemonUpgradeState {
  status: "upgrading" | "failed";
  mode: "local" | "ssh" | "wire";
  error?: string;
  startedAt: number;
}

export type { SlashDirective };

export interface UndoArchiveEntry {
  hostId: string;
  agentId: string;
  title: string;
}

export interface PastedAttachment {
  id: string;
  label: string;
  filename?: string;
  lineCount: number;
  content: string;
}

export type InlineMentionTarget =
  | { token: string; kind: "file"; path: string }
  | { token: string; kind: "git_diff"; staged: boolean; label: string }
  | {
      token: string;
      kind: "terminal_output";
      label: string;
      lineCount: number;
      content: string;
    };

export interface QueuedPromptItem {
  id: string;
  wirePrompt: string;
  displayText: string;
  badges: PromptBadge[];
  slashDirective: SlashDirective | null;
  contexts: ContextMentionItem[];
  pastes: PastedAttachment[];
  quotes: DiffQuoteSnippet[];
}

export interface DiffQuoteSnippet {
  id: string;
  path: string;
  lineNumber: number | null;
  code: string;
}

export type ContextMentionItem =
  | {
      id: string;
      kind: "file";
      path: string;
      filename?: string;
      sizeBytes?: number;
      stagedHostId?: string;
      localFile?: File;
      uploading?: boolean;
    }
  | { id: string; kind: "git_diff"; staged: boolean; label: string }
  | { id: string; kind: "terminal_output"; label: string; lineCount: number; content: string }
  | {
      id: string;
      kind: "image";
      filename: string;
      path: string;
      previewUrl: string;
      sizeBytes: number;
      stagedHostId?: string;
      localFile?: File;
      uploading?: boolean;
    };

export interface TerminalTabSession {
  key: string;
  hostId: string;
  terminalId: string | null;
  slot: number;
  cwd: string;
  title: string;
}

export type SidePanelTabKind =
  | "review"
  | "files"
  | "file"
  | "terminal"
  | "browser"
  | "side_question"
  | "subagent";

export interface SidePanelTab {
  id: string;
  kind: SidePanelTabKind;
  title: string;
  filePath?: string;
  hostId?: string;
  cwd?: string;
  focusLine?: number | null;
  focusEndLine?: number | null;
  focusNonce?: number;
  url?: string;
  subagentCallId?: string;
  subagentRole?: string;
  subagentPrompt?: string;
  childAgentId?: string | null;
  providerSubagentId?: string | null;
  parentAgentId?: string | null;
}

export interface FileViewCacheEntry {
  status: "loading" | "ready" | "error";
  data?: WorkspaceFileView;
  error?: string;
}

export interface SideQuestionEntry {
  id: string;
  question: string;
  answer: string;
  blocks: RichBlockView[];
  status: "streaming" | "done" | "error";
  error: string | null;
  createdAtMs: number;
  providerLabel: string;
}

export type OverviewSectionId = "review" | "files" | "terminal" | "tasks" | "subagents";

export type ReviewScope = "working_tree" | "session_turns";
export type DiffLayoutMode = "unified" | "split";
export type ThemeMode = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export type ActiveViewMode = "workspace" | "settings";
export type WorkspacePanelMode = "conversation" | "automations";
export type HostSettingsTab = "agents" | "usage" | "connection";
export type SettingsSection =
  | "general"
  | "appearance"
  | "shortcuts"
  | "notifications"
  | "add_host"
  | `host:${string}:${HostSettingsTab}`;
export type SettingsSectionTarget = SettingsSection | "hosts" | "providers";
export type TerminalCursorStyle = "bar" | "block" | "underline";
export type SidebarProjectSort = "recent" | "alphabetical";
export type SidebarThreadFilter = "all" | "active_unread";

export interface ArchivedThreadEntry {
  key: string;
  hostId: string;
  agentId: string;
  archivedAtMs: number;
}

export function threadScopeKey(hostId: string, agentId: string): string {
  return `${hostId}::${agentId}`;
}

export function fileCacheKey(hostId: string, cwd: string, path: string): string {
  return `${hostId}::${cwd}::${path}`;
}

export function sideQuestionScopeKey(s: Pick<OrbitUiState, "selectedHostId" | "selectedAgentId" | "isDraftNewSession">): string {
  if (s.isDraftNewSession || !s.selectedAgentId) {
    return `draft::${s.selectedHostId ?? "none"}`;
  }
  return `${s.selectedHostId ?? "none"}::${s.selectedAgentId}`;
}

export interface OrbitUiState {
  fleet: FleetOverviewView;
  fleetHydrated: boolean;
  hostDetails: Record<string, HostDetailView>;
  selectedHostId: string | null;
  selectedAgentId: string | null;
  selectedWorkspaceCwd: string;
  isDraftNewSession: boolean;
  sessionScrollNonce: number;

  // Draft new-session launch configuration
  draftProvider: ProviderKind;
  draftModels: Record<ProviderKind, string | null>;
  draftEffort: ReasoningEffort;
  draftMode: SessionMode;
  draftProfileId: string | null;
  draftUseWorktree: boolean;
  draftWorktreeBranch: string;

  // Composer drafts, slash workflow directives, context mentions, pasted attachments, diff quotes, and queued prompts
  composerDrafts: Record<string, string>;
  slashDirectives: Record<string, SlashDirective | null>;
  inlineMentions: Record<string, InlineMentionTarget[]>;
  contextItems: Record<string, ContextMentionItem[]>;
  pastedAttachments: Record<string, PastedAttachment[]>;
  diffQuotes: Record<string, DiffQuoteSnippet[]>;
  queuedPrompts: Record<string, QueuedPromptItem[]>;
  workspaceFilesCache: Record<string, string[]>;
  promptHistory: string[];

  // Incremental stage projections keyed by `hostId::agentId` + Rust sidebar tree + slash commands
  stages: Record<string, AgentStageView>;
  sidebarView: FleetSidebarView | null;
  slashCommands: SlashCommandView[];
  rewoundSessions: Record<string, true>;

  // Unified Right Side Panel (Multi-Tab Workbench: Review, Open File, Files, Terminal, Browser, Side Question, Subagent)
  sidePanelOpen: boolean;
  sidePanelWidth: number;
  sidePanelTabs: SidePanelTab[];
  activeSidePanelTabId: string | null;
  fileViewCache: Record<string, FileViewCacheEntry>;
  sideQuestionsByScope: Record<string, SideQuestionEntry[]>;
  sideQuestionDrafts: Record<string, string>;
  sideQuestionFocusNonce: number;
  sidePanelLaunchpadFocusNonce: number;

  // Git & Review State (rendered inside Review tab)
  reviewScope: ReviewScope;
  diffStaged: boolean;
  diffLayout: DiffLayoutMode;
  selectedDiffPath: string | null;
  gitReviews: Record<string, GitReviewView>;

  // Main Stage Overview Panel
  overviewDesktopOpenPref: boolean;
  overviewPopoverOpen: boolean;
  stageCanDockOverview: boolean;
  overviewCollapsedSections: Record<OverviewSectionId, boolean>;
  quickOpenFilesOpen: boolean;

  // Sidebar & Layout
  sidebarOpen: boolean;
  sidebarDesktopOpenPref: boolean;
  sidebarAutoYieldedForSidePanel: boolean;
  isCompactViewport: boolean;
  isTightForDualSidebars: boolean;
  sidebarWidth: number;
  collapsedHostIds: Record<string, boolean>;
  collapsedProjectIds: Record<string, boolean>;
  hiddenProjectKeys: Record<string, boolean>;
  threadCustomTitles: Record<string, string>;
  archivedThreads: Record<string, ArchivedThreadEntry>;
  sidebarProjectSort: SidebarProjectSort;
  sidebarThreadFilter: SidebarThreadFilter;

  // Persistent Multi-Slot PTY Sessions (rendered inside Side Panel Terminal tab)
  terminalTabs: TerminalTabSession[];
  activeTerminalTabKey: string | null;
  terminalFocused: boolean;

  // Modals, Views & Overlays
  activeView: ActiveViewMode;
  workspacePanel: WorkspacePanelMode;
  settingsSection: SettingsSection;
  commandPaletteOpen: boolean;
  pairDialogOpen: boolean;
  relayDialogOpen: boolean;
  remoteFolderPickerHostId: string | null;
  sshHosts: SshHostItem[];
  externalEditors: ExternalEditorInfo[];
  toastNotice: string | null;
  lastArchivedUndo: UndoArchiveEntry | null;
  appVersion: string | null;
  isBrowserOffline: boolean;
  dismissedVersionMismatchHosts: Record<string, string>;
  hostUpgradeStates: Record<string, HostDaemonUpgradeState>;

  // User ergonomic & ADE preferences
  theme: ThemeMode;
  resolvedTheme: ResolvedTheme;
  enterToSend: boolean;
  notifyOnApproval: boolean;
  notifyOnTurnComplete: boolean;
  coalesceExploration: boolean;
  worktreeBranchPrefix: string;
  preferredEditor: ExternalEditorKind | null;
  terminalFontSize: number;
  terminalCursorStyle: TerminalCursorStyle;
  terminalScrollback: number;
}

const PREFS_STORAGE_KEY = "orbit.desktop.prefs.v3";

interface PersistedPrefs {
  theme?: ThemeMode;
  enterToSend?: boolean;
  notifyOnApproval?: boolean;
  notifyOnTurnComplete?: boolean;
  coalesceExploration?: boolean;
  worktreeBranchPrefix?: string;
  preferredEditor?: ExternalEditorKind | null;
  terminalFontSize?: number;
  terminalCursorStyle?: TerminalCursorStyle;
  terminalScrollback?: number;
  sidebarOpen?: boolean;
  sidebarWidth?: number;
  hiddenProjectKeys?: Record<string, boolean>;
  pinnedThreadKeys?: string[];
  threadCustomTitles?: Record<string, string>;
  archivedThreads?: Record<string, ArchivedThreadEntry>;
  seenAgentSeqs?: Record<string, number>;
  sidebarProjectSort?: SidebarProjectSort;
  sidebarThreadFilter?: SidebarThreadFilter;
  inspectorWidth?: number;
  sidePanelWidth?: number;
  overviewDesktopOpenPref?: boolean;
  overviewCollapsedSections?: Partial<Record<OverviewSectionId, boolean>>;
  diffLayout?: DiffLayoutMode;
  draftProvider?: ProviderKind;
  draftModels?: Partial<Record<ProviderKind, string | null>>;
  draftEffort?: ReasoningEffort;
  draftMode?: SessionMode;
  draftUseWorktree?: boolean;
  promptHistory?: string[];
}

function loadPrefs(): PersistedPrefs {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(PREFS_STORAGE_KEY);
    if (!raw) return {};
    return JSON.parse(raw) as PersistedPrefs;
  } catch {
    return {};
  }
}

let savePrefsTimer: ReturnType<typeof setTimeout> | null = null;
let savePrefsPending = false;

function writePrefsNow(snap: OrbitUiState): void {
  if (typeof window === "undefined") return;
  savePrefsPending = false;
  if (savePrefsTimer !== null) {
    clearTimeout(savePrefsTimer);
    savePrefsTimer = null;
  }
  try {
    const payload: PersistedPrefs = {
      theme: snap.theme,
      enterToSend: snap.enterToSend,
      notifyOnApproval: snap.notifyOnApproval,
      notifyOnTurnComplete: snap.notifyOnTurnComplete,
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
      sidebarProjectSort: snap.sidebarProjectSort,
      sidebarThreadFilter: snap.sidebarThreadFilter,
      sidePanelWidth: snap.sidePanelWidth,
      overviewDesktopOpenPref: snap.overviewDesktopOpenPref,
      overviewCollapsedSections: snap.overviewCollapsedSections,
      diffLayout: snap.diffLayout,
      draftProvider: snap.draftProvider,
      draftModels: snap.draftModels,
      draftEffort: snap.draftEffort,
      draftMode: snap.draftMode,
      draftUseWorktree: snap.draftUseWorktree,
      promptHistory: snap.promptHistory.slice(-30),
    };
    window.localStorage.setItem(PREFS_STORAGE_KEY, JSON.stringify(payload));
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
    if (savePrefsPending) {
      writePrefsNow(state);
    }
  }, 180);
}

if (typeof window !== "undefined") {
  window.addEventListener("beforeunload", () => {
    if (savePrefsPending) {
      writePrefsNow(state);
    }
  });
}

function detectSystemTheme(): ResolvedTheme {
  if (typeof window === "undefined" || !window.matchMedia) return "light";
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

function resolveTheme(mode: ThemeMode): ResolvedTheme {
  return mode === "system" ? detectSystemTheme() : mode;
}

function applyDocumentTheme(resolved: ResolvedTheme): void {
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

const initialPrefs = loadPrefs();
const initialTheme: ThemeMode = initialPrefs.theme ?? "system";
const initialResolvedTheme: ResolvedTheme = resolveTheme(initialTheme);
applyDocumentTheme(initialResolvedTheme);
if (typeof document !== "undefined" && isMacDesktop()) {
  document.documentElement.classList.add("orbit-mac");
}

if (typeof window !== "undefined" && window.matchMedia) {
  const mql = window.matchMedia("(prefers-color-scheme: dark)");
  mql.addEventListener("change", () => {
    if (state.theme === "system") {
      const nextResolved = detectSystemTheme();
      applyDocumentTheme(nextResolved);
      setState((s) => ({ ...s, resolvedTheme: nextResolved }));
    }
  });
}

const THREAD_PREF_MIGRATION_KEY = "orbit.desktop.prefs.migrated_v2";
const COMPACT_VIEWPORT_COLLAPSE_PX = 800;
const COMPACT_VIEWPORT_EXPAND_PX = 824;
const DUAL_SIDEBAR_YIELD_PX = 1040;
const DUAL_SIDEBAR_RESTORE_PX = 1080;

const initialDesktopSidebarOpenPref = initialPrefs.sidebarOpen ?? true;
const initialIsCompactViewport =
  typeof window !== "undefined" ? window.innerWidth < COMPACT_VIEWPORT_COLLAPSE_PX : false;
const initialIsTightForDualSidebars =
  typeof window !== "undefined" ? window.innerWidth < DUAL_SIDEBAR_YIELD_PX : false;

let state: OrbitUiState = {
  fleet: { revision: 0, total_pending_approvals: 0, hosts: [], projects: [] },
  fleetHydrated: false,
  hostDetails: {},
  selectedHostId: null,
  selectedAgentId: null,
  selectedWorkspaceCwd: "",
  isDraftNewSession: false,
  sessionScrollNonce: 0,

  draftProvider: initialPrefs.draftProvider ?? "codex",
  draftModels: {
    claude_code: initialPrefs.draftModels?.claude_code ?? null,
    codex: initialPrefs.draftModels?.codex ?? null,
    antigravity: initialPrefs.draftModels?.antigravity ?? null,
  },
  draftEffort: initialPrefs.draftEffort ?? "medium",
  draftMode:
    initialPrefs.draftMode && initialPrefs.draftMode !== "plan"
      ? initialPrefs.draftMode
      : "accept_edits",
  draftProfileId: null,
  draftUseWorktree: initialPrefs.draftUseWorktree ?? true,
  draftWorktreeBranch: "",

  composerDrafts: {},
  slashDirectives: {},
  inlineMentions: {},
  contextItems: {},
  pastedAttachments: {},
  diffQuotes: {},
  queuedPrompts: {},
  workspaceFilesCache: {},
  promptHistory: initialPrefs.promptHistory ?? [],

  stages: {},
  sidebarView: null,
  slashCommands: [],
  rewoundSessions: {},

  sidePanelOpen: false,
  sidePanelWidth: initialPrefs.sidePanelWidth ?? initialPrefs.inspectorWidth ?? 480,
  sidePanelTabs: [],
  activeSidePanelTabId: null,
  fileViewCache: {},
  sideQuestionsByScope: {},
  sideQuestionDrafts: {},
  sideQuestionFocusNonce: 0,
  sidePanelLaunchpadFocusNonce: 0,

  reviewScope: "working_tree",
  diffStaged: false,
  diffLayout: initialPrefs.diffLayout ?? "unified",
  selectedDiffPath: null,
  gitReviews: {},

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
  collapsedProjectIds: {},
  hiddenProjectKeys: initialPrefs.hiddenProjectKeys ?? {},
  threadCustomTitles: initialPrefs.threadCustomTitles ?? {},
  archivedThreads: initialPrefs.archivedThreads ?? {},
  sidebarProjectSort: initialPrefs.sidebarProjectSort ?? "recent",
  sidebarThreadFilter: initialPrefs.sidebarThreadFilter ?? "all",

  terminalTabs: [],
  activeTerminalTabKey: null,
  terminalFocused: false,

  activeView: "workspace",
  workspacePanel: "conversation",
  settingsSection: "general",
  commandPaletteOpen: false,
  pairDialogOpen: false,
  relayDialogOpen: false,
  remoteFolderPickerHostId: null,
  sshHosts: [],
  externalEditors: [],
  toastNotice: null,
  lastArchivedUndo: null,
  appVersion: null,
  isBrowserOffline: typeof navigator !== "undefined" ? !navigator.onLine : false,
  dismissedVersionMismatchHosts: {},
  hostUpgradeStates: {},

  theme: initialTheme,
  resolvedTheme: initialResolvedTheme,
  enterToSend: initialPrefs.enterToSend ?? true,
  notifyOnApproval: initialPrefs.notifyOnApproval ?? true,
  notifyOnTurnComplete: initialPrefs.notifyOnTurnComplete ?? false,
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

const listeners = new Set<() => void>();
const composerDraftListeners = new Set<() => void>();
const composerFocusListeners = new Set<() => void>();

const autoUpgradeAttempted = new Map<string, string>();

function maybeAutoSyncOutdatedHosts(): void {
  const normApp = normalizeSemver(state.appVersion);
  if (!normApp || state.isBrowserOffline) return;

  for (const host of state.fleet.hosts) {
    const isOnline =
      host.connection_state === "online_direct" || host.connection_state === "online_relay";
    if (!isOnline) continue;

    const normDaemon = normalizeSemver(host.daemon_version);
    if (!normDaemon) continue;

    // Clear stale upgrade failure once the daemon catches up
    if (compareSemver(normDaemon, normApp) >= 0) {
      if (state.hostUpgradeStates[host.host_id]?.status === "failed") {
        setState((s) => {
          const next = { ...s.hostUpgradeStates };
          delete next[host.host_id];
          return { ...s, hostUpgradeStates: next };
        });
      }
      continue;
    }

    // Only auto-upgrade strictly older daemons, and at most once per (host, daemonVer -> appVer)
    const attemptKey = `${normDaemon}->${normApp}`;
    if (autoUpgradeAttempted.get(host.host_id) === attemptKey) continue;
    if (state.hostUpgradeStates[host.host_id]?.status === "upgrading") continue;

    const isLocal = isLocalHostCard(host);
    const supportsNativeWireUpgrade = (host.daemon_protocol_version ?? 0) >= 4;
    if (isLocal || host.ssh_alias || supportsNativeWireUpgrade) {
      autoUpgradeAttempted.set(host.host_id, attemptKey);
      void orbitActions.upgradeHostDaemon(host.host_id, { silent: true });
    }
  }
}

// Coalesced single-flight gates so daemon bursts never pile up overlapping IPC roundtrips or duplicate renders
interface StagePullGate {
  inFlight: boolean;
  queued: boolean;
  forceFull: boolean;
}
interface SimpleSyncGate {
  inFlight: boolean;
  queued: boolean;
  rafId: number | null;
}
const stagePullGates = new Map<string, StagePullGate>();
const hostSyncGates = new Map<string, SimpleSyncGate>();
const fleetSyncGate: SimpleSyncGate = { inFlight: false, queued: false, rafId: null };
const sidebarSyncGate: SimpleSyncGate = { inFlight: false, queued: false, rafId: null };
const lastReportedSeenSeq = new Map<string, number>();
const submittingComposerScopes = new Set<string>();
const cancelledSideQuestionIds = new Set<string>();
let gitChangedDebounceTimer: number | null = null;

function markThreadSeenInCore(hostId: string, agentId: string, seq: number): void {
  const key = threadScopeKey(hostId, agentId);
  const prev = lastReportedSeenSeq.get(key) ?? -1;
  if (seq <= prev) return;
  lastReportedSeenSeq.set(key, seq);
  void orbitIpc.markAgentSeen(hostId, agentId, seq);
}

function scheduleCoalescedGate(gate: SimpleSyncGate, run: () => Promise<void>): void {
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

// Terminal raw stream ring buffers and subscribers per `hostId::slot`
interface TerminalStreamChannel {
  chunks: Uint8Array[];
  subs: Set<(bytes: Uint8Array) => void>;
}
const terminalChannels = new Map<string, TerminalStreamChannel>();

function emitChange(): void {
  for (const l of listeners) {
    l();
  }
}

function emitComposerDraftChange(): void {
  for (const l of composerDraftListeners) {
    l();
  }
}

function setState(updater: (prev: OrbitUiState) => OrbitUiState, persist = false): void {
  const prev = state;
  const next = updater(prev);
  if (next === prev) return;
  state = next;
  if (persist) {
    scheduleSavePrefs();
  }
  if (next.composerDrafts !== prev.composerDrafts) {
    emitComposerDraftChange();
  }
  emitChange();
}

export function composerScopeKey(s: OrbitUiState): string {
  if (s.isDraftNewSession || !s.selectedAgentId) {
    return `draft::${s.selectedHostId ?? "none"}`;
  }
  return `${s.selectedHostId ?? "none"}::${s.selectedAgentId}`;
}

let initialized = false;
let toastTimer: ReturnType<typeof setTimeout> | null = null;
const notifiedApprovalAgents = new Set<string>();

function notifyToast(msg: string, undo?: UndoArchiveEntry | null): void {
  if (toastTimer) clearTimeout(toastTimer);
  setState((s) => ({
    ...s,
    toastNotice: msg,
    lastArchivedUndo: undo ?? null,
  }));
  toastTimer = setTimeout(() => {
    setState((s) => ({ ...s, toastNotice: null, lastArchivedUndo: null }));
  }, undo ? 6000 : 3000);
}

function syncAttentionIndicators(overview: FleetOverviewView, details: Record<string, HostDetailView>): void {
  if (typeof document !== "undefined") {
    document.title =
      overview.total_pending_approvals > 0
        ? `(${overview.total_pending_approvals}) Orbit`
        : "Orbit";
  }

  if (!state.notifyOnApproval) return;

  for (const detail of Object.values(details)) {
    for (const ag of detail.agents) {
      const key = `${detail.host.host_id}::${ag.agent_id}`;
      if (ag.is_awaiting_permission) {
        if (!notifiedApprovalAgents.has(key)) {
          notifiedApprovalAgents.add(key);
          if (
            typeof document !== "undefined" &&
            document.hidden &&
            "Notification" in window &&
            Notification.permission === "granted"
          ) {
            new Notification(`Orbit · ${detail.host.display_name}`, {
              body: `${ag.title} requires your approval`,
            });
          }
        }
      } else {
        notifiedApprovalAgents.delete(key);
      }
    }
  }
}

/** Tracks an explicit user-initiated draft creation so unrelated background/child agents never hijack draft mode. */
let pendingDraftCreate: {
  hostId: string;
  optimisticAgentId: string | null;
  startedAt: number;
} | null = null;
const PENDING_DRAFT_CREATE_TTL_MS = 30_000;

function slugifyPrefixedBranch(prefix: string, prompt: string): string {
  const cleanPrefix = prefix.trim().replace(/\/+$/, "");
  const slug = prompt
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 28);
  const suffix = Math.random().toString(36).slice(2, 6);
  return `${cleanPrefix}/${slug || "task"}-${suffix}`;
}

function basenameFromPath(rawPath: string): string {
  const clean = rawPath.replace(/\\/g, "/").replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) : clean;
}

async function drainPromptQueueIfIdle(hostId: string, detail: HostDetailView): Promise<void> {
  for (const ag of detail.agents) {
    const key = `${hostId}::${ag.agent_id}`;
    const queue = state.queuedPrompts[key];
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

function omitRecordKeys<T>(
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

function resolveSettingsSectionTarget(
  s: OrbitUiState,
  section?: SettingsSectionTarget,
): SettingsSection {
  const targetHostId = s.selectedHostId ?? s.fleet.hosts[0]?.host_id ?? null;
  if (section === "hosts") {
    return targetHostId ? `host:${targetHostId}:connection` : "add_host";
  }
  if (section === "providers") {
    return targetHostId ? `host:${targetHostId}:agents` : "general";
  }
  return section ?? s.settingsSection;
}

const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;
const inflightAttachmentUploads = new Map<string, Promise<void>>();

function isLikelyTextFile(file: File, bytes: Uint8Array): boolean {
  if (file.size > 64 * 1024) return false;
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

function updateContextMentionItem(
  scopeKey: string,
  id: string,
  updater: (item: ContextMentionItem) => ContextMentionItem,
): void {
  setState((s) => ({
    ...s,
    contextItems: {
      ...s.contextItems,
      [scopeKey]: (s.contextItems[scopeKey] ?? []).map((item) =>
        item.id === id ? updater(item) : item,
      ),
    },
  }));
}

function isOfflineUploadError(errText: string): boolean {
  return /disconnected|reconnect|offline/i.test(errText);
}

async function uploadContextAttachmentToHost(
  scopeKey: string,
  itemId: string,
  file: File,
  filename: string,
  hostId: string,
  throwOnError: boolean,
): Promise<void> {
  updateContextMentionItem(scopeKey, itemId, (cur) =>
    cur.kind === "image" || cur.kind === "file" ? { ...cur, uploading: true } : cur,
  );

  const task = (async () => {
    try {
      const buf = await file.arrayBuffer();
      const staged = await orbitIpc.stageHostAttachmentBytes(
        hostId,
        filename,
        new Uint8Array(buf),
      );
      updateContextMentionItem(scopeKey, itemId, (cur) => {
        if (cur.kind === "image") {
          return {
            ...cur,
            filename: staged.filename,
            path: staged.path,
            sizeBytes: staged.size_bytes,
            stagedHostId: hostId,
            uploading: false,
          };
        }
        if (cur.kind === "file") {
          return {
            ...cur,
            path: staged.path,
            filename: staged.filename,
            sizeBytes: staged.size_bytes,
            stagedHostId: hostId,
            uploading: false,
          };
        }
        return cur;
      });
    } catch (err) {
      const msg = String(err);
      updateContextMentionItem(scopeKey, itemId, (cur) =>
        cur.kind === "image" || cur.kind === "file" ? { ...cur, uploading: false } : cur,
      );
      if (throwOnError) {
        throw err;
      }
      if (isOfflineUploadError(msg)) {
        notifyToast("Host is offline; attachment will upload when reconnected before sending");
      } else {
        orbitActions.removeContextMention(itemId);
        notifyToast(`Attachment upload failed: ${msg}`);
      }
    } finally {
      inflightAttachmentUploads.delete(itemId);
    }
  })();

  inflightAttachmentUploads.set(itemId, task);
  await task;
}

const localQueuedDraftExtras = new Map<
  string,
  Pick<QueuedPromptItem, "slashDirective" | "contexts" | "pastes" | "quotes">
>();

function rememberLocalQueuedDraftExtras(
  queueId: string,
  extras: Pick<QueuedPromptItem, "slashDirective" | "contexts" | "pastes" | "quotes">,
): void {
  localQueuedDraftExtras.set(queueId, extras);
  if (localQueuedDraftExtras.size > 32) {
    const oldest = localQueuedDraftExtras.keys().next().value;
    if (oldest) localQueuedDraftExtras.delete(oldest);
  }
}

export const orbitActions = {
  async init(): Promise<void> {
    if (initialized) return;
    initialized = true;

    // 1. One-time migration of legacy localStorage pin/seen prefs into Rust SQLite
    if (typeof window !== "undefined") {
      try {
        if (window.localStorage.getItem(THREAD_PREF_MIGRATION_KEY) !== "1") {
          const seedsByKey = new Map<string, ClientThreadPrefSeed>();
          const pinnedKeys = initialPrefs.pinnedThreadKeys ?? [];
          const seenMap = initialPrefs.seenAgentSeqs ?? {};
          const nowMs = Date.now();
          pinnedKeys.forEach((rawKey, idx) => {
            const sep = rawKey.indexOf("::");
            if (sep <= 0) return;
            const hostId = rawKey.slice(0, sep);
            const agentId = rawKey.slice(sep + 2);
            seedsByKey.set(rawKey, {
              host_id: hostId,
              agent_id: agentId,
              seen_seq: seenMap[rawKey] ?? 0,
              is_pinned: true,
              pinned_order: idx + 1,
              updated_at_ms: nowMs,
            });
          });
          for (const [rawKey, seq] of Object.entries(seenMap)) {
            if (seedsByKey.has(rawKey)) continue;
            const sep = rawKey.indexOf("::");
            if (sep <= 0) continue;
            seedsByKey.set(rawKey, {
              host_id: rawKey.slice(0, sep),
              agent_id: rawKey.slice(sep + 2),
              seen_seq: seq,
              is_pinned: false,
              pinned_order: 0,
              updated_at_ms: nowMs,
            });
          }
          if (seedsByKey.size > 0) {
            await orbitIpc.seedThreadPrefs(Array.from(seedsByKey.values())).catch(() => undefined);
          }
          window.localStorage.setItem(THREAD_PREF_MIGRATION_KEY, "1");
          scheduleSavePrefs();
        }
      } catch {
        // Ignore storage access errors
      }
    }

    // 2. Non-blocking secondary metadata bootstrap (never blocks or aborts fleet hydration)
    void (async () => {
      const [editors, sshList, version, fallbackCatalogs, slashCommands] = await Promise.all([
        orbitIpc.listExternalEditors().catch(() => []),
        orbitIpc.listSshHosts().catch(() => []),
        orbitIpc.getAppVersion().catch(() => null),
        orbitIpc.getFallbackProviderCatalogs().catch(() => []),
        orbitIpc.getSlashCommands("desktop").catch(() => []),
      ]);
      seedFallbackProviderCatalogs(fallbackCatalogs);
      setState((s) => ({
        ...s,
        externalEditors: editors,
        sshHosts: sshList,
        appVersion: version,
        slashCommands,
      }));
      maybeAutoSyncOutdatedHosts();

      // Sanitize persisted draft models against Rust catalog & retired-slug table
      const providers: ProviderKind[] = ["claude_code", "codex", "antigravity"];
      const sanitizedEntries = await Promise.all(
        providers.map(async (provider) => {
          const current = state.draftModels[provider] ?? null;
          if (!current) return [provider, null] as const;
          const resolved = await orbitIpc
            .resolveModelSelection({
              host_id: null,
              provider,
              desired_model: current,
              desired_effort: state.draftEffort,
            })
            .catch(() => null);
          return [provider, resolved ? resolved.selected_model_id : current] as const;
        }),
      );
      const nextDraftModels = { ...state.draftModels };
      let modelsChanged = false;
      for (const [provider, nextModel] of sanitizedEntries) {
        if (nextDraftModels[provider] !== nextModel) {
          nextDraftModels[provider] = nextModel;
          modelsChanged = true;
        }
      }
      if (modelsChanged) {
        setState((s) => ({ ...s, draftModels: nextDraftModels }), true);
      }
    })();

    // 3. Highest priority: subscribe to live core events and hydrate fleet/local host immediately
    const subscribePromise = orbitIpc
      .subscribe((ev: CoreNotification) => {
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
          // Only stream live stage deltas for the actively viewed session (Rust already coalesces at 16ms).
          if (
            !state.isDraftNewSession &&
            ev.host_id === state.selectedHostId &&
            ev.agent_id === state.selectedAgentId
          ) {
            void orbitActions.pullStageDelta(ev.host_id, ev.agent_id);
          }
        } else if (ev.kind === "git_changed") {
          if (ev.host_id === state.selectedHostId && ev.cwd === state.selectedWorkspaceCwd) {
            const hostId = ev.host_id;
            const cwd = ev.cwd;
            if (gitChangedDebounceTimer !== null) {
              window.clearTimeout(gitChangedDebounceTimer);
            }
            gitChangedDebounceTimer = window.setTimeout(() => {
              gitChangedDebounceTimer = null;
              void orbitActions.loadGitReviewSnapshot(hostId, cwd, state.selectedDiffPath);
              void orbitActions.ensureWorkspaceFiles(hostId, cwd, true);
              orbitActions.markOpenFilesStaleForWorkspace(hostId, cwd);
            }, 150);
          }
        } else if (ev.kind === "terminal_changed") {
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

    try {
      await orbitActions.reloadFleetAndHosts(true);
    } catch {
      setState((s) => (s.fleetHydrated ? s : { ...s, fleetHydrated: true }));
    }
    orbitActions.refreshSidebarView();
    await subscribePromise;
  },

  refreshSidebarView(): void {
    scheduleCoalescedGate(sidebarSyncGate, async () => {
      const view = await orbitIpc.getFleetSidebar({
        sort: state.sidebarProjectSort,
        filter: state.sidebarThreadFilter,
        query: "",
        active_host_id: state.selectedHostId,
        active_agent_id: state.isDraftNewSession ? null : state.selectedAgentId,
        active_cwd: state.selectedWorkspaceCwd || null,
      });
      setState((s) => ({ ...s, sidebarView: view }));
    });
  },

  async reloadFleetOnly(): Promise<void> {
    const snap = await orbitIpc.getFleetOverview();
    setState((s) => {
      const nextHostId = s.selectedHostId ?? snap.hosts[0]?.host_id ?? null;
      return {
        ...s,
        fleet: snap,
        selectedHostId: nextHostId,
      };
    });
    syncAttentionIndicators(state.fleet, state.hostDetails);
    orbitActions.refreshSidebarView();
  },

  setBrowserOffline(offline: boolean): void {
    const prev = state.isBrowserOffline;
    if (prev === offline) return;
    setState((s) => ({ ...s, isBrowserOffline: offline }));
    if (!offline) {
      notifyToast("Back online");
      for (const h of state.fleet.hosts) {
        if (h.connection_state === "disconnected") {
          void orbitActions.reconnectHost(h.host_id);
        }
      }
    }
  },

  dismissVersionMismatch(hostId: string, daemonVersion: string): void {
    setState((s) => ({
      ...s,
      dismissedVersionMismatchHosts: {
        ...s.dismissedVersionMismatchHosts,
        [hostId]: daemonVersion,
      },
    }));
  },

  async reconnectHost(hostId: string): Promise<void> {
    try {
      await orbitIpc.updateHostConfig({ hostId, reconnect: true });
      await orbitActions.reloadFleetAndHosts(false);
    } catch (err) {
      notifyToast(`Reconnect failed: ${String(err)}`);
    }
  },

  async upgradeHostDaemon(
    hostId: string,
    opts?: { force?: boolean; silent?: boolean },
  ): Promise<void> {
    const host = state.fleet.hosts.find((h) => h.host_id === hostId);
    if (!host) return;
    if (state.hostUpgradeStates[hostId]?.status === "upgrading") return;

    const isLocal = isLocalHostCard(host);
    const mode: "local" | "ssh" | "wire" = isLocal ? "local" : host.ssh_alias ? "ssh" : "wire";

    setState((s) => ({
      ...s,
      hostUpgradeStates: {
        ...s.hostUpgradeStates,
        [hostId]: {
          status: "upgrading",
          mode,
          startedAt: Date.now(),
        },
      },
    }));

    try {
      if (mode === "local") {
        const res = await orbitIpc.syncLocalDaemon(hostId, Boolean(opts?.force));
        setState((s) => {
          const next = { ...s.hostUpgradeStates };
          delete next[hostId];
          return { ...s, hostUpgradeStates: next };
        });
        if (res.status === "skipped_dev") {
          return;
        }
        await orbitActions.reloadFleetAndHosts(false);
        if (!opts?.silent) {
          notifyToast(
            res.status === "updated"
              ? `Updated local Orbit service${res.daemon_version ? ` to v${res.daemon_version}` : ""}`
              : "Restarted local Orbit service",
          );
        }
      } else if (mode === "ssh" && host.ssh_alias) {
        await orbitIpc.upgradeRemoteDaemon(
          host.host_id,
          host.ssh_alias,
          host.remote_daemon_port || undefined,
        );
        setState((s) => {
          const next = { ...s.hostUpgradeStates };
          delete next[hostId];
          return { ...s, hostUpgradeStates: next };
        });
        await orbitIpc.connectHost(host.host_id);
        await orbitActions.reloadFleetAndHosts(false);
        if (!opts?.silent) {
          notifyToast(`Updated ${host.display_name}`);
        }
      } else {
        await orbitIpc.upgradeHostDaemonOverWire(host.host_id, normalizeSemver(state.appVersion));
        setState((s) => {
          const next = { ...s.hostUpgradeStates };
          delete next[hostId];
          return { ...s, hostUpgradeStates: next };
        });
        await orbitIpc.connectHost(host.host_id);
        await orbitActions.reloadFleetAndHosts(false);
        if (!opts?.silent) {
          notifyToast(`Updated ${host.display_name}`);
        }
      }
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setState((s) => ({
        ...s,
        hostUpgradeStates: {
          ...s.hostUpgradeStates,
          [hostId]: {
            status: "failed",
            mode,
            error: msg,
            startedAt: Date.now(),
          },
        },
      }));
      if (!opts?.silent) {
        notifyToast(`Upgrade failed: ${msg}`);
      }
    }
  },

  async reloadFleetAndHosts(initialBootstrap = false): Promise<void> {
    const snap = await orbitIpc.getFleetOverview();

    const detailEntries = await Promise.all(
      snap.hosts.map(async (h) => {
        const d = await orbitIpc.getHostDetail(h.host_id);
        return [h.host_id, d] as const;
      }),
    );

    const nextDetails: Record<string, HostDetailView> = {};
    for (const [hid, d] of detailEntries) {
      if (d) nextDetails[hid] = d;
    }

    setState((s) => {
      const targetHostId = s.selectedHostId ?? snap.hosts[0]?.host_id ?? null;
      const activeHostId =
        targetHostId && snap.hosts.some((h) => h.host_id === targetHostId)
          ? targetHostId
          : (snap.hosts[0]?.host_id ?? null);
      const activeDetail = activeHostId ? nextDetails[activeHostId] : undefined;

      let nextAgentId = s.isDraftNewSession ? null : s.selectedAgentId;
      let nextDraft = s.isDraftNewSession;
      if (!s.isDraftNewSession && activeDetail && activeHostId) {
        const stillExists =
          nextAgentId &&
          !s.archivedThreads[threadScopeKey(activeHostId, nextAgentId)] &&
          activeDetail.agents.some((a) => a.agent_id === nextAgentId);
        if (!stillExists) {
          const firstVisible = activeDetail.agents.find(
            (a) => !s.archivedThreads[threadScopeKey(activeHostId, a.agent_id)],
          );
          nextAgentId = firstVisible?.agent_id ?? null;
          nextDraft = nextAgentId === null;
        }
      }

      const activeAgent =
        !nextDraft && nextAgentId
          ? activeDetail?.agents.find((a) => a.agent_id === nextAgentId)
          : undefined;
      const nextCwd =
        activeAgent?.cwd ||
        s.selectedWorkspaceCwd ||
        activeDetail?.workspaces[0]?.cwd ||
        activeDetail?.discovered_projects[0]?.path ||
        "~";

      const avail = activeDetail?.host.available_providers ?? [];
      const nextDraftProvider =
        avail.length > 0 && !avail.includes(s.draftProvider) ? avail[0] : s.draftProvider;

      return {
        ...s,
        fleet: snap,
        fleetHydrated: true,
        hostDetails: nextDetails,
        selectedHostId: activeHostId,
        selectedAgentId: nextAgentId,
        selectedWorkspaceCwd: nextCwd,
        isDraftNewSession: nextDraft,
        draftProvider: nextDraftProvider,
      };
    }, true);

    syncAttentionIndicators(state.fleet, state.hostDetails);
    orbitActions.refreshSidebarView();

    if (state.selectedHostId) {
      if (initialBootstrap) {
        void orbitIpc.refreshQuotas(state.selectedHostId);
        if (state.selectedWorkspaceCwd && state.selectedWorkspaceCwd !== "~") {
          void orbitActions.refreshGit(
            state.selectedHostId,
            state.selectedWorkspaceCwd,
            null,
            state.diffStaged,
          );
        }
      }
      if (state.selectedAgentId && !state.isDraftNewSession) {
        const tlKey = `${state.selectedHostId}::${state.selectedAgentId}`;
        const activeAg = state.hostDetails[state.selectedHostId]?.agents.find(
          (a) => a.agent_id === state.selectedAgentId,
        );
        if (activeAg) {
          markThreadSeenInCore(state.selectedHostId, state.selectedAgentId, activeAg.latest_seq);
        }
        if (initialBootstrap || !state.stages[tlKey]) {
          void orbitIpc.openAgentTimeline(state.selectedHostId, state.selectedAgentId);
          void orbitIpc.listProviderSubagents(state.selectedHostId, state.selectedAgentId);
          void orbitActions.pullStageDelta(state.selectedHostId, state.selectedAgentId, true);
        }
      }
    }
    maybeAutoSyncOutdatedHosts();
  },

  async reloadHostDetail(hostId: string, includeFleetOverview = false): Promise<void> {
    const [detail, overview] = includeFleetOverview
      ? await Promise.all([orbitIpc.getHostDetail(hostId), orbitIpc.getFleetOverview()])
      : [await orbitIpc.getHostDetail(hostId), null];
    if (!detail) return;

    const prevAgents = state.hostDetails[hostId]?.agents ?? [];
    if (
      pendingDraftCreate &&
      Date.now() - pendingDraftCreate.startedAt > PENDING_DRAFT_CREATE_TTL_MS
    ) {
      pendingDraftCreate = null;
    }
    const awaitingDraftForHost =
      pendingDraftCreate !== null && pendingDraftCreate.hostId === hostId;
    const newestCreated =
      state.selectedHostId === hostId && state.isDraftNewSession && awaitingDraftForHost
        ? (pendingDraftCreate?.optimisticAgentId
            ? detail.agents.find((a) => a.agent_id === pendingDraftCreate?.optimisticAgentId)
            : undefined) ??
          detail.agents.find(
            (a) =>
              !a.parent_agent_id &&
              !a.background &&
              !prevAgents.some((p) => p.agent_id === a.agent_id),
          )
        : undefined;
    if (newestCreated) {
      pendingDraftCreate = null;
    }

    if (
      state.notifyOnTurnComplete &&
      typeof document !== "undefined" &&
      document.hidden &&
      "Notification" in window &&
      Notification.permission === "granted"
    ) {
      for (const nextAg of detail.agents) {
        const prevAg = prevAgents.find((p) => p.agent_id === nextAg.agent_id);
        if (
          isAgentForegroundRunning(prevAg) &&
          !isAgentForegroundRunning(nextAg) &&
          !nextAg.is_awaiting_permission
        ) {
          new Notification(`Orbit · ${detail.host.display_name}`, {
            body: `Completed: ${nextAg.title}`,
          });
        }
      }
    }

    let viewedAgentLatestSeq: number | null = null;
    let viewedAgentIdToMark: string | null = null;

    setState((s) => {
      const nextDetails = { ...s.hostDetails, [hostId]: detail };
      let nextAgentId = s.selectedAgentId;
      let nextDraft = s.isDraftNewSession;
      let nextCwd = s.selectedWorkspaceCwd;

      if (newestCreated) {
        nextAgentId = newestCreated.agent_id;
        nextDraft = false;
        nextCwd = newestCreated.cwd;
      } else if (s.selectedHostId === hostId && !s.isDraftNewSession) {
        if (
          !nextAgentId ||
          s.archivedThreads[threadScopeKey(hostId, nextAgentId)] ||
          !detail.agents.some((a) => a.agent_id === nextAgentId)
        ) {
          const firstVisible = detail.agents.find(
            (a) => !s.archivedThreads[threadScopeKey(hostId, a.agent_id)],
          );
          nextAgentId = firstVisible?.agent_id ?? null;
          nextDraft = nextAgentId === null;
        }
      }

      if (s.workspacePanel === "conversation" && !nextDraft && s.selectedHostId === hostId && nextAgentId) {
        const viewed = detail.agents.find((a) => a.agent_id === nextAgentId);
        if (viewed) {
          viewedAgentIdToMark = viewed.agent_id;
          viewedAgentLatestSeq = viewed.latest_seq;
        }
      }

      // Sync attached terminal tab metadata if server assigned/attached a terminal
      let nextTabs = s.terminalTabs;
      let nextActiveTab = s.activeTerminalTabKey;
      if (detail.active_terminal_id) {
        const termMeta = detail.terminals.find((t) => t.terminal_id === detail.active_terminal_id);
        const tabKey = `${hostId}::slot-${detail.active_terminal_slot}`;
        const existingIdx = nextTabs.findIndex((t) => t.key === tabKey);
        const entry: TerminalTabSession = {
          key: tabKey,
          hostId,
          terminalId: detail.active_terminal_id,
          slot: detail.active_terminal_slot,
          cwd: termMeta?.cwd ?? nextCwd,
          title: termMeta?.title ?? "shell",
        };
        if (existingIdx >= 0) {
          nextTabs = [...nextTabs];
          nextTabs[existingIdx] = entry;
        } else {
          nextTabs = [...nextTabs, entry];
          nextActiveTab = tabKey;
        }
      }

      const avail =
        s.selectedHostId === hostId ? detail.host.available_providers : [];
      const nextDraftProvider =
        avail.length > 0 && !avail.includes(s.draftProvider) ? avail[0] : s.draftProvider;

      return {
        ...s,
        ...(overview
          ? {
              fleet: overview,
              selectedHostId: s.selectedHostId ?? overview.hosts[0]?.host_id ?? null,
            }
          : {}),
        hostDetails: nextDetails,
        selectedAgentId: nextAgentId,
        isDraftNewSession: nextDraft,
        selectedWorkspaceCwd: nextCwd,
        draftProvider: nextDraftProvider,
        terminalTabs: nextTabs,
        activeTerminalTabKey: nextActiveTab,
      };
    });

    if (viewedAgentIdToMark !== null && viewedAgentLatestSeq !== null) {
      markThreadSeenInCore(hostId, viewedAgentIdToMark, viewedAgentLatestSeq);
    }

    syncAttentionIndicators(state.fleet, state.hostDetails);
    orbitActions.refreshSidebarView();
    maybeAutoSyncOutdatedHosts();
    void drainPromptQueueIfIdle(hostId, detail);

    if (newestCreated) {
      void orbitIpc.openAgentTimeline(hostId, newestCreated.agent_id);
      void orbitIpc.listProviderSubagents(hostId, newestCreated.agent_id);
      void orbitActions.pullStageDelta(hostId, newestCreated.agent_id, true);
    }
  },

  async pullStageDelta(hostId: string, agentId: string, forceFull = false): Promise<void> {
    const tlKey = `${hostId}::${agentId}`;
    let gate = stagePullGates.get(tlKey);
    if (!gate) {
      gate = { inFlight: false, queued: false, forceFull: false };
      stagePullGates.set(tlKey, gate);
    }
    if (gate.inFlight) {
      gate.queued = true;
      gate.forceFull = gate.forceFull || forceFull;
      return;
    }

    gate.inFlight = true;
    let nextForceFull = forceFull;

    try {
      while (true) {
        gate.queued = false;
        gate.forceFull = false;

        const existing = state.stages[tlKey];
        const sinceRevision = nextForceFull || !existing ? 0 : existing.revision;
        const sinceAuxRevision =
          nextForceFull || !existing ? null : (existing.aux_revision ?? null);
        const delta = await orbitIpc.getAgentStageDelta(
          hostId,
          agentId,
          sinceRevision,
          sinceAuxRevision,
        );

        const prevAgent = existing?.agent ?? null;
        const nextAgent = delta.agent;
        const agentChanged =
          nextAgent !== null &&
          (prevAgent === null ||
            prevAgent.status_label !== nextAgent.status_label ||
            prevAgent.latest_seq !== nextAgent.latest_seq ||
            prevAgent.is_running !== nextAgent.is_running ||
            Boolean(prevAgent.has_background_tasks) !== Boolean(nextAgent.has_background_tasks) ||
            prevAgent.is_awaiting_permission !== nextAgent.is_awaiting_permission ||
            (prevAgent.queued_prompts?.length ?? 0) !== (nextAgent.queued_prompts?.length ?? 0) ||
            prevAgent.queued_prompts?.[0]?.queue_id !== nextAgent.queued_prompts?.[0]?.queue_id ||
            prevAgent.model !== nextAgent.model ||
            prevAgent.effort !== nextAgent.effort ||
            prevAgent.title !== nextAgent.title ||
            prevAgent.cwd !== nextAgent.cwd ||
            prevAgent.updated_at_rfc3339 !== nextAgent.updated_at_rfc3339 ||
            JSON.stringify(prevAgent.mode) !== JSON.stringify(nextAgent.mode));

        const isReset = delta.reset || sinceRevision === 0 || !existing;
        const auxChanged =
          !delta.aux_unchanged &&
          (delta.aux_revision ?? 0) !== (existing?.aux_revision ?? -1);
        const hasChanges =
          isReset ||
          delta.revision !== existing.revision ||
          delta.changed_turns.length > 0 ||
          agentChanged ||
          auxChanged;

        if (hasChanges) {
          let latestSeqToMark: number | null = null;
          setState((s) => {
            const prev = s.stages[tlKey];
            const resetTurns = delta.reset || sinceRevision === 0 || !prev;
            const turns = resetTurns
              ? delta.changed_turns
              : [
                  ...prev.turns.slice(0, delta.replace_from_turn_index),
                  ...delta.changed_turns,
                ];
            const reusePrevAux = Boolean(delta.aux_unchanged && !resetTurns && prev);
            const mergedStage: AgentStageView = {
              revision: delta.revision,
              host_id: delta.host_id,
              agent: delta.agent ?? prev?.agent ?? null,
              turns,
              pending_permissions: reusePrevAux
                ? prev.pending_permissions
                : delta.pending_permissions,
              pending_questions: reusePrevAux
                ? prev.pending_questions
                : delta.pending_questions,
              session_turn_diffs: reusePrevAux
                ? prev.session_turn_diffs
                : delta.session_turn_diffs,
              insights: reusePrevAux ? prev.insights : delta.insights,
              latest_seq: delta.latest_seq,
              aux_revision: delta.aux_revision ?? prev?.aux_revision ?? 0,
            };
            const isViewing =
              s.workspacePanel === "conversation" &&
              !s.isDraftNewSession &&
              s.selectedHostId === hostId &&
              s.selectedAgentId === agentId;
            if (isViewing) {
              latestSeqToMark = Math.max(mergedStage.latest_seq, mergedStage.agent?.latest_seq ?? 0);
            }
            return {
              ...s,
              stages: {
                ...s.stages,
                [tlKey]: mergedStage,
              },
            };
          });
          if (latestSeqToMark !== null) {
            markThreadSeenInCore(hostId, agentId, latestSeqToMark);
          }
        }

        if (!gate.queued) break;
        nextForceFull = gate.forceFull;
      }
    } finally {
      gate.inFlight = false;
    }
  },

  selectHost(hostId: string): void {
    pendingDraftCreate = null;
    const detail = state.hostDetails[hostId];
    const firstAgent =
      detail?.agents.find(
        (a) => !state.archivedThreads[threadScopeKey(hostId, a.agent_id)],
      ) ?? null;
    const nextCwd =
      firstAgent?.cwd ??
      detail?.workspaces[0]?.cwd ??
      detail?.discovered_projects[0]?.path ??
      "~";
    const avail = detail?.host.available_providers ?? [];

    setState(
      (s) => ({
        ...s,
        activeView: "workspace",
        workspacePanel: "conversation",
        selectedHostId: hostId,
        selectedAgentId: firstAgent?.agent_id ?? null,
        isDraftNewSession: firstAgent === null,
        sessionScrollNonce: s.sessionScrollNonce + 1,
        selectedWorkspaceCwd: nextCwd,
        selectedDiffPath: null,
        draftProvider:
          avail.length > 0 && !avail.includes(s.draftProvider) ? avail[0] : s.draftProvider,
      }),
    );

    orbitActions.refreshSidebarView();
    void orbitIpc.refreshQuotas(hostId);
    if (firstAgent) {
      markThreadSeenInCore(hostId, firstAgent.agent_id, firstAgent.latest_seq);
      void orbitIpc.openAgentTimeline(hostId, firstAgent.agent_id);
      void orbitIpc.listProviderSubagents(hostId, firstAgent.agent_id);
      void orbitActions.pullStageDelta(hostId, firstAgent.agent_id, false);
    }
    if (nextCwd && nextCwd !== "~") {
      void orbitActions.refreshGit(hostId, nextCwd, null, state.diffStaged);
    }
  },

  selectAgent(hostId: string, agentId: string): void {
    pendingDraftCreate = null;
    const detail = state.hostDetails[hostId];
    const ag = detail?.agents.find((a) => a.agent_id === agentId);
    const nextCwd = ag?.cwd ?? state.selectedWorkspaceCwd;
    const tKey = threadScopeKey(hostId, agentId);
    const seenSeq = Math.max(ag?.latest_seq ?? 0, state.stages[tKey]?.latest_seq ?? 0);

    setState(
      (s) => {
        let nextArchived = s.archivedThreads;
        if (nextArchived[tKey]) {
          nextArchived = { ...nextArchived };
          delete nextArchived[tKey];
        }
        return {
          ...s,
          activeView: "workspace",
          workspacePanel: "conversation",
          selectedHostId: hostId,
          selectedAgentId: agentId,
          isDraftNewSession: false,
          sessionScrollNonce: s.sessionScrollNonce + 1,
          selectedWorkspaceCwd: nextCwd,
          selectedDiffPath: null,
          archivedThreads: nextArchived,
        };
      },
      true,
    );

    markThreadSeenInCore(hostId, agentId, seenSeq);
    orbitActions.refreshSidebarView();
    void orbitIpc.openAgentTimeline(hostId, agentId);
    void orbitIpc.markAgentRead(hostId, agentId);
    void orbitIpc.listProviderSubagents(hostId, agentId);
    void orbitActions.pullStageDelta(hostId, agentId, false);
    if (nextCwd && nextCwd !== "~") {
      void orbitActions.refreshGit(hostId, nextCwd, null, state.diffStaged);
    }
  },

  startNewThreadDraft(
    hostId?: string,
    cwd?: string,
    prefillPrompt?: string,
    prefillSlash?: SlashDirective | null,
  ): void {
    pendingDraftCreate = null;
    const targetHostId = hostId ?? state.selectedHostId ?? state.fleet.hosts[0]?.host_id ?? null;
    const detail = targetHostId ? state.hostDetails[targetHostId] : undefined;
    const defaultCwd =
      cwd ||
      (targetHostId === state.selectedHostId ? state.selectedWorkspaceCwd : "") ||
      detail?.workspaces[0]?.cwd ||
      detail?.discovered_projects[0]?.path ||
      "~";

    const available = detail?.host.available_providers ?? [];
    const nextProvider =
      available.length > 0 && !available.includes(state.draftProvider)
        ? available[0]
        : state.draftProvider;
    const draftKey = `draft::${targetHostId ?? "none"}`;

    setState((s) => ({
      ...s,
      activeView: "workspace",
      workspacePanel: "conversation",
      selectedHostId: targetHostId,
      selectedAgentId: null,
      isDraftNewSession: true,
      selectedWorkspaceCwd: defaultCwd || "~",
      draftProvider: nextProvider,
      commandPaletteOpen: false,
      ...(prefillPrompt !== undefined
        ? {
            composerDrafts: {
              ...s.composerDrafts,
              [draftKey]: prefillPrompt,
            },
          }
        : {}),
      ...(prefillSlash !== undefined
        ? {
            slashDirectives: {
              ...s.slashDirectives,
              [draftKey]: prefillSlash,
            },
          }
        : {}),
    }));

    orbitActions.refreshSidebarView();
    orbitActions.focusComposer();
  },

  openAutomations(): void {
    setState((s) => ({
      ...s,
      activeView: "workspace",
      workspacePanel: "automations",
      commandPaletteOpen: false,
    }));
  },

  focusComposer(): void {
    setTimeout(() => {
      for (const fn of composerFocusListeners) {
        fn();
      }
    }, 16);
  },

  onComposerFocusRequest(cb: () => void): () => void {
    composerFocusListeners.add(cb);
    return () => composerFocusListeners.delete(cb);
  },

  setSelectedWorkspaceCwd(cwd: string): void {
    setState((s) => ({ ...s, selectedWorkspaceCwd: cwd, selectedDiffPath: null }));
    if (state.selectedHostId && cwd && cwd !== "~") {
      void orbitActions.refreshGit(state.selectedHostId, cwd, null, state.diffStaged);
    }
  },

  selectProjectWorkspace(hostId: string, cwd: string): void {
    const detail = state.hostDetails[hostId];
    const avail = detail?.host.available_providers ?? [];
    setState((s) => ({
      ...s,
      selectedHostId: hostId,
      selectedAgentId: s.isDraftNewSession ? null : s.selectedAgentId,
      selectedWorkspaceCwd: cwd,
      selectedDiffPath: null,
      draftProvider:
        avail.length > 0 && !avail.includes(s.draftProvider) ? avail[0] : s.draftProvider,
    }));
    void orbitActions.ensureWorkspaceFiles(hostId, cwd);
    if (cwd && cwd !== "~") {
      void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
    }
  },

  setDraftConfig(patch: {
    provider?: ProviderKind;
    model?: string | null;
    effort?: ReasoningEffort;
    mode?: SessionMode;
    profileId?: string | null;
    useWorktree?: boolean;
    worktreeBranch?: string;
  }): void {
    setState(
      (s) => {
        const nextProvider = patch.provider ?? s.draftProvider;
        const nextModels =
          patch.model !== undefined
            ? { ...s.draftModels, [nextProvider]: patch.model }
            : s.draftModels;
        return {
          ...s,
          draftProvider: nextProvider,
          draftModels: nextModels,
          draftEffort: patch.effort ?? s.draftEffort,
          draftMode: patch.mode ?? s.draftMode,
          draftProfileId:
            patch.profileId !== undefined
              ? patch.profileId
              : patch.provider !== undefined || patch.model !== undefined
                ? null
                : s.draftProfileId,
          draftUseWorktree: patch.useWorktree ?? s.draftUseWorktree,
          draftWorktreeBranch: patch.worktreeBranch ?? s.draftWorktreeBranch,
        };
      },
      true,
    );
  },

  cycleActiveMode(): void {
    const cycle: SessionMode[] = ["default", "accept_edits", "full_access"];
    if (state.isDraftNewSession || !state.selectedHostId || !state.selectedAgentId) {
      const idx = cycle.findIndex((m) => m === state.draftMode);
      const next = cycle[(idx + 1) % cycle.length];
      orbitActions.setDraftConfig({ mode: next });
      return;
    }
    const detail = state.hostDetails[state.selectedHostId];
    const ag = detail?.agents.find((a) => a.agent_id === state.selectedAgentId);
    if (!ag) return;
    const idx = cycle.findIndex((m) => m === ag.mode);
    const next = cycle[(idx + 1) % cycle.length];
    void orbitIpc.setSessionMode(state.selectedHostId, ag.agent_id, next);
  },

  setComposerDraft(text: string): void {
    const key = composerScopeKey(state);
    if ((state.composerDrafts[key] ?? "") === text) return;
    // Update draft slice without broadcasting global `emitChange()` across unrelated components
    state = {
      ...state,
      composerDrafts: {
        ...state.composerDrafts,
        [key]: text,
      },
    };
    emitComposerDraftChange();
  },

  setSlashDirective(directive: SlashDirective | null): void {
    const key = composerScopeKey(state);
    setState((s) => ({
      ...s,
      slashDirectives: {
        ...s.slashDirectives,
        [key]: directive,
      },
    }));
  },

  addPastedAttachment(content: string): void {
    const key = composerScopeKey(state);
    const lineCount = content.split("\n").length;
    const item: PastedAttachment = {
      id: `paste_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
      label: `Pasted text · ${lineCount} lines`,
      lineCount,
      content,
    };
    setState((s) => ({
      ...s,
      pastedAttachments: {
        ...s.pastedAttachments,
        [key]: [...(s.pastedAttachments[key] ?? []), item],
      },
    }));
  },

  removePastedAttachment(id: string): void {
    const key = composerScopeKey(state);
    setState((s) => ({
      ...s,
      pastedAttachments: {
        ...s.pastedAttachments,
        [key]: (s.pastedAttachments[key] ?? []).filter((p) => p.id !== id),
      },
    }));
  },

  quoteDiffLineToComposer(path: string, lineNumber: number | null, code: string): void {
    const key = composerScopeKey(state);
    const item: DiffQuoteSnippet = {
      id: `quote_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
      path,
      lineNumber,
      code,
    };
    setState((s) => ({
      ...s,
      diffQuotes: {
        ...s.diffQuotes,
        [key]: [...(s.diffQuotes[key] ?? []), item],
      },
    }));
    orbitActions.refreshSidebarView();
    orbitActions.focusComposer();
  },

  removeDiffQuote(id: string): void {
    const key = composerScopeKey(state);
    setState((s) => ({
      ...s,
      diffQuotes: {
        ...s.diffQuotes,
        [key]: (s.diffQuotes[key] ?? []).filter((q) => q.id !== id),
      },
    }));
  },

  async ensureWorkspaceFiles(hostId: string, cwd: string, force = false): Promise<void> {
    if (!hostId || !cwd || cwd === "~") return;
    const cacheKey = `${hostId}::${cwd}`;
    if (!force && (state.workspaceFilesCache[cacheKey]?.length ?? 0) > 0) return;
    const files = await orbitIpc.getWorkspaceFiles(hostId, cwd, force);
    if (files.length > 0 || force) {
      setState((s) => ({
        ...s,
        workspaceFilesCache: {
          ...s.workspaceFilesCache,
          [cacheKey]: files,
        },
      }));
    }
  },

  addContextMention(item: ContextMentionItem): void {
    const key = composerScopeKey(state);
    const existing = state.contextItems[key] ?? [];
    // Prevent duplicate file or git_diff mentions
    const duplicate = existing.some((x) => {
      if (x.kind === "file" && item.kind === "file") {
        return x.path.length > 0 && x.path === item.path;
      }
      if (x.kind === "git_diff" && item.kind === "git_diff") return x.staged === item.staged;
      return false;
    });
    if (duplicate) return;

    setState((s) => ({
      ...s,
      contextItems: {
        ...s.contextItems,
        [key]: [...(s.contextItems[key] ?? []), item],
      },
    }));
  },

  removeContextMention(id: string): void {
    const key = composerScopeKey(state);
    const removed = (state.contextItems[key] ?? []).find((c) => c.id === id);
    if (removed && removed.kind === "image" && removed.previewUrl?.startsWith("blob:")) {
      URL.revokeObjectURL(removed.previewUrl);
    }
    setState((s) => ({
      ...s,
      contextItems: {
        ...s.contextItems,
        [key]: (s.contextItems[key] ?? []).filter((c) => c.id !== id),
      },
    }));
  },

  registerInlineMention(target: InlineMentionTarget): void {
    const key = composerScopeKey(state);
    setState((s) => {
      const prev = s.inlineMentions[key] ?? [];
      const filtered = prev.filter((m) => m.token !== target.token);
      return {
        ...s,
        inlineMentions: {
          ...s.inlineMentions,
          [key]: [...filtered, target],
        },
      };
    });
  },

  captureActiveTerminalTail(): { label: string; lineCount: number; content: string } | null {
    const hostId = state.selectedHostId;
    if (!hostId) return null;
    const activeTab =
      state.terminalTabs.find(
        (t) => t.hostId === hostId && t.key === state.activeTerminalTabKey,
      ) ?? state.terminalTabs.find((t) => t.hostId === hostId);
    const slot = activeTab?.slot ?? 1;
    const ch = terminalChannels.get(`${hostId}::${slot}`);
    if (!ch || ch.chunks.length === 0) {
      notifyToast("No active terminal output buffered yet");
      return null;
    }

    const decoder = new TextDecoder();
    const rawCombined = ch.chunks.map((b) => decoder.decode(b, { stream: true })).join("");
    const stripped = rawCombined.replace(
      // eslint-disable-next-line no-control-regex
      /[\u001b\u009b][[()#;?]*(?:[0-9]{1,4}(?:;[0-9]{0,4})*)?[0-9A-ORZcf-nqry=><]/g,
      "",
    );
    const tailLines = stripped
      .split(/\r?\n/)
      .map((l) => l.trimEnd())
      .filter((l) => l.length > 0)
      .slice(-80);

    if (tailLines.length === 0) {
      notifyToast("Active terminal buffer is empty");
      return null;
    }

    return {
      label: `${activeTab?.title ?? "Terminal"} · ${tailLines.length} lines`,
      lineCount: tailLines.length,
      content: tailLines.join("\n"),
    };
  },

  attachActiveTerminalOutput(): boolean {
    const snap = orbitActions.captureActiveTerminalTail();
    if (!snap) return false;
    orbitActions.addContextMention({
      id: `term_${Date.now()}`,
      kind: "terminal_output",
      label: snap.label,
      lineCount: snap.lineCount,
      content: snap.content,
    });
    return true;
  },

  async stageAndAttachFile(file: File): Promise<void> {
    if (file.size > MAX_ATTACHMENT_BYTES) {
      notifyToast(`Attachment "${file.name}" exceeds 25 MiB limit`);
      return;
    }

    const scopeKey = composerScopeKey(state);
    const hostId = state.selectedHostId;
    const isImage = file.type.startsWith("image/");

    if (isImage) {
      const previewUrl = URL.createObjectURL(file);
      const name = file.name || `screenshot-${Date.now()}.png`;
      const id = `img_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
      orbitActions.addContextMention({
        id,
        kind: "image",
        filename: name,
        path: "",
        previewUrl,
        sizeBytes: file.size,
        localFile: file,
        uploading: Boolean(hostId),
      });
      if (hostId) {
        await uploadContextAttachmentToHost(scopeKey, id, file, name, hostId, false);
      } else {
        notifyToast("Select a host before sending to upload attached image");
      }
      return;
    }

    const buf = await file.arrayBuffer();
    const rawBytes = new Uint8Array(buf);

    // Text / code files <= 64 KiB without NUL bytes: inline into pastedAttachments
    if (isLikelyTextFile(file, rawBytes)) {
      const text = new TextDecoder().decode(rawBytes);
      const lineCount = text.split("\n").length;
      setState((s) => ({
        ...s,
        pastedAttachments: {
          ...s.pastedAttachments,
          [scopeKey]: [
            ...(s.pastedAttachments[scopeKey] ?? []),
            {
              id: `file_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
              label: `${file.name} · ${lineCount} lines`,
              filename: file.name,
              lineCount,
              content: `// File: ${file.name}\n${text}`,
            },
          ],
        },
      }));
      return;
    }

    const name = file.name || `attachment-${Date.now()}.bin`;
    const id = `fref_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
    orbitActions.addContextMention({
      id,
      kind: "file",
      path: "",
      filename: name,
      sizeBytes: file.size,
      localFile: file,
      uploading: Boolean(hostId),
    });
    if (hostId) {
      await uploadContextAttachmentToHost(scopeKey, id, file, name, hostId, false);
    } else {
      notifyToast("Select a host before sending to upload attached file");
    }
  },

  async submitSlashCompact(extraFocus = ""): Promise<void> {
    const hostId = state.selectedHostId;
    const agentId = state.selectedAgentId;
    if (!hostId || state.isDraftNewSession || !agentId) {
      notifyToast("Start an active session first to compact context");
      return;
    }
    const detail = state.hostDetails[hostId];
    const activeAgent = detail?.agents.find((a) => a.agent_id === agentId);
    const provider = activeAgent?.provider ?? state.draftProvider;
    const cwd = activeAgent?.cwd || state.selectedWorkspaceCwd || "~";
    const composed = await orbitIpc.composePrompt({
      provider,
      raw_text: extraFocus.trim(),
      explicit_directive: "compact",
      host_id: hostId,
      cwd,
      contexts: [],
      quotes: [],
      pastes: [],
    });

    await orbitIpc.sendPrompt(hostId, agentId, composed.wire_prompt, {
      displayPrompt: composed.display_prompt,
      badges: composed.badges,
    });
  },

  async submitComposer(steerImmediately = false): Promise<void> {
    const hostId = state.selectedHostId;
    if (!hostId) return;

    const key = composerScopeKey(state);
    if (submittingComposerScopes.has(key)) return;
    submittingComposerScopes.add(key);
    try {
      await orbitActions._submitComposerUnlocked(hostId, key, steerImmediately);
    } finally {
      submittingComposerScopes.delete(key);
    }
  },

  async _submitComposerUnlocked(
    hostId: string,
    key: string,
    steerImmediately: boolean,
  ): Promise<void> {
    const rawDraft = (state.composerDrafts[key] ?? "").trim();
    const storedDirective = state.slashDirectives[key] ?? null;

    const initialContexts = state.contextItems[key] ?? [];
    const registeredInline = state.inlineMentions[key] ?? [];
    const pastes = state.pastedAttachments[key] ?? [];
    const quotes = state.diffQuotes[key] ?? [];

    // Ensure every attached image/file has completed uploading to the target `hostId`
    // BEFORE composing the wire prompt or clearing the composer draft.
    if (initialContexts.some((ctx) => ctx.kind === "image" || ctx.kind === "file")) {
      try {
        const pendingPromises: Promise<void>[] = [];
        for (const ctx of initialContexts) {
          const inflight = inflightAttachmentUploads.get(ctx.id);
          if (inflight) {
            pendingPromises.push(inflight);
          }
        }
        if (pendingPromises.length > 0) {
          await Promise.all(pendingPromises);
        }

        const refreshedContexts = state.contextItems[key] ?? [];
        for (const ctx of refreshedContexts) {
          if (
            (ctx.kind === "image" || ctx.kind === "file") &&
            ctx.localFile &&
            (ctx.stagedHostId !== hostId || !ctx.path)
          ) {
            const fname = ctx.filename || ctx.localFile.name || "attachment.bin";
            await uploadContextAttachmentToHost(key, ctx.id, ctx.localFile, fname, hostId, true);
          }
        }
      } catch (err) {
        notifyToast(`Attachment upload failed: ${String(err)}`);
        return;
      }
    }

    const existingContexts = state.contextItems[key] ?? [];

    // Resolve any inline `@tokens` still present in `rawDraft` without showing attachment boxes
    const resolvedInlineContexts: ContextMentionItem[] = [];
    for (const m of registeredInline) {
      if (!rawDraft.includes(m.token)) continue;
      if (m.kind === "file") {
        resolvedInlineContexts.push({ id: `inl_${m.path}`, kind: "file", path: m.path });
      } else if (m.kind === "git_diff") {
        resolvedInlineContexts.push({
          id: `inl_diff_${m.staged ? "s" : "w"}`,
          kind: "git_diff",
          staged: m.staged,
          label: m.label,
        });
      } else if (m.kind === "terminal_output") {
        resolvedInlineContexts.push({
          id: "inl_term",
          kind: "terminal_output",
          label: m.label,
          lineCount: m.lineCount,
          content: m.content,
        });
      }
    }

    const mergedContexts: ContextMentionItem[] = [...existingContexts];
    for (const rc of resolvedInlineContexts) {
      const dup = mergedContexts.some((x) => {
        if (x.kind === "file" && rc.kind === "file") return x.path === rc.path;
        if (x.kind === "git_diff" && rc.kind === "git_diff") return x.staged === rc.staged;
        if (x.kind === "terminal_output" && rc.kind === "terminal_output") return true;
        return false;
      });
      if (!dup) mergedContexts.push(rc);
    }

    const cwd = state.selectedWorkspaceCwd || "~";
    const needsWorkingDiff =
      storedDirective === "review" ||
      (!storedDirective && /^\/review(?:\s|$)/i.test(rawDraft)) ||
      mergedContexts.some((c) => c.kind === "git_diff" && !c.staged);
    const needsStagedDiff = mergedContexts.some((c) => c.kind === "git_diff" && c.staged);

    let workingDiffText: string | null = null;
    let stagedDiffText: string | null = null;
    if (needsWorkingDiff) {
      await orbitIpc.fetchGitDiff(hostId, cwd, null, false);
      const gr = await orbitIpc.getGitReview(hostId, cwd, null);
      if (gr.raw_diff.trim()) workingDiffText = gr.raw_diff;
    }
    if (needsStagedDiff) {
      await orbitIpc.fetchGitDiff(hostId, cwd, null, true);
      const gr = await orbitIpc.getGitReview(hostId, cwd, null);
      if (gr.raw_diff.trim()) stagedDiffText = gr.raw_diff;
    }

    const detailForCompose = state.hostDetails[hostId];
    const installedForCompose = detailForCompose?.host.available_providers ?? [];
    const activeForCompose =
      !state.isDraftNewSession && state.selectedAgentId
        ? detailForCompose?.agents.find((a) => a.agent_id === state.selectedAgentId) ?? null
        : null;
    const providerForCompose: ProviderKind = activeForCompose
      ? activeForCompose.provider
      : installedForCompose.length > 0 && !installedForCompose.includes(state.draftProvider)
        ? installedForCompose[0]
        : state.draftProvider;

    const contextInputs: ComposerContextInput[] = mergedContexts.map((ctx) => {
      switch (ctx.kind) {
        case "image":
          return { kind: "image", path: ctx.path, filename: ctx.filename };
        case "file":
          return ctx.stagedHostId
            ? {
                kind: "staged_file",
                path: ctx.path,
                filename: ctx.filename || ctx.path,
              }
            : { kind: "workspace_file", path: ctx.path };
        case "git_diff":
          return {
            kind: "git_diff",
            staged: ctx.staged,
            label: ctx.label,
            diff_text: ctx.staged ? stagedDiffText : workingDiffText,
          };
        case "terminal_output":
          return {
            kind: "terminal_output",
            label: ctx.label,
            content: ctx.content,
          };
      }
    });

    const composed = await orbitIpc.composePrompt({
      provider: providerForCompose,
      raw_text: rawDraft,
      explicit_directive: storedDirective,
      host_id: hostId,
      cwd,
      contexts: contextInputs,
      quotes: quotes.map((q) => ({
        path: q.path,
        line_number: q.lineNumber,
        code: q.code,
      })),
      pastes: pastes.map((p) => ({
        label: p.label,
        filename: p.filename ?? null,
        content: p.content,
      })),
    });

    if (composed.is_btw) {
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [key]: "" },
      }));
      orbitActions.openSideQuestionTab();
      if (composed.btw_question) {
        void orbitActions.submitSideQuestion(composed.btw_question);
      }
      return;
    }

    if (composed.is_compact) {
      if (state.isDraftNewSession || !state.selectedAgentId) {
        notifyToast("Start an active session first to compact context");
        return;
      }
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [key]: "" },
        slashDirectives: { ...s.slashDirectives, [key]: null },
      }));
      await orbitIpc.sendPrompt(hostId, state.selectedAgentId, composed.wire_prompt, {
        displayPrompt: composed.display_prompt,
        badges: composed.badges,
      });
      return;
    }

    if (composed.is_empty) {
      return;
    }

    const effectiveDirective = composed.effective_directive;
    const cleanUserText = composed.display_prompt;
    const imagePathQueueByLabel = new Map<string, string[]>();
    const filePathQueueByLabel = new Map<string, string[]>();
    const sentImages: SentImagePreviewMeta[] = [];
    const targetAgentId = state.isDraftNewSession ? null : state.selectedAgentId;
    for (const ctx of mergedContexts) {
      if (ctx.kind === "image") {
        if (ctx.path && ctx.previewUrl) {
          seedPromptImagePreview(hostId, ctx.path, ctx.previewUrl, true);
        } else if (ctx.previewUrl?.startsWith("blob:")) {
          URL.revokeObjectURL(ctx.previewUrl);
        }
        sentImages.push({
          label: ctx.filename,
          path: ctx.path || null,
          previewUrl: ctx.previewUrl || null,
        });
        if (ctx.path) {
          const queue = imagePathQueueByLabel.get(ctx.filename);
          if (queue) queue.push(ctx.path);
          else imagePathQueueByLabel.set(ctx.filename, [ctx.path]);
        }
      } else if (ctx.kind === "file" && ctx.path) {
        const label = ctx.filename || basenameFromPath(ctx.path);
        const queue = filePathQueueByLabel.get(label);
        if (queue) queue.push(ctx.path);
        else filePathQueueByLabel.set(label, [ctx.path]);
      }
    }
    if (sentImages.length > 0) {
      recordRecentSentTurnImages(hostId, targetAgentId, cleanUserText, sentImages);
    }
    const badges: PromptBadge[] = composed.badges.map((b) => {
      if (b.path) return b;
      if (b.kind === "image") {
        const fallbackPath = imagePathQueueByLabel.get(b.label)?.shift();
        if (fallbackPath) return { ...b, path: fallbackPath };
      } else if (b.kind === "file") {
        const fallbackPath = filePathQueueByLabel.get(b.label)?.shift();
        if (fallbackPath) return { ...b, path: fallbackPath };
      }
      return b;
    });
    const assembledWirePrompt = composed.wire_prompt;

    const nextHistory =
      cleanUserText && state.promptHistory[state.promptHistory.length - 1] !== cleanUserText
        ? [...state.promptHistory.slice(-29), cleanUserText]
        : state.promptHistory;

    setState(
      (s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [key]: "" },
        slashDirectives: { ...s.slashDirectives, [key]: null },
        inlineMentions: { ...s.inlineMentions, [key]: [] },
        contextItems: { ...s.contextItems, [key]: [] },
        pastedAttachments: { ...s.pastedAttachments, [key]: [] },
        diffQuotes: { ...s.diffQuotes, [key]: [] },
        promptHistory: nextHistory,
      }),
      true,
    );

    if (state.isDraftNewSession || !state.selectedAgentId) {
      const hostDetailForCreate = state.hostDetails[hostId];
      const installedProviders = hostDetailForCreate?.host.available_providers ?? [];
      if (hostDetailForCreate && installedProviders.length === 0) {
        orbitActions.showToast("No installed Agent CLI detected on this host");
        return;
      }
      const effectiveProvider =
        installedProviders.length > 0 && !installedProviders.includes(state.draftProvider)
          ? installedProviders[0]
          : state.draftProvider;

      const branchSeed =
        cleanUserText ||
        (effectiveDirective === "review"
          ? "code-review"
          : effectiveDirective === "plan"
            ? "plan"
            : badges[0]?.label ?? "task");

      let worktreeBranch: string | null = null;
      if (state.draftUseWorktree) {
        const manual = state.draftWorktreeBranch.trim();
        if (manual) {
          worktreeBranch = manual;
        } else if (
          state.worktreeBranchPrefix.trim() &&
          state.worktreeBranchPrefix.trim() !== "auto"
        ) {
          worktreeBranch = slugifyPrefixedBranch(state.worktreeBranchPrefix, branchSeed);
        } else {
          worktreeBranch = "auto";
        }
      }
      const resolvedSel = await orbitIpc
        .resolveModelSelection({
          host_id: hostId,
          provider: effectiveProvider,
          desired_model: state.draftModels[effectiveProvider] ?? null,
          desired_effort: state.draftEffort,
        })
        .catch(() => null);
      const cat = getCatalogForProvider(
        hostDetailForCreate?.provider_catalogs,
        effectiveProvider,
      );
      const explicitDraftModel = state.draftModels[effectiveProvider] ?? null;
      const spec = resolveActiveModelSpec(cat, explicitDraftModel);
      const resolvedModel =
        resolvedSel?.selected_model_id ?? explicitDraftModel ?? spec?.id ?? null;
      const resolvedEffort =
        resolvedSel?.selected_effort ??
        resolveSupportedEffortForModel(
          spec,
          state.draftEffort,
          cat.configured_default_effort,
        );

      pendingDraftCreate = { hostId, optimisticAgentId: null, startedAt: Date.now() };
      let optimisticId: string | undefined;
      try {
        optimisticId = await orbitIpc.createAgent({
          hostId,
          cwd: state.selectedWorkspaceCwd || "~",
          provider: effectiveProvider,
          model: resolvedModel,
          effort: resolvedEffort,
          mode: state.draftMode,
          initialPrompt: assembledWirePrompt,
          initialDisplayPrompt: cleanUserText,
          initialBadges: badges,
          worktreeBranch,
          profileId: state.draftProfileId,
        });
      } catch (err) {
        pendingDraftCreate = null;
        throw err;
      }
      if (optimisticId && pendingDraftCreate && pendingDraftCreate.hostId === hostId) {
        pendingDraftCreate = {
          hostId,
          optimisticAgentId: optimisticId,
          startedAt: Date.now(),
        };
      }
      setState((s) => ({
        ...s,
        draftProvider: effectiveProvider,
        draftWorktreeBranch: "",
      }));
      return;
    }

    const agentId = state.selectedAgentId;
    const detail = state.hostDetails[hostId];
    const activeAgent = detail?.agents.find((a) => a.agent_id === agentId);
    const supportsServerQueue = Boolean(detail?.host.supports_server_queue);

    if (isAgentForegroundRunning(activeAgent) || activeAgent?.is_awaiting_permission) {
      if (steerImmediately) {
        await orbitIpc.sendPrompt(hostId, agentId, assembledWirePrompt, {
          displayPrompt: cleanUserText,
          badges,
          activeTurnBehavior: "steer",
        });
      } else if (supportsServerQueue) {
        const queueId = `q_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
        rememberLocalQueuedDraftExtras(queueId, {
          slashDirective: effectiveDirective,
          contexts: mergedContexts,
          pastes,
          quotes,
        });
        await orbitIpc.enqueuePrompt(hostId, agentId, assembledWirePrompt, {
          displayPrompt: cleanUserText,
          badges,
          queueId,
        });
      } else {
        const queuedItem: QueuedPromptItem = {
          id: `q_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
          wirePrompt: assembledWirePrompt,
          displayText: cleanUserText,
          badges,
          slashDirective: effectiveDirective,
          contexts: mergedContexts,
          pastes,
          quotes,
        };
        setState((s) => ({
          ...s,
          queuedPrompts: {
            ...s.queuedPrompts,
            [key]: [...(s.queuedPrompts[key] ?? []), queuedItem],
          },
        }));
      }
      return;
    }

    setState((s) => ({ ...s, sessionScrollNonce: s.sessionScrollNonce + 1 }));
    await orbitIpc.sendPrompt(hostId, agentId, assembledWirePrompt, {
      displayPrompt: cleanUserText,
      badges,
    });
  },

  removeQueuedPrompt(index: number, queueId?: string): void {
    const hostId = state.selectedHostId;
    const agentId = state.selectedAgentId;
    const detail = hostId ? state.hostDetails[hostId] : undefined;
    if (hostId && agentId && detail?.host.supports_server_queue && queueId) {
      localQueuedDraftExtras.delete(queueId);
      void orbitIpc.removeQueuedPrompt(hostId, agentId, queueId);
      return;
    }

    const key = composerScopeKey(state);
    setState((s) => ({
      ...s,
      queuedPrompts: {
        ...s.queuedPrompts,
        [key]: (s.queuedPrompts[key] ?? []).filter((_, idx) => idx !== index),
      },
    }));
  },

  popQueuedToDraft(index: number, queueId?: string): void {
    const key = composerScopeKey(state);
    const hostId = state.selectedHostId;
    const agentId = state.selectedAgentId;
    const detail = hostId ? state.hostDetails[hostId] : undefined;
    if (hostId && agentId && detail?.host.supports_server_queue && queueId) {
      const activeAgent = detail.agents.find((a) => a.agent_id === agentId);
      const serverTarget = activeAgent?.queued_prompts?.find((q) => q.queue_id === queueId);
      if (!serverTarget) return;
      const extras = localQueuedDraftExtras.get(queueId);
      localQueuedDraftExtras.delete(queueId);
      const restoredContexts: ContextMentionItem[] =
        extras?.contexts ??
        (serverTarget.badges ?? [])
          .filter((b) => b.kind === "file" && b.path)
          .map((b, bIdx) => ({
            id: `ctx_q_${Date.now()}_${bIdx}`,
            kind: "file" as const,
            path: b.path as string,
            filename: b.label,
          }));
      setState((s) => ({
        ...s,
        composerDrafts: {
          ...s.composerDrafts,
          [key]:
            s.composerDrafts[key] && serverTarget.display_text
              ? `${s.composerDrafts[key]}\n${serverTarget.display_text}`
              : serverTarget.display_text || s.composerDrafts[key] || "",
        },
        slashDirectives: {
          ...s.slashDirectives,
          [key]: extras?.slashDirective ?? s.slashDirectives[key] ?? null,
        },
        contextItems: {
          ...s.contextItems,
          [key]: [...(s.contextItems[key] ?? []), ...restoredContexts],
        },
        pastedAttachments: {
          ...s.pastedAttachments,
          [key]: [...(s.pastedAttachments[key] ?? []), ...(extras?.pastes ?? [])],
        },
        diffQuotes: {
          ...s.diffQuotes,
          [key]: [...(s.diffQuotes[key] ?? []), ...(extras?.quotes ?? [])],
        },
      }));
      void orbitIpc.removeQueuedPrompt(hostId, agentId, queueId);
      orbitActions.refreshSidebarView();
      orbitActions.focusComposer();
      return;
    }

    const list = state.queuedPrompts[key] ?? [];
    const target = list[index];
    if (!target) return;
    setState((s) => ({
      ...s,
      composerDrafts: {
        ...s.composerDrafts,
        [key]:
          s.composerDrafts[key] && target.displayText
            ? `${s.composerDrafts[key]}\n${target.displayText}`
            : target.displayText || s.composerDrafts[key] || "",
      },
      slashDirectives: {
        ...s.slashDirectives,
        [key]: target.slashDirective ?? s.slashDirectives[key] ?? null,
      },
      contextItems: {
        ...s.contextItems,
        [key]: [...(s.contextItems[key] ?? []), ...target.contexts],
      },
      pastedAttachments: {
        ...s.pastedAttachments,
        [key]: [...(s.pastedAttachments[key] ?? []), ...target.pastes],
      },
      diffQuotes: {
        ...s.diffQuotes,
        [key]: [...(s.diffQuotes[key] ?? []), ...target.quotes],
      },
      queuedPrompts: {
        ...s.queuedPrompts,
        [key]: list.filter((_, idx) => idx !== index),
      },
    }));
    orbitActions.refreshSidebarView();
    orbitActions.focusComposer();
  },

  async flushQueuedPromptImmediately(index: number, queueId?: string): Promise<void> {
    const hostId = state.selectedHostId;
    const agentId = state.selectedAgentId;
    if (!hostId || !agentId) return;
    const detail = state.hostDetails[hostId];
    if (detail?.host.supports_server_queue && queueId) {
      localQueuedDraftExtras.delete(queueId);
      setState((s) => ({ ...s, sessionScrollNonce: s.sessionScrollNonce + 1 }));
      await orbitIpc.flushQueuedPromptNow(hostId, agentId, queueId);
      return;
    }

    const key = `${hostId}::${agentId}`;
    const list = state.queuedPrompts[key] ?? [];
    const target = list[index];
    if (!target) return;

    setState((s) => ({
      ...s,
      queuedPrompts: {
        ...s.queuedPrompts,
        [key]: list.filter((_, idx) => idx !== index),
      },
    }));
    const activeAgent = state.hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId);
    if (isAgentForegroundRunning(activeAgent) || activeAgent?.is_awaiting_permission) {
      await orbitIpc.interruptAgent(hostId, agentId);
    }
    await orbitIpc.sendPrompt(hostId, agentId, target.wirePrompt, {
      displayPrompt: target.displayText,
      badges: target.badges,
    });
  },

  reorderQueuedPrompts(fromIndex: number, toIndex: number): void {
    if (fromIndex === toIndex || fromIndex < 0 || toIndex < 0) return;
    const hostId = state.selectedHostId;
    const agentId = state.selectedAgentId;
    const detail = hostId ? state.hostDetails[hostId] : undefined;
    if (hostId && agentId && detail?.host.supports_server_queue) {
      const activeAgent = detail.agents.find((a) => a.agent_id === agentId);
      const current = activeAgent?.queued_prompts ?? [];
      if (fromIndex >= current.length || toIndex >= current.length) return;
      const nextIds = current.map((q) => q.queue_id);
      const [moved] = nextIds.splice(fromIndex, 1);
      if (!moved) return;
      nextIds.splice(toIndex, 0, moved);
      void orbitIpc.reorderQueuedPrompts(hostId, agentId, nextIds);
      return;
    }

    const key = composerScopeKey(state);
    const list = state.queuedPrompts[key] ?? [];
    if (fromIndex >= list.length || toIndex >= list.length) return;
    const next = list.slice();
    const [moved] = next.splice(fromIndex, 1);
    if (!moved) return;
    next.splice(toIndex, 0, moved);
    setState((s) => ({
      ...s,
      queuedPrompts: {
        ...s.queuedPrompts,
        [key]: next,
      },
    }));
  },

  jumpToNextAttentionItem(): void {
    for (const host of state.fleet.hosts) {
      const detail = state.hostDetails[host.host_id];
      if (!detail) continue;
      const blocked = detail.agents.find(
        (a) =>
          a.is_awaiting_permission &&
          !state.archivedThreads[threadScopeKey(host.host_id, a.agent_id)],
      );
      if (blocked) {
        orbitActions.selectAgent(host.host_id, blocked.agent_id);
        return;
      }
    }
    notifyToast("All agents clear — no pending approvals");
  },

  selectThreadByIndex(indexZeroBased: number): void {
    const ordered: { hostId: string; agentId: string }[] = [];
    for (const h of state.fleet.hosts) {
      const detail = state.hostDetails[h.host_id];
      if (!detail) continue;
      for (const ag of detail.agents) {
        if (!state.archivedThreads[threadScopeKey(h.host_id, ag.agent_id)]) {
          ordered.push({ hostId: h.host_id, agentId: ag.agent_id });
        }
      }
    }
    const target = ordered[indexZeroBased];
    if (target) {
      orbitActions.selectAgent(target.hostId, target.agentId);
    }
  },

  togglePinThread(hostId: string, agentId: string): void {
    void orbitIpc.togglePinAgent(hostId, agentId).then(() => {
      orbitActions.refreshSidebarView();
    });
  },

  renameThread(hostId: string, agentId: string, newTitle: string): void {
    const tKey = threadScopeKey(hostId, agentId);
    const trimmed = newTitle.trim();
    const originalTitle =
      state.hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId)?.title ?? "";
    setState(
      (s) => {
        const nextTitles = { ...s.threadCustomTitles };
        if (!trimmed || trimmed === originalTitle) {
          delete nextTitles[tKey];
        } else {
          nextTitles[tKey] = trimmed;
        }
        return { ...s, threadCustomTitles: nextTitles };
      },
      true,
    );
  },

  markThreadUnread(hostId: string, agentId: string): void {
    const tKey = threadScopeKey(hostId, agentId);
    const isCurrentlySelected =
      !state.isDraftNewSession &&
      state.selectedHostId === hostId &&
      state.selectedAgentId === agentId;

    lastReportedSeenSeq.delete(tKey);
    if (isCurrentlySelected) {
      setState(
        (s) => ({
          ...s,
          isDraftNewSession: true,
          selectedAgentId: null,
        }),
        true,
      );
    }
    void orbitIpc.markAgentUnread(hostId, agentId).then(() => {
      orbitActions.refreshSidebarView();
    });
  },

  markThreadRead(hostId: string, agentId: string): void {
    const tKey = threadScopeKey(hostId, agentId);
    const detail = state.hostDetails[hostId];
    const ag =
      detail?.agents.find((a) => a.agent_id === agentId) ??
      state.stages[tKey]?.agent ??
      null;
    const targetSeq = Math.max(
      ag?.latest_seq ?? 0,
      state.stages[tKey]?.latest_seq ?? 0,
      1,
    );
    lastReportedSeenSeq.set(tKey, targetSeq);
    void orbitIpc.markAgentSeen(hostId, agentId, targetSeq).then(() => {
      orbitActions.refreshSidebarView();
    });
  },

  async copyThread(hostId: string, agentId: string): Promise<void> {
    const tKey = threadScopeKey(hostId, agentId);
    const sections: string[] = [];
    const cachedStage = state.stages[tKey];
    if (cachedStage && cachedStage.turns.length > 0) {
      for (const turn of cachedStage.turns) {
        if (turn.user_prompt?.item_type === "user_prompt") {
          const txt = turn.user_prompt.text.trim();
          if (txt) sections.push(`## User\n\n${txt}`);
        }
        for (const seg of turn.live_segments) {
          if (seg.kind === "assistant_message" && seg.item.item_type === "assistant_message") {
            const txt = seg.item.text.trim();
            if (txt) sections.push(`## Assistant\n\n${txt}`);
          }
        }
      }
    } else {
      let items: Awaited<ReturnType<typeof orbitIpc.getAgentTimeline>>["items"];
      try {
        const full = await orbitIpc.getAgentTimeline(hostId, agentId);
        items = full.items;
      } catch {
        items = [];
      }
      for (const item of items) {
        if (item.item_type === "user_prompt") {
          const txt = item.text.trim();
          if (txt) sections.push(`## User\n\n${txt}`);
        } else if (item.item_type === "assistant_message") {
          const txt = item.text.trim();
          if (txt) sections.push(`## Assistant\n\n${txt}`);
        }
      }
    }

    const fallbackTitle =
      state.threadCustomTitles[tKey] ||
      state.hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId)?.title ||
      agentId;
    const payload = sections.length > 0 ? sections.join("\n\n---\n\n") : fallbackTitle;
    try {
      await navigator.clipboard.writeText(payload);
      notifyToast("Copied conversation");
    } catch {
      notifyToast("Failed to copy conversation");
    }
  },

  async rewindAgent(
    hostId: string,
    agentId: string,
    toBeforeSeq: number,
    restoreFiles: boolean,
    fallbackPromptText?: string,
  ): Promise<boolean> {
    const tKey = threadScopeKey(hostId, agentId);
    // Clear queued follow-ups before calling the daemon so transitioning to Idle never auto-dispatches stale prompts
    setState((s) => {
      if (!s.queuedPrompts[tKey]?.length) return s;
      const nextQueued = { ...s.queuedPrompts };
      delete nextQueued[tKey];
      return { ...s, queuedPrompts: nextQueued };
    });

    try {
      const res = await orbitIpc.rewindAgent(hostId, agentId, toBeforeSeq, restoreFiles);
      const restoredDraft = (res.restored_prompt ?? fallbackPromptText ?? "").trim();
      const cwd = res.agent?.cwd || state.selectedWorkspaceCwd;

      const openFileTabsToReload: Array<{ hostId: string; cwd: string; path: string }> = [];
      setState((s) => {
        let nextFileCache = s.fileViewCache;
        if (restoreFiles && res.restored_files > 0 && cwd && cwd !== "~") {
          const prefix = `${hostId}::${cwd}::`;
          nextFileCache = {};
          for (const [k, v] of Object.entries(s.fileViewCache)) {
            if (!k.startsWith(prefix)) {
              nextFileCache[k] = v;
            }
          }
          for (const tab of s.sidePanelTabs) {
            if (tab.kind === "file" && tab.filePath) {
              const tabHost = tab.hostId ?? s.selectedHostId;
              const tabCwd = tab.cwd ?? s.selectedWorkspaceCwd;
              if (tabHost === hostId && tabCwd === cwd) {
                openFileTabsToReload.push({ hostId: tabHost, cwd: tabCwd, path: tab.filePath });
              }
            }
          }
        }
        return {
          ...s,
          fileViewCache: nextFileCache,
          rewoundSessions: { ...s.rewoundSessions, [tKey]: true },
          composerDrafts: restoredDraft
            ? { ...s.composerDrafts, [tKey]: restoredDraft }
            : s.composerDrafts,
        };
      });

      await Promise.all([
        orbitActions.pullStageDelta(hostId, agentId, true),
        orbitActions.reloadHostDetail(hostId, true),
      ]);

      for (const tabInfo of openFileTabsToReload) {
        void orbitActions.loadWorkspaceFile(tabInfo.hostId, tabInfo.cwd, tabInfo.path, true);
      }

      if (cwd && cwd !== "~") {
        void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
      }

      if (res.notice) {
        notifyToast(res.notice);
      } else if (restoreFiles && res.restored_files > 0) {
        notifyToast(
          `Rewound conversation · Restored ${res.restored_files} ${res.restored_files === 1 ? "file" : "files"}`,
        );
      } else {
        notifyToast("Rewound conversation to this point");
      }

      setTimeout(() => {
        for (const fn of composerFocusListeners) {
          fn();
        }
      }, 16);
      return true;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      notifyToast(msg || "Failed to rewind conversation");
      return false;
    }
  },

  archiveThread(hostId: string, agentId: string): void {
    const tKey = threadScopeKey(hostId, agentId);
    const wasSelected =
      !state.isDraftNewSession &&
      state.selectedHostId === hostId &&
      state.selectedAgentId === agentId;

    const detail = state.hostDetails[hostId];
    const targetAgent = detail?.agents.find((a) => a.agent_id === agentId);
    let fallbackAgentId: string | null = null;
    if (wasSelected) {
      const nextCandidate = detail?.agents.find(
        (a) =>
          a.agent_id !== agentId &&
          !a.archived_at_rfc3339 &&
          !state.archivedThreads[threadScopeKey(hostId, a.agent_id)],
      );
      fallbackAgentId = nextCandidate?.agent_id ?? null;
    }

    setState(
      (s) => ({
        ...s,
        archivedThreads: {
          ...s.archivedThreads,
          [tKey]: {
            key: tKey,
            hostId,
            agentId,
            archivedAtMs: Date.now(),
          },
        },
        ...(wasSelected && !fallbackAgentId
          ? {
              selectedAgentId: null,
              isDraftNewSession: true,
            }
          : {}),
      }),
      true,
    );

    void orbitIpc.archiveAgent(hostId, agentId);
    orbitActions.refreshSidebarView();
    if (wasSelected && fallbackAgentId) {
      orbitActions.selectAgent(hostId, fallbackAgentId);
    }
    notifyToast("Conversation archived", {
      hostId,
      agentId,
      title: targetAgent?.title ?? agentId,
    });
  },

  restoreArchivedThread(hostId: string, agentId: string): void {
    const tKey = threadScopeKey(hostId, agentId);
    setState(
      (s) => {
        const nextArchived = { ...s.archivedThreads };
        delete nextArchived[tKey];
        return {
          ...s,
          archivedThreads: nextArchived,
          lastArchivedUndo: null,
        };
      },
      true,
    );
    void orbitIpc.unarchiveAgent(hostId, agentId);
    orbitActions.refreshSidebarView();
    orbitActions.selectAgent(hostId, agentId);
    notifyToast("Conversation restored");
  },

  undoLastArchive(): void {
    const entry = state.lastArchivedUndo;
    if (!entry) return;
    orbitActions.restoreArchivedThread(entry.hostId, entry.agentId);
  },

  async detachAgent(hostId: string, agentId: string): Promise<void> {
    await orbitIpc.detachAgent(hostId, agentId);
    orbitActions.refreshSidebarView();
    notifyToast("Detached subagent to standalone thread");
  },

  async deleteThreadPermanently(hostId: string, agentId: string): Promise<void> {
    const tKey = threadScopeKey(hostId, agentId);
    const wasSelected =
      !state.isDraftNewSession &&
      state.selectedHostId === hostId &&
      state.selectedAgentId === agentId;

    setState(
      (s) => {
        const nextArchived = { ...s.archivedThreads };
        delete nextArchived[tKey];
        const nextTitles = { ...s.threadCustomTitles };
        delete nextTitles[tKey];
        const nextStages = { ...s.stages };
        delete nextStages[tKey];
        const nextDrafts = { ...s.composerDrafts };
        delete nextDrafts[tKey];
        const nextDirectives = { ...s.slashDirectives };
        delete nextDirectives[tKey];
        const nextInline = { ...s.inlineMentions };
        delete nextInline[tKey];
        const nextContexts = { ...s.contextItems };
        for (const ctx of nextContexts[tKey] ?? []) {
          if (ctx.kind === "image" && ctx.previewUrl?.startsWith("blob:")) {
            URL.revokeObjectURL(ctx.previewUrl);
          }
        }
        delete nextContexts[tKey];
        const nextPastes = { ...s.pastedAttachments };
        delete nextPastes[tKey];
        const nextQuotes = { ...s.diffQuotes };
        delete nextQuotes[tKey];
        const nextQueued = { ...s.queuedPrompts };
        delete nextQueued[tKey];
        const nextSideQ = { ...s.sideQuestionsByScope };
        delete nextSideQ[tKey];
        const nextRewound = { ...s.rewoundSessions };
        delete nextRewound[tKey];
        return {
          ...s,
          archivedThreads: nextArchived,
          threadCustomTitles: nextTitles,
          stages: nextStages,
          composerDrafts: nextDrafts,
          slashDirectives: nextDirectives,
          inlineMentions: nextInline,
          contextItems: nextContexts,
          pastedAttachments: nextPastes,
          diffQuotes: nextQuotes,
          queuedPrompts: nextQueued,
          sideQuestionsByScope: nextSideQ,
          rewoundSessions: nextRewound,
          ...(wasSelected ? { selectedAgentId: null, isDraftNewSession: true } : {}),
        };
      },
      true,
    );

    lastReportedSeenSeq.delete(tKey);
    stagePullGates.delete(tKey);
    await orbitIpc.deleteAgent(hostId, agentId);
    orbitActions.refreshSidebarView();
    notifyToast("Conversation deleted");
  },

  setSidebarViewPrefs(patch: {
    projectSort?: SidebarProjectSort;
    threadFilter?: SidebarThreadFilter;
  }): void {
    setState(
      (s) => ({
        ...s,
        sidebarProjectSort: patch.projectSort ?? s.sidebarProjectSort,
        sidebarThreadFilter: patch.threadFilter ?? s.sidebarThreadFilter,
      }),
      true,
    );
    orbitActions.refreshSidebarView();
  },

  toggleSidebar(): void {
    const isCompact = state.isCompactViewport;
    setState(
      (s) => {
        const nextOpen = !s.sidebarOpen;
        return {
          ...s,
          sidebarOpen: nextOpen,
          sidebarAutoYieldedForSidePanel: false,
          sidebarDesktopOpenPref: s.isCompactViewport ? s.sidebarDesktopOpenPref : nextOpen,
        };
      },
      !isCompact,
    );
  },

  updateViewportWidth(viewportWidth: number): void {
    const nextCompact = state.isCompactViewport
      ? viewportWidth < COMPACT_VIEWPORT_EXPAND_PX
      : viewportWidth < COMPACT_VIEWPORT_COLLAPSE_PX;
    const nextTight = state.isTightForDualSidebars
      ? viewportWidth < DUAL_SIDEBAR_RESTORE_PX
      : viewportWidth < DUAL_SIDEBAR_YIELD_PX;

    if (
      nextCompact === state.isCompactViewport &&
      nextTight === state.isTightForDualSidebars
    ) {
      return;
    }

    setState((s) => {
      let nextSidebarOpen = s.sidebarOpen;
      let nextYielded = s.sidebarAutoYieldedForSidePanel;

      if (nextCompact) {
        nextSidebarOpen = false;
        nextYielded = false;
      } else if (nextTight && s.sidePanelOpen && s.sidebarOpen) {
        nextSidebarOpen = false;
        nextYielded = true;
      } else if (!nextTight && s.sidebarAutoYieldedForSidePanel) {
        nextSidebarOpen = s.sidebarDesktopOpenPref;
        nextYielded = false;
      } else if (s.isCompactViewport && !nextCompact) {
        nextSidebarOpen = nextTight && s.sidePanelOpen ? false : s.sidebarDesktopOpenPref;
        nextYielded = nextTight && s.sidePanelOpen && s.sidebarDesktopOpenPref;
      }

      return {
        ...s,
        isCompactViewport: nextCompact,
        isTightForDualSidebars: nextTight,
        sidebarOpen: nextSidebarOpen,
        sidebarAutoYieldedForSidePanel: nextYielded,
      };
    });
  },

  setSidebarWidth(w: number, persist = true): void {
    setState((s) => ({ ...s, sidebarWidth: Math.min(400, Math.max(230, w)) }), persist);
  },

  toggleHostCollapsed(hostId: string): void {
    setState((s) => ({
      ...s,
      collapsedHostIds: {
        ...s.collapsedHostIds,
        [hostId]: !s.collapsedHostIds[hostId],
      },
    }));
  },

  toggleProjectCollapsed(projectId: string): void {
    setState((s) => ({
      ...s,
      collapsedProjectIds: {
        ...s.collapsedProjectIds,
        [projectId]: !s.collapsedProjectIds[projectId],
      },
    }));
  },

  async addProjectDirectory(hostId: string, dirPath: string, name?: string): Promise<void> {
    const cleanPath = dirPath.trim();
    if (!hostId || !cleanPath) return;
    try {
      const created = await orbitIpc.upsertProject({
        hostId,
        name: (name ?? basenameFromPath(cleanPath)) || cleanPath,
        directories: [cleanPath],
      });
      const keysToUnhide = new Set<string>([projectPathScopeKey(hostId, cleanPath)]);
      if (created.directories[0]) {
        keysToUnhide.add(projectPathScopeKey(hostId, created.directories[0]));
      }
      setState(
        (s) => ({
          ...s,
          remoteFolderPickerHostId: null,
          hiddenProjectKeys: omitRecordKeys(s.hiddenProjectKeys, keysToUnhide),
          collapsedProjectIds: {
            ...s.collapsedProjectIds,
            [created.project_id]: false,
          },
        }),
        true,
      );
      await orbitActions.reloadFleetOnly();
      orbitActions.startNewThreadDraft(hostId, created.directories[0] ?? cleanPath);
    } catch (err) {
      notifyToast(`Failed to add project: ${String(err)}`);
    }
  },

  async promptAddProject(preferredHostId?: string): Promise<void> {
    const targetHost =
      (preferredHostId
        ? state.fleet.hosts.find((h) => h.host_id === preferredHostId)
        : undefined) ??
      state.fleet.hosts.find((h) => h.host_id === state.selectedHostId) ??
      state.fleet.hosts[0];

    if (!targetHost) {
      orbitActions.openSettings("hosts");
      return;
    }

    if (isLocalHostCard(targetHost)) {
      const initialDir =
        state.selectedHostId === targetHost.host_id ? state.selectedWorkspaceCwd : null;
      try {
        const picked = await orbitIpc.pickProjectFolder(initialDir);
        if (picked) {
          await orbitActions.addProjectDirectory(targetHost.host_id, picked);
        }
      } catch (err) {
        notifyToast(`Could not open folder picker: ${String(err)}`);
      }
      return;
    }

    // Remote host (SSH / LAN / Relay): open click-to-navigate Remote Folder Browser
    const existingListing = state.hostDetails[targetHost.host_id]?.directory_listing;
    setState((s) => ({ ...s, remoteFolderPickerHostId: targetHost.host_id }));
    void orbitIpc.listDirectory(
      targetHost.host_id,
      existingListing?.current_path ?? null,
    );
  },

  closeRemoteFolderPicker(): void {
    setState((s) => ({ ...s, remoteFolderPickerHostId: null }));
  },

  async removePinnedProject(projectId: string): Promise<void> {
    try {
      await orbitIpc.removeProject(projectId);
      await orbitActions.reloadFleetOnly();
    } catch (err) {
      notifyToast(`Failed to remove project: ${String(err)}`);
    }
  },

  async deleteProject(project: SidebarProjectGroupView): Promise<void> {
    const hostId = project.host_id;
    try {
      const running = project.conversations.filter(
        ({ agent }) =>
          isAgentForegroundRunning(agent) ||
          isAgentBackgroundWaiting(agent) ||
          agent.is_awaiting_permission,
      );
      if (running.length > 0) {
        await Promise.allSettled(
          running.map(({ agent }) => orbitIpc.interruptAgent(hostId, agent.agent_id)),
        );
      }

      if (project.conversations.length > 0) {
        await Promise.allSettled(
          project.conversations.map(({ agent }) => orbitIpc.deleteAgent(hostId, agent.agent_id)),
        );
      }

      const removedPinnedIds = new Set<string>();
      if (project.pinned_project_id) {
        removedPinnedIds.add(project.pinned_project_id);
      }
      for (const pinned of state.fleet.projects ?? []) {
        if (
          pinned.host_id === hostId &&
          pinned.directories.some((dir) => matchesProjectDirectory(project.directories, dir))
        ) {
          removedPinnedIds.add(pinned.project_id);
        }
      }
      if (removedPinnedIds.size > 0) {
        await Promise.all(
          Array.from(removedPinnedIds).map((pid) => orbitIpc.removeProject(pid)),
        );
      }

      const deletedAgentIds = new Set(project.conversations.map((c) => c.agent.agent_id));
      const deletedThreadKeys = new Set(
        project.conversations.map((c) => threadScopeKey(hostId, c.agent.agent_id)),
      );

      setState((s) => {
        const nextHidden = { ...s.hiddenProjectKeys };
        nextHidden[projectPathScopeKey(hostId, project.primary_cwd)] = true;
        for (const dir of project.directories) {
          nextHidden[projectPathScopeKey(hostId, dir)] = true;
        }

        const nextCollapsed = { ...s.collapsedProjectIds };
        delete nextCollapsed[project.id];

        const wasSelectedAgent =
          !s.isDraftNewSession &&
          s.selectedHostId === hostId &&
          s.selectedAgentId !== null &&
          deletedAgentIds.has(s.selectedAgentId);
        const wasSelectedProjectCwd =
          s.selectedHostId === hostId &&
          matchesProjectDirectory(project.directories, s.selectedWorkspaceCwd);

        let nextHostId = s.selectedHostId;
        let nextAgentId = s.selectedAgentId;
        let nextCwd = s.selectedWorkspaceCwd;
        let nextDraft = s.isDraftNewSession;

        if (wasSelectedAgent || wasSelectedProjectCwd) {
          const nextProject =
            (s.sidebarView?.projects ?? []).find(
              (p) =>
                p.id !== project.id &&
                !nextHidden[projectPathScopeKey(p.host_id, p.primary_cwd)] &&
                !(
                  p.host_id === hostId &&
                  matchesProjectDirectory(project.directories, p.primary_cwd)
                ),
            ) ?? null;

          if (nextProject) {
            nextHostId = nextProject.host_id;
            nextCwd = nextProject.primary_cwd;
            nextAgentId = null;
            nextDraft = true;
          } else {
            nextAgentId = null;
            nextCwd = "";
            nextDraft = true;
          }
        }

        return {
          ...s,
          selectedHostId: nextHostId,
          selectedAgentId: nextAgentId,
          selectedWorkspaceCwd: nextCwd,
          isDraftNewSession: nextDraft,
          hiddenProjectKeys: nextHidden,
          collapsedProjectIds: nextCollapsed,
          archivedThreads: omitRecordKeys(s.archivedThreads, deletedThreadKeys),
          threadCustomTitles: omitRecordKeys(s.threadCustomTitles, deletedThreadKeys),
          stages: omitRecordKeys(s.stages, deletedThreadKeys),
          composerDrafts: omitRecordKeys(s.composerDrafts, deletedThreadKeys),
          slashDirectives: omitRecordKeys(s.slashDirectives, deletedThreadKeys),
          inlineMentions: omitRecordKeys(s.inlineMentions, deletedThreadKeys),
          contextItems: omitRecordKeys(s.contextItems, deletedThreadKeys),
          pastedAttachments: omitRecordKeys(s.pastedAttachments, deletedThreadKeys),
          diffQuotes: omitRecordKeys(s.diffQuotes, deletedThreadKeys),
          queuedPrompts: omitRecordKeys(s.queuedPrompts, deletedThreadKeys),
        };
      }, true);

      await orbitActions.reloadFleetOnly();
      await orbitActions.reloadHostDetail(hostId, false);
      orbitActions.refreshSidebarView();
      notifyToast("Project deleted");
    } catch (err) {
      notifyToast(`Failed to delete project: ${String(err)}`);
      throw err;
    }
  },

  // =========================================================================
  // Unified Right Side Panel (Multi-Tab Workbench) & Stage Overview Actions
  // =========================================================================

  toggleSidePanel(openOverride?: boolean): void {
    const nextOpen = openOverride ?? !state.sidePanelOpen;

    setState((s) => {
      let nextSidebarOpen = s.sidebarOpen;
      let nextYielded = s.sidebarAutoYieldedForSidePanel;

      if (nextOpen) {
        if (s.isTightForDualSidebars && s.sidebarOpen) {
          nextSidebarOpen = false;
          nextYielded = true;
        }
      } else if (s.sidebarAutoYieldedForSidePanel && !s.isCompactViewport) {
        nextSidebarOpen = s.sidebarDesktopOpenPref;
        nextYielded = false;
      }

      const willShowLaunchpad =
        nextOpen && (s.sidePanelTabs.length === 0 || !s.activeSidePanelTabId);

      return {
        ...s,
        sidePanelOpen: nextOpen,
        overviewPopoverOpen: nextOpen ? false : s.overviewPopoverOpen,
        sidebarOpen: nextSidebarOpen,
        sidebarAutoYieldedForSidePanel: nextYielded,
        sidePanelLaunchpadFocusNonce: willShowLaunchpad
          ? s.sidePanelLaunchpadFocusNonce + 1
          : s.sidePanelLaunchpadFocusNonce,
      };
    });

    if (nextOpen && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.ensureWorkspaceFiles(state.selectedHostId, state.selectedWorkspaceCwd);
    }
  },

  openSidePanelNewTab(): void {
    setState((s) => {
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        activeSidePanelTabId: null,
        sidePanelLaunchpadFocusNonce: s.sidePanelLaunchpadFocusNonce + 1,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    if (state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.ensureWorkspaceFiles(state.selectedHostId, state.selectedWorkspaceCwd);
    }
  },

  setSidePanelWidth(w: number, persist = true): void {
    setState((s) => ({ ...s, sidePanelWidth: Math.min(780, Math.max(360, w)) }), persist);
  },

  markOpenFilesStaleForWorkspace(hostId: string, cwd: string): void {
    const prefix = `${hostId}::${cwd}::`;
    const matchingKeys = Object.keys(state.fileViewCache).filter((k) => k.startsWith(prefix));
    if (matchingKeys.length === 0) return;

    const activeTab = state.sidePanelTabs.find((t) => t.id === state.activeSidePanelTabId);
    const activeFileKey =
      state.sidePanelOpen &&
      activeTab?.kind === "file" &&
      activeTab.hostId &&
      activeTab.cwd &&
      activeTab.filePath
        ? `${activeTab.hostId}::${activeTab.cwd}::${activeTab.filePath}`
        : null;

    setState((s) => {
      const nextCache = { ...s.fileViewCache };
      for (const k of matchingKeys) {
        if (k !== activeFileKey) {
          delete nextCache[k];
        }
      }
      return { ...s, fileViewCache: nextCache };
    });

    if (
      activeFileKey &&
      activeTab?.kind === "file" &&
      activeTab.hostId &&
      activeTab.cwd &&
      activeTab.filePath &&
      activeFileKey.startsWith(prefix) &&
      state.fileViewCache[activeFileKey]?.status !== "error"
    ) {
      void orbitActions.loadWorkspaceFile(activeTab.hostId, activeTab.cwd, activeTab.filePath, true);
    }
  },

  focusSidePanelTab(tabId: string): void {
    const target = state.sidePanelTabs.find((t) => t.id === tabId);
    if (!target) return;
    setState((s) => {
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        activeSidePanelTabId: tabId,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    if (target.kind === "review" && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        state.selectedDiffPath,
        state.diffStaged,
      );
    } else if (target.kind === "files" && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.ensureWorkspaceFiles(state.selectedHostId, state.selectedWorkspaceCwd);
    } else if (target.kind === "terminal" && state.selectedHostId) {
      const hostTabs = state.terminalTabs.filter((t) => t.hostId === state.selectedHostId);
      if (hostTabs.length === 0) {
        orbitActions.spawnNewTerminalTab(state.selectedHostId, state.selectedWorkspaceCwd || "~");
      } else if (!hostTabs.some((t) => t.key === state.activeTerminalTabKey)) {
        setState((s) => ({ ...s, activeTerminalTabKey: hostTabs[0].key }));
      }
    }
  },

  closeSidePanelTab(tabId: string): void {
    let closedPanel = false;
    setState((s) => {
      const idx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
      if (idx < 0) return s;
      const remaining = s.sidePanelTabs.filter((t) => t.id !== tabId);
      const nextActive =
        s.activeSidePanelTabId === tabId
          ? (remaining[Math.min(idx, remaining.length - 1)]?.id ?? null)
          : s.activeSidePanelTabId;
      const shouldClosePanel = remaining.length === 0;
      closedPanel = shouldClosePanel;
      const shouldRestoreSidebar =
        shouldClosePanel && s.sidebarAutoYieldedForSidePanel && !s.isCompactViewport;
      const showingLaunchpad = !shouldClosePanel && nextActive === null;
      return {
        ...s,
        sidePanelOpen: shouldClosePanel ? false : s.sidePanelOpen,
        sidebarOpen: shouldRestoreSidebar ? s.sidebarDesktopOpenPref : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldRestoreSidebar
          ? false
          : s.sidebarAutoYieldedForSidePanel,
        sidePanelTabs: remaining,
        activeSidePanelTabId: nextActive,
        sidePanelLaunchpadFocusNonce: showingLaunchpad
          ? s.sidePanelLaunchpadFocusNonce + 1
          : s.sidePanelLaunchpadFocusNonce,
      };
    });

    if (!closedPanel && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.ensureWorkspaceFiles(state.selectedHostId, state.selectedWorkspaceCwd);
    }
  },

  openReviewTab(opts?: { path?: string | null; scope?: ReviewScope }): void {
    const tabId = "tab:review";
    setState((s) => {
      const exists = s.sidePanelTabs.some((t) => t.id === tabId);
      const nextTabs = exists
        ? s.sidePanelTabs
        : [...s.sidePanelTabs, { id: tabId, kind: "review" as const, title: "Review" }];
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        reviewScope: opts?.scope ?? s.reviewScope,
        selectedDiffPath: opts?.path !== undefined ? opts.path : s.selectedDiffPath,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    const effectiveScope = opts?.scope ?? state.reviewScope;
    const effectivePath = opts?.path !== undefined ? opts.path : state.selectedDiffPath;
    if (
      effectiveScope === "working_tree" &&
      state.selectedHostId &&
      state.selectedWorkspaceCwd
    ) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        effectivePath,
        state.diffStaged,
      );
    }
  },

  toggleReviewTab(openOverride?: boolean): void {
    const tabId = "tab:review";
    const isCurrentlyActive = state.sidePanelOpen && state.activeSidePanelTabId === tabId;
    if (openOverride === false || (openOverride === undefined && isCurrentlyActive)) {
      orbitActions.toggleSidePanel(false);
      return;
    }
    orbitActions.openReviewTab();
  },

  /** Backward-compatible alias for diff navigation hooks */
  toggleInspector(openOverride?: boolean): void {
    orbitActions.toggleReviewTab(openOverride);
  },

  /** Backward-compatible alias for jumping straight to a file diff in Review tab */
  openInspectorForFile(path: string, scope: ReviewScope = "session_turns"): void {
    orbitActions.openReviewTab({ path, scope });
  },

  openFilesExplorerTab(): void {
    const tabId = "tab:files";
    setState((s) => {
      const exists = s.sidePanelTabs.some((t) => t.id === tabId);
      const nextTabs = exists
        ? s.sidePanelTabs
        : [...s.sidePanelTabs, { id: tabId, kind: "files" as const, title: "Files" }];
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    if (state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.ensureWorkspaceFiles(state.selectedHostId, state.selectedWorkspaceCwd);
    }
  },

  openFileInSidePanel(
    rawPath: string,
    opts?: {
      focusLine?: number | null;
      focusEndLine?: number | null;
      hostId?: string;
      cwd?: string;
      fallbackPath?: string;
    },
  ): void {
    const hostId = opts?.hostId ?? state.selectedHostId;
    const cwd = opts?.cwd ?? state.selectedWorkspaceCwd ?? "~";
    const normalizedCandidate = normalizeWorkspacePathCandidate(rawPath, cwd);
    if (!hostId || !normalizedCandidate) return;

    const cachedFiles = state.workspaceFilesCache[`${hostId}::${cwd}`];
    const resolvedFromCache =
      !normalizedCandidate.startsWith("/") && !normalizedCandidate.startsWith("~/")
        ? resolveUniqueWorkspaceFileSuffix(normalizedCandidate, cachedFiles)
        : null;
    const cleanPath = resolvedFromCache ?? normalizedCandidate;

    const focusRange = normalizeLineRange(opts?.focusLine, opts?.focusEndLine);
    const hasExplicitFocus = opts?.focusLine !== undefined || opts?.focusEndLine !== undefined;
    const focusNonce = focusRange.startLine != null ? nextFileFocusNonce++ : undefined;

    const tabId = `tab:file:${hostId}::${cwd}::${cleanPath}`;
    const title = basenameFromPath(cleanPath) || cleanPath;

    setState((s) => {
      const existingIdx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
      let nextTabs = s.sidePanelTabs;
      if (existingIdx >= 0) {
        if (hasExplicitFocus) {
          nextTabs = [...s.sidePanelTabs];
          nextTabs[existingIdx] = {
            ...nextTabs[existingIdx],
            focusLine: focusRange.startLine,
            focusEndLine: focusRange.endLine,
            ...(focusNonce !== undefined ? { focusNonce } : {}),
          };
        }
      } else {
        nextTabs = [
          ...s.sidePanelTabs,
          {
            id: tabId,
            kind: "file" as const,
            title,
            filePath: cleanPath,
            hostId,
            cwd,
            focusLine: focusRange.startLine,
            focusEndLine: focusRange.endLine,
            focusNonce,
          },
        ];
      }
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        quickOpenFilesOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    const replaceFailedFileTab = (resolvedAltPath: string) => {
      const nextTabId = `tab:file:${hostId}::${cwd}::${resolvedAltPath}`;
      const nextTitle = basenameFromPath(resolvedAltPath) || resolvedAltPath;
      setState((s) => {
        if (!s.sidePanelTabs.some((t) => t.id === tabId)) return s;
        const alreadyHasNext = s.sidePanelTabs.some((t) => t.id === nextTabId);
        const nextTabs = alreadyHasNext
          ? s.sidePanelTabs
              .filter((t) => t.id !== tabId)
              .map((t) =>
                t.id === nextTabId
                  ? {
                      ...t,
                      focusLine: focusRange.startLine ?? t.focusLine ?? null,
                      focusEndLine: focusRange.endLine ?? t.focusEndLine ?? null,
                      ...(focusNonce !== undefined ? { focusNonce } : {}),
                    }
                  : t,
              )
          : s.sidePanelTabs.map((t) =>
              t.id === tabId
                ? {
                    ...t,
                    id: nextTabId,
                    title: nextTitle,
                    filePath: resolvedAltPath,
                  }
                : t,
            );
        return {
          ...s,
          sidePanelTabs: nextTabs,
          activeSidePanelTabId:
            s.activeSidePanelTabId === tabId ? nextTabId : s.activeSidePanelTabId,
        };
      });
    };

    void (async () => {
      await orbitActions.loadWorkspaceFile(hostId, cwd, cleanPath, false);
      const primaryKey = `${hostId}::${cwd}::${cleanPath}`;
      if (state.fileViewCache[primaryKey]?.status !== "error") return;

      const trySuffixResolve = async (candidate: string): Promise<string | null> => {
        if (!cwd || cwd === "~" || candidate.startsWith("/") || candidate.startsWith("~/")) {
          return null;
        }
        await orbitActions.ensureWorkspaceFiles(hostId, cwd);
        const cacheFiles = state.workspaceFilesCache[`${hostId}::${cwd}`];
        const match = resolveUniqueWorkspaceFileSuffix(candidate, cacheFiles);
        return match && match !== cleanPath ? match : null;
      };

      const fallbackCandidate = opts?.fallbackPath
        ? normalizeWorkspacePathCandidate(opts.fallbackPath, cwd)
        : null;
      if (fallbackCandidate && fallbackCandidate !== cleanPath) {
        const fallbackFromCache =
          !fallbackCandidate.startsWith("/") && !fallbackCandidate.startsWith("~/")
            ? resolveUniqueWorkspaceFileSuffix(
                fallbackCandidate,
                state.workspaceFilesCache[`${hostId}::${cwd}`],
              )
            : null;
        const directAlt = fallbackFromCache ?? fallbackCandidate;
        await orbitActions.loadWorkspaceFile(hostId, cwd, directAlt, false);
        const altKey = `${hostId}::${cwd}::${directAlt}`;
        if (state.fileViewCache[altKey]?.status === "ready") {
          replaceFailedFileTab(directAlt);
          return;
        }
        const suffixFromFallback = await trySuffixResolve(fallbackCandidate);
        if (suffixFromFallback) {
          await orbitActions.loadWorkspaceFile(hostId, cwd, suffixFromFallback, false);
          if (state.fileViewCache[`${hostId}::${cwd}::${suffixFromFallback}`]?.status === "ready") {
            replaceFailedFileTab(suffixFromFallback);
          }
        }
        return;
      }

      const suffixMatch = await trySuffixResolve(cleanPath);
      if (suffixMatch) {
        await orbitActions.loadWorkspaceFile(hostId, cwd, suffixMatch, false);
        if (state.fileViewCache[`${hostId}::${cwd}::${suffixMatch}`]?.status === "ready") {
          replaceFailedFileTab(suffixMatch);
        }
      }
    })();
  },

  async loadWorkspaceFile(
    hostId: string,
    cwd: string,
    path: string,
    forceReload = false,
  ): Promise<void> {
    const cKey = fileCacheKey(hostId, cwd, path);
    const existing = state.fileViewCache[cKey];
    if (!forceReload && (existing?.status === "loading" || existing?.status === "ready")) {
      return;
    }

    setState((s) => ({
      ...s,
      fileViewCache: {
        ...s.fileViewCache,
        [cKey]: { status: "loading", data: existing?.data },
      },
    }));

    try {
      const data = await orbitIpc.readWorkspaceFile(hostId, cwd, path);
      setState((s) => {
        // Maintain a bounded 20-entry LRU file cache so large files never grow memory unboundedly
        const nextCache: Record<string, FileViewCacheEntry> = {};
        const entries = Object.entries(s.fileViewCache).filter(([k]) => k !== cKey);
        const startIdx = Math.max(0, entries.length - 19);
        for (let i = startIdx; i < entries.length; i += 1) {
          const [k, v] = entries[i];
          nextCache[k] = v;
        }
        nextCache[cKey] = { status: "ready", data };
        return {
          ...s,
          fileViewCache: nextCache,
        };
      });
    } catch (err) {
      setState((s) => ({
        ...s,
        fileViewCache: {
          ...s.fileViewCache,
          [cKey]: { status: "error", error: String(err) },
        },
      }));
    }
  },

  openTerminalTab(opts?: { spawnIfEmpty?: boolean }): void {
    const tabId = "tab:terminal";
    setState((s) => {
      const exists = s.sidePanelTabs.some((t) => t.id === tabId);
      const nextTabs = exists
        ? s.sidePanelTabs
        : [...s.sidePanelTabs, { id: tabId, kind: "terminal" as const, title: "Terminal" }];
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    if (opts?.spawnIfEmpty !== false && state.selectedHostId) {
      const hostTabs = state.terminalTabs.filter((t) => t.hostId === state.selectedHostId);
      if (hostTabs.length === 0) {
        orbitActions.spawnNewTerminalTab(state.selectedHostId, state.selectedWorkspaceCwd || "~");
      } else if (!hostTabs.some((t) => t.key === state.activeTerminalTabKey)) {
        setState((s) => ({ ...s, activeTerminalTabKey: hostTabs[0].key }));
      }
    }
  },

  toggleTerminalTab(openOverride?: boolean): void {
    const tabId = "tab:terminal";
    const isCurrentlyActive = state.sidePanelOpen && state.activeSidePanelTabId === tabId;
    if (openOverride === false || (openOverride === undefined && isCurrentlyActive)) {
      orbitActions.toggleSidePanel(false);
      return;
    }
    orbitActions.openTerminalTab();
  },

  openBrowserTab(initialUrl?: string): void {
    const tabId = "tab:browser";
    const rawUrl = initialUrl?.trim() ?? "";
    let cleanUrl = "";
    if (rawUrl) {
      try {
        const parsed = new URL(rawUrl);
        if (parsed.protocol === "http:" || parsed.protocol === "https:") {
          cleanUrl = parsed.toString();
        }
      } catch {
        cleanUrl = "";
      }
    }
    setState((s) => {
      const existingIdx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
      let nextTabs = s.sidePanelTabs;
      if (existingIdx >= 0) {
        if (cleanUrl) {
          nextTabs = [...s.sidePanelTabs];
          nextTabs[existingIdx] = {
            ...nextTabs[existingIdx],
            url: cleanUrl,
          };
        }
      } else {
        nextTabs = [
          ...s.sidePanelTabs,
          {
            id: tabId,
            kind: "browser" as const,
            title: "Browser",
            url: cleanUrl,
          },
        ];
      }
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });
  },

  updateBrowserTabUrl(tabId: string, url: string): void {
    let safeUrl = "";
    if (url.trim()) {
      try {
        const parsed = new URL(url.trim());
        if (parsed.protocol !== "http:" && parsed.protocol !== "https:") return;
        safeUrl = parsed.toString();
      } catch {
        return;
      }
    }
    setState((s) => {
      const idx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
      if (idx < 0) return s;
      const nextTabs = [...s.sidePanelTabs];
      nextTabs[idx] = { ...nextTabs[idx], url: safeUrl };
      return { ...s, sidePanelTabs: nextTabs };
    });
  },

  openSideQuestionTab(): void {
    const tabId = "tab:side_question";
    setState((s) => {
      const exists = s.sidePanelTabs.some((t) => t.id === tabId);
      const nextTabs = exists
        ? s.sidePanelTabs
        : [
            ...s.sidePanelTabs,
            { id: tabId, kind: "side_question" as const, title: "Side Question" },
          ];
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        sideQuestionFocusNonce: s.sideQuestionFocusNonce + 1,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });
  },

  toggleSideQuestionTab(openOverride?: boolean): void {
    const tabId = "tab:side_question";
    const isCurrentlyActive = state.sidePanelOpen && state.activeSidePanelTabId === tabId;
    if (openOverride === false || (openOverride === undefined && isCurrentlyActive)) {
      orbitActions.toggleSidePanel(false);
      return;
    }
    orbitActions.openSideQuestionTab();
  },

  setSideQuestionDraft(text: string): void {
    const scope = sideQuestionScopeKey(state);
    setState((s) => ({
      ...s,
      sideQuestionDrafts: { ...s.sideQuestionDrafts, [scope]: text },
    }));
  },

  clearSideQuestions(): void {
    const scope = sideQuestionScopeKey(state);
    for (const e of state.sideQuestionsByScope[scope] ?? []) {
      if (e.status === "streaming") cancelledSideQuestionIds.add(e.id);
    }
    setState((s) => {
      const next = { ...s.sideQuestionsByScope };
      delete next[scope];
      return { ...s, sideQuestionsByScope: next };
    });
  },

  cancelSideQuestion(scopeOverride?: string): void {
    const scope = scopeOverride ?? sideQuestionScopeKey(state);
    const list = state.sideQuestionsByScope[scope];
    if (!list) return;
    const streamingIds = list.filter((e) => e.status === "streaming").map((e) => e.id);
    if (streamingIds.length === 0) return;
    for (const id of streamingIds) {
      cancelledSideQuestionIds.add(id);
    }
    setState((s) => {
      const cur = s.sideQuestionsByScope[scope];
      if (!cur) return s;
      return {
        ...s,
        sideQuestionsByScope: {
          ...s.sideQuestionsByScope,
          [scope]: cur.map((e) =>
            streamingIds.includes(e.id)
              ? {
                  ...e,
                  status: e.answer.trim() ? ("done" as const) : ("error" as const),
                  error: e.answer.trim() ? null : "Cancelled",
                }
              : e,
          ),
        },
      };
    });
  },

  promoteSideQuestionToComposer(text: string): void {
    const trimmed = text.trim();
    if (!trimmed) return;
    const key = composerScopeKey(state);
    const existing = (state.composerDrafts[key] ?? "").trim();
    const nextDraft = existing ? `${existing}\n\n${trimmed}` : trimmed;
    setState((s) => ({
      ...s,
      composerDrafts: { ...s.composerDrafts, [key]: nextDraft },
    }));
    orbitActions.focusComposer();
    notifyToast("Copied to main Composer");
  },

  async submitSideQuestion(rawQuestion?: string): Promise<void> {
    const hostId = state.selectedHostId;
    if (!hostId) {
      notifyToast("Connect an Orbit host first");
      return;
    }

    const scope = sideQuestionScopeKey(state);
    const question = (rawQuestion ?? state.sideQuestionDrafts[scope] ?? "").trim();
    if (!question) return;

    const detail = state.hostDetails[hostId];
    const currentAgent =
      !state.isDraftNewSession && state.selectedAgentId
        ? detail?.agents.find((a) => a.agent_id === state.selectedAgentId) ??
          state.stages[`${hostId}::${state.selectedAgentId}`]?.agent ??
          null
        : null;

    const provider = currentAgent?.provider ?? state.draftProvider;
    const model = currentAgent?.model ?? state.draftModels[provider] ?? null;
    const cwd = currentAgent?.cwd || state.selectedWorkspaceCwd || "~";
    const providerLabel =
      currentAgent?.provider_label ??
      (provider === "claude_code"
        ? "Claude Code"
        : provider === "antigravity"
          ? "Antigravity"
          : "Codex");

    const gitReview = state.gitReviews[`${hostId}::${cwd}`] ?? null;
    const branchName =
      gitReview?.status?.branch ??
      detail?.workspaces.find((w) => w.cwd === cwd)?.git_branch ??
      null;
    const rustContext =
      !state.isDraftNewSession && state.selectedAgentId
        ? await orbitIpc
            .buildSideQuestionContext(hostId, state.selectedAgentId, 3000)
            .catch(() => "")
        : "";
    const priorSideTurns = (state.sideQuestionsByScope[scope] ?? [])
      .filter((e) => e.status === "done" && e.answer.trim().length > 0)
      .slice(-4);
    const priorHistoryBlock =
      priorSideTurns.length > 0
        ? `\n\nPrior side-thread Q&A:\n${priorSideTurns
            .map(
              (e, i) =>
                `Q${i + 1}: ${e.question.slice(0, 400)}\nA${i + 1}: ${e.answer.slice(0, 900)}`,
            )
            .join("\n\n")}`
        : "";
    const baseContext =
      rustContext || `Workspace: ${cwd || "~"}${branchName ? ` (branch: ${branchName})` : ""}`;
    const contextSummary = `${baseContext}${priorHistoryBlock}`;

    const entryId = `sq_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
    const newEntry: SideQuestionEntry = {
      id: entryId,
      question,
      answer: "",
      blocks: [],
      status: "streaming",
      error: null,
      createdAtMs: Date.now(),
      providerLabel,
    };

    orbitActions.openSideQuestionTab();
    setState((s) => ({
      ...s,
      sideQuestionDrafts: { ...s.sideQuestionDrafts, [scope]: "" },
      sideQuestionsByScope: {
        ...s.sideQuestionsByScope,
        [scope]: [...(s.sideQuestionsByScope[scope] ?? []), newEntry],
      },
    }));

    let accumText = "";
    let parseTimer: ReturnType<typeof setTimeout> | null = null;
    let parseRevision = 0;

    const patchEntry = (mutator: (entry: SideQuestionEntry) => SideQuestionEntry) => {
      setState((s) => {
        const list = s.sideQuestionsByScope[scope];
        if (!list) return s;
        const idx = list.findIndex((item) => item.id === entryId);
        if (idx < 0) return s;
        const nextList = [...list];
        nextList[idx] = mutator(nextList[idx]);
        return {
          ...s,
          sideQuestionsByScope: {
            ...s.sideQuestionsByScope,
            [scope]: nextList,
          },
        };
      });
    };

    const scheduleMarkdownParse = (text: string, streaming: boolean) => {
      const rev = ++parseRevision;
      if (parseTimer) {
        clearTimeout(parseTimer);
        parseTimer = null;
      }
      const run = () => {
        void orbitIpc
          .parseMarkdownPreview(text, streaming)
          .then((blocks) => {
            if (rev !== parseRevision && streaming) return;
            if (blocks.length > 0) {
              patchEntry((e) => ({ ...e, blocks }));
            }
          })
          .catch(() => {});
      };
      if (streaming) {
        parseTimer = setTimeout(run, 85);
      } else {
        run();
      }
    };

    try {
      await orbitIpc.askSideQuestion(
        {
          hostId,
          agentId: currentAgent?.agent_id ?? null,
          cwd,
          provider,
          model,
          question,
          contextSummary,
        },
        (ev) => {
          if (cancelledSideQuestionIds.has(entryId)) {
            if (parseTimer) {
              clearTimeout(parseTimer);
              parseTimer = null;
            }
            return;
          }
          if (ev.kind === "delta") {
            accumText += ev.text_delta;
            patchEntry((e) => ({ ...e, answer: accumText, status: "streaming" }));
            scheduleMarkdownParse(accumText, true);
          } else if (ev.kind === "done") {
            const finalText = ev.full_text || accumText;
            const hasError = Boolean(ev.error && ev.error.trim().length > 0);
            patchEntry((e) => ({
              ...e,
              answer: finalText,
              status: hasError ? "error" : "done",
              error: ev.error,
            }));
            if (finalText.trim()) {
              scheduleMarkdownParse(finalText, false);
            }
          }
        },
      );
    } catch (err) {
      if (parseTimer) {
        clearTimeout(parseTimer);
        parseTimer = null;
      }
      if (!cancelledSideQuestionIds.has(entryId)) {
        patchEntry((e) => ({
          ...e,
          status: "error",
          error: String(err),
        }));
      }
    } finally {
      cancelledSideQuestionIds.delete(entryId);
    }
  },

  openSubagentTab(params: {
    callId: string;
    role: string;
    promptPreview: string;
    childAgentId: string | null;
    providerSubagentId?: string | null;
    parentAgentId?: string | null;
  }): void {
    const hostId = state.selectedHostId;
    const parentAgentId = params.parentAgentId ?? state.selectedAgentId;
    const hasManagedChild = Boolean(
      hostId &&
        params.childAgentId &&
        state.hostDetails[hostId]?.agents.some((a) => a.agent_id === params.childAgentId),
    );
    const effectiveChildAgentId = hasManagedChild ? params.childAgentId : null;
    const effectiveProviderSubagentId =
      params.providerSubagentId ?? (!hasManagedChild ? params.childAgentId : null);
    const tabId = `tab:subagent:${effectiveChildAgentId || effectiveProviderSubagentId || params.callId}`;
    setState((s) => {
      const existingIdx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
      let nextTabs = s.sidePanelTabs;
      const tabObj: SidePanelTab = {
        id: tabId,
        kind: "subagent",
        title: params.role || "Subagent",
        hostId: hostId ?? undefined,
        subagentCallId: params.callId,
        subagentRole: params.role,
        subagentPrompt: params.promptPreview,
        childAgentId: effectiveChildAgentId,
        providerSubagentId: effectiveProviderSubagentId ?? null,
        parentAgentId: parentAgentId ?? null,
      };
      if (existingIdx >= 0) {
        nextTabs = [...s.sidePanelTabs];
        nextTabs[existingIdx] = tabObj;
      } else {
        nextTabs = [...s.sidePanelTabs, tabObj];
      }
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    if (hostId && effectiveChildAgentId) {
      void orbitIpc.openAgentTimeline(hostId, effectiveChildAgentId);
      void orbitActions.pullStageDelta(hostId, effectiveChildAgentId, false);
    } else if (hostId && parentAgentId && effectiveProviderSubagentId) {
      void orbitIpc.openProviderSubagentTimeline(hostId, parentAgentId, effectiveProviderSubagentId);
    }
  },

  setStageCanDockOverview(canDock: boolean): void {
    if (state.stageCanDockOverview === canDock) return;
    setState((s) => ({
      ...s,
      stageCanDockOverview: canDock,
      overviewPopoverOpen: canDock ? false : s.overviewPopoverOpen,
    }));
  },

  toggleOverviewPanel(openOverride?: boolean): void {
    if (!state.stageCanDockOverview) {
      const nextPop = openOverride ?? !state.overviewPopoverOpen;
      setState((s) => ({ ...s, overviewPopoverOpen: nextPop }));
      return;
    }
    setState(
      (s) => ({
        ...s,
        overviewDesktopOpenPref: openOverride ?? !s.overviewDesktopOpenPref,
      }),
      true,
    );
  },

  setOverviewPopoverOpen(open: boolean): void {
    setState((s) => ({ ...s, overviewPopoverOpen: open }));
  },

  toggleOverviewSection(section: OverviewSectionId): void {
    setState(
      (s) => ({
        ...s,
        overviewCollapsedSections: {
          ...s.overviewCollapsedSections,
          [section]: !s.overviewCollapsedSections[section],
        },
      }),
      true,
    );
  },

  setQuickOpenFilesOpen(open: boolean): void {
    setState((s) => ({ ...s, quickOpenFilesOpen: open }));
    if (open && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.ensureWorkspaceFiles(state.selectedHostId, state.selectedWorkspaceCwd);
    }
  },

  setReviewScope(scope: ReviewScope): void {
    setState((s) => ({ ...s, reviewScope: scope, selectedDiffPath: null }));
    if (scope === "working_tree" && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        null,
        state.diffStaged,
      );
    }
  },

  setDiffStaged(staged: boolean): void {
    setState((s) => ({ ...s, diffStaged: staged }));
    if (state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        state.selectedDiffPath,
        staged,
      );
    }
  },

  setDiffLayout(mode: DiffLayoutMode): void {
    setState((s) => ({ ...s, diffLayout: mode }), true);
  },

  selectDiffFile(path: string | null): void {
    setState((s) => ({ ...s, selectedDiffPath: path }));
    if (
      state.reviewScope === "working_tree" &&
      state.selectedHostId &&
      state.selectedWorkspaceCwd
    ) {
      const rKey = `${state.selectedHostId}::${state.selectedWorkspaceCwd}`;
      const cached = state.gitReviews[rKey];
      // Fast-path: if we already have the full diff cached and the requested file is present
      // (or user clicked "All"), filter client-side in 0ms without spawning git subprocesses.
      if (path === null && cached && cached.selected_path === null && cached.diff_files.length > 0) {
        return;
      }
      if (
        path !== null &&
        cached &&
        cached.selected_path === null &&
        cached.diff_files.some((f) => f.path === path)
      ) {
        return;
      }
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        path,
        state.diffStaged,
      );
    }
  },

  async refreshGit(
    hostId: string,
    cwd: string,
    path: string | null,
    staged: boolean,
  ): Promise<void> {
    await orbitIpc.fetchGitStatus(hostId, cwd);
    await orbitIpc.fetchGitDiff(hostId, cwd, path, staged);
    await orbitActions.loadGitReviewSnapshot(hostId, cwd, path);
  },

  async loadGitReviewSnapshot(hostId: string, cwd: string, path: string | null): Promise<void> {
    const gr = await orbitIpc.getGitReview(hostId, cwd, path);
    const rKey = `${hostId}::${cwd}`;
    setState((s) => ({
      ...s,
      gitReviews: {
        ...s.gitReviews,
        [rKey]: gr,
      },
    }));
  },

  setTerminalFocused(focused: boolean): void {
    if (state.terminalFocused === focused) return;
    setState((s) => ({ ...s, terminalFocused: focused }));
  },

  spawnNewTerminalTab(hostId: string, cwd: string): void {
    void orbitIpc.startTerminal(hostId, cwd, 110, 28);
  },

  attachExistingTerminal(hostId: string, terminalId: string, cwd: string, title: string): void {
    const existing = state.terminalTabs.find(
      (t) => t.hostId === hostId && t.terminalId === terminalId,
    );
    if (existing) {
      setState((s) => ({ ...s, activeTerminalTabKey: existing.key }));
      orbitActions.openTerminalTab({ spawnIfEmpty: false });
      return;
    }
    const usedSlots = new Set(
      state.terminalTabs.filter((t) => t.hostId === hostId).map((t) => t.slot),
    );
    let slot = 1;
    while (usedSlots.has(slot) && slot < 240) {
      slot += 1;
    }
    const tabKey = `${hostId}::slot-${slot}`;
    setState((s) => ({
      ...s,
      terminalTabs: [
        ...s.terminalTabs,
        { key: tabKey, hostId, terminalId, slot, cwd, title },
      ],
      activeTerminalTabKey: tabKey,
    }));
    orbitActions.openTerminalTab({ spawnIfEmpty: false });
    void orbitIpc.attachTerminal(hostId, terminalId, slot);
  },

  closeTerminalTab(tabKey: string, killRemoteShim: boolean): void {
    const target = state.terminalTabs.find((t) => t.key === tabKey);
    if (target) {
      if (killRemoteShim && target.terminalId) {
        void orbitIpc.killTerminal(target.hostId, target.terminalId);
      }
      terminalChannels.delete(`${target.hostId}::${target.slot}`);
    }
    setState((s) => {
      const remaining = s.terminalTabs.filter((t) => t.key !== tabKey);
      const nextActive =
        s.activeTerminalTabKey === tabKey
          ? (remaining.find((t) => t.hostId === s.selectedHostId)?.key ?? remaining[0]?.key ?? null)
          : s.activeTerminalTabKey;
      return {
        ...s,
        terminalTabs: remaining,
        activeTerminalTabKey: nextActive,
      };
    });
  },

  setActiveTerminalTab(tabKey: string): void {
    setState((s) => ({ ...s, activeTerminalTabKey: tabKey }));
  },

  subscribeTerminalSlot(
    hostId: string,
    slot: number,
    onBytes: (bytes: Uint8Array) => void,
  ): () => void {
    const chKey = `${hostId}::${slot}`;
    let ch = terminalChannels.get(chKey);
    if (!ch) {
      ch = { chunks: [], subs: new Set() };
      terminalChannels.set(chKey, ch);
    }
    // Replay buffered bytes when mounting
    for (const buffered of ch.chunks) {
      onBytes(buffered);
    }
    ch.subs.add(onBytes);
    return () => {
      ch?.subs.delete(onBytes);
    };
  },

  async openWorkspaceInEditor(preferredKind?: ExternalEditorKind): Promise<void> {
    const hostId = state.selectedHostId;
    if (!hostId) return;
    const detail = state.hostDetails[hostId];
    const cwd = state.selectedWorkspaceCwd || "~";

    const chosenKind =
      preferredKind ??
      state.preferredEditor ??
      state.externalEditors.find((e) => e.installed)?.kind ??
      "cursor";

    // Infer whether this host is local loopback or remote SSH
    const isLocal = detail?.host.direct_endpoints.some(
      (ep) => ep.includes("127.0.0.1") || ep.includes("localhost"),
    );
    let sshTarget: string | null = null;
    if (!isLocal && detail) {
      const matchedSsh = state.sshHosts.find(
        (s) =>
          s.alias.toLowerCase() === detail.host.display_name.toLowerCase() ||
          s.host_name.toLowerCase() === detail.host.display_name.toLowerCase(),
      );
      sshTarget = matchedSsh?.alias ?? null;
    }

    try {
      await orbitIpc.openInEditor(chosenKind, cwd, sshTarget);
    } catch (err) {
      notifyToast(String(err));
    }
  },

  openSettings(section?: SettingsSectionTarget): void {
    void orbitIpc.listSshHosts().then((sshList) => {
      setState((s) => ({ ...s, sshHosts: sshList }));
    });
    setState((s) => ({
      ...s,
      activeView: "settings",
      settingsSection: resolveSettingsSectionTarget(s, section),
      commandPaletteOpen: false,
    }));
  },

  closeSettings(): void {
    setState((s) => ({ ...s, activeView: "workspace" }));
  },

  toggleSettings(section?: SettingsSectionTarget): void {
    const resolved = section ? resolveSettingsSectionTarget(state, section) : null;
    if (state.activeView === "settings" && (!resolved || state.settingsSection === resolved)) {
      orbitActions.closeSettings();
    } else {
      orbitActions.openSettings(section);
    }
  },

  setSettingsSection(section: SettingsSection): void {
    setState((s) => ({ ...s, settingsSection: section }));
  },

  setCommandPaletteOpen(open: boolean): void {
    setState((s) => ({ ...s, commandPaletteOpen: open }));
  },

  setPairDialogOpen(open: boolean): void {
    if (open) {
      orbitActions.openSettings("add_host");
    } else {
      setState((s) => ({ ...s, pairDialogOpen: false }));
    }
  },

  async openRelayDialog(): Promise<void> {
    orbitActions.openSettings("hosts");
  },

  setRelayDialogOpen(open: boolean): void {
    setState((s) => ({ ...s, relayDialogOpen: open }));
  },

  setTheme(theme: ThemeMode): void {
    const resolved = resolveTheme(theme);
    applyDocumentTheme(resolved);
    setState((s) => ({ ...s, theme, resolvedTheme: resolved }), true);
  },

  cycleTheme(): void {
    const order: ThemeMode[] = ["system", "light", "dark"];
    const next = order[(order.indexOf(state.theme) + 1) % order.length];
    orbitActions.setTheme(next);
  },

  setEnterToSend(enabled: boolean): void {
    setState((s) => ({ ...s, enterToSend: enabled }), true);
  },

  setNotificationPrefs(patch: {
    notifyOnApproval?: boolean;
    notifyOnTurnComplete?: boolean;
  }): void {
    setState(
      (s) => ({
        ...s,
        notifyOnApproval: patch.notifyOnApproval ?? s.notifyOnApproval,
        notifyOnTurnComplete: patch.notifyOnTurnComplete ?? s.notifyOnTurnComplete,
      }),
      true,
    );
  },

  setWorktreePrefs(patch: {
    worktreeBranchPrefix?: string;
    coalesceExploration?: boolean;
  }): void {
    setState(
      (s) => ({
        ...s,
        worktreeBranchPrefix: patch.worktreeBranchPrefix ?? s.worktreeBranchPrefix,
        coalesceExploration: patch.coalesceExploration ?? s.coalesceExploration,
      }),
      true,
    );
  },

  setPreferredEditor(editor: ExternalEditorKind | null): void {
    setState((s) => ({ ...s, preferredEditor: editor }), true);
  },

  setTerminalPrefs(patch: {
    fontSize?: number;
    cursorStyle?: TerminalCursorStyle;
    scrollback?: number;
  }): void {
    setState(
      (s) => ({
        ...s,
        terminalFontSize:
          patch.fontSize !== undefined
            ? Math.min(18, Math.max(10, patch.fontSize))
            : s.terminalFontSize,
        terminalCursorStyle: patch.cursorStyle ?? s.terminalCursorStyle,
        terminalScrollback:
          patch.scrollback !== undefined
            ? Math.min(50000, Math.max(500, patch.scrollback))
            : s.terminalScrollback,
      }),
      true,
    );
  },

  showToast(msg: string): void {
    notifyToast(msg);
  },

  getSnapshot(): OrbitUiState {
    return state;
  },
};

let nextFileFocusNonce = 1;

registerOrbitLinkOpener({
  openFile(rawPath, opts) {
    orbitActions.openFileInSidePanel(rawPath, opts);
  },
  openBrowser(url) {
    orbitActions.openBrowserTab(url);
  },
  openExternal(url) {
    void orbitIpc.openExternalBrowserUrl(url);
  },
});

function subscribeStore(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange);
  return () => {
    listeners.delete(onStoreChange);
  };
}

function subscribeComposerDraft(onDraftChange: () => void): () => void {
  composerDraftListeners.add(onDraftChange);
  return () => {
    composerDraftListeners.delete(onDraftChange);
  };
}

export function useOrbitSelector<T>(selector: (s: OrbitUiState) => T): T {
  return useSyncExternalStore(subscribeStore, () => selector(state));
}

/**
 * Isolated Composer draft text subscription:
 * Fires strictly when `composerDrafts` mutates, keeping keystrokes 100% isolated from the rest of the app.
 */
export function useComposerDraft(scopeKey: string): string {
  return useSyncExternalStore(
    subscribeComposerDraft,
    () => state.composerDrafts[scopeKey] ?? "",
  );
}
