import { useSyncExternalStore } from "react";
import { PROVIDERS } from "@/components/composer/composer-constants";
import {
  createDraftPersistenceController,
  hasNonEmptyScopeDraft,
} from "@/lib/draft-persistence";
import {
  normalizeLineRange,
  normalizeWorkspacePathCandidate,
  registerOrbitLinkOpener,
  resolveUniqueWorkspaceFileSuffix,
} from "@/lib/file-links";
import {
  defaultMimeForMedia,
  detectRichMediaKind,
  estimatePdfPageCount,
  type RichMediaKind,
} from "@/lib/media-utils";
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
  isBareHomeDir,
  isLocalHostCard,
  matchesProjectDirectory,
  projectPathScopeKey,
} from "@/lib/projects";
import {
  MAX_CACHED_STAGE_VIEWS,
  collectPinnedStageKeys,
  computeGitReviewSignature,
  evictInactiveStageEntries,
  mergeGitReviewSnapshot,
  normalizeCwdForCompare,
  recordStageAccess,
  removeStageAccessKeys,
  upsertSubagentSidePanelTab,
} from "@/lib/stage-cache";
import {
  SIDE_PANEL_DEFAULT_WIDTH_PX,
  SIDEBAR_DEFAULT_WIDTH_PX,
  clampSidePanelPrefWidth,
  clampSidebarPrefWidth,
  computeEqualizedSidePanelWidth,
} from "@/lib/workspace-layout";
import {
  type AgentStageView,
  type ClientThreadPrefSeed,
  type CloudSyncResultView,
  type CloudSyncStatusView,
  type ComposerContextInput,
  type CoreNotification,
  type ExternalEditorInfo,
  type ExternalEditorKind,
  type ExternalMcpServerRecord,
  type ExtractedCodeBlock,
  type FleetOverviewView,
  type FleetSidebarView,
  type GitHubDeviceFlowStartView,
  type GitReposInspectionView,
  type GitReviewView,
  type HostDetailView,
  type McpProbeResult,
  type PromptBadge,
  PROVIDER_LIST,
  formatProviderLabel,
  type ProviderKind,
  type ReasoningEffort,
  type ReleaseUpdateInfo,
  type RichBlockView,
  type SessionMode,
  type SidebarProjectGroupView,
  type SlashCommandView,
  type SlashDirective,
  type SpawnWorkspacePeerRequest,
  type SshHostItem,
  type UpsertExternalMcpServerRequest,
  type WorkspaceFileConflict,
  type WorktreeHandoffDirection,
  type WorktreeHandoffMode,
  type WorkspaceFileView,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  isMacDesktop,
  compareSemver,
  normalizeSemver,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { containsInlineMentionToken, extractLineSnippet } from "@/lib/symbol-mentions";

export interface HostDaemonUpgradeState {
  status: "upgrading" | "done" | "failed" | "skipped";
  mode: "local" | "ssh" | "wire";
  fromVersion?: string | null;
  toVersion?: string | null;
  error?: string;
  startedAt: number;
  finishedAt?: number;
}

export type FleetUpdatePhase =
  | "idle"
  | "checking"
  | "upgrading_hosts"
  | "paused_with_failures"
  | "upgrading_desktop"
  | "completed";

export interface FleetUpdateState {
  phase: FleetUpdatePhase;
  targetVersion: string | null;
  desktopStatus: string | null;
  desktopProgress: number | null;
  desktopError: string | null;
  startedAt: number | null;
  summaryMessage: string | null;
  pendingDesktopUpdate: boolean;
}

export interface HostUpgradeOutcome {
  ok: boolean;
  skipped?: boolean;
  error?: string;
  version?: string | null;
}

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
    }
  | {
      token: string;
      kind: "code_symbol";
      path: string;
      startLine: number;
      endLine: number;
      symbolName: string;
      symbolKind: string;
      signature?: string | null;
      container?: string | null;
      language: string;
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

export interface MediaPreviewTarget {
  kind: RichMediaKind;
  filename: string;
  path?: string | null;
  hostId?: string | null;
  cwd?: string | null;
  localUrl?: string | null;
  mimeType?: string | null;
  sizeBytes?: number | null;
  pdfPageCount?: number | null;
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
      mediaKind?: RichMediaKind;
      mimeType?: string;
      pdfPageCount?: number | null;
      previewUrl?: string;
    }
  | { id: string; kind: "git_diff"; staged: boolean; label: string }
  | { id: string; kind: "terminal_output"; label: string; lineCount: number; content: string }
  | {
      id: string;
      kind: "code_symbol";
      path: string;
      startLine: number;
      endLine: number;
      symbolName: string;
      symbolKind: string;
      signature?: string | null;
      container?: string | null;
      language: string;
      snippet?: string | null;
    }
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
  | "subagent"
  | "board"
  | "peer"
  | "artifacts";

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
  workspaceId?: string;
  peerAgentId?: string;
  artifactId?: string;
}

export interface FileViewCacheEntry {
  status: "loading" | "ready" | "error";
  data?: WorkspaceFileView;
  error?: string;
}

export type WorkspaceLineEnding = "lf" | "crlf" | "mixed";

export interface FileEditorDraftState {
  hostId: string;
  cwd: string;
  path: string;
  content: string;
  baseEtag: string | null;
  baseContent: string;
  lineEnding: WorkspaceLineEnding;
  dirty: boolean;
  mode: "view" | "edit";
  saving: boolean;
  saveError: string | null;
  conflict: WorkspaceFileConflict | null;
}

function normalizeLineEndingKind(raw?: string | null): WorkspaceLineEnding {
  if (raw === "crlf" || raw === "mixed") return raw;
  return "lf";
}

function normalizeEditorText(raw: string): string {
  return raw.replace(/\r\n/g, "\n");
}

export interface PendingUnsavedFileAction {
  fileKey: string;
  label: string;
  action:
    | { kind: "focus_tab"; tabId: string }
    | { kind: "close_tab"; tabId: string }
    | {
        kind: "open_file";
        rawPath: string;
        opts?: {
          focusLine?: number | null;
          focusEndLine?: number | null;
          hostId?: string;
          cwd?: string;
          fallbackPath?: string;
          initialMode?: "view" | "edit";
        };
      }
    | { kind: "new_tab" }
    | { kind: "switch_to_view"; hostId: string; cwd: string; path: string }
    | { kind: "reload"; hostId: string; cwd: string; path: string };
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
export type HostSettingsTab = "agents" | "skills" | "acp" | "mcp" | "usage" | "connection";
export type SettingsSection =
  | "general"
  | "instructions"
  | "cloud_sync"
  | "appearance"
  | "shortcuts"
  | "notifications"
  | "archived"
  | "add_host"
  | `host:${string}:${HostSettingsTab}`;
export type SettingsSectionTarget = SettingsSection | "hosts" | "providers" | "skills";
export type TerminalCursorStyle = "bar" | "block" | "underline";
export type SidebarProjectSort = "recent" | "alphabetical" | "manual";
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

export function sideQuestionScopeKey(s: Pick<OrbitUiState, "selectedHostId" | "selectedAgentId" | "isDraftNewSession">): string {
  if (s.isDraftNewSession || !s.selectedAgentId) {
    return `draft::${s.selectedHostId ?? "none"}`;
  }
  return `${s.selectedHostId ?? "none"}::${s.selectedAgentId}`;
}

export interface PendingJumpTarget {
  hostId: string;
  agentId: string;
  seq: number;
  nonce: number;
}

export interface WorktreeHandoffDialogTarget {
  hostId: string;
  workspaceId: string;
  direction: WorktreeHandoffDirection;
  defaultMode?: WorktreeHandoffMode;
  sourceCwd?: string;
  targetWorkspaceId?: string | null;
}

export interface InstructionsTarget {
  hostId: string;
  cwd: string;
}

export interface OrbitUiState {
  fleet: FleetOverviewView;
  fleetHydrated: boolean;
  bootstrapError: string | null;
  hostDetails: Record<string, HostDetailView>;
  selectedHostId: string | null;
  instructionsTarget: InstructionsTarget | null;
  selectedAgentId: string | null;
  selectedWorkspaceCwd: string;
  isDraftNewSession: boolean;
  sessionScrollNonce: number;
  pendingJumpTarget: PendingJumpTarget | null;

  // Draft new-session launch configuration
  draftProvider: ProviderKind;
  draftAcpAgentId: string | null;
  draftModels: Partial<Record<ProviderKind, string | null>>;
  draftEffort: ReasoningEffort;
  draftFastMode: boolean;
  draftMode: SessionMode;
  draftProfileId: string | null;
  draftUseWorktree: boolean;
  draftWorktreeBranch: string;
  draftWorktreeRepoPath: string | null;
  draftWorkspaceId: string | null;
  gitReposCache: Record<string, GitReposInspectionView>;

  // Composer drafts, slash workflow directives, context mentions, pasted attachments, diff quotes, and queued prompts
  composerDrafts: Record<string, string>;
  slashDirectives: Record<string, SlashDirective | null>;
  inlineMentions: Record<string, InlineMentionTarget[]>;
  contextItems: Record<string, ContextMentionItem[]>;
  pastedAttachments: Record<string, PastedAttachment[]>;
  diffQuotes: Record<string, DiffQuoteSnippet[]>;
  queuedPrompts: Record<string, QueuedPromptItem[]>;
  workspaceFilesCache: Record<string, string[]>;
  workspaceEmptyDirsCache: Record<string, string[]>;
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
  fileEditorDrafts: Record<string, FileEditorDraftState>;
  pendingUnsavedFileAction: PendingUnsavedFileAction | null;
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
  collapsedWorkspaceIds: Record<string, boolean>;
  collapsedInboxProjectIds: Record<string, boolean>;
  hiddenProjectKeys: Record<string, boolean>;
  threadCustomTitles: Record<string, string>;
  archivedThreads: Record<string, ArchivedThreadEntry>;
  archivedSubagents: Record<string, true>;
  threadManualOrder: Record<string, string[]>;
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
  exportDialogTarget: { hostId: string; agentId: string } | null;
  copyCodeBlocksTarget: {
    hostId: string;
    agentId: string;
    blocks: ExtractedCodeBlock[];
  } | null;
  handoffDialogTarget: WorktreeHandoffDialogTarget | null;
  mediaPreviewTarget: MediaPreviewTarget | null;
  sshHosts: SshHostItem[];
  externalEditors: ExternalEditorInfo[];
  externalMcpServersByHost: Record<string, ExternalMcpServerRecord[]>;
  toastNotice: string | null;
  lastArchivedUndo: UndoArchiveEntry | null;
  appVersion: string | null;
  isBrowserOffline: boolean;
  dismissedVersionMismatchHosts: Record<string, string>;
  hostUpgradeStates: Record<string, HostDaemonUpgradeState>;
  releaseUpdateInfo: ReleaseUpdateInfo | null;
  checkingReleaseUpdate: boolean;
  releaseCheckError: string | null;
  fleetUpdate: FleetUpdateState;

  // Zero-Server E2EE Cloud Sync (GitHub Device Flow + Secret Gist)
  cloudSyncStatus: CloudSyncStatusView | null;
  cloudSyncBusy: boolean;
  cloudSyncError: string | null;
  cloudSyncDeviceFlow: GitHubDeviceFlowStartView | null;
  cloudSyncLastResult: CloudSyncResultView | null;

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
  archivedSubagents?: Record<string, true>;
  collapsedProjectIds?: Record<string, boolean>;
  collapsedInboxProjectIds?: Record<string, boolean>;
  seenAgentSeqs?: Record<string, number>;
  threadManualOrder?: Record<string, string[]>;
  sidebarProjectSort?: SidebarProjectSort;
  sidebarThreadFilter?: SidebarThreadFilter;
  inspectorWidth?: number;
  sidePanelWidth?: number;
  overviewDesktopOpenPref?: boolean;
  overviewCollapsedSections?: Partial<Record<OverviewSectionId, boolean>>;
  diffLayout?: DiffLayoutMode;
  draftProvider?: ProviderKind;
  draftAcpAgentId?: string | null;
  draftModels?: Partial<Record<ProviderKind, string | null>>;
  draftEffort?: ReasoningEffort;
  draftFastMode?: boolean;
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
    if (disposed) return;
    if (savePrefsPending) {
      writePrefsNow(state);
    }
  }, 180);
}

const beforeUnloadListener = (e: BeforeUnloadEvent) => {
  if (savePrefsPending) {
    writePrefsNow(state);
  }
  if (Object.values(state.fileEditorDrafts).some((d) => d.dirty)) {
    e.preventDefault();
    e.returnValue = "";
  }
};

if (typeof window !== "undefined") {
  window.addEventListener("beforeunload", beforeUnloadListener);
}

const handleAppFocusGitRefresh = () => {
  if (!initialized || disposed) return;
  if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
  if (!state.selectedHostId || !state.selectedWorkspaceCwd) return;
  scheduleAutoRefreshGit(state.selectedHostId, state.selectedWorkspaceCwd, {
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
  window.addEventListener("focus", handleAppFocusGitRefresh);
}
if (typeof document !== "undefined") {
  document.addEventListener("visibilitychange", handleVisibilityGitRefresh);
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
if (typeof document !== "undefined") {
  if (isMacDesktop()) {
    document.documentElement.classList.add("orbit-mac");
  } else if (
    typeof navigator !== "undefined" &&
    /Linux/i.test(navigator.platform || navigator.userAgent)
  ) {
    document.documentElement.classList.add("orbit-linux");
  }
}

let themeMediaQuery: MediaQueryList | null = null;
let themeMediaListener: (() => void) | null = null;
if (typeof window !== "undefined" && window.matchMedia) {
  themeMediaQuery = window.matchMedia("(prefers-color-scheme: dark)");
  themeMediaListener = () => {
    if (state.theme === "system") {
      const nextResolved = detectSystemTheme();
      applyDocumentTheme(nextResolved);
      setState((s) => ({ ...s, resolvedTheme: nextResolved }));
    }
  };
  themeMediaQuery.addEventListener("change", themeMediaListener);
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

interface OrbitStoreHmrData {
  state?: OrbitUiState;
  terminalChannels?: Map<string, TerminalStreamChannel>;
  wasInitialized?: boolean;
  draftUpdatedAtByScope?: Record<string, number>;
}

const viteHot = (
  import.meta as {
    hot?: {
      data: OrbitStoreHmrData;
      dispose: (cb: (data: OrbitStoreHmrData) => void) => void;
    };
  }
).hot;
const hmrData = viteHot?.data;

const draftPersistence = createDraftPersistenceController(() => state);
const initialDrafts = draftPersistence.hydrate(initialPrefs.archivedThreads);
if (hmrData?.draftUpdatedAtByScope) {
  draftPersistence.restoreUpdatedAtByScope(hmrData.draftUpdatedAtByScope);
}

const externalMcpSnapshotSeqByHost: Record<string, number> = {};

const defaultInitialState: OrbitUiState = {
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
    ...Object.fromEntries([...PROVIDER_LIST.map((p) => p.id), "acp" as const].map((id) => [id, null])),
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
  contextItems: {},
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
  cloudSyncBusy: false,
  cloudSyncError: null,
  cloudSyncDeviceFlow: null,
  cloudSyncLastResult: null,

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

let state: OrbitUiState = hmrData?.state
  ? { ...defaultInitialState, ...hmrData.state }
  : defaultInitialState;

const listeners = new Set<() => void>();
const composerDraftListeners = new Set<() => void>();
const draftPresenceListeners = new Set<() => void>();
const composerFocusListeners = new Set<() => void>();

const autoUpgradeAttempted = new Map<string, string>();

function maybeAutoSyncOutdatedHosts(): void {
  if ((import.meta as { env?: { DEV?: boolean } }).env?.DEV) return;
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
      void orbitActions.upgradeHostDaemon(host.host_id, {
        silent: true,
        targetVersion: normApp,
      });
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
let stageAccessOrder: string[] = [];

function applyStageCacheUpdate(
  s: OrbitUiState,
  tlKey: string,
  nextStage: AgentStageView,
): Record<string, AgentStageView> {
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

function hasVisibleActiveTerminal(s: {
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
const hostSyncGates = new Map<string, SimpleSyncGate>();
const fleetSyncGate: SimpleSyncGate = { inFlight: false, queued: false, rafId: null };
const sidebarSyncGate: SimpleSyncGate = { inFlight: false, queued: false, rafId: null };
const lastReportedSeenSeq = new Map<string, number>();
let copyBlocksReqSeq = 0;
const pendingSeenTimers = new Map<
  string,
  { hostId: string; agentId: string; seq: number; timerId: number }
>();
const SEEN_REPORT_DEBOUNCE_MS = 1500;
const submittingComposerScopes = new Set<string>();
const cancelledSideQuestionIds = new Set<string>();
let gitChangedDebounceTimer: number | null = null;

interface AutoGitRefreshGate {
  timerId: number | null;
  timerDueAt: number;
  trailingTimerId: number | null;
  inFlight: boolean;
  queued: boolean;
  lastStartedAt: number;
  lastFocusTriggerAt: number;
  cwd: string;
}

const autoGitRefreshGates = new Map<string, AutoGitRefreshGate>();

function isAutoRefreshableCwd(cwd: string | null | undefined): cwd is string {
  if (!cwd) return false;
  const trimmed = cwd.trim();
  if (!trimmed || trimmed === "~" || isBareHomeDir(trimmed)) return false;
  return true;
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
    .refreshGit(hostId, targetCwd, null, state.diffStaged)
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

  // Periodic throttled refresh while streaming (1.8s debounce, max once per 5s)
  scheduleAutoRefreshGit(hostId, cwd, {
    delayMs: 1800,
    minIntervalMs: 5000,
    reason: "stream",
  });

  // Trailing settle refresh ~1.2s after streaming stops
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

  // Throttle continuous seen-cursor ticks while streaming so rapid timeline deltas
  // don't spam SQLite/WebSocket `MarkAgentRead` echo loops; flush promptly on force/switch/hide.
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
const terminalChannels = hmrData?.terminalChannels ?? new Map<string, TerminalStreamChannel>();

let disposed = false;

function emitChange(): void {
  if (disposed) return;
  for (const l of listeners) {
    l();
  }
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

function collectChangedRecordKeys<T>(
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

function syncDraftMutations(prev: OrbitUiState, next: OrbitUiState): void {
  if (
    prev.composerDrafts === next.composerDrafts &&
    prev.pastedAttachments === next.pastedAttachments &&
    prev.diffQuotes === next.diffQuotes &&
    prev.slashDirectives === next.slashDirectives &&
    prev.sideQuestionDrafts === next.sideQuestionDrafts
  ) {
    return;
  }

  const changedKeys = new Set<string>();
  collectChangedRecordKeys(prev.composerDrafts, next.composerDrafts, changedKeys);
  collectChangedRecordKeys(prev.pastedAttachments, next.pastedAttachments, changedKeys);
  collectChangedRecordKeys(prev.diffQuotes, next.diffQuotes, changedKeys);
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
  const prev = state;
  const next = updater(prev);
  if (next === prev) return;
  state = next;
  if (persist) {
    scheduleSavePrefs();
  }
  syncDraftMutations(prev, next);
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
let autoInitScheduled = false;
const BOOTSTRAP_TIMEOUT_MS = 6000;

function withBootstrapTimeout<T>(promise: Promise<T>, label: string): Promise<T> {
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

function subscribeCoreEvents(): Promise<void> {
  return orbitIpc
    .subscribe((ev: CoreNotification) => {
      if (disposed) return;
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
        // Stream live stage deltas for the actively viewed session and any open split-view peer tab.
        const isMainStage =
          !state.isDraftNewSession &&
          ev.host_id === state.selectedHostId &&
          ev.agent_id === state.selectedAgentId;
        const isOpenPeerTab =
          state.sidePanelOpen &&
          state.sidePanelTabs.some(
            (t) =>
              t.kind === "peer" &&
              t.hostId === ev.host_id &&
              t.peerAgentId === ev.agent_id,
          );
        if (isMainStage || isOpenPeerTab) {
          void orbitActions.pullStageDelta(ev.host_id, ev.agent_id);
        }
        if (isMainStage && state.selectedWorkspaceCwd) {
          scheduleStreamAutoRefreshGit(ev.host_id, state.selectedWorkspaceCwd);
        }
      } else if (ev.kind === "git_changed") {
        if (
          ev.host_id === state.selectedHostId &&
          normalizeCwdForCompare(ev.cwd) === normalizeCwdForCompare(state.selectedWorkspaceCwd)
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
              const prevSig = computeGitReviewSignature(state.gitReviews[rKey]);
              await orbitActions.loadGitReviewSnapshot(hostId, cwd, null);
              if (disposed) return;

              // Preserve single-file diff from `selectDiffFile(path)` when omitted from `__all__`
              const activeDiffPath = state.selectedDiffPath;
              if (activeDiffPath) {
                const loadedFiles = state.gitReviews[rKey]?.diff_files ?? [];
                const normTarget = activeDiffPath.replace(/^\.\//, "");
                const hasSelected = loadedFiles.some(
                  (f) => f.path === activeDiffPath || f.path.replace(/^\.\//, "") === normTarget,
                );
                if (!hasSelected) {
                  await orbitActions.loadGitReviewSnapshot(hostId, cwd, activeDiffPath);
                  if (disposed) return;
                }
              }

              // Only force-refresh workspace files when git status/diff actually changed;
              // prevents `git_changed -> ListWorkspaceFiles -> WorkspaceFilesResponse -> git_changed` loops.
              const nextSig = computeGitReviewSignature(state.gitReviews[rKey]);
              if (nextSig !== prevSig) {
                orbitActions.markOpenFilesStaleForWorkspace(hostId, cwd);
                void orbitActions.ensureWorkspaceFiles(hostId, cwd, true);
              } else if (!state.workspaceFilesCache[rKey]) {
                void orbitActions.ensureWorkspaceFiles(hostId, cwd, false);
              }
            })().catch(() => undefined);
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
}
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

function resolveAvailableDraftProvider(
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

function collectRemotelyArchivedDraftScopes(
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
  if (section === "skills") {
    return targetHostId ? `host:${targetHostId}:skills` : "general";
  }
  return section ?? s.settingsSection;
}

const MAX_ATTACHMENT_BYTES = 25 * 1024 * 1024;
const inflightAttachmentUploads = new Map<string, Promise<void>>();

function isLikelyTextFile(file: File, bytes: Uint8Array): boolean {
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

function removeContextMentionInScope(scopeKey: string, id: string): void {
  const removed = (state.contextItems[scopeKey] ?? []).find((c) => c.id === id);
  if (
    removed &&
    (removed.kind === "image" || removed.kind === "file") &&
    removed.previewUrl?.startsWith("blob:")
  ) {
    URL.revokeObjectURL(removed.previewUrl);
  }
  setState((s) => ({
    ...s,
    contextItems: {
      ...s.contextItems,
      [scopeKey]: (s.contextItems[scopeKey] ?? []).filter((c) => c.id !== id),
    },
    mediaPreviewTarget:
      removed &&
      (removed.kind === "image" || removed.kind === "file") &&
      removed.previewUrl &&
      s.mediaPreviewTarget?.localUrl === removed.previewUrl
        ? null
        : s.mediaPreviewTarget,
  }));
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
        removeContextMentionInScope(scopeKey, itemId);
        notifyToast(`Attachment upload failed: ${msg}`);
      }
    } finally {
      inflightAttachmentUploads.delete(itemId);
    }
  })();

  inflightAttachmentUploads.set(itemId, task);
  await task;
}

const MAX_FILE_CACHE_ENTRIES = 20;
const MAX_FILE_CACHE_BYTES = 64 * 1024 * 1024;
const fileLoadSeqByKey = new Map<string, number>();

function bumpFileLoadSeq(cKey: string): number {
  const next = (fileLoadSeqByKey.get(cKey) ?? 0) + 1;
  fileLoadSeqByKey.set(cKey, next);
  return next;
}

function estimateFileCacheEntryBytes(entry: FileViewCacheEntry): number {
  return (entry.data?.content.length ?? 0) + 4096;
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

const inFlightForks = new Set<string>();

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
      const [editors, sshList, version, fallbackCatalogs, slashCommands, cloudSyncStatus] =
        await Promise.all([
          orbitIpc.listExternalEditors().catch(() => []),
          orbitIpc.listSshHosts().catch(() => []),
          orbitIpc.getAppVersion().catch(() => null),
          orbitIpc.getFallbackProviderCatalogs().catch(() => []),
          orbitIpc.getSlashCommands("desktop").catch(() => []),
          orbitIpc.getCloudSyncStatus().catch(() => null),
        ]);
      seedFallbackProviderCatalogs(fallbackCatalogs);
      setState((s) => ({
        ...s,
        externalEditors: editors,
        sshHosts: sshList,
        appVersion: version,
        slashCommands,
        cloudSyncStatus,
      }));
      if (cloudSyncStatus?.state === "ready") {
        void orbitActions.hydrateSyncedPreferencesFromCore();
      }
      maybeAutoSyncOutdatedHosts();

      // Sanitize persisted draft models against Rust catalog & retired-slug table
      const providers: ProviderKind[] = PROVIDER_LIST.map((p) => p.id);
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
    const subscribePromise = subscribeCoreEvents();

    try {
      await withBootstrapTimeout(
        orbitActions.reloadFleetAndHosts(true),
        "Workspace hydration",
      );
    } catch (err) {
      setState((s) =>
        s.fleetHydrated
          ? s
          : {
              ...s,
              bootstrapError: err instanceof Error ? err.message : String(err),
            },
      );
    }
    orbitActions.refreshSidebarView();
    await subscribePromise;
  },

  async retryBootstrap(): Promise<void> {
    setState((s) => ({ ...s, bootstrapError: null }));
    const subscribePromise = subscribeCoreEvents();
    try {
      await withBootstrapTimeout(
        orbitActions.reloadFleetAndHosts(true),
        "Workspace hydration",
      );
    } catch (err) {
      setState((s) =>
        s.fleetHydrated
          ? s
          : {
              ...s,
              bootstrapError: err instanceof Error ? err.message : String(err),
            },
      );
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

  async checkFleetUpdates(opts?: { silent?: boolean }): Promise<ReleaseUpdateInfo | null> {
    if (state.checkingReleaseUpdate) {
      return state.releaseUpdateInfo;
    }
    setState((s) => ({
      ...s,
      checkingReleaseUpdate: true,
      releaseCheckError: null,
    }));
    try {
      const [info] = await Promise.all([
        orbitIpc.checkAppUpdate(),
        orbitActions.reloadFleetAndHosts(false).catch(() => {}),
      ]);
      setState((s) => ({
        ...s,
        releaseUpdateInfo: info,
        checkingReleaseUpdate: false,
        releaseCheckError: null,
      }));
      return info;
    } catch (err) {
      const detail =
        err instanceof Error ? err.message : typeof err === "string" ? err.trim() : "";
      const errMsg = detail
        ? `检查更新失败: ${detail}`
        : "暂时无法检查更新，请稍后重试。";
      setState((s) => ({
        ...s,
        checkingReleaseUpdate: false,
        releaseCheckError: errMsg,
      }));
      if (!opts?.silent) {
        notifyToast(errMsg);
      }
      return null;
    }
  },

  async installDesktopAppUpdate(
    explicitRelease?: ReleaseUpdateInfo | null,
  ): Promise<boolean> {
    const release = explicitRelease ?? state.releaseUpdateInfo;
    if (!release) return false;
    setState((s) => ({
      ...s,
      fleetUpdate: {
        ...s.fleetUpdate,
        phase: "upgrading_desktop",
        targetVersion: normalizeSemver(release.latest_version) ?? s.fleetUpdate.targetVersion,
        desktopStatus: `正在下载 Orbit v${release.latest_version} 安装包…`,
        desktopProgress: 0.02,
        desktopError: null,
        pendingDesktopUpdate: false,
        startedAt: s.fleetUpdate.startedAt ?? Date.now(),
      },
    }));
    try {
      await orbitIpc.performAppSelfUpdate(release, (ev) => {
        const rawProgress = Number.isFinite(ev.progress) ? ev.progress : 0;
        const normalized = rawProgress > 1 ? Math.min(1, rawProgress / 100) : Math.max(0, rawProgress);
        const pct = Math.round(normalized * 100);
        const label = ev.status
          ? pct > 0 && pct < 100
            ? `${ev.status} (${pct}%)`
            : ev.status
          : `正在更新桌面端 (${pct}%)…`;
        setState((s) => ({
          ...s,
          fleetUpdate: {
            ...s.fleetUpdate,
            phase: "upgrading_desktop",
            desktopStatus: label,
            desktopProgress: normalized,
            desktopError: null,
          },
        }));
      });
      return true;
    } catch (err) {
      const detail =
        err instanceof Error ? err.message : typeof err === "string" ? err.trim() : "";
      const errMsg = detail
        ? `桌面端更新失败: ${detail}`
        : "桌面端更新失败，请稍后重试或手动下载。";
      setState((s) => ({
        ...s,
        fleetUpdate: {
          ...s.fleetUpdate,
          phase: "completed",
          desktopStatus: null,
          desktopProgress: null,
          desktopError: errMsg,
          summaryMessage: errMsg,
        },
      }));
      notifyToast(errMsg);
      return false;
    }
  },

  async upgradeHostDaemon(
    hostId: string,
    opts?: {
      force?: boolean;
      silent?: boolean;
      targetVersion?: string | null;
      skipReload?: boolean;
    },
  ): Promise<HostUpgradeOutcome> {
    const host = state.fleet.hosts.find((h) => h.host_id === hostId);
    if (!host) return { ok: false, error: "Host not found" };
    if (state.hostUpgradeStates[hostId]?.status === "upgrading") {
      return { ok: true, skipped: true };
    }

    const isLocal = isLocalHostCard(host);
    const isOnline =
      host.connection_state === "online_direct" || host.connection_state === "online_relay";
    const supportsWire = (host.daemon_protocol_version ?? 0) >= 4;
    const normApp = normalizeSemver(state.appVersion);
    const fromVersion = normalizeSemver(host.daemon_version);
    const explicitTarget = normalizeSemver(opts?.targetVersion);
    const targetVersion =
      explicitTarget ?? resolveFleetTargetVersion(state.appVersion, state.releaseUpdateInfo);

    // Unreachable remote host without SSH fallback cannot be upgraded while offline.
    if (!isLocal && !isOnline && !host.ssh_alias) {
      const reason = "主机离线，已跳过";
      setState((s) => ({
        ...s,
        hostUpgradeStates: {
          ...s.hostUpgradeStates,
          [hostId]: {
            status: "skipped",
            mode: "wire",
            fromVersion,
            toVersion: fromVersion,
            error: reason,
            startedAt: Date.now(),
            finishedAt: Date.now(),
          },
        },
      }));
      return { ok: false, skipped: true, error: reason, version: fromVersion };
    }

    // Decide primary transport:
    // - Local host: use wire upgrade if online v4+ AND targeting a release newer than bundled Desktop;
    //   otherwise use 0ms bundled sidecar sync (`syncLocalDaemon`).
    // - Remote host: prefer wire on online v4+ (fast, works over Direct/Relay/ssh+relay with exact targetVersion),
    //   with automatic SSH fallback if `ssh_alias` is configured; use SSH directly when offline.
    const localNeedsRemoteDownload =
      isLocal &&
      isOnline &&
      supportsWire &&
      Boolean(targetVersion && normApp && compareSemver(targetVersion, normApp) > 0);
    const mode: "local" | "ssh" | "wire" = isLocal
      ? localNeedsRemoteDownload
        ? "wire"
        : "local"
      : isOnline && supportsWire
        ? "wire"
        : host.ssh_alias
          ? "ssh"
          : "wire";

    const startedAt = Date.now();
    setState((s) => ({
      ...s,
      hostUpgradeStates: {
        ...s.hostUpgradeStates,
        [hostId]: {
          status: "upgrading",
          mode,
          fromVersion,
          toVersion: targetVersion,
          startedAt,
        },
      },
    }));

    const waitForReconnectedVersion = async (): Promise<string | null> => {
      try {
        await orbitIpc.connectHost(host.host_id);
        await orbitIpc.refreshHost(host.host_id);
      } catch {
        // best-effort reconnect trigger
      }
      let latestVer = fromVersion;
      for (let attempt = 0; attempt < 8; attempt += 1) {
        try {
          const snap = await orbitIpc.getFleetOverview();
          const refreshed = snap.hosts.find((h) => h.host_id === host.host_id);
          const refreshedVer = normalizeSemver(refreshed?.daemon_version);
          if (refreshedVer) {
            latestVer = refreshedVer;
          }
          const onlineNow =
            refreshed?.connection_state === "online_direct" ||
            refreshed?.connection_state === "online_relay";
          if (
            onlineNow &&
            refreshedVer &&
            (!targetVersion || compareSemver(refreshedVer, targetVersion) >= 0)
          ) {
            break;
          }
        } catch {
          // ignore transient poll failure while daemon restarts
        }
        await new Promise((r) => window.setTimeout(r, 750));
      }
      return latestVer;
    };

    try {
      let resolvedVer: string | null = targetVersion ?? fromVersion;

      if (mode === "local") {
        const res = await orbitIpc.syncLocalDaemon(hostId, Boolean(opts?.force));
        if (res.status === "skipped_dev") {
          setState((s) => {
            const next = { ...s.hostUpgradeStates };
            delete next[hostId];
            return { ...s, hostUpgradeStates: next };
          });
          return { ok: true, skipped: true, version: fromVersion };
        }
        resolvedVer = normalizeSemver(res.daemon_version) ?? (await waitForReconnectedVersion());
        if (!opts?.skipReload) {
          await orbitActions.reloadFleetAndHosts(false);
        }
        setState((s) => ({
          ...s,
          hostUpgradeStates: {
            ...s.hostUpgradeStates,
            [hostId]: {
              status: "done",
              mode,
              fromVersion,
              toVersion: resolvedVer,
              startedAt,
              finishedAt: Date.now(),
            },
          },
        }));
        if (!opts?.silent) {
          notifyToast(
            res.status === "updated"
              ? `已更新本机服务${resolvedVer ? `至 v${resolvedVer}` : ""}`
              : "已重启本机 Orbit 服务",
          );
        }
        return { ok: true, version: resolvedVer };
      }

      if (mode === "wire") {
        try {
          await orbitIpc.upgradeHostDaemonOverWire(host.host_id, targetVersion ?? null);
        } catch (wireErr) {
          if (!isLocal && host.ssh_alias) {
            await orbitIpc.upgradeRemoteDaemon(
              host.host_id,
              host.ssh_alias,
              host.remote_daemon_port || undefined,
            );
          } else {
            throw wireErr;
          }
        }
      } else if (host.ssh_alias) {
        await orbitIpc.upgradeRemoteDaemon(
          host.host_id,
          host.ssh_alias,
          host.remote_daemon_port || undefined,
        );
      }

      resolvedVer = await waitForReconnectedVersion();
      if (!opts?.skipReload) {
        await orbitActions.reloadFleetAndHosts(false);
      }
      setState((s) => ({
        ...s,
        hostUpgradeStates: {
          ...s.hostUpgradeStates,
          [hostId]: {
            status: "done",
            mode,
            fromVersion,
            toVersion: resolvedVer,
            startedAt,
            finishedAt: Date.now(),
          },
        },
      }));
      if (!opts?.silent) {
        notifyToast(
          `已更新 ${host.display_name}${resolvedVer ? ` 至 v${resolvedVer}` : ""}`,
        );
      }
      return { ok: true, version: resolvedVer };
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setState((s) => ({
        ...s,
        hostUpgradeStates: {
          ...s.hostUpgradeStates,
          [hostId]: {
            status: "failed",
            mode,
            fromVersion,
            toVersion: targetVersion,
            error: msg,
            startedAt,
            finishedAt: Date.now(),
          },
        },
      }));
      if (!opts?.silent) {
        notifyToast(`${host.display_name} 更新失败: ${msg}`);
      }
      return { ok: false, error: msg, version: fromVersion };
    }
  },

  async upgradeAllConnectedHosts(opts?: {
    force?: boolean;
    includeDesktopApp?: boolean;
    onlyHostIds?: string[];
  }): Promise<void> {
    if (
      state.fleetUpdate.phase === "checking" ||
      state.fleetUpdate.phase === "upgrading_hosts" ||
      state.fleetUpdate.phase === "upgrading_desktop"
    ) {
      return;
    }

    const force = Boolean(opts?.force);
    const includeDesktopApp = opts?.includeDesktopApp ?? true;
    const onlyIdSet = opts?.onlyHostIds ? new Set(opts.onlyHostIds) : null;

    setState((s) => ({
      ...s,
      fleetUpdate: {
        phase: "checking",
        targetVersion: resolveFleetTargetVersion(s.appVersion, s.releaseUpdateInfo),
        desktopStatus: null,
        desktopProgress: null,
        desktopError: null,
        startedAt: Date.now(),
        summaryMessage: "正在检查最新版本并同步全部主机状态…",
        pendingDesktopUpdate: false,
      },
    }));

    // Always resolve latest release before kicking off a fleet update so remote hosts
    // upgrade to the true latest GitHub Release (not stale local appVersion).
    let releaseInfo = state.releaseUpdateInfo;
    if (!releaseInfo || force) {
      const fresh = await orbitActions.checkFleetUpdates({ silent: true });
      if (fresh) {
        releaseInfo = fresh;
      }
    } else {
      await orbitActions.reloadFleetAndHosts(false).catch(() => {});
    }

    const targetVersion = resolveFleetTargetVersion(state.appVersion, releaseInfo);
    const desktopNeedsUpdate = Boolean(
      includeDesktopApp && releaseInfo?.has_newer_version,
    );

    const allHosts = state.fleet.hosts;
    const candidateHosts = onlyIdSet
      ? allHosts.filter((h) => onlyIdSet.has(h.host_id))
      : allHosts;

    const hostsToUpgrade: typeof candidateHosts = [];
    let skippedOffline = 0;
    let alreadyUpToDate = 0;

    for (const host of candidateHosts) {
      const isLocal = isLocalHostCard(host);
      const isOnline =
        host.connection_state === "online_direct" ||
        host.connection_state === "online_relay";
      const normDaemon = normalizeSemver(host.daemon_version);

      // When Desktop itself will run `performAppSelfUpdate`, its updater script copies
      // the newly bundled `orbit` sidecar and hot-restarts the local daemon atomically,
      // so skip double-restarting the local daemon here unless explicitly retrying only local.
      if (isLocal && desktopNeedsUpdate && !onlyIdSet) {
        continue;
      }

      if (!isLocal && !isOnline && !host.ssh_alias) {
        skippedOffline += 1;
        setState((s) => ({
          ...s,
          hostUpgradeStates: {
            ...s.hostUpgradeStates,
            [host.host_id]: {
              status: "skipped",
              mode: "wire",
              fromVersion: normDaemon,
              toVersion: normDaemon,
              error: "主机离线，已跳过",
              startedAt: Date.now(),
              finishedAt: Date.now(),
            },
          },
        }));
        continue;
      }

      const needsUpgrade =
        force ||
        !normDaemon ||
        (targetVersion !== null && compareSemver(normDaemon, targetVersion) < 0);

      if (needsUpgrade) {
        hostsToUpgrade.push(host);
      } else {
        alreadyUpToDate += 1;
        setState((s) => ({
          ...s,
          hostUpgradeStates: {
            ...s.hostUpgradeStates,
            [host.host_id]: {
              status: "done",
              mode: isLocal ? "local" : host.ssh_alias ? "ssh" : "wire",
              fromVersion: normDaemon,
              toVersion: normDaemon,
              startedAt: Date.now(),
              finishedAt: Date.now(),
            },
          },
        }));
      }
    }

    // Nothing to update anywhere
    if (hostsToUpgrade.length === 0 && !desktopNeedsUpdate) {
      const msg =
        skippedOffline > 0
          ? `在线主机均已是最新${targetVersion ? ` v${targetVersion}` : ""}，${skippedOffline} 台离线主机已跳过。`
          : `本机与全部已联通机器均已是最新版本${targetVersion ? ` (v${targetVersion})` : ""}。`;
      setState((s) => ({
        ...s,
        fleetUpdate: {
          ...s.fleetUpdate,
          phase: "completed",
          targetVersion,
          summaryMessage: msg,
          pendingDesktopUpdate: false,
        },
      }));
      notifyToast(msg);
      return;
    }

    let succeeded = 0;
    let failed = 0;

    if (hostsToUpgrade.length > 0) {
      setState((s) => ({
        ...s,
        fleetUpdate: {
          ...s.fleetUpdate,
          phase: "upgrading_hosts",
          targetVersion,
          summaryMessage: `正在并行更新 ${hostsToUpgrade.length} 台主机${targetVersion ? `至 v${targetVersion}` : ""}（运行中的会话不受影响）…`,
          pendingDesktopUpdate: desktopNeedsUpdate,
        },
      }));

      const results = await Promise.allSettled(
        hostsToUpgrade.map((host) =>
          orbitActions.upgradeHostDaemon(host.host_id, {
            force: true,
            silent: true,
            targetVersion,
            skipReload: true,
          }),
        ),
      );

      for (const r of results) {
        if (r.status === "fulfilled" && r.value.ok) {
          succeeded += 1;
        } else if (r.status === "fulfilled" && r.value.skipped) {
          skippedOffline += 1;
        } else {
          failed += 1;
        }
      }

      await orbitActions.reloadFleetAndHosts(false).catch(() => {});
    }

    // Phase 2: Desktop self-update (exits & relaunches Desktop + syncs bundled local daemon)
    if (desktopNeedsUpdate && releaseInfo) {
      if (failed > 0) {
        const pausedMsg = `已完成 ${succeeded} 台主机更新，${failed} 台失败${skippedOffline > 0 ? `，${skippedOffline} 台离线跳过` : ""}。可先重试失败主机，或直接继续更新本机桌面端。`;
        setState((s) => ({
          ...s,
          fleetUpdate: {
            ...s.fleetUpdate,
            phase: "paused_with_failures",
            targetVersion,
            summaryMessage: pausedMsg,
            pendingDesktopUpdate: true,
          },
        }));
        notifyToast(`有 ${failed} 台主机更新失败，已暂停桌面端重启`);
        return;
      }

      await orbitActions.installDesktopAppUpdate(releaseInfo);
      return;
    }

    const parts: string[] = [];
    if (succeeded > 0) parts.push(`成功更新 ${succeeded} 台`);
    if (alreadyUpToDate > 0) parts.push(`${alreadyUpToDate} 台已是最新`);
    if (skippedOffline > 0) parts.push(`${skippedOffline} 台离线跳过`);
    if (failed > 0) parts.push(`${failed} 台失败`);
    const summary =
      parts.length > 0
        ? `全集群更新完成：${parts.join("，")}${targetVersion ? `（目标 v${targetVersion}）` : ""}。`
        : "全集群更新完成。";

    setState((s) => ({
      ...s,
      fleetUpdate: {
        ...s.fleetUpdate,
        phase: "completed",
        targetVersion,
        summaryMessage: summary,
        pendingDesktopUpdate: false,
      },
    }));
    notifyToast(summary);
  },

  async retryFailedFleetHosts(): Promise<void> {
    const failedIds = Object.entries(state.hostUpgradeStates)
      .filter(([, st]) => st.status === "failed")
      .map(([hostId]) => hostId);
    if (failedIds.length === 0) return;
    await orbitActions.upgradeAllConnectedHosts({
      force: true,
      includeDesktopApp: state.fleetUpdate.pendingDesktopUpdate,
      onlyHostIds: failedIds,
    });
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
      if (d) {
        nextDetails[hid] = d;
        // Fleet reload already fetched fresh host detail for `hid`; cancel redundant
        // same-frame per-host sync scheduled by a paired `HostChanged` notification.
        const hGate = hostSyncGates.get(hid);
        if (hGate && !hGate.inFlight) {
          if (hGate.rafId !== null && typeof window !== "undefined") {
            window.cancelAnimationFrame(hGate.rafId);
            hGate.rafId = null;
          }
          hGate.queued = false;
        }
      }
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

      const nextDraftProvider = resolveAvailableDraftProvider(activeDetail, s.draftProvider);

      const activeScopeKey =
        !nextDraft && activeHostId && nextAgentId
          ? threadScopeKey(activeHostId, nextAgentId)
          : null;
      const remotelyArchivedDraftKeys = collectRemotelyArchivedDraftScopes(
        s,
        Object.entries(nextDetails),
        activeScopeKey,
      );

      return {
        ...s,
        fleet: snap,
        fleetHydrated: true,
        bootstrapError: null,
        hostDetails: nextDetails,
        selectedHostId: activeHostId,
        selectedAgentId: nextAgentId,
        selectedWorkspaceCwd: nextCwd,
        isDraftNewSession: nextDraft,
        draftProvider: nextDraftProvider,
        ...(remotelyArchivedDraftKeys.size > 0
          ? {
              composerDrafts: omitRecordKeys(s.composerDrafts, remotelyArchivedDraftKeys),
              slashDirectives: omitRecordKeys(s.slashDirectives, remotelyArchivedDraftKeys),
              pastedAttachments: omitRecordKeys(s.pastedAttachments, remotelyArchivedDraftKeys),
              diffQuotes: omitRecordKeys(s.diffQuotes, remotelyArchivedDraftKeys),
              sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, remotelyArchivedDraftKeys),
            }
          : {}),
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

    const prevHostDetail = state.hostDetails[hostId];
    const prevAgents = prevHostDetail?.agents ?? [];

    let settledGitRefreshCwd: string | null = null;
    if (state.selectedHostId === hostId && !state.isDraftNewSession && state.selectedAgentId) {
      const selId = state.selectedAgentId;
      const prevActive = prevAgents.find((a) => a.agent_id === selId);
      const nextActive = detail.agents.find((a) => a.agent_id === selId);
      const activeTurnSettled =
        isAgentForegroundRunning(prevActive) && !isAgentForegroundRunning(nextActive);
      const childSubagentSettled = detail.agents.some((nextAg) => {
        if (nextAg.parent_agent_id !== selId) return false;
        const prevAg = prevAgents.find((p) => p.agent_id === nextAg.agent_id);
        return isAgentForegroundRunning(prevAg) && !isAgentForegroundRunning(nextAg);
      });
      const prevProviderSubs = prevHostDetail?.provider_subagents?.[selId] ?? [];
      const nextProviderSubs = detail.provider_subagents?.[selId] ?? [];
      const providerSubagentSettled = nextProviderSubs.some((nextPs) => {
        if (
          nextPs.status !== "completed" &&
          nextPs.status !== "failed" &&
          nextPs.status !== "canceled" &&
          nextPs.status !== "stopped"
        ) {
          return false;
        }
        const prevPs = prevProviderSubs.find((p) => p.subagent_id === nextPs.subagent_id);
        return !prevPs || prevPs.status === "running";
      });
      if (activeTurnSettled || childSubagentSettled || providerSubagentSettled) {
        settledGitRefreshCwd = nextActive?.cwd || state.selectedWorkspaceCwd || null;
      }
    }
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
    let viewedAgentWasUnread = false;

    for (const ag of detail.agents) {
      if (ag.is_unread) {
        lastReportedSeenSeq.delete(threadScopeKey(hostId, ag.agent_id));
      }
    }

    const prevSyncUpdatedAt = state.hostDetails[hostId]?.host_sync_state?.updated_at_ms ?? 0;
    const nextSync = detail.host_sync_state ?? null;
    const syncedSortMode =
      nextSync &&
      nextSync.updated_at_ms > prevSyncUpdatedAt &&
      (nextSync.project_sort_mode === "recent" ||
        nextSync.project_sort_mode === "alphabetical" ||
        nextSync.project_sort_mode === "manual")
        ? (nextSync.project_sort_mode as SidebarProjectSort)
        : null;

    setState((s) => {
      const nextDetails = { ...s.hostDetails, [hostId]: detail };
      let nextGitReposCache = s.gitReposCache;
      if (detail.git_repos_by_cwd && Object.keys(detail.git_repos_by_cwd).length > 0) {
        nextGitReposCache = { ...s.gitReposCache };
        for (const [cwdKey, insp] of Object.entries(detail.git_repos_by_cwd)) {
          nextGitReposCache[`${hostId}::${cwdKey}`] = insp;
        }
      }
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
          viewedAgentWasUnread = Boolean(viewed.is_unread || viewed.attention === "manual_unread");
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

      const nextDraftProvider =
        s.selectedHostId === hostId
          ? resolveAvailableDraftProvider(detail, s.draftProvider)
          : s.draftProvider;

      const activeScopeKey =
        !nextDraft && s.selectedHostId && nextAgentId
          ? threadScopeKey(s.selectedHostId, nextAgentId)
          : null;
      const remotelyArchivedDraftKeys = collectRemotelyArchivedDraftScopes(
        s,
        [[hostId, detail]],
        activeScopeKey,
      );

      return {
        ...s,
        ...(overview
          ? {
              fleet: overview,
              selectedHostId: s.selectedHostId ?? overview.hosts[0]?.host_id ?? null,
            }
          : {}),
        hostDetails: nextDetails,
        gitReposCache: nextGitReposCache,
        ...(syncedSortMode ? { sidebarProjectSort: syncedSortMode } : {}),
        selectedAgentId: nextAgentId,
        isDraftNewSession: nextDraft,
        selectedWorkspaceCwd: nextCwd,
        draftProvider: nextDraftProvider,
        terminalTabs: nextTabs,
        activeTerminalTabKey: nextActiveTab,
        ...(remotelyArchivedDraftKeys.size > 0
          ? {
              composerDrafts: omitRecordKeys(s.composerDrafts, remotelyArchivedDraftKeys),
              slashDirectives: omitRecordKeys(s.slashDirectives, remotelyArchivedDraftKeys),
              pastedAttachments: omitRecordKeys(s.pastedAttachments, remotelyArchivedDraftKeys),
              diffQuotes: omitRecordKeys(s.diffQuotes, remotelyArchivedDraftKeys),
              sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, remotelyArchivedDraftKeys),
            }
          : {}),
      };
    });

    if (viewedAgentIdToMark !== null && viewedAgentLatestSeq !== null) {
      markThreadSeenInCore(
        hostId,
        viewedAgentIdToMark,
        viewedAgentLatestSeq,
        viewedAgentWasUnread,
      );
    }

    syncAttentionIndicators(state.fleet, state.hostDetails);
    orbitActions.refreshSidebarView();
    maybeAutoSyncOutdatedHosts();
    void drainPromptQueueIfIdle(hostId, detail);

    if (settledGitRefreshCwd) {
      scheduleAutoRefreshGit(hostId, settledGitRefreshCwd, {
        delayMs: 800,
        reason: "settled",
      });
    }

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
            (prevAgent.queued_prompts ?? []).some(
              (q, i) => q.queue_id !== nextAgent.queued_prompts?.[i]?.queue_id,
            ) ||
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
          let needsFullRetry = false;
          setState((s) => {
            const prev = s.stages[tlKey];
            if (sinceRevision > 0 && !prev && !delta.reset) {
              needsFullRetry = true;
              return s;
            }
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
              stages: applyStageCacheUpdate(s, tlKey, mergedStage),
            };
          });
          if (needsFullRetry) {
            gate.queued = true;
            gate.forceFull = true;
          }
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

  flushPendingSeenMarks(): void {
    flushPendingSeenMarks();
  },

  selectHost(hostId: string, opts?: { keepDraft?: boolean }): void {
    flushPendingSeenMarks();
    pendingDraftCreate = null;
    const detail = state.hostDetails[hostId];
    const stayInDraft = Boolean(opts?.keepDraft);
    const firstAgent = stayInDraft
      ? null
      : (detail?.agents.find(
          (a) => !state.archivedThreads[threadScopeKey(hostId, a.agent_id)],
        ) ?? null);
    const sidebarHostProj = state.sidebarView?.projects.find(
      (p) => p.host_id === hostId && !p.is_home_workspace,
    );
    const firstValidWorkspace = detail?.workspaces.find(
      (w) => !w.archived_at && !w.background && !w.is_worktree && !isBareHomeDir(w.cwd),
    );
    const firstDiscovered = detail?.discovered_projects.find(
      (p) => !isBareHomeDir(p.path) && !p.path.includes("/worktrees/"),
    );
    const nextCwd =
      firstAgent?.cwd ??
      sidebarHostProj?.primary_cwd ??
      firstValidWorkspace?.project_root ??
      firstValidWorkspace?.cwd ??
      firstDiscovered?.path ??
      detail?.discovered_projects[0]?.path ??
      "~";
    setState(
      (s) => ({
        ...s,
        activeView: "workspace",
        workspacePanel: "conversation",
        selectedHostId: hostId,
        selectedAgentId: firstAgent?.agent_id ?? null,
        isDraftNewSession: stayInDraft || firstAgent === null,
        sessionScrollNonce: s.sessionScrollNonce + 1,
        pendingJumpTarget: null,
        selectedWorkspaceCwd: nextCwd,
        selectedDiffPath: null,
        draftWorktreeRepoPath: null,
        draftWorkspaceId: null,
        draftProvider: resolveAvailableDraftProvider(detail, s.draftProvider),
      }),
    );

    orbitActions.refreshSidebarView();
    void orbitIpc.refreshQuotas(hostId);
    if (firstAgent) {
      const firstKey = threadScopeKey(hostId, firstAgent.agent_id);
      if (state.stages[firstKey]) {
        stageAccessOrder = recordStageAccess(stageAccessOrder, firstKey);
      }
      markThreadSeenInCore(hostId, firstAgent.agent_id, firstAgent.latest_seq);
      void orbitIpc.openAgentTimeline(hostId, firstAgent.agent_id);
      void orbitIpc.listProviderSubagents(hostId, firstAgent.agent_id);
      void orbitActions.pullStageDelta(hostId, firstAgent.agent_id, false);
    }
    if (nextCwd && nextCwd !== "~") {
      void orbitActions.refreshGit(hostId, nextCwd, null, state.diffStaged);
      void orbitActions.inspectGitRepos(hostId, nextCwd);
    }
  },

  selectAgent(hostId: string, agentId: string, jumpToSeq?: number | null): void {
    flushPendingSeenMarks();
    pendingDraftCreate = null;
    const detail = state.hostDetails[hostId];
    const ag = detail?.agents.find((a) => a.agent_id === agentId);
    const nextCwd = ag?.cwd ?? state.selectedWorkspaceCwd;
    const tKey = threadScopeKey(hostId, agentId);
    if (state.stages[tKey]) {
      stageAccessOrder = recordStageAccess(stageAccessOrder, tKey);
    }
    const seenSeq = Math.max(ag?.latest_seq ?? 0, state.stages[tKey]?.latest_seq ?? 0);

    setState(
      (s) => {
        let nextArchived = s.archivedThreads;
        if (nextArchived[tKey]) {
          nextArchived = { ...nextArchived };
          delete nextArchived[tKey];
        }
        // Automatically dismiss stale subagent inspector tabs belonging to a different parent thread
        const remainingSideTabs = s.sidePanelTabs.filter(
          (t) => t.kind !== "subagent" || t.parentAgentId === agentId,
        );
        const tabsChanged = remainingSideTabs.length !== s.sidePanelTabs.length;
        const nextActiveSideTab = tabsChanged
          ? remainingSideTabs.some((t) => t.id === s.activeSidePanelTabId)
            ? s.activeSidePanelTabId
            : (remainingSideTabs[remainingSideTabs.length - 1]?.id ?? null)
          : s.activeSidePanelTabId;
        const shouldCloseSidePanel = tabsChanged && remainingSideTabs.length === 0;
        const nextNonce = s.sessionScrollNonce + 1;
        const nextJumpTarget =
          typeof jumpToSeq === "number" && jumpToSeq > 0
            ? { hostId, agentId, seq: jumpToSeq, nonce: nextNonce }
            : null;
        return {
          ...s,
          activeView: "workspace",
          workspacePanel: "conversation",
          selectedHostId: hostId,
          selectedAgentId: agentId,
          isDraftNewSession: false,
          sessionScrollNonce: nextNonce,
          pendingJumpTarget: nextJumpTarget,
          selectedWorkspaceCwd: nextCwd,
          selectedDiffPath: null,
          archivedThreads: nextArchived,
          ...(tabsChanged
            ? {
                sidePanelTabs: remainingSideTabs,
                activeSidePanelTabId: nextActiveSideTab,
                sidePanelOpen: shouldCloseSidePanel ? false : s.sidePanelOpen,
              }
            : {}),
        };
      },
      true,
    );

    markThreadSeenInCore(hostId, agentId, seenSeq, true);
    orbitActions.refreshSidebarView();
    void orbitIpc.openAgentTimeline(hostId, agentId);
    void orbitIpc.markAgentRead(hostId, agentId);
    void orbitIpc.listProviderSubagents(hostId, agentId);
    void orbitActions.pullStageDelta(hostId, agentId, false);
    if (ag?.workspace_id) {
      const ws = detail?.workspaces.find((w) => w.workspace_id === ag.workspace_id);
      const peerCount =
        detail?.agents.filter(
          (a) => a.workspace_id === ag.workspace_id && !a.archived_at_rfc3339,
        ).length ?? 0;
      if (peerCount >= 2 || (ws?.board_revision ?? 0) > 0) {
        void orbitActions.fetchWorkspaceBoardAndEvents(hostId, ag.workspace_id);
      }
    }
    if (nextCwd && nextCwd !== "~") {
      void orbitActions.refreshGit(hostId, nextCwd, null, state.diffStaged);
    }
  },

  clearPendingJumpTarget(nonce: number): void {
    setState((s) =>
      s.pendingJumpTarget && s.pendingJumpTarget.nonce === nonce
        ? { ...s, pendingJumpTarget: null }
        : s,
    );
  },

  startNewThreadDraft(
    hostId?: string,
    cwd?: string,
    prefillPrompt?: string,
    prefillSlash?: SlashDirective | null,
    workspaceId?: string | null,
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

    const nextProvider = resolveAvailableDraftProvider(detail, state.draftProvider);
    const draftKey = `draft::${targetHostId ?? "none"}`;

    setState((s) => ({
      ...s,
      activeView: "workspace",
      workspacePanel: "conversation",
      selectedHostId: targetHostId,
      selectedAgentId: null,
      isDraftNewSession: true,
      pendingJumpTarget: null,
      selectedWorkspaceCwd: defaultCwd || "~",
      draftWorktreeRepoPath: null,
      draftWorkspaceId: workspaceId ?? null,
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
    if (targetHostId && defaultCwd && defaultCwd !== "~") {
      void orbitActions.inspectGitRepos(targetHostId, defaultCwd);
    }
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
    setState((s) => ({
      ...s,
      selectedWorkspaceCwd: cwd,
      selectedDiffPath: null,
      draftWorktreeRepoPath: null,
      draftWorkspaceId: null,
    }));
    if (state.selectedHostId && cwd && cwd !== "~") {
      void orbitActions.refreshGit(state.selectedHostId, cwd, null, state.diffStaged);
      void orbitActions.inspectGitRepos(state.selectedHostId, cwd);
    }
  },

  selectProjectWorkspace(hostId: string, cwd: string): void {
    const detail = state.hostDetails[hostId];
    setState((s) => ({
      ...s,
      selectedHostId: hostId,
      selectedAgentId: s.isDraftNewSession ? null : s.selectedAgentId,
      selectedWorkspaceCwd: cwd,
      selectedDiffPath: null,
      draftWorktreeRepoPath: null,
      draftWorkspaceId: null,
      draftProvider: resolveAvailableDraftProvider(detail, s.draftProvider),
    }));
    orbitActions.refreshSidebarView();
    void orbitActions.ensureWorkspaceFiles(hostId, cwd);
    if (cwd && cwd !== "~") {
      void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
      void orbitActions.inspectGitRepos(hostId, cwd);
    }
  },

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
          draftAcpAgentId:
            patch.acpAgentId !== undefined ? patch.acpAgentId : s.draftAcpAgentId,
          draftModels: nextModels,
          draftEffort: patch.effort ?? s.draftEffort,
          draftFastMode: patch.fastMode ?? s.draftFastMode,
          draftMode: patch.mode ?? s.draftMode,
          draftProfileId:
            patch.profileId !== undefined
              ? patch.profileId
              : patch.provider !== undefined || patch.model !== undefined
                ? null
                : s.draftProfileId,
          draftUseWorktree: patch.useWorktree ?? s.draftUseWorktree,
          draftWorktreeBranch: patch.worktreeBranch ?? s.draftWorktreeBranch,
          draftWorktreeRepoPath:
            patch.worktreeRepoPath !== undefined
              ? patch.worktreeRepoPath
              : patch.useWorktree === false
                ? null
                : s.draftWorktreeRepoPath,
          draftWorkspaceId:
            patch.workspaceId !== undefined ? patch.workspaceId : s.draftWorkspaceId,
        };
      },
      true,
    );
  },

  toggleActiveFastMode(action: "toggle" | "enable" | "disable" | boolean = "toggle"): void {
    const hostId = state.selectedHostId;
    const detail = hostId ? state.hostDetails[hostId] : undefined;
    const activeAgent =
      !state.isDraftNewSession && hostId && state.selectedAgentId
        ? detail?.agents.find((a) => a.agent_id === state.selectedAgentId)
        : undefined;

    const availableProviders = detail?.host.available_providers ?? [];
    const draftProvider: ProviderKind =
      availableProviders.length > 0 && !availableProviders.includes(state.draftProvider)
        ? (PROVIDERS.find((p) => availableProviders.includes(p.id))?.id ?? availableProviders[0])
        : state.draftProvider;

    const activeProvider: ProviderKind = activeAgent ? activeAgent.provider : draftProvider;
    const rawModel = activeAgent ? activeAgent.model : (state.draftModels[activeProvider] ?? null);
    const cat = getCatalogForProvider(detail?.provider_catalogs, activeProvider);
    const spec = resolveActiveModelSpec(cat, rawModel);
    const isDraft = !activeAgent;

    if (!spec?.supports_fast_mode && (isDraft || !activeAgent.fast_mode)) {
      if (isDraft && (action === false || action === "disable" || state.draftFastMode)) {
        orbitActions.setDraftConfig({ fastMode: false });
      }
      notifyToast("Fast mode unavailable for this model");
      return;
    }

    const current = activeAgent ? Boolean(activeAgent.fast_mode) : state.draftFastMode;

    const next =
      action === true || action === "enable"
        ? true
        : action === false || action === "disable"
          ? false
          : !current;

    if (next && !spec?.supports_fast_mode) {
      notifyToast("Fast mode unavailable for this model");
      return;
    }

    if (activeAgent && hostId) {
      void orbitIpc.updateAgent({
        hostId,
        agentId: activeAgent.agent_id,
        fastMode: next,
      });
    } else {
      orbitActions.setDraftConfig({ fastMode: next });
    }
    notifyToast(next ? "Fast mode on" : "Fast mode off");
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
    const prev = state;
    // Update draft slice without broadcasting global `emitChange()` across unrelated components
    state = {
      ...state,
      composerDrafts: {
        ...state.composerDrafts,
        [key]: text,
      },
    };
    const hadDraft = hasNonEmptyScopeDraft(prev, key);
    const hasDraft = hasNonEmptyScopeDraft(state, key);
    if (hasDraft) {
      draftPersistence.markDirty(key);
    } else {
      draftPersistence.removeScopes([key], false);
    }
    if (hadDraft !== hasDraft) {
      emitDraftPresenceChange();
    }
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

  openMediaPreview(target: MediaPreviewTarget): void {
    setState((s) => ({ ...s, mediaPreviewTarget: target }));
  },

  closeMediaPreview(): void {
    setState((s) => ({ ...s, mediaPreviewTarget: null }));
  },

  insertCitationIntoComposer(citation: string): void {
    const clean = citation.trim();
    if (!clean) return;
    const key = composerScopeKey(state);
    const current = state.composerDrafts[key] ?? "";
    const sep = current.length === 0 || /\s$/.test(current) ? "" : " ";
    const next = `${current}${sep}\`${clean}\` `;
    setState((s) => ({
      ...s,
      composerDrafts: { ...s.composerDrafts, [key]: next },
    }));
    draftPersistence.markDirty(key);
    notifyToast(`Cited ${clean}`);
    orbitActions.focusComposer();
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

  appendToComposerDraft(text: string): void {
    const clean = text.trim();
    if (!clean) return;
    const key = composerScopeKey(state);
    setState((s) => {
      const existing = (s.composerDrafts[key] ?? "").trimEnd();
      const nextText = existing ? `${existing}\n\n${clean}` : clean;
      return {
        ...s,
        composerDrafts: {
          ...s.composerDrafts,
          [key]: nextText,
        },
      };
    });
    orbitActions.refreshSidebarView();
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
    removeContextMentionInScope(composerScopeKey(state), id);
  },

  registerInlineMention(target: InlineMentionTarget): void {
    const key = composerScopeKey(state);
    setState((s) => {
      const prev = s.inlineMentions[key] ?? [];
      const filtered =
        target.kind === "code_symbol"
          ? prev.filter(
              (m) =>
                !(
                  m.kind === "code_symbol" &&
                  m.path === target.path &&
                  m.symbolName === target.symbolName &&
                  m.startLine === target.startLine
                ),
            )
          : prev.filter((m) => m.token !== target.token);
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
    const mediaKind = detectRichMediaKind(file.name, file.type);

    if (mediaKind) {
      const name = file.name || `attachment-${Date.now()}.${mediaKind === "pdf" ? "pdf" : mediaKind === "audio" ? "mp3" : "mp4"}`;
      const id = `${mediaKind}_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`;
      const mimeType = file.type || defaultMimeForMedia(name, mediaKind);
      const previewUrl = URL.createObjectURL(file);
      const pdfPageCount = mediaKind === "pdf" ? estimatePdfPageCount(rawBytes) : null;
      orbitActions.addContextMention({
        id,
        kind: "file",
        path: "",
        filename: name,
        sizeBytes: file.size,
        localFile: file,
        uploading: Boolean(hostId),
        mediaKind,
        mimeType,
        pdfPageCount,
        previewUrl,
      });
      if (hostId) {
        await uploadContextAttachmentToHost(scopeKey, id, file, name, hostId, false);
      } else {
        notifyToast(`Select a host before sending to upload attached ${mediaKind.toUpperCase()}`);
      }
      return;
    }

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
    // Snapshot ALL routing and draft config fields up front before any async upload/diff/compose
    // awaits so switching Agent or Workspace mid-submit can never misroute the prompt or settings.
    const initialHostDetail = state.hostDetails[hostId];
    const initialIsDraft = state.isDraftNewSession || !state.selectedAgentId;
    const initialAgentId = initialIsDraft ? null : state.selectedAgentId;
    const initialActiveAgent = initialAgentId
      ? (initialHostDetail?.agents.find((a) => a.agent_id === initialAgentId) ?? null)
      : null;
    const submitTarget = {
      hostId,
      scopeKey: key,
      isDraftNewSession: initialIsDraft,
      agentId: initialAgentId,
      workspaceCwd: state.selectedWorkspaceCwd || "~",
      draftProvider: state.draftProvider,
      draftAcpAgentId: state.draftAcpAgentId,
      draftModels: { ...state.draftModels },
      draftEffort: state.draftEffort,
      draftFastMode: state.draftFastMode,
      draftMode: state.draftMode,
      draftProfileId: state.draftProfileId,
      draftWorkspaceId: state.draftWorkspaceId,
      draftUseWorktree: state.draftUseWorktree,
      draftWorktreeBranch: state.draftWorktreeBranch,
      draftWorktreeRepoPath: state.draftWorktreeRepoPath,
      worktreeBranchPrefix: state.worktreeBranchPrefix,
      providerForCompose: initialActiveAgent
        ? initialActiveAgent.provider
        : resolveAvailableDraftProvider(initialHostDetail, state.draftProvider),
    };

    const rawDraft = (state.composerDrafts[submitTarget.scopeKey] ?? "").trim();
    const storedDirective = state.slashDirectives[submitTarget.scopeKey] ?? null;

    const initialContexts = state.contextItems[submitTarget.scopeKey] ?? [];
    const registeredInline = state.inlineMentions[submitTarget.scopeKey] ?? [];
    const pastes = state.pastedAttachments[submitTarget.scopeKey] ?? [];
    const quotes = state.diffQuotes[submitTarget.scopeKey] ?? [];

    // Ensure every attached image/file has completed uploading to the target `submitTarget.hostId`
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

        const refreshedContexts = state.contextItems[submitTarget.scopeKey] ?? [];
        for (const ctx of refreshedContexts) {
          if (
            (ctx.kind === "image" || ctx.kind === "file") &&
            ctx.localFile &&
            (ctx.stagedHostId !== submitTarget.hostId || !ctx.path)
          ) {
            const fname = ctx.filename || ctx.localFile.name || "attachment.bin";
            await uploadContextAttachmentToHost(
              submitTarget.scopeKey,
              ctx.id,
              ctx.localFile,
              fname,
              submitTarget.hostId,
              true,
            );
          }
        }
      } catch (err) {
        notifyToast(`Attachment upload failed: ${String(err)}`);
        return;
      }
    }

    const existingContexts = state.contextItems[submitTarget.scopeKey] ?? [];

    // Resolve any inline `@tokens` still present in `rawDraft` without showing attachment boxes
    const resolvedInlineContexts: ContextMentionItem[] = [];
    for (const m of registeredInline) {
      if (!containsInlineMentionToken(rawDraft, m.token)) continue;
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
      } else if (m.kind === "code_symbol") {
        resolvedInlineContexts.push({
          id: `inl_sym_${m.path}_${m.symbolName}_${m.startLine}`,
          kind: "code_symbol",
          path: m.path,
          startLine: m.startLine,
          endLine: m.endLine,
          symbolName: m.symbolName,
          symbolKind: m.symbolKind,
          signature: m.signature ?? null,
          container: m.container ?? null,
          language: m.language,
        });
      }
    }

    const mergedContexts: ContextMentionItem[] = [...existingContexts];
    for (const rc of resolvedInlineContexts) {
      const dup = mergedContexts.some((x) => {
        if (x.kind === "file" && rc.kind === "file") return x.path === rc.path;
        if (x.kind === "git_diff" && rc.kind === "git_diff") return x.staged === rc.staged;
        if (x.kind === "terminal_output" && rc.kind === "terminal_output") return true;
        if (x.kind === "code_symbol" && rc.kind === "code_symbol") {
          return (
            x.path === rc.path &&
            x.symbolName === rc.symbolName &&
            x.startLine === rc.startLine
          );
        }
        return false;
      });
      if (!dup) mergedContexts.push(rc);
    }

    const cwd = submitTarget.workspaceCwd;

    // Resolve line-numbered code snippets for any cited `@symbol` mentions (bounded & file-deduplicated)
    const symbolContexts = mergedContexts.filter(
      (c): c is Extract<ContextMentionItem, { kind: "code_symbol" }> =>
        c.kind === "code_symbol" && !c.snippet,
    );
    const resolvedSymbolSnippets = new Map<string, string>();
    if (symbolContexts.length > 0 && cwd && cwd !== "~") {
      const uniquePaths = Array.from(new Set(symbolContexts.map((s) => s.path))).slice(0, 8);
      const fileTextByPath = new Map<string, string>();
      await Promise.all(
        uniquePaths.map(async (relPath) => {
          try {
            const view = await orbitIpc.readWorkspaceFile(submitTarget.hostId, cwd, relPath);
            if (view.content_kind === "text" && view.content) {
              fileTextByPath.set(relPath, view.content);
            }
          } catch {
            // Gracefully fall back to signature + path:lines metadata if file read fails
          }
        }),
      );
      for (const sym of symbolContexts) {
        const text = fileTextByPath.get(sym.path);
        if (!text) continue;
        const snippet = extractLineSnippet(text, sym.startLine, sym.endLine, 6000);
        if (snippet) {
          resolvedSymbolSnippets.set(sym.id, snippet);
        }
      }
    }
    const needsWorkingDiff =
      storedDirective === "review" ||
      (!storedDirective && /^\/review(?:\s|$)/i.test(rawDraft)) ||
      mergedContexts.some((c) => c.kind === "git_diff" && !c.staged);
    const needsStagedDiff = mergedContexts.some((c) => c.kind === "git_diff" && c.staged);

    let workingDiffText: string | null = null;
    let stagedDiffText: string | null = null;
    if (needsWorkingDiff) {
      await orbitIpc.fetchGitDiff(submitTarget.hostId, cwd, null, false);
      const gr = await orbitIpc.getGitReview(submitTarget.hostId, cwd, null);
      if (gr.raw_diff.trim()) workingDiffText = gr.raw_diff;
    }
    if (needsStagedDiff) {
      await orbitIpc.fetchGitDiff(submitTarget.hostId, cwd, null, true);
      const gr = await orbitIpc.getGitReview(submitTarget.hostId, cwd, null);
      if (gr.raw_diff.trim()) stagedDiffText = gr.raw_diff;
    }

    const providerForCompose: ProviderKind = submitTarget.providerForCompose;

    const contextInputs: ComposerContextInput[] = mergedContexts.map((ctx) => {
      switch (ctx.kind) {
        case "image":
          return { kind: "image", path: ctx.path, filename: ctx.filename };
        case "file": {
          if (ctx.stagedHostId) {
            const fname = ctx.filename || ctx.path;
            if (ctx.mediaKind === "pdf") {
              return { kind: "pdf", path: ctx.path, filename: fname };
            }
            if (ctx.mediaKind === "audio") {
              return { kind: "audio", path: ctx.path, filename: fname };
            }
            if (ctx.mediaKind === "video") {
              return { kind: "video", path: ctx.path, filename: fname };
            }
            return {
              kind: "staged_file",
              path: ctx.path,
              filename: fname,
            };
          }
          return { kind: "workspace_file", path: ctx.path };
        }
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
        case "code_symbol":
          return {
            kind: "code_symbol",
            name: ctx.symbolName,
            symbol_kind: ctx.symbolKind,
            path: ctx.path,
            start_line: ctx.startLine,
            end_line: ctx.endLine,
            signature: ctx.signature ?? null,
            container: ctx.container ?? null,
            language: ctx.language,
            snippet: ctx.snippet ?? resolvedSymbolSnippets.get(ctx.id) ?? null,
          };
      }
    });

    const composed = await orbitIpc.composePrompt({
      provider: providerForCompose,
      raw_text: rawDraft,
      explicit_directive: storedDirective,
      host_id: submitTarget.hostId,
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

    if (composed.is_fork) {
      if (submitTarget.isDraftNewSession || !submitTarget.agentId) {
        notifyToast("Start an active session first to fork");
        return;
      }
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
        slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
      }));
      draftPersistence.flushNow();
      void orbitActions.forkAgent({
        hostId: submitTarget.hostId,
        sourceAgentId: submitTarget.agentId,
        worktree: Boolean(composed.fork_worktree),
        branch: composed.fork_branch ?? undefined,
        initialPrompt: composed.fork_prompt ?? undefined,
      });
      return;
    }

    if (composed.fast_mode_action) {
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
        slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
      }));
      draftPersistence.flushNow();
      const stillOnOrigin =
        state.selectedHostId === submitTarget.hostId &&
        (state.isDraftNewSession || !state.selectedAgentId) === submitTarget.isDraftNewSession &&
        (submitTarget.isDraftNewSession || state.selectedAgentId === submitTarget.agentId);
      if (stillOnOrigin) {
        orbitActions.toggleActiveFastMode(composed.fast_mode_action);
      }
      return;
    }

    if (composed.is_btw) {
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
      }));
      draftPersistence.flushNow();
      const stillOnOrigin =
        state.selectedHostId === submitTarget.hostId &&
        (state.isDraftNewSession || !state.selectedAgentId) === submitTarget.isDraftNewSession &&
        (submitTarget.isDraftNewSession || state.selectedAgentId === submitTarget.agentId);
      if (stillOnOrigin) {
        orbitActions.openSideQuestionTab();
      }
      if (composed.btw_question) {
        void orbitActions.submitSideQuestion(composed.btw_question, {
          hostId: submitTarget.hostId,
          scopeKey: submitTarget.scopeKey,
          isDraftNewSession: submitTarget.isDraftNewSession,
          agentId: submitTarget.agentId,
          workspaceCwd: submitTarget.workspaceCwd,
          draftProvider: submitTarget.draftProvider,
          draftAcpAgentId: submitTarget.draftAcpAgentId,
          draftModels: submitTarget.draftModels,
        });
      }
      return;
    }

    if (composed.is_compact) {
      if (submitTarget.isDraftNewSession || !submitTarget.agentId) {
        notifyToast("Start an active session first to compact context");
        return;
      }
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
        slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
      }));
      draftPersistence.flushNow();
      await orbitIpc.sendPrompt(submitTarget.hostId, submitTarget.agentId, composed.wire_prompt, {
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
    const targetAgentId = submitTarget.agentId;
    const revokedLocalUrls = new Set<string>();
    for (const ctx of mergedContexts) {
      if (ctx.kind === "image") {
        if (ctx.path && ctx.previewUrl) {
          seedPromptImagePreview(submitTarget.hostId, ctx.path, ctx.previewUrl, true);
        } else if (ctx.previewUrl?.startsWith("blob:")) {
          URL.revokeObjectURL(ctx.previewUrl);
          revokedLocalUrls.add(ctx.previewUrl);
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
      } else if (ctx.kind === "file") {
        if (ctx.previewUrl?.startsWith("blob:")) {
          URL.revokeObjectURL(ctx.previewUrl);
          revokedLocalUrls.add(ctx.previewUrl);
        }
        if (ctx.path) {
          const label = ctx.filename || basenameFromPath(ctx.path);
          const queue = filePathQueueByLabel.get(label);
          if (queue) queue.push(ctx.path);
          else filePathQueueByLabel.set(label, [ctx.path]);
        }
      }
    }
    if (sentImages.length > 0) {
      recordRecentSentTurnImages(submitTarget.hostId, targetAgentId, cleanUserText, sentImages);
    }
    const badges: PromptBadge[] = composed.badges.map((b) => {
      if (b.path) return b;
      if (b.kind === "image") {
        const fallbackPath = imagePathQueueByLabel.get(b.label)?.shift();
        if (fallbackPath) return { ...b, path: fallbackPath };
      } else if (
        b.kind === "file" ||
        b.kind === "pdf" ||
        b.kind === "audio" ||
        b.kind === "video"
      ) {
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
        composerDrafts: { ...s.composerDrafts, [submitTarget.scopeKey]: "" },
        slashDirectives: { ...s.slashDirectives, [submitTarget.scopeKey]: null },
        inlineMentions: { ...s.inlineMentions, [submitTarget.scopeKey]: [] },
        contextItems: { ...s.contextItems, [submitTarget.scopeKey]: [] },
        pastedAttachments: { ...s.pastedAttachments, [submitTarget.scopeKey]: [] },
        diffQuotes: { ...s.diffQuotes, [submitTarget.scopeKey]: [] },
        promptHistory: nextHistory,
        mediaPreviewTarget:
          s.mediaPreviewTarget?.localUrl && revokedLocalUrls.has(s.mediaPreviewTarget.localUrl)
            ? null
            : s.mediaPreviewTarget,
      }),
      true,
    );
    draftPersistence.flushNow();

    if (submitTarget.isDraftNewSession || !submitTarget.agentId) {
      const hostDetailForCreate = state.hostDetails[submitTarget.hostId];
      const installedProviders = hostDetailForCreate?.host.available_providers ?? [];
      const installedAcpList = (hostDetailForCreate?.acp_agents ?? []).filter(
        (a) => a.installed && a.spec.enabled,
      );
      if (hostDetailForCreate && installedProviders.length === 0 && installedAcpList.length === 0) {
        orbitActions.showToast("No installed Agent CLI detected on this host");
        return;
      }
      const effectiveProvider = resolveAvailableDraftProvider(
        hostDetailForCreate,
        submitTarget.draftProvider,
      );
      const effectiveAcpAgentId =
        effectiveProvider === "acp"
          ? submitTarget.draftAcpAgentId &&
            installedAcpList.some((a) => a.spec.id === submitTarget.draftAcpAgentId)
            ? submitTarget.draftAcpAgentId
            : (installedAcpList[0]?.spec.id ?? submitTarget.draftAcpAgentId)
          : null;

      const branchSeed =
        cleanUserText ||
        (effectiveDirective === "review"
          ? "code-review"
          : effectiveDirective === "plan"
            ? "plan"
            : badges[0]?.label ?? "task");

      const selectedCwd = submitTarget.workspaceCwd;
      const gitCacheKey = `${submitTarget.hostId}::${selectedCwd}`;
      const cachedInspection =
        state.gitReposCache[gitCacheKey] ??
        hostDetailForCreate?.git_repos_by_cwd?.[selectedCwd] ??
        null;
      const sidebarProj = state.sidebarView?.projects.find(
        (p) =>
          p.host_id === submitTarget.hostId &&
          matchesProjectDirectory(p.directories, selectedCwd),
      );
      const discoveredProj = hostDetailForCreate?.discovered_projects.find(
        (p) => p.path === selectedCwd,
      );
      const isDirectGitRepo =
        cachedInspection?.is_git_repo ??
        sidebarProj?.is_git_repo ??
        discoveredProj?.is_git_repo ??
        false;
      const nestedRepos =
        cachedInspection?.nested_git_repos ??
        sidebarProj?.nested_git_repos ??
        discoveredProj?.nested_git_repos ??
        [];
      const chosenNestedRepo = submitTarget.draftWorktreeRepoPath
        ? nestedRepos.find((r) => r.path === submitTarget.draftWorktreeRepoPath)?.path ??
          submitTarget.draftWorktreeRepoPath
        : null;

      let effectiveCwd = selectedCwd;
      let effectiveExtraDirs: string[] = [];
      let worktreeBranch: string | null = null;
      if (submitTarget.draftUseWorktree) {
        const canUseWorktree = isDirectGitRepo || Boolean(chosenNestedRepo);
        if (canUseWorktree) {
          const manual = submitTarget.draftWorktreeBranch.trim();
          if (manual) {
            worktreeBranch = manual;
          } else if (
            submitTarget.worktreeBranchPrefix.trim() &&
            submitTarget.worktreeBranchPrefix.trim() !== "auto"
          ) {
            worktreeBranch = slugifyPrefixedBranch(submitTarget.worktreeBranchPrefix, branchSeed);
          } else {
            worktreeBranch = "auto";
          }
          if (!isDirectGitRepo && chosenNestedRepo) {
            effectiveCwd = chosenNestedRepo;
            const parentDirs = new Set<string>([
              selectedCwd,
              ...(sidebarProj?.directories ?? []),
            ]);
            parentDirs.delete(chosenNestedRepo);
            parentDirs.delete("~");
            effectiveExtraDirs = Array.from(parentDirs).filter(Boolean);
          }
        }
      }
      const explicitDraftModel = submitTarget.draftModels[effectiveProvider] ?? null;
      const resolvedSel = await orbitIpc
        .resolveModelSelection({
          host_id: submitTarget.hostId,
          provider: effectiveProvider,
          desired_model: explicitDraftModel,
          desired_effort: submitTarget.draftEffort,
        })
        .catch(() => null);
      const cat = getCatalogForProvider(
        hostDetailForCreate?.provider_catalogs,
        effectiveProvider,
      );
      const spec = resolveActiveModelSpec(cat, explicitDraftModel);
      const resolvedModel =
        effectiveProvider === "acp"
          ? explicitDraftModel
          : (resolvedSel?.selected_model_id ?? explicitDraftModel ?? spec?.id ?? null);
      const resolvedEffort =
        effectiveProvider === "acp"
          ? null
          : (resolvedSel?.selected_effort ??
            resolveSupportedEffortForModel(
              spec,
              submitTarget.draftEffort,
              cat.configured_default_effort,
            ));
      const supportsFast =
        effectiveProvider === "acp"
          ? false
          : (resolvedSel?.supports_fast_mode ?? Boolean(spec?.supports_fast_mode));

      const stillOnOriginDraft =
        state.selectedHostId === submitTarget.hostId &&
        state.isDraftNewSession &&
        (state.selectedWorkspaceCwd || "~") === submitTarget.workspaceCwd;
      let localPendingRef: typeof pendingDraftCreate = null;
      if (stillOnOriginDraft) {
        localPendingRef = {
          hostId: submitTarget.hostId,
          optimisticAgentId: null,
          startedAt: Date.now(),
        };
        pendingDraftCreate = localPendingRef;
      }
      let optimisticId: string | undefined;
      try {
        optimisticId = await orbitIpc.createAgent({
          hostId: submitTarget.hostId,
          cwd: effectiveCwd,
          extraDirs: effectiveExtraDirs,
          provider: effectiveProvider,
          acpAgentId: effectiveAcpAgentId,
          model: resolvedModel,
          effort: resolvedEffort,
          fastMode: Boolean(submitTarget.draftFastMode && supportsFast),
          mode: submitTarget.draftMode,
          initialPrompt: assembledWirePrompt,
          initialDisplayPrompt: cleanUserText,
          initialBadges: badges,
          worktreeBranch,
          profileId: submitTarget.draftProfileId,
          workspaceId: submitTarget.draftWorkspaceId,
        });
      } catch (err) {
        if (localPendingRef && pendingDraftCreate === localPendingRef) {
          pendingDraftCreate = null;
        }
        throw err;
      }
      const stillOnOriginDraftAfterCreate =
        state.selectedHostId === submitTarget.hostId &&
        state.isDraftNewSession &&
        (state.selectedWorkspaceCwd || "~") === submitTarget.workspaceCwd;
      if (
        optimisticId &&
        stillOnOriginDraftAfterCreate &&
        localPendingRef &&
        pendingDraftCreate === localPendingRef
      ) {
        pendingDraftCreate = {
          hostId: submitTarget.hostId,
          optimisticAgentId: optimisticId,
          startedAt: Date.now(),
        };
      }
      if (stillOnOriginDraftAfterCreate) {
        setState((s) => ({
          ...s,
          draftProvider: effectiveProvider,
          draftWorktreeBranch: "",
          draftWorkspaceId: null,
        }));
      }
      return;
    }

    const agentId = submitTarget.agentId;
    const detail = state.hostDetails[submitTarget.hostId];
    const activeAgent = detail?.agents.find((a) => a.agent_id === agentId);
    const supportsServerQueue = Boolean(detail?.host.supports_server_queue);

    if (isAgentForegroundRunning(activeAgent) || activeAgent?.is_awaiting_permission) {
      if (steerImmediately) {
        await orbitIpc.sendPrompt(submitTarget.hostId, agentId, assembledWirePrompt, {
          displayPrompt: cleanUserText,
          badges,
          activeTurnBehavior: "steer",
        });
      } else if (supportsServerQueue) {
        const queuedContexts: ContextMentionItem[] = mergedContexts.map((ctx) => {
          if (ctx.kind === "file" && ctx.previewUrl && revokedLocalUrls.has(ctx.previewUrl)) {
            return { ...ctx, previewUrl: undefined };
          }
          if (ctx.kind === "image" && ctx.previewUrl && revokedLocalUrls.has(ctx.previewUrl)) {
            return { ...ctx, previewUrl: "" };
          }
          return ctx;
        });
        const queueId = `q_${Date.now()}_${Math.random().toString(36).slice(2, 8)}`;
        rememberLocalQueuedDraftExtras(queueId, {
          slashDirective: effectiveDirective,
          contexts: queuedContexts,
          pastes,
          quotes,
        });
        await orbitIpc.enqueuePrompt(submitTarget.hostId, agentId, assembledWirePrompt, {
          displayPrompt: cleanUserText,
          badges,
          queueId,
        });
      } else {
        const queuedContexts: ContextMentionItem[] = mergedContexts.map((ctx) => {
          if (ctx.kind === "file" && ctx.previewUrl && revokedLocalUrls.has(ctx.previewUrl)) {
            return { ...ctx, previewUrl: undefined };
          }
          if (ctx.kind === "image" && ctx.previewUrl && revokedLocalUrls.has(ctx.previewUrl)) {
            return { ...ctx, previewUrl: "" };
          }
          return ctx;
        });
        const queuedItem: QueuedPromptItem = {
          id: `q_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
          wirePrompt: assembledWirePrompt,
          displayText: cleanUserText,
          badges,
          slashDirective: effectiveDirective,
          contexts: queuedContexts,
          pastes,
          quotes,
        };
        setState((s) => ({
          ...s,
          queuedPrompts: {
            ...s.queuedPrompts,
            [submitTarget.scopeKey]: [...(s.queuedPrompts[submitTarget.scopeKey] ?? []), queuedItem],
          },
        }));
      }
      return;
    }

    setState((s) =>
      s.selectedHostId === submitTarget.hostId &&
      !s.isDraftNewSession &&
      s.selectedAgentId === agentId
        ? { ...s, sessionScrollNonce: s.sessionScrollNonce + 1 }
        : s,
    );
    await orbitIpc.sendPrompt(submitTarget.hostId, agentId, assembledWirePrompt, {
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
      const nextQueued = current.slice();
      const [moved] = nextQueued.splice(fromIndex, 1);
      if (!moved) return;
      nextQueued.splice(toIndex, 0, moved);
      const nextIds = nextQueued.map((q) => q.queue_id);
      const tlKey = `${hostId}::${agentId}`;
      setState((s) => {
        const curDetail = s.hostDetails[hostId];
        if (!curDetail) return s;
        const nextAgents = curDetail.agents.map((ag) =>
          ag.agent_id === agentId ? { ...ag, queued_prompts: nextQueued } : ag,
        );
        const curStage = s.stages[tlKey];
        const nextStages = curStage?.agent
          ? applyStageCacheUpdate(s, tlKey, {
              ...curStage,
              agent: { ...curStage.agent, queued_prompts: nextQueued },
            })
          : s.stages;
        return {
          ...s,
          hostDetails: {
            ...s.hostDetails,
            [hostId]: {
              ...curDetail,
              agents: nextAgents,
            },
          },
          stages: nextStages,
        };
      });
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
    if (trimmed && trimmed !== originalTitle) {
      void orbitIpc.updateAgent({ hostId, agentId, title: trimmed });
    }
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
    void Promise.all([
      orbitIpc.markAgentSeen(hostId, agentId, targetSeq),
      orbitIpc.markAgentRead(hostId, agentId),
    ]).then(() => {
      orbitActions.refreshSidebarView();
    });
  },

  openExportDialog(hostId: string, agentId: string): void {
    setState((s) => ({
      ...s,
      exportDialogTarget: { hostId, agentId },
    }));
  },

  closeExportDialog(): void {
    setState((s) => (s.exportDialogTarget ? { ...s, exportDialogTarget: null } : s));
  },

  openWorktreeHandoffDialog(target: WorktreeHandoffDialogTarget): void {
    setState((s) => ({
      ...s,
      handoffDialogTarget: target,
    }));
  },

  closeWorktreeHandoffDialog(): void {
    setState((s) => (s.handoffDialogTarget ? { ...s, handoffDialogTarget: null } : s));
  },

  async openCopyCodeBlocksModal(hostId: string, agentId: string): Promise<void> {
    const reqSeq = ++copyBlocksReqSeq;
    let blocks: ExtractedCodeBlock[] = [];
    try {
      blocks = await orbitIpc.listAgentCodeBlocks(hostId, agentId);
    } catch {
      blocks = [];
    }
    if (
      reqSeq !== copyBlocksReqSeq ||
      state.selectedHostId !== hostId ||
      state.selectedAgentId !== agentId
    ) {
      return;
    }

    if (blocks.length > 0) {
      setState((s) => ({
        ...s,
        copyCodeBlocksTarget: { hostId, agentId, blocks },
      }));
      return;
    }

    // Fallback when 0 code blocks exist: copy the latest assistant reply directly.
    const tKey = threadScopeKey(hostId, agentId);
    let lastReply = "";
    const cachedStage = state.stages[tKey];
    if (cachedStage && cachedStage.turns.length > 0) {
      for (let t = cachedStage.turns.length - 1; t >= 0 && !lastReply; t -= 1) {
        const segs = cachedStage.turns[t]?.live_segments ?? [];
        for (let sIdx = segs.length - 1; sIdx >= 0; sIdx -= 1) {
          const seg = segs[sIdx];
          if (seg?.kind === "assistant_message" && seg.item.item_type === "assistant_message") {
            const trimmed = seg.item.text.trim();
            if (trimmed) {
              lastReply = trimmed;
              break;
            }
          }
        }
      }
    }
    if (!lastReply) {
      try {
        const full = await orbitIpc.getAgentTimeline(hostId, agentId);
        if (
          reqSeq !== copyBlocksReqSeq ||
          state.selectedHostId !== hostId ||
          state.selectedAgentId !== agentId
        ) {
          return;
        }
        for (let i = full.items.length - 1; i >= 0; i -= 1) {
          const item = full.items[i];
          if (item?.item_type === "assistant_message") {
            const trimmed = item.text.trim();
            if (trimmed) {
              lastReply = trimmed;
              break;
            }
          }
        }
      } catch {
        // ignore fallback fetch errors
      }
    }

    if (
      reqSeq !== copyBlocksReqSeq ||
      state.selectedHostId !== hostId ||
      state.selectedAgentId !== agentId
    ) {
      return;
    }
    if (!lastReply) {
      notifyToast("No assistant reply or code block to copy");
      return;
    }
    try {
      await navigator.clipboard.writeText(lastReply);
      notifyToast("Copied last assistant reply");
    } catch {
      notifyToast("Failed to copy reply");
    }
  },

  closeCopyCodeBlocksModal(): void {
    copyBlocksReqSeq += 1;
    setState((s) => (s.copyCodeBlocksTarget ? { ...s, copyCodeBlocksTarget: null } : s));
  },

  async copyThread(hostId: string, agentId: string): Promise<void> {
    try {
      const exported = await orbitIpc.exportAgentTranscript(hostId, agentId, {
        format: "markdown",
        include_thinking: false,
        include_tool_calls: false,
        include_timestamps: false,
      });
      await navigator.clipboard.writeText(exported.content);
      notifyToast("Copied conversation");
      return;
    } catch {
      // Fallback for non-Tauri browser preview
    }

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
            } else {
              bumpFileLoadSeq(k);
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

  async forkAgent(opts: {
    hostId: string;
    sourceAgentId: string;
    upToSeq?: number | null;
    worktree?: boolean;
    branch?: string;
    baseRef?: string;
    title?: string;
    initialPrompt?: string;
  }): Promise<boolean> {
    const dedupKey = `${opts.hostId}::${opts.sourceAgentId}`;
    if (inFlightForks.has(dedupKey)) {
      return false;
    }
    inFlightForks.add(dedupKey);
    try {
      const useWorktree = Boolean(
        opts.worktree || opts.branch?.trim() || opts.baseRef?.trim(),
      );
      const res = await orbitIpc.forkAgent({
        host_id: opts.hostId,
        source_agent_id: opts.sourceAgentId,
        up_to_seq: opts.upToSeq ?? null,
        target: useWorktree
          ? {
              kind: "new_worktree",
              branch: opts.branch?.trim() || null,
              base_ref: opts.baseRef?.trim() || null,
            }
          : { kind: "same_directory" },
        title: opts.title?.trim() || null,
        initial_prompt: opts.initialPrompt?.trim() || null,
      });

      await orbitActions.reloadHostDetail(opts.hostId, true);
      orbitActions.selectAgent(opts.hostId, res.agent.agent_id);

      if (res.workspace?.is_worktree) {
        const branchNote = res.workspace.git_branch
          ? ` · ${res.workspace.git_branch}`
          : "";
        notifyToast(`Forked in new worktree${branchNote}`);
      }

      setTimeout(() => {
        for (const fn of composerFocusListeners) {
          fn();
        }
      }, 24);
      return true;
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      notifyToast(msg || "Failed to fork conversation");
      return false;
    } finally {
      inFlightForks.delete(dedupKey);
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

    const archivedKeySet = new Set([tKey]);
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
        composerDrafts: omitRecordKeys(s.composerDrafts, archivedKeySet),
        slashDirectives: omitRecordKeys(s.slashDirectives, archivedKeySet),
        pastedAttachments: omitRecordKeys(s.pastedAttachments, archivedKeySet),
        diffQuotes: omitRecordKeys(s.diffQuotes, archivedKeySet),
        sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, archivedKeySet),
        ...(wasSelected && !fallbackAgentId
          ? {
              selectedAgentId: null,
              isDraftNewSession: true,
            }
          : {}),
      }),
      true,
    );
    draftPersistence.removeScopes(archivedKeySet, true);

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

  restoreArchivedThread(
    hostId: string,
    agentId: string,
    options?: { open?: boolean },
  ): void {
    const shouldOpen = options?.open !== false;
    const tKey = threadScopeKey(hostId, agentId);
    const targetAgent = state.hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId);
    const keysToUnhide = new Set<string>();
    if (targetAgent?.cwd) {
      keysToUnhide.add(projectPathScopeKey(hostId, targetAgent.cwd));
      for (const proj of state.sidebarView?.projects ?? []) {
        if (proj.host_id === hostId && matchesProjectDirectory(proj.directories, targetAgent.cwd)) {
          keysToUnhide.add(projectPathScopeKey(hostId, proj.primary_cwd));
          for (const dir of proj.directories) {
            keysToUnhide.add(projectPathScopeKey(hostId, dir));
          }
        }
      }
    }

    setState(
      (s) => {
        const nextArchived = { ...s.archivedThreads };
        delete nextArchived[tKey];
        return {
          ...s,
          archivedThreads: nextArchived,
          hiddenProjectKeys:
            keysToUnhide.size > 0
              ? omitRecordKeys(s.hiddenProjectKeys, keysToUnhide)
              : s.hiddenProjectKeys,
          lastArchivedUndo: null,
          ...(shouldOpen ? { activeView: "workspace" as const } : {}),
        };
      },
      true,
    );
    void orbitIpc.unarchiveAgent(hostId, agentId);
    orbitActions.refreshSidebarView();
    if (shouldOpen) {
      orbitActions.selectAgent(hostId, agentId);
    }
    notifyToast("Conversation restored");
  },

  async deleteArchivedThreadsBatch(
    items: { hostId: string; agentId: string }[],
  ): Promise<void> {
    if (items.length === 0) return;
    const deletedThreadKeys = new Set(items.map((it) => threadScopeKey(it.hostId, it.agentId)));

    setState(
      (s) => {
        for (const tKey of deletedThreadKeys) {
          for (const ctx of s.contextItems[tKey] ?? []) {
            if (
              (ctx.kind === "image" || ctx.kind === "file") &&
              ctx.previewUrl?.startsWith("blob:")
            ) {
              URL.revokeObjectURL(ctx.previewUrl);
            }
          }
        }
        return {
          ...s,
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
          sideQuestionsByScope: omitRecordKeys(s.sideQuestionsByScope, deletedThreadKeys),
          sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, deletedThreadKeys),
          rewoundSessions: omitRecordKeys(s.rewoundSessions, deletedThreadKeys),
        };
      },
      true,
    );
    draftPersistence.removeScopes(deletedThreadKeys, true);

    for (const tKey of deletedThreadKeys) {
      lastReportedSeenSeq.delete(tKey);
      stagePullGates.delete(tKey);
    }
    stageAccessOrder = removeStageAccessKeys(stageAccessOrder, deletedThreadKeys);

    await Promise.allSettled(
      items.map((it) => orbitIpc.deleteAgent(it.hostId, it.agentId)),
    );
    const affectedHosts = new Set(items.map((it) => it.hostId));
    await Promise.allSettled(
      Array.from(affectedHosts).map((hid) => orbitActions.reloadHostDetail(hid, false)),
    );
    orbitActions.refreshSidebarView();
    notifyToast(
      items.length === 1
        ? "Deleted 1 archived conversation"
        : `Deleted ${items.length} archived conversations`,
    );
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
          if (
            (ctx.kind === "image" || ctx.kind === "file") &&
            ctx.previewUrl?.startsWith("blob:")
          ) {
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
        const nextSideQDrafts = { ...s.sideQuestionDrafts };
        delete nextSideQDrafts[tKey];
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
          sideQuestionDrafts: nextSideQDrafts,
          rewoundSessions: nextRewound,
          ...(wasSelected ? { selectedAgentId: null, isDraftNewSession: true } : {}),
        };
      },
      true,
    );
    draftPersistence.removeScopes([tKey], true);

    lastReportedSeenSeq.delete(tKey);
    stagePullGates.delete(tKey);
    stageAccessOrder = removeStageAccessKeys(stageAccessOrder, new Set([tKey]));
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

  reorderThreads(scopeKey: string, orderedAgentIds: string[]): void {
    if (!scopeKey || orderedAgentIds.length === 0) return;
    const prevSort = state.sidebarProjectSort;
    setState(
      (s) => ({
        ...s,
        sidebarProjectSort: "manual",
        threadManualOrder: {
          ...s.threadManualOrder,
          [scopeKey]: orderedAgentIds,
        },
      }),
      true,
    );
    if (prevSort !== "manual") {
      orbitActions.refreshSidebarView();
    }
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
    setState((s) => ({ ...s, sidebarWidth: clampSidebarPrefWidth(w) }), persist);
  },

  resetSidebarWidth(): void {
    setState((s) => ({ ...s, sidebarWidth: SIDEBAR_DEFAULT_WIDTH_PX }), true);
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
    setState(
      (s) => ({
        ...s,
        collapsedProjectIds: {
          ...s.collapsedProjectIds,
          [projectId]: !s.collapsedProjectIds[projectId],
        },
      }),
      true,
    );
  },

  setAllProjectsCollapsed(projectIds: string[], collapsed: boolean): void {
    if (projectIds.length === 0) return;
    setState(
      (s) => {
        const next = { ...s.collapsedProjectIds };
        for (const id of projectIds) {
          next[id] = collapsed;
        }
        return {
          ...s,
          collapsedProjectIds: next,
        };
      },
      true,
    );
  },

  async renameProject(project: SidebarProjectGroupView, nextName: string): Promise<void> {
    const cleanName = nextName.trim();
    if (!cleanName || cleanName === project.name) return;
    try {
      const directories =
        project.directories.length > 0 ? project.directories : [project.primary_cwd];
      await orbitIpc.upsertProject({
        projectId: project.pinned_project_id ?? undefined,
        hostId: project.host_id,
        name: cleanName,
        directories,
      });
      await orbitActions.reloadFleetOnly();
      orbitActions.refreshSidebarView();
      notifyToast(`Renamed project to "${cleanName}"`);
    } catch (err) {
      notifyToast(`Failed to rename project: ${String(err)}`);
    }
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
          sideQuestionDrafts: omitRecordKeys(s.sideQuestionDrafts, deletedThreadKeys),
        };
      }, true);
      draftPersistence.removeScopes(deletedThreadKeys, true);
      stageAccessOrder = removeStageAccessKeys(stageAccessOrder, deletedThreadKeys);

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
        terminalFocused: nextOpen ? s.terminalFocused : false,
        sidePanelLaunchpadFocusNonce: willShowLaunchpad
          ? s.sidePanelLaunchpadFocusNonce + 1
          : s.sidePanelLaunchpadFocusNonce,
      };
    });

    if (nextOpen && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.ensureWorkspaceFiles(state.selectedHostId, state.selectedWorkspaceCwd);
    }
  },

  openSidePanelNewTab(opts?: { skipUnsavedGuard?: boolean }): void {
    if (!opts?.skipUnsavedGuard) {
      const activeTab = state.sidePanelTabs.find((t) => t.id === state.activeSidePanelTabId);
      if (state.sidePanelOpen && activeTab?.kind === "file" && activeTab.filePath) {
        const activeKey = fileCacheKey(
          activeTab.hostId ?? state.selectedHostId ?? "",
          activeTab.cwd ?? state.selectedWorkspaceCwd ?? "~",
          activeTab.filePath,
        );
        if (state.fileEditorDrafts[activeKey]?.dirty) {
          setState((s) => ({
            ...s,
            pendingUnsavedFileAction: {
              fileKey: activeKey,
              label: "Open new tab",
              action: { kind: "new_tab" },
            },
          }));
          return;
        }
      }
    }
    setState((s) => {
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        activeSidePanelTabId: null,
        terminalFocused: false,
        pendingUnsavedFileAction: null,
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
    setState((s) => ({ ...s, sidePanelWidth: clampSidePanelPrefWidth(w) }), persist);
  },

  equalizeSidePanel(viewportWidth: number, activeLeftWidthPx: number): void {
    const width = computeEqualizedSidePanelWidth(viewportWidth, activeLeftWidthPx);
    setState((s) => ({ ...s, sidePanelWidth: width }), true);
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
        if (k !== activeFileKey && !s.fileEditorDrafts[k]?.dirty) {
          bumpFileLoadSeq(k);
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
      state.fileViewCache[activeFileKey]?.status !== "error" &&
      !state.fileEditorDrafts[activeFileKey]?.dirty
    ) {
      void orbitActions.loadWorkspaceFile(activeTab.hostId, activeTab.cwd, activeTab.filePath, true);
    }
  },

  focusSidePanelTab(tabId: string, opts?: { skipUnsavedGuard?: boolean }): void {
    const target = state.sidePanelTabs.find((t) => t.id === tabId);
    if (!target) return;
    if (!opts?.skipUnsavedGuard && state.activeSidePanelTabId !== tabId) {
      const activeTab = state.sidePanelTabs.find((t) => t.id === state.activeSidePanelTabId);
      if (state.sidePanelOpen && activeTab?.kind === "file" && activeTab.filePath) {
        const activeKey = fileCacheKey(
          activeTab.hostId ?? state.selectedHostId ?? "",
          activeTab.cwd ?? state.selectedWorkspaceCwd ?? "~",
          activeTab.filePath,
        );
        if (state.fileEditorDrafts[activeKey]?.dirty) {
          setState((s) => ({
            ...s,
            pendingUnsavedFileAction: {
              fileKey: activeKey,
              label: `Switch to ${target.title}`,
              action: { kind: "focus_tab", tabId },
            },
          }));
          return;
        }
      }
    }
    setState((s) => {
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      const nextTerminalFocused =
        s.terminalFocused &&
        hasVisibleActiveTerminal({
          sidePanelOpen: true,
          sidePanelTabs: s.sidePanelTabs,
          activeSidePanelTabId: tabId,
          terminalTabs: s.terminalTabs,
          activeTerminalTabKey: s.activeTerminalTabKey,
        });
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        activeSidePanelTabId: tabId,
        terminalFocused: nextTerminalFocused,
        pendingUnsavedFileAction: null,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    if (target.kind === "review" && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        null,
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
    } else if (target.kind === "subagent") {
      const subHostId = target.hostId ?? state.selectedHostId;
      const parentHostId = state.selectedHostId ?? subHostId;
      const parentAgentId = target.parentAgentId ?? state.selectedAgentId;
      if (subHostId && target.childAgentId) {
        void orbitIpc.openAgentTimeline(subHostId, target.childAgentId);
        void orbitActions.pullStageDelta(subHostId, target.childAgentId, false);
      } else if (parentHostId && parentAgentId && target.providerSubagentId) {
        void orbitActions.openProviderSubagentTimeline(
          parentHostId,
          parentAgentId,
          target.providerSubagentId,
        );
      }
    }
  },

  closeSidePanelTab(tabId: string, opts?: { skipUnsavedGuard?: boolean }): void {
    const closingTab = state.sidePanelTabs.find((t) => t.id === tabId);
    if (!opts?.skipUnsavedGuard && closingTab?.kind === "file" && closingTab.filePath) {
      const closingKey = fileCacheKey(
        closingTab.hostId ?? state.selectedHostId ?? "",
        closingTab.cwd ?? state.selectedWorkspaceCwd ?? "~",
        closingTab.filePath,
      );
      if (state.fileEditorDrafts[closingKey]?.dirty) {
        setState((s) => ({
          ...s,
          sidePanelOpen: true,
          activeSidePanelTabId: tabId,
          pendingUnsavedFileAction: {
            fileKey: closingKey,
            label: `Close ${closingTab.title}`,
            action: { kind: "close_tab", tabId },
          },
        }));
        return;
      }
    }

    let closedPanel = false;
    setState((s) => {
      const idx = s.sidePanelTabs.findIndex((t) => t.id === tabId);
      if (idx < 0) return s;
      const targetTab = s.sidePanelTabs[idx];
      let nextDrafts = s.fileEditorDrafts;
      if (targetTab.kind === "file" && targetTab.filePath) {
        const k = fileCacheKey(
          targetTab.hostId ?? s.selectedHostId ?? "",
          targetTab.cwd ?? s.selectedWorkspaceCwd ?? "~",
          targetTab.filePath,
        );
        if (k in s.fileEditorDrafts) {
          nextDrafts = { ...s.fileEditorDrafts };
          delete nextDrafts[k];
        }
      }
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
      const nextSidePanelOpen = shouldClosePanel ? false : s.sidePanelOpen;
      const nextTerminalFocused =
        s.terminalFocused &&
        hasVisibleActiveTerminal({
          sidePanelOpen: nextSidePanelOpen,
          sidePanelTabs: remaining,
          activeSidePanelTabId: nextActive,
          terminalTabs: s.terminalTabs,
          activeTerminalTabKey: s.activeTerminalTabKey,
        });
      return {
        ...s,
        sidePanelOpen: nextSidePanelOpen,
        sidebarOpen: shouldRestoreSidebar ? s.sidebarDesktopOpenPref : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldRestoreSidebar
          ? false
          : s.sidebarAutoYieldedForSidePanel,
        sidePanelTabs: remaining,
        activeSidePanelTabId: nextActive,
        terminalFocused: nextTerminalFocused,
        fileEditorDrafts: nextDrafts,
        pendingUnsavedFileAction: null,
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
        terminalFocused: false,
        reviewScope: opts?.scope ?? s.reviewScope,
        selectedDiffPath: opts?.path !== undefined ? opts.path : s.selectedDiffPath,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    const effectiveScope = opts?.scope ?? state.reviewScope;
    if (
      effectiveScope === "working_tree" &&
      state.selectedHostId &&
      state.selectedWorkspaceCwd
    ) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        null,
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
        terminalFocused: false,
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
      initialMode?: "view" | "edit";
      skipUnsavedGuard?: boolean;
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

    if (!opts?.skipUnsavedGuard && state.activeSidePanelTabId !== tabId) {
      const activeTab = state.sidePanelTabs.find((t) => t.id === state.activeSidePanelTabId);
      if (state.sidePanelOpen && activeTab?.kind === "file" && activeTab.filePath) {
        const activeKey = fileCacheKey(
          activeTab.hostId ?? state.selectedHostId ?? "",
          activeTab.cwd ?? state.selectedWorkspaceCwd ?? "~",
          activeTab.filePath,
        );
        if (state.fileEditorDrafts[activeKey]?.dirty) {
          setState((s) => ({
            ...s,
            quickOpenFilesOpen: false,
            pendingUnsavedFileAction: {
              fileKey: activeKey,
              label: `Open ${title}`,
              action: {
                kind: "open_file",
                rawPath,
                opts: {
                  focusLine: opts?.focusLine,
                  focusEndLine: opts?.focusEndLine,
                  hostId: opts?.hostId,
                  cwd: opts?.cwd,
                  fallbackPath: opts?.fallbackPath,
                  initialMode: opts?.initialMode,
                },
              },
            },
          }));
          return;
        }
      }
    }

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
        terminalFocused: false,
        pendingUnsavedFileAction: null,
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
      if (state.fileViewCache[primaryKey]?.status !== "error") {
        if (opts?.initialMode === "edit") {
          orbitActions.enterFileEditMode(hostId, cwd, cleanPath);
        }
        return;
      }

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

    const loadSeq = bumpFileLoadSeq(cKey);
    setState((s) => ({
      ...s,
      fileViewCache: {
        ...s.fileViewCache,
        [cKey]: { status: "loading", data: existing?.data },
      },
    }));

    try {
      const data = await orbitIpc.readWorkspaceFile(hostId, cwd, path);
      if (fileLoadSeqByKey.get(cKey) !== loadSeq) {
        return;
      }
      setState((s) => {
        // Maintain a bounded LRU file cache capped by BOTH 20 entries and ~64 MiB total memory
        // (so repeated 25 MiB PDF/video previews never blow renderer RAM), while never evicting
        // files with active editor drafts.
        const nextEntry: FileViewCacheEntry = { status: "ready", data };
        const pinnedEntries: Array<[string, FileViewCacheEntry]> = [];
        const evictableEntries: Array<[string, FileViewCacheEntry]> = [];
        for (const [k, v] of Object.entries(s.fileViewCache)) {
          if (k === cKey) continue;
          if (k in s.fileEditorDrafts) {
            pinnedEntries.push([k, v]);
          } else {
            evictableEntries.push([k, v]);
          }
        }
        let totalBytes = estimateFileCacheEntryBytes(nextEntry);
        for (const [, v] of pinnedEntries) {
          totalBytes += estimateFileCacheEntryBytes(v);
        }
        const maxEvictable = Math.max(0, MAX_FILE_CACHE_ENTRIES - 1 - pinnedEntries.length);
        const keptReverse: Array<[string, FileViewCacheEntry]> = [];
        for (let i = evictableEntries.length - 1; i >= 0; i -= 1) {
          if (keptReverse.length >= maxEvictable) break;
          const [k, v] = evictableEntries[i];
          const entryBytes = estimateFileCacheEntryBytes(v);
          if (totalBytes + entryBytes > MAX_FILE_CACHE_BYTES) continue;
          totalBytes += entryBytes;
          keptReverse.push([k, v]);
        }
        keptReverse.reverse();
        const nextCache: Record<string, FileViewCacheEntry> = {};
        for (const [k, v] of pinnedEntries) {
          nextCache[k] = v;
        }
        for (const [k, v] of keptReverse) {
          nextCache[k] = v;
        }
        nextCache[cKey] = nextEntry;

        // If this file is currently open in Edit mode with no unsaved changes, keep its draft synced with disk
        let nextDrafts = s.fileEditorDrafts;
        const existingDraft = s.fileEditorDrafts[cKey];
        if (existingDraft && !existingDraft.dirty && data.content_kind === "text") {
          const normalized = normalizeEditorText(data.content);
          nextDrafts = {
            ...s.fileEditorDrafts,
            [cKey]: {
              ...existingDraft,
              content: normalized,
              baseContent: normalized,
              baseEtag: data.etag ?? null,
              lineEnding: normalizeLineEndingKind(data.line_ending),
              conflict: null,
              saveError: null,
            },
          };
        }
        return {
          ...s,
          fileViewCache: nextCache,
          fileEditorDrafts: nextDrafts,
        };
      });
    } catch (err) {
      if (fileLoadSeqByKey.get(cKey) !== loadSeq) {
        return;
      }
      setState((s) => ({
        ...s,
        fileViewCache: {
          ...s.fileViewCache,
          [cKey]: { status: "error", error: String(err) },
        },
      }));
    }
  },

  enterFileEditMode(hostId: string, cwd: string, path: string): void {
    const cKey = fileCacheKey(hostId, cwd, path);
    const data = state.fileViewCache[cKey]?.data;
    if (!data || data.content_kind !== "text" || data.truncated || data.writable === false) {
      return;
    }
    const normalized = normalizeEditorText(data.content);
    const lineEnding = normalizeLineEndingKind(data.line_ending);
    setState((s) => {
      const existing = s.fileEditorDrafts[cKey];
      if (existing) {
        return {
          ...s,
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: { ...existing, mode: "edit" },
          },
        };
      }
      return {
        ...s,
        fileEditorDrafts: {
          ...s.fileEditorDrafts,
          [cKey]: {
            hostId,
            cwd,
            path,
            content: normalized,
            baseEtag: data.etag ?? null,
            baseContent: normalized,
            lineEnding,
            dirty: false,
            mode: "edit",
            saving: false,
            saveError: null,
            conflict: null,
          },
        },
      };
    });
  },

  requestFileViewMode(hostId: string, cwd: string, path: string): void {
    const cKey = fileCacheKey(hostId, cwd, path);
    const draft = state.fileEditorDrafts[cKey];
    if (!draft) return;
    if (draft.dirty) {
      setState((s) => ({
        ...s,
        pendingUnsavedFileAction: {
          fileKey: cKey,
          label: "Switch to View mode",
          action: { kind: "switch_to_view", hostId, cwd, path },
        },
      }));
      return;
    }
    setState((s) => {
      const nextDrafts = { ...s.fileEditorDrafts };
      delete nextDrafts[cKey];
      return {
        ...s,
        fileEditorDrafts: nextDrafts,
        pendingUnsavedFileAction:
          s.pendingUnsavedFileAction?.fileKey === cKey ? null : s.pendingUnsavedFileAction,
      };
    });
  },

  requestFileReload(hostId: string, cwd: string, path: string): void {
    const cKey = fileCacheKey(hostId, cwd, path);
    if (state.fileEditorDrafts[cKey]?.dirty) {
      setState((s) => ({
        ...s,
        pendingUnsavedFileAction: {
          fileKey: cKey,
          label: "Reload from disk",
          action: { kind: "reload", hostId, cwd, path },
        },
      }));
      return;
    }
    void orbitActions.loadWorkspaceFile(hostId, cwd, path, true);
  },

  updateFileEditorDraft(hostId: string, cwd: string, path: string, nextContent: string): void {
    const cKey = fileCacheKey(hostId, cwd, path);
    const normalizedNext = normalizeEditorText(nextContent);
    setState((s) => {
      const existing = s.fileEditorDrafts[cKey];
      const diskData = s.fileViewCache[cKey]?.data;
      const baseContent = existing?.baseContent ?? normalizeEditorText(diskData?.content ?? "");
      const baseEtag = existing?.baseEtag ?? diskData?.etag ?? null;
      const lineEnding = existing?.lineEnding ?? normalizeLineEndingKind(diskData?.line_ending);
      return {
        ...s,
        fileEditorDrafts: {
          ...s.fileEditorDrafts,
          [cKey]: {
            hostId,
            cwd,
            path,
            content: normalizedNext,
            baseEtag,
            baseContent,
            lineEnding,
            dirty: normalizedNext !== baseContent,
            mode: "edit",
            saving: existing?.saving ?? false,
            saveError: null,
            conflict: existing?.conflict ?? null,
          },
        },
      };
    });
  },

  clearFileEditorError(hostId: string, cwd: string, path: string): void {
    const cKey = fileCacheKey(hostId, cwd, path);
    setState((s) => {
      const existing = s.fileEditorDrafts[cKey];
      if (!existing || !existing.saveError) return s;
      return {
        ...s,
        fileEditorDrafts: {
          ...s.fileEditorDrafts,
          [cKey]: { ...existing, saveError: null },
        },
      };
    });
  },

  discardFileEditorDraft(hostId: string, cwd: string, path: string): void {
    const cKey = fileCacheKey(hostId, cwd, path);
    setState((s) => {
      const nextDrafts = { ...s.fileEditorDrafts };
      delete nextDrafts[cKey];
      return {
        ...s,
        fileEditorDrafts: nextDrafts,
        pendingUnsavedFileAction:
          s.pendingUnsavedFileAction?.fileKey === cKey ? null : s.pendingUnsavedFileAction,
      };
    });
  },

  async saveWorkspaceFile(
    hostId: string,
    cwd: string,
    path: string,
    force = false,
  ): Promise<"saved" | "conflict" | "error"> {
    const cKey = fileCacheKey(hostId, cwd, path);
    const draft = state.fileEditorDrafts[cKey];
    if (!draft || draft.saving) return "saved";

    const savedNormalizedContent = draft.content;
    const savedLineEnding = draft.lineEnding;
    const wireContent =
      savedLineEnding === "crlf"
        ? savedNormalizedContent.replace(/\n/g, "\r\n")
        : savedNormalizedContent;

    setState((s) => {
      const cur = s.fileEditorDrafts[cKey];
      if (!cur) return s;
      return {
        ...s,
        fileEditorDrafts: {
          ...s.fileEditorDrafts,
          [cKey]: { ...cur, saving: true, saveError: null },
        },
      };
    });

    try {
      const res = await orbitIpc.writeWorkspaceFile({
        hostId,
        cwd,
        path,
        content: wireContent,
        expectedEtag: draft.baseEtag,
        force,
      });

      if (res.conflict) {
        setState((s) => {
          const cur = s.fileEditorDrafts[cKey];
          if (!cur) return s;
          return {
            ...s,
            fileEditorDrafts: {
              ...s.fileEditorDrafts,
              [cKey]: {
                ...cur,
                saving: false,
                saveError: null,
                conflict: res.conflict ?? null,
              },
            },
          };
        });
        return "conflict";
      }

      const cachedData = state.fileViewCache[cKey]?.data;
      const language = cachedData?.language ?? "text";
      const [highlightedLines, markdownBlocks] = await Promise.all([
        orbitIpc
          .highlightWorkspaceCode(
            language,
            savedNormalizedContent,
            Math.max(2500, savedNormalizedContent.split("\n").length + 50),
          )
          .catch(() => cachedData?.highlighted_lines ?? []),
        language === "markdown"
          ? orbitIpc
              .parseMarkdownPreview(savedNormalizedContent, false)
              .catch(() => cachedData?.markdown_blocks ?? [])
          : Promise.resolve(cachedData?.markdown_blocks ?? []),
      ]);

      setState((s) => {
        const curDraft = s.fileEditorDrafts[cKey];
        const stillEdited = curDraft && curDraft.content !== savedNormalizedContent;
        const curCache = s.fileViewCache[cKey]?.data;
        const nextCache = curCache
          ? {
              ...s.fileViewCache,
              [cKey]: {
                status: "ready" as const,
                data: {
                  ...curCache,
                  content: wireContent,
                  size_bytes: res.size_bytes,
                  etag: res.etag,
                  mtime_unix_ms: res.mtime_unix_ms ?? curCache.mtime_unix_ms ?? null,
                  line_ending: savedLineEnding,
                  highlighted_lines: highlightedLines,
                  markdown_blocks: markdownBlocks,
                },
              },
            }
          : s.fileViewCache;

        const nextDrafts = curDraft
          ? {
              ...s.fileEditorDrafts,
              [cKey]: {
                ...curDraft,
                baseEtag: res.etag,
                baseContent: savedNormalizedContent,
                lineEnding: savedLineEnding,
                dirty: Boolean(stillEdited),
                saving: false,
                saveError: null,
                conflict: null,
              },
            }
          : s.fileEditorDrafts;

        return {
          ...s,
          fileViewCache: nextCache,
          fileEditorDrafts: nextDrafts,
        };
      });

      if (cwd && cwd !== "~") {
        void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
      }
      notifyToast(`Saved ${basenameFromPath(path) || path}`);
      return "saved";
    } catch (err) {
      setState((s) => {
        const cur = s.fileEditorDrafts[cKey];
        if (!cur) return s;
        return {
          ...s,
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: {
              ...cur,
              saving: false,
              saveError: String(err),
            },
          },
        };
      });
      return "error";
    }
  },

  async reloadWorkspaceFileFromConflict(hostId: string, cwd: string, path: string): Promise<void> {
    const cKey = fileCacheKey(hostId, cwd, path);
    try {
      const refreshed = await orbitIpc.readWorkspaceFile(hostId, cwd, path);
      const normalized = normalizeEditorText(refreshed.content);
      const lineEnding = normalizeLineEndingKind(refreshed.line_ending);
      setState((s) => ({
        ...s,
        fileViewCache: {
          ...s.fileViewCache,
          [cKey]: { status: "ready", data: refreshed },
        },
        fileEditorDrafts: {
          ...s.fileEditorDrafts,
          [cKey]: {
            hostId,
            cwd,
            path,
            content: normalized,
            baseEtag: refreshed.etag ?? null,
            baseContent: normalized,
            lineEnding,
            dirty: false,
            mode: "edit",
            saving: false,
            saveError: null,
            conflict: null,
          },
        },
      }));
      notifyToast(`Reloaded ${basenameFromPath(path) || path}`);
    } catch (err) {
      // Preserve existing cache & editor buffer so the user retains access to `Copy mine`
      setState((s) => {
        const cur = s.fileEditorDrafts[cKey];
        if (!cur) return s;
        return {
          ...s,
          fileEditorDrafts: {
            ...s.fileEditorDrafts,
            [cKey]: {
              ...cur,
              saving: false,
              saveError: `Reload failed: ${String(err)}`,
            },
          },
        };
      });
    }
  },

  async resolveUnsavedFileAction(choice: "save" | "discard" | "cancel"): Promise<void> {
    const pending = state.pendingUnsavedFileAction;
    if (!pending) return;

    if (choice === "cancel") {
      setState((s) => ({ ...s, pendingUnsavedFileAction: null }));
      return;
    }

    const draft = state.fileEditorDrafts[pending.fileKey];
    if (choice === "save" && draft) {
      const outcome = await orbitActions.saveWorkspaceFile(
        draft.hostId,
        draft.cwd,
        draft.path,
        false,
      );
      if (outcome !== "saved") {
        // Keep user on current file so conflict banner or save error is visible
        setState((s) => ({ ...s, pendingUnsavedFileAction: null }));
        return;
      }
    }

    if (choice === "discard" && draft) {
      orbitActions.discardFileEditorDraft(draft.hostId, draft.cwd, draft.path);
    } else {
      setState((s) => ({ ...s, pendingUnsavedFileAction: null }));
    }

    const act = pending.action;
    switch (act.kind) {
      case "focus_tab":
        orbitActions.focusSidePanelTab(act.tabId, { skipUnsavedGuard: true });
        break;
      case "close_tab":
        orbitActions.closeSidePanelTab(act.tabId, { skipUnsavedGuard: true });
        break;
      case "open_file":
        orbitActions.openFileInSidePanel(act.rawPath, {
          ...act.opts,
          skipUnsavedGuard: true,
        });
        break;
      case "new_tab":
        orbitActions.openSidePanelNewTab({ skipUnsavedGuard: true });
        break;
      case "switch_to_view":
        orbitActions.discardFileEditorDraft(act.hostId, act.cwd, act.path);
        break;
      case "reload":
        orbitActions.discardFileEditorDraft(act.hostId, act.cwd, act.path);
        void orbitActions.loadWorkspaceFile(act.hostId, act.cwd, act.path, true);
        break;
    }
  },

  async createWorkspaceEntry(
    hostId: string,
    cwd: string,
    path: string,
    isDir: boolean,
    initialContent?: string,
  ): Promise<string> {
    const res = await orbitIpc.createWorkspaceEntry({
      hostId,
      cwd,
      path,
      isDir,
      initialContent: initialContent ?? (isDir ? null : ""),
    });
    const wsKey = `${hostId}::${cwd}`;
    const normalized = res.path;
    if (isDir) {
      setState((s) => {
        const cur = s.workspaceEmptyDirsCache[wsKey] ?? [];
        if (cur.includes(normalized)) return s;
        return {
          ...s,
          workspaceEmptyDirsCache: {
            ...s.workspaceEmptyDirsCache,
            [wsKey]: [...cur, normalized],
          },
        };
      });
    }
    await orbitActions.ensureWorkspaceFiles(hostId, cwd, true);
    if (!isDir) {
      orbitActions.openFileInSidePanel(normalized, { hostId, cwd, initialMode: "edit" });
    }
    if (cwd && cwd !== "~") {
      void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
    }
    return normalized;
  },

  async renameWorkspaceEntry(
    hostId: string,
    cwd: string,
    oldPath: string,
    newPath: string,
  ): Promise<string> {
    const res = await orbitIpc.renameWorkspaceEntry({ hostId, cwd, oldPath, newPath });
    const oldNorm = res.path;
    const newNorm = res.new_path ?? newPath;
    const wsKey = `${hostId}::${cwd}`;
    const remapPath = (p: string): string | null => {
      if (p === oldNorm) return newNorm;
      if (p.startsWith(`${oldNorm}/`)) return `${newNorm}${p.slice(oldNorm.length)}`;
      return null;
    };

    const pathsNeedingHighlightReload: string[] = [];
    const extOf = (p: string): string => {
      const base = basenameFromPath(p);
      const dot = base.lastIndexOf(".");
      return dot > 0 ? base.slice(dot + 1).toLowerCase() : "";
    };

    setState((s) => {
      const prevEmptyDirs = s.workspaceEmptyDirsCache[wsKey] ?? [];
      const nextEmptyDirs = prevEmptyDirs.map((d) => remapPath(d) ?? d);

      const nextTabs = s.sidePanelTabs.map((t) => {
        if (t.kind !== "file" || !t.filePath) return t;
        const tHost = t.hostId ?? s.selectedHostId;
        const tCwd = t.cwd ?? s.selectedWorkspaceCwd;
        if (tHost !== hostId || tCwd !== cwd) return t;
        const mapped = remapPath(t.filePath);
        if (!mapped) return t;
        const oldKey = `${hostId}::${cwd}::${t.filePath}`;
        if (extOf(t.filePath) !== extOf(mapped) && !s.fileEditorDrafts[oldKey]?.dirty) {
          pathsNeedingHighlightReload.push(mapped);
        }
        return {
          ...t,
          id: `tab:file:${hostId}::${cwd}::${mapped}`,
          title: basenameFromPath(mapped) || mapped,
          filePath: mapped,
        };
      });

      let nextActiveTabId = s.activeSidePanelTabId;
      for (let i = 0; i < s.sidePanelTabs.length; i += 1) {
        if (s.sidePanelTabs[i].id === s.activeSidePanelTabId) {
          nextActiveTabId = nextTabs[i]?.id ?? s.activeSidePanelTabId;
          break;
        }
      }

      const prefix = `${hostId}::${cwd}::`;
      const nextCache: Record<string, FileViewCacheEntry> = {};
      for (const [k, v] of Object.entries(s.fileViewCache)) {
        if (k.startsWith(prefix)) {
          const rel = k.slice(prefix.length);
          const mapped = remapPath(rel);
          if (mapped) {
            nextCache[`${prefix}${mapped}`] = v.data
              ? { ...v, data: { ...v.data, path: mapped } }
              : v;
            continue;
          }
        }
        nextCache[k] = v;
      }

      const nextDrafts: Record<string, FileEditorDraftState> = {};
      for (const [k, v] of Object.entries(s.fileEditorDrafts)) {
        if (k.startsWith(prefix)) {
          const rel = k.slice(prefix.length);
          const mapped = remapPath(rel);
          if (mapped) {
            nextDrafts[`${prefix}${mapped}`] = { ...v, path: mapped };
            continue;
          }
        }
        nextDrafts[k] = v;
      }

      // Clear/remap any pending unsaved guard referencing the renamed file or tab
      let nextPending = s.pendingUnsavedFileAction;
      if (nextPending && nextPending.fileKey.startsWith(prefix)) {
        const mappedKeyRel = remapPath(nextPending.fileKey.slice(prefix.length));
        if (mappedKeyRel) {
          nextPending = null;
        }
      }

      return {
        ...s,
        workspaceEmptyDirsCache: {
          ...s.workspaceEmptyDirsCache,
          [wsKey]: nextEmptyDirs,
        },
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: nextActiveTabId,
        fileViewCache: nextCache,
        fileEditorDrafts: nextDrafts,
        pendingUnsavedFileAction: nextPending,
      };
    });

    for (const reloadPath of pathsNeedingHighlightReload) {
      void orbitActions.loadWorkspaceFile(hostId, cwd, reloadPath, true);
    }

    await orbitActions.ensureWorkspaceFiles(hostId, cwd, true);
    if (cwd && cwd !== "~") {
      void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
    }
    return newNorm;
  },

  async deleteWorkspaceEntry(
    hostId: string,
    cwd: string,
    path: string,
    opts?: { allowDirty?: boolean },
  ): Promise<void> {
    const cleanTarget = path.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
    const prefix = `${hostId}::${cwd}::`;
    if (!opts?.allowDirty) {
      const hasDirtyMatch = Object.entries(state.fileEditorDrafts).some(([k, d]) => {
        if (!d.dirty || !k.startsWith(prefix)) return false;
        const rel = k.slice(prefix.length);
        return rel === cleanTarget || rel.startsWith(`${cleanTarget}/`);
      });
      if (hasDirtyMatch) {
        throw new Error("Cannot delete entry while it has unsaved edits in the editor");
      }
    }

    const res = await orbitIpc.deleteWorkspaceEntry({ hostId, cwd, path });
    const delNorm = res.path;
    const wsKey = `${hostId}::${cwd}`;
    const matchesDeleted = (p: string): boolean => p === delNorm || p.startsWith(`${delNorm}/`);

    setState((s) => {
      const prevEmptyDirs = s.workspaceEmptyDirsCache[wsKey] ?? [];
      const nextEmptyDirs = prevEmptyDirs.filter((d) => !matchesDeleted(d));

      const remainingTabs = s.sidePanelTabs.filter((t) => {
        if (t.kind !== "file" || !t.filePath) return true;
        const tHost = t.hostId ?? s.selectedHostId;
        const tCwd = t.cwd ?? s.selectedWorkspaceCwd;
        if (tHost !== hostId || tCwd !== cwd) return true;
        return !matchesDeleted(t.filePath);
      });

      const activeRemoved = !remainingTabs.some((t) => t.id === s.activeSidePanelTabId);
      const nextActive = activeRemoved
        ? (remainingTabs[remainingTabs.length - 1]?.id ?? null)
        : s.activeSidePanelTabId;

      const nextCache: Record<string, FileViewCacheEntry> = {};
      for (const [k, v] of Object.entries(s.fileViewCache)) {
        if (k.startsWith(prefix) && matchesDeleted(k.slice(prefix.length))) continue;
        nextCache[k] = v;
      }

      const nextDrafts: Record<string, FileEditorDraftState> = {};
      for (const [k, v] of Object.entries(s.fileEditorDrafts)) {
        if (k.startsWith(prefix) && matchesDeleted(k.slice(prefix.length))) continue;
        nextDrafts[k] = v;
      }

      const nextPending =
        s.pendingUnsavedFileAction &&
        s.pendingUnsavedFileAction.fileKey.startsWith(prefix) &&
        matchesDeleted(s.pendingUnsavedFileAction.fileKey.slice(prefix.length))
          ? null
          : s.pendingUnsavedFileAction;

      return {
        ...s,
        workspaceEmptyDirsCache: {
          ...s.workspaceEmptyDirsCache,
          [wsKey]: nextEmptyDirs,
        },
        sidePanelTabs: remainingTabs,
        activeSidePanelTabId: nextActive,
        fileViewCache: nextCache,
        fileEditorDrafts: nextDrafts,
        pendingUnsavedFileAction: nextPending,
      };
    });

    await orbitActions.ensureWorkspaceFiles(hostId, cwd, true);
    if (cwd && cwd !== "~") {
      void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
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
        terminalFocused: false,
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
        terminalFocused: false,
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

  async submitSideQuestion(
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
  ): Promise<void> {
    const hostId = targetOverride?.hostId ?? state.selectedHostId;
    if (!hostId) {
      notifyToast("Connect an Orbit host first");
      return;
    }

    const isDraft = targetOverride ? targetOverride.isDraftNewSession : state.isDraftNewSession;
    const targetAgentId = targetOverride ? targetOverride.agentId : state.selectedAgentId;
    const scope =
      targetOverride?.scopeKey ??
      (isDraft || !targetAgentId ? `draft::${hostId}` : `${hostId}::${targetAgentId}`);
    const question = (rawQuestion ?? state.sideQuestionDrafts[scope] ?? "").trim();
    if (!question) return;

    const detail = state.hostDetails[hostId];
    const currentAgent =
      !isDraft && targetAgentId
        ? detail?.agents.find((a) => a.agent_id === targetAgentId) ??
          state.stages[`${hostId}::${targetAgentId}`]?.agent ??
          null
        : null;

    const provider = currentAgent?.provider ?? targetOverride?.draftProvider ?? state.draftProvider;
    const draftModels = targetOverride?.draftModels ?? state.draftModels;
    const draftAcpAgentId =
      targetOverride?.draftAcpAgentId !== undefined
        ? targetOverride.draftAcpAgentId
        : state.draftAcpAgentId;
    const model = currentAgent?.model ?? draftModels[provider] ?? null;
    const cwd = currentAgent?.cwd || targetOverride?.workspaceCwd || state.selectedWorkspaceCwd || "~";
    const providerLabel =
      currentAgent?.provider_label ??
      (provider === "acp"
        ? (draftAcpAgentId ?? formatProviderLabel("acp"))
        : formatProviderLabel(provider));

    const gitReview = state.gitReviews[`${hostId}::${cwd}`] ?? null;
    const branchName =
      gitReview?.status?.branch ??
      detail?.workspaces.find((w) => w.cwd === cwd)?.git_branch ??
      null;
    const rustContext =
      !isDraft && targetAgentId
        ? await orbitIpc
            .buildSideQuestionContext(hostId, targetAgentId, 3000)
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

    const stillViewingTarget =
      state.selectedHostId === hostId &&
      (state.isDraftNewSession || !state.selectedAgentId) === (isDraft || !targetAgentId) &&
      (isDraft || !targetAgentId || state.selectedAgentId === targetAgentId);
    if (!targetOverride || stillViewingTarget) {
      orbitActions.openSideQuestionTab();
    }
    setState((s) => ({
      ...s,
      sideQuestionDrafts: { ...s.sideQuestionDrafts, [scope]: "" },
      sideQuestionsByScope: {
        ...s.sideQuestionsByScope,
        [scope]: [...(s.sideQuestionsByScope[scope] ?? []), newEntry],
      },
    }));
    draftPersistence.flushNow();

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
    childHostId?: string | null;
    providerSubagentId?: string | null;
    parentAgentId?: string | null;
  }): void {
    const parentHostId = state.selectedHostId;
    const targetHostId = params.childHostId ?? parentHostId;
    const parentAgentId = params.parentAgentId ?? state.selectedAgentId;
    const hasManagedChild = Boolean(
      targetHostId &&
        params.childAgentId &&
        state.hostDetails[targetHostId]?.agents.some((a) => a.agent_id === params.childAgentId),
    );
    const effectiveChildAgentId = hasManagedChild ? params.childAgentId : null;
    const effectiveProviderSubagentId =
      params.providerSubagentId ?? (!hasManagedChild ? params.childAgentId : null);
    const tabHostId = hasManagedChild ? targetHostId : parentHostId;
    setState((s) => {
      const { tabs: nextTabs, activeTabId } = upsertSubagentSidePanelTab(s.sidePanelTabs, {
        callId: params.callId,
        role: params.role,
        promptPreview: params.promptPreview,
        tabHostId,
        parentHostId,
        parentAgentId,
        effectiveChildAgentId,
        effectiveProviderSubagentId,
      });
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: activeTabId,
        terminalFocused: false,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });

    if (tabHostId && effectiveChildAgentId) {
      void orbitIpc.openAgentTimeline(tabHostId, effectiveChildAgentId);
      void orbitActions.pullStageDelta(tabHostId, effectiveChildAgentId, false);
    } else if (parentHostId && parentAgentId && effectiveProviderSubagentId) {
      void orbitActions.openProviderSubagentTimeline(
        parentHostId,
        parentAgentId,
        effectiveProviderSubagentId,
      );
    }
  },

  openProviderSubagentTimeline(
    hostId: string,
    parentAgentId: string,
    subagentId: string,
  ): Promise<void> {
    return orbitIpc.openProviderSubagentTimeline(hostId, parentAgentId, subagentId);
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
        null,
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
    const normIncomingCwd = normalizeCwdForCompare(cwd);
    setState((s) => {
      const mergedPrimary = mergeGitReviewSnapshot(s.gitReviews[rKey], gr);
      const nextGitReviews: Record<string, GitReviewView> = {
        ...s.gitReviews,
        [rKey]: mergedPrimary,
      };
      // Keep selectedWorkspaceCwd cache synchronized even if trailing slashes differ
      if (
        s.selectedHostId === hostId &&
        s.selectedWorkspaceCwd &&
        s.selectedWorkspaceCwd !== cwd &&
        normalizeCwdForCompare(s.selectedWorkspaceCwd) === normIncomingCwd
      ) {
        const selectedKey = `${hostId}::${s.selectedWorkspaceCwd}`;
        nextGitReviews[selectedKey] = mergeGitReviewSnapshot(s.gitReviews[selectedKey], gr);
      }
      return {
        ...s,
        gitReviews: nextGitReviews,
      };
    });
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
      const nextTerminalFocused =
        s.terminalFocused &&
        s.activeTerminalTabKey !== tabKey &&
        hasVisibleActiveTerminal({
          sidePanelOpen: s.sidePanelOpen,
          sidePanelTabs: s.sidePanelTabs,
          activeSidePanelTabId: s.activeSidePanelTabId,
          terminalTabs: remaining,
          activeTerminalTabKey: nextActive,
        });
      return {
        ...s,
        terminalTabs: remaining,
        activeTerminalTabKey: nextActive,
        terminalFocused: nextTerminalFocused,
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

  openProjectInstructions(target?: InstructionsTarget | null): void {
    setState((s) => ({
      ...s,
      activeView: "settings",
      settingsSection: "instructions",
      instructionsTarget: target ?? null,
      commandPaletteOpen: false,
      overviewPopoverOpen: false,
    }));
  },

  setInstructionsTarget(target: InstructionsTarget | null): void {
    setState((s) => ({ ...s, instructionsTarget: target }));
  },

  closeSettings(): void {
    setState((s) => ({ ...s, activeView: "workspace", instructionsTarget: null }));
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

  async refreshExternalMcpServers(
    hostId: string,
    cwd?: string | null,
  ): Promise<ExternalMcpServerRecord[]> {
    const seq = (externalMcpSnapshotSeqByHost[hostId] ?? 0) + 1;
    externalMcpSnapshotSeqByHost[hostId] = seq;
    const servers = await orbitIpc.listExternalMcpServers(hostId, cwd);
    if (externalMcpSnapshotSeqByHost[hostId] === seq) {
      setState((s) => ({
        ...s,
        externalMcpServersByHost: {
          ...s.externalMcpServersByHost,
          [hostId]: servers,
        },
      }));
    }
    return servers;
  },

  async upsertExternalMcpServer(
    request: UpsertExternalMcpServerRequest,
  ): Promise<ExternalMcpServerRecord[]> {
    const seq = (externalMcpSnapshotSeqByHost[request.host_id] ?? 0) + 1;
    externalMcpSnapshotSeqByHost[request.host_id] = seq;
    const servers = await orbitIpc.upsertExternalMcpServer(request);
    if (externalMcpSnapshotSeqByHost[request.host_id] === seq) {
      setState((s) => ({
        ...s,
        externalMcpServersByHost: {
          ...s.externalMcpServersByHost,
          [request.host_id]: servers,
        },
      }));
    }
    return servers;
  },

  async toggleExternalMcpServer(
    hostId: string,
    serverId: string,
    enabled: boolean,
    cwd?: string | null,
  ): Promise<ExternalMcpServerRecord[]> {
    const seq = (externalMcpSnapshotSeqByHost[hostId] ?? 0) + 1;
    externalMcpSnapshotSeqByHost[hostId] = seq;
    const servers = await orbitIpc.toggleExternalMcpServer(hostId, serverId, enabled, cwd);
    if (externalMcpSnapshotSeqByHost[hostId] === seq) {
      setState((s) => ({
        ...s,
        externalMcpServersByHost: {
          ...s.externalMcpServersByHost,
          [hostId]: servers,
        },
      }));
    }
    return servers;
  },

  async deleteExternalMcpServer(
    hostId: string,
    serverId: string,
    cwd?: string | null,
  ): Promise<ExternalMcpServerRecord[]> {
    const seq = (externalMcpSnapshotSeqByHost[hostId] ?? 0) + 1;
    externalMcpSnapshotSeqByHost[hostId] = seq;
    const servers = await orbitIpc.deleteExternalMcpServer(hostId, serverId, cwd);
    if (externalMcpSnapshotSeqByHost[hostId] === seq) {
      setState((s) => ({
        ...s,
        externalMcpServersByHost: {
          ...s.externalMcpServersByHost,
          [hostId]: servers,
        },
      }));
    }
    return servers;
  },

  async probeExternalMcpServer(
    hostId: string,
    serverId: string,
    cwd?: string | null,
  ): Promise<McpProbeResult> {
    const result = await orbitIpc.probeExternalMcpServer(hostId, serverId, cwd);
    setState((s) => {
      const existing = s.externalMcpServersByHost[hostId];
      if (!existing) return s;
      return {
        ...s,
        externalMcpServersByHost: {
          ...s.externalMcpServersByHost,
          [hostId]: existing.map((srv) =>
            srv.id === serverId ? { ...srv, last_probe: result } : srv,
          ),
        },
      };
    });
    return result;
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
    void pushCloudPrefPatch({ theme });
  },

  cycleTheme(): void {
    const order: ThemeMode[] = ["system", "light", "dark"];
    const next = order[(order.indexOf(state.theme) + 1) % order.length];
    orbitActions.setTheme(next);
  },

  setEnterToSend(enabled: boolean): void {
    setState((s) => ({ ...s, enterToSend: enabled }), true);
    void pushCloudPrefPatch({ enterToSend: enabled });
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
    void pushCloudPrefPatch(patch);
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
    void pushCloudPrefPatch(patch);
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

  // =========================================================================
  // Git Repo Inspection, Project Reordering & Mission Workspace Actions
  // =========================================================================

  async inspectGitRepos(
    hostId: string,
    path: string,
    extraDirs?: string[],
  ): Promise<GitReposInspectionView | null> {
    if (!hostId || !path || path === "~") return null;
    try {
      const res = await orbitIpc.inspectGitRepos(hostId, path, extraDirs);
      const cacheKey = `${hostId}::${path}`;
      setState((s) => ({
        ...s,
        gitReposCache: {
          ...s.gitReposCache,
          [cacheKey]: res,
        },
      }));
      orbitActions.refreshSidebarView();
      return res;
    } catch {
      return null;
    }
  },

  async reorderProjects(orderedKeys: string[]): Promise<void> {
    try {
      await orbitIpc.reorderProjects(orderedKeys);
      await orbitActions.reloadFleetOnly();
      orbitActions.refreshSidebarView();
    } catch (err) {
      notifyToast(`Failed to reorder projects: ${String(err)}`);
    }
  },

  toggleWorkspaceCollapsed(workspaceId: string): void {
    setState((s) => ({
      ...s,
      collapsedWorkspaceIds: {
        ...s.collapsedWorkspaceIds,
        [workspaceId]: !s.collapsedWorkspaceIds[workspaceId],
      },
    }));
  },

  toggleProjectInboxCollapsed(projectId: string): void {
    setState(
      (s) => ({
        ...s,
        collapsedInboxProjectIds: {
          ...s.collapsedInboxProjectIds,
          [projectId]: !s.collapsedInboxProjectIds[projectId],
        },
      }),
      true,
    );
  },

  archiveSubagent(
    hostId: string,
    parentAgentId: string,
    target: {
      id: string;
      providerSubagentId?: string | null;
      childAgentId?: string | null;
      role?: string;
    },
  ): void {
    const keys: string[] = [];
    if (target.id) keys.push(subagentArchiveKey(hostId, parentAgentId, target.id));
    if (target.providerSubagentId) {
      keys.push(subagentArchiveKey(hostId, parentAgentId, target.providerSubagentId));
    }
    if (target.childAgentId) {
      keys.push(subagentArchiveKey(hostId, parentAgentId, target.childAgentId));
    }
    if (target.childAgentId) {
      const hasManaged = state.hostDetails[hostId]?.agents.some(
        (a) => a.agent_id === target.childAgentId,
      );
      if (hasManaged) {
        orbitActions.archiveThread(hostId, target.childAgentId);
      }
    }
    setState(
      (s) => {
        const nextArchived = { ...s.archivedSubagents };
        for (const k of keys) {
          nextArchived[k] = true;
        }
        const remainingTabs = s.sidePanelTabs.filter((t) => {
          if (t.kind !== "subagent") return true;
          const matchesId =
            (target.id && t.subagentCallId === target.id) ||
            (target.providerSubagentId && t.providerSubagentId === target.providerSubagentId) ||
            (target.childAgentId && t.childAgentId === target.childAgentId);
          return !matchesId;
        });
        const removedActive =
          remainingTabs.length !== s.sidePanelTabs.length &&
          !remainingTabs.some((t) => t.id === s.activeSidePanelTabId);
        const shouldClosePanel =
          remainingTabs.length !== s.sidePanelTabs.length && remainingTabs.length === 0;
        return {
          ...s,
          archivedSubagents: nextArchived,
          sidePanelTabs: remainingTabs,
          activeSidePanelTabId: removedActive
            ? (remainingTabs[remainingTabs.length - 1]?.id ?? null)
            : s.activeSidePanelTabId,
          sidePanelOpen: shouldClosePanel ? false : s.sidePanelOpen,
        };
      },
      true,
    );
    if (!target.childAgentId) {
      notifyToast(`已归档子代理${target.role ? `「${target.role}」` : ""}`);
    }
  },

  archiveFinishedSubagents(
    hostId: string,
    parentAgentId: string,
    targets: Array<{
      id: string;
      providerSubagentId?: string | null;
      childAgentId?: string | null;
    }>,
  ): void {
    if (targets.length === 0) return;
    const managedIds = new Set<string>();
    const keys: string[] = [];
    for (const target of targets) {
      if (target.id) keys.push(subagentArchiveKey(hostId, parentAgentId, target.id));
      if (target.providerSubagentId) {
        keys.push(subagentArchiveKey(hostId, parentAgentId, target.providerSubagentId));
      }
      if (target.childAgentId) {
        keys.push(subagentArchiveKey(hostId, parentAgentId, target.childAgentId));
        if (state.hostDetails[hostId]?.agents.some((a) => a.agent_id === target.childAgentId)) {
          managedIds.add(target.childAgentId);
        }
      }
    }
    for (const cid of managedIds) {
      orbitActions.archiveThread(hostId, cid);
    }
    setState(
      (s) => {
        const nextArchived = { ...s.archivedSubagents };
        for (const k of keys) {
          nextArchived[k] = true;
        }
        return {
          ...s,
          archivedSubagents: nextArchived,
        };
      },
      true,
    );
    notifyToast(`已归档 ${targets.length} 个已完成子代理`);
  },

  openArtifactsTab(opts?: {
    hostId?: string;
    workspaceId?: string;
    artifactId?: string;
  }): void {
    const resolvedHostId = opts?.hostId || state.selectedHostId || "";
    const detail = resolvedHostId ? state.hostDetails[resolvedHostId] : null;
    const activeAgent =
      !state.isDraftNewSession && state.selectedAgentId
        ? detail?.agents.find((a) => a.agent_id === state.selectedAgentId) ?? null
        : null;
    const resolvedWorkspaceId = opts?.workspaceId ?? activeAgent?.workspace_id ?? "";
    const tabId = "tab:artifacts";
    const nonce = Date.now();

    setState((s) => {
      const exists = s.sidePanelTabs.some((t) => t.id === tabId);
      const nextTab: SidePanelTab = {
        id: tabId,
        kind: "artifacts",
        title: "Artifacts",
        hostId: resolvedHostId || undefined,
        workspaceId: resolvedWorkspaceId || undefined,
        artifactId: opts?.artifactId,
        focusNonce: nonce,
      };
      const nextTabs = exists
        ? s.sidePanelTabs.map((t) =>
            t.id === tabId
              ? {
                  ...t,
                  hostId: resolvedHostId || t.hostId,
                  workspaceId: resolvedWorkspaceId || t.workspaceId,
                  artifactId: opts?.artifactId ?? t.artifactId,
                  focusNonce: nonce,
                }
              : t,
          )
        : [...s.sidePanelTabs, nextTab];
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        terminalFocused: false,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });
    if (resolvedHostId) {
      void orbitIpc.listArtifacts(resolvedHostId, null, null).catch(() => {});
    }
  },

  openWorkspaceBoardTab(hostId: string, workspaceId: string, title?: string): void {
    const tabId = `tab:board:${hostId}::${workspaceId}`;
    const resolvedTitle = title ? `Board · ${title}` : "Workspace Board";
    setState((s) => {
      const exists = s.sidePanelTabs.some((t) => t.id === tabId);
      const nextTabs = exists
        ? s.sidePanelTabs.map((t) => (t.id === tabId ? { ...t, title: resolvedTitle } : t))
        : [
            ...s.sidePanelTabs,
            {
              id: tabId,
              kind: "board" as const,
              title: resolvedTitle,
              hostId,
              workspaceId,
            },
          ];
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        terminalFocused: false,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });
    void orbitActions.fetchWorkspaceBoardAndEvents(hostId, workspaceId);
  },

  openPeerSplitTab(hostId: string, peerAgentId: string, title?: string): void {
    const detail = state.hostDetails[hostId];
    const peer = detail?.agents.find((a) => a.agent_id === peerAgentId);
    const tabId = `tab:peer:${hostId}::${peerAgentId}`;
    const resolvedTitle =
      title ?? (peer?.handle ? `@${peer.handle}` : peer?.title ?? "Peer");
    setState((s) => {
      const exists = s.sidePanelTabs.some((t) => t.id === tabId);
      const nextTabs = exists
        ? s.sidePanelTabs.map((t) => (t.id === tabId ? { ...t, title: resolvedTitle } : t))
        : [
            ...s.sidePanelTabs,
            {
              id: tabId,
              kind: "peer" as const,
              title: resolvedTitle,
              hostId,
              peerAgentId,
              workspaceId: peer?.workspace_id,
              cwd: peer?.cwd,
            },
          ];
      const shouldYield = !s.sidePanelOpen && s.isTightForDualSidebars && s.sidebarOpen;
      return {
        ...s,
        sidePanelOpen: true,
        overviewPopoverOpen: false,
        sidePanelTabs: nextTabs,
        activeSidePanelTabId: tabId,
        terminalFocused: false,
        sidebarOpen: shouldYield ? false : s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: shouldYield ? true : s.sidebarAutoYieldedForSidePanel,
      };
    });
    void orbitIpc.openAgentTimeline(hostId, peerAgentId);
    void orbitActions.pullStageDelta(hostId, peerAgentId, true);
  },

  async fetchWorkspaceBoardAndEvents(hostId: string, workspaceId: string): Promise<void> {
    await Promise.all([
      orbitIpc.fetchWorkspaceBoard(hostId, workspaceId),
      orbitIpc.fetchWorkspaceEvents(hostId, workspaceId, 80),
    ]);
  },

  async updateWorkspaceBoard(
    hostId: string,
    workspaceId: string,
    content: string,
    section?: string | null,
  ): Promise<void> {
    await orbitIpc.updateWorkspaceBoard({
      hostId,
      workspaceId,
      content,
      section: section ?? null,
      updatedBy: "user",
    });
  },

  async updateWorkspaceGoal(hostId: string, workspaceId: string, goal: string): Promise<void> {
    await orbitIpc.updateWorkspaceGoal(hostId, workspaceId, goal);
    notifyToast("Mission goal updated");
  },

  async renameWorkspace(hostId: string, workspaceId: string, title: string): Promise<void> {
    const trimmed = title.trim();
    if (!trimmed) return;
    await orbitIpc.renameWorkspace(hostId, workspaceId, trimmed);
    orbitActions.refreshSidebarView();
    notifyToast(`Workspace 已重命名为 "${trimmed}"`);
  },

  async createMissionWorkspace(params: {
    hostId: string;
    sourceCwd: string;
    title: string;
    missionGoal?: string | null;
    isolation?: "local" | "worktree";
    worktreeBranch?: string | null;
  }): Promise<void> {
    const trimmed = params.title.trim();
    if (!trimmed) return;
    await orbitIpc.createMissionWorkspace({
      hostId: params.hostId,
      sourceCwd: params.sourceCwd,
      title: trimmed,
      missionGoal: params.missionGoal ?? null,
      isolation: params.isolation ?? "local",
      worktreeBranch: params.worktreeBranch ?? null,
    });
    orbitActions.refreshSidebarView();
    notifyToast(`已创建 Workspace「${trimmed}」`);
  },

  async archiveWorkspace(
    hostId: string,
    workspaceId: string,
    keepAgentsUnclassified = false,
  ): Promise<void> {
    await orbitIpc.archiveWorkspace(hostId, workspaceId, keepAgentsUnclassified);
    orbitActions.refreshSidebarView();
    notifyToast(
      keepAgentsUnclassified
        ? "已解散 Workspace，会话已移回「待归类」"
        : "已归档 Workspace",
    );
  },

  async moveAgentToWorkspace(
    hostId: string,
    agentId: string,
    targetWorkspaceId?: string | null,
    opts?: { newWorkspaceTitle?: string | null; makeUnclassified?: boolean },
  ): Promise<void> {
    await orbitIpc.moveAgentToWorkspace(hostId, agentId, targetWorkspaceId ?? null, opts);
    orbitActions.refreshSidebarView();
    notifyToast(
      opts?.makeUnclassified
        ? "已移回「待归类」"
        : targetWorkspaceId
          ? "已切换会话到目标 Workspace"
          : opts?.newWorkspaceTitle
            ? `已新建并移入「${opts.newWorkspaceTitle}」`
            : "已移入独立 Workspace",
    );
  },

  async organizeProjectWorkspaces(
    hostId: string,
    projectRoot: string,
    agentIds: string[] = [],
    autoApply = true,
  ): Promise<void> {
    await orbitIpc.organizeProjectWorkspaces(hostId, projectRoot, agentIds, autoApply);
    notifyToast(
      agentIds.length === 1 ? "AI 正在理解会话并归类…" : "AI 正在自动归类待归类会话…",
    );
  },

  async summarizeAgent(hostId: string, agentId: string): Promise<void> {
    await orbitIpc.summarizeAgent(hostId, agentId);
    notifyToast("AI 正在生成会话标签与摘要…");
  },

  async updateAgentIdentity(params: {
    hostId: string;
    agentId: string;
    handle?: string | null;
    title?: string | null;
    taskSummary?: string | null;
    lockHandle?: boolean;
    lockTitle?: boolean;
  }): Promise<void> {
    await orbitIpc.updateAgentIdentity(params);
    orbitActions.refreshSidebarView();
    if (params.handle) {
      notifyToast(`已更新会话标签为「${params.handle.trim().replace(/^@+/, "")}」`);
    } else if (params.taskSummary !== undefined) {
      notifyToast("已更新会话摘要");
    }
  },

  async mergeWorkspaces(
    hostId: string,
    sourceWorkspaceId: string,
    targetWorkspaceId: string,
  ): Promise<void> {
    await orbitIpc.mergeWorkspaces(hostId, sourceWorkspaceId, targetWorkspaceId);
    orbitActions.refreshSidebarView();
    notifyToast("Workspaces merged");
  },

  async dismissClusterSuggestion(hostId: string, agentId: string): Promise<void> {
    await orbitIpc.dismissClusterSuggestion(hostId, agentId);
  },

  async spawnWorkspacePeer(
    req: SpawnWorkspacePeerRequest,
    opts?: { openInSplit?: boolean },
  ): Promise<string> {
    const optimisticId = await orbitIpc.spawnWorkspacePeer(req);
    notifyToast(req.handle ? `Spawning @${req.handle}…` : "Spawning workspace peer…");
    if (opts?.openInSplit && optimisticId) {
      orbitActions.openPeerSplitTab(
        req.host_id,
        optimisticId,
        req.handle ? `@${req.handle}` : undefined,
      );
    }
    return optimisticId;
  },

  showToast(msg: string): void {
    notifyToast(msg);
  },

  // --- Zero-Server E2EE Cloud Sync Actions ---

  async refreshCloudSyncStatus(): Promise<CloudSyncStatusView | null> {
    try {
      const status = await orbitIpc.getCloudSyncStatus();
      setState((s) => ({ ...s, cloudSyncStatus: status }));
      return status;
    } catch {
      return null;
    }
  },

  async hydrateSyncedPreferencesFromCore(): Promise<void> {
    try {
      const prefs = await orbitIpc.cloudSyncGetPreferences();
      if (!prefs || Object.keys(prefs).length === 0) return;
      let nextTheme: ThemeMode | null = null;
      if (prefs.theme === "system" || prefs.theme === "light" || prefs.theme === "dark") {
        nextTheme = prefs.theme;
      }
      if (nextTheme) {
        applyDocumentTheme(resolveTheme(nextTheme));
      }
      setState(
        (s) => ({
          ...s,
          theme: nextTheme ?? s.theme,
          resolvedTheme: nextTheme ? resolveTheme(nextTheme) : s.resolvedTheme,
          enterToSend: typeof prefs.enterToSend === "boolean" ? prefs.enterToSend : s.enterToSend,
          notifyOnApproval:
            typeof prefs.notifyOnApproval === "boolean"
              ? prefs.notifyOnApproval
              : s.notifyOnApproval,
          notifyOnTurnComplete:
            typeof prefs.notifyOnTurnComplete === "boolean"
              ? prefs.notifyOnTurnComplete
              : s.notifyOnTurnComplete,
          coalesceExploration:
            typeof prefs.coalesceExploration === "boolean"
              ? prefs.coalesceExploration
              : s.coalesceExploration,
          worktreeBranchPrefix:
            typeof prefs.worktreeBranchPrefix === "string"
              ? prefs.worktreeBranchPrefix
              : s.worktreeBranchPrefix,
          draftUseWorktree:
            typeof prefs.draftUseWorktree === "boolean"
              ? prefs.draftUseWorktree
              : s.draftUseWorktree,
          draftMode:
            prefs.draftMode === "default" ||
            prefs.draftMode === "plan" ||
            prefs.draftMode === "accept_edits" ||
            prefs.draftMode === "full_access"
              ? prefs.draftMode
              : s.draftMode,
        }),
        true,
      );
    } catch {
      // Ignore preference hydration errors
    }
  },

  async startGithubCloudLogin(clientIdOverride?: string | null): Promise<void> {
    cloudDeviceFlowPollToken += 1;
    const pollToken = cloudDeviceFlowPollToken;
    setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
    try {
      const started = await orbitIpc.cloudSyncStartGithubDeviceFlow(clientIdOverride ?? null);
      if (pollToken !== cloudDeviceFlowPollToken) return;
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncDeviceFlow: started,
        cloudSyncError: null,
      }));

      // Poll GitHub Device Flow until authorized, cancelled, or expired
      let intervalMs = Math.max(2000, started.interval_secs * 1000);
      const deadline = Date.now() + started.expires_in_secs * 1000;
      while (pollToken === cloudDeviceFlowPollToken && Date.now() < deadline) {
        await new Promise((r) => window.setTimeout(r, intervalMs));
        if (pollToken !== cloudDeviceFlowPollToken) return;
        try {
          const polled = await orbitIpc.cloudSyncPollGithubDeviceFlow(
            started.device_code,
            clientIdOverride ?? null,
          );
          if (pollToken !== cloudDeviceFlowPollToken) return;
          if (polled.status === "pending") {
            continue;
          }
          if (polled.status === "slow_down") {
            intervalMs = Math.max(intervalMs + 5000, (polled.interval_secs ?? 10) * 1000);
            continue;
          }
          if (polled.status === "authorized") {
            const status = polled.sync_status ?? (await orbitIpc.getCloudSyncStatus());
            setState((s) => ({
              ...s,
              cloudSyncDeviceFlow: null,
              cloudSyncStatus: status,
              cloudSyncError: null,
            }));
            notifyToast(
              status.account_login
                ? `Signed in to GitHub as @${status.account_login}`
                : "Signed in to GitHub",
            );
            return;
          }
          setState((s) => ({
            ...s,
            cloudSyncDeviceFlow: null,
            cloudSyncError: polled.error ?? "GitHub authorization did not complete.",
          }));
          return;
        } catch (err) {
          if (pollToken !== cloudDeviceFlowPollToken) return;
          setState((s) => ({
            ...s,
            cloudSyncDeviceFlow: null,
            cloudSyncError: String(err),
          }));
          return;
        }
      }
      if (pollToken === cloudDeviceFlowPollToken) {
        setState((s) => ({
          ...s,
          cloudSyncDeviceFlow: null,
          cloudSyncError: "GitHub verification code expired — please start again.",
        }));
      }
    } catch (err) {
      if (pollToken !== cloudDeviceFlowPollToken) return;
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncDeviceFlow: null,
        cloudSyncError: String(err),
      }));
    }
  },

  cancelGithubCloudLogin(): void {
    cloudDeviceFlowPollToken += 1;
    setState((s) => ({
      ...s,
      cloudSyncBusy: false,
      cloudSyncDeviceFlow: null,
      cloudSyncError: null,
    }));
  },

  async loginCloudSyncWithToken(token: string): Promise<boolean> {
    setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
    try {
      const status = await orbitIpc.cloudSyncLoginWithToken(token.trim());
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncStatus: status,
        cloudSyncError: null,
        cloudSyncDeviceFlow: null,
      }));
      notifyToast(
        status.account_login
          ? `Signed in to GitHub as @${status.account_login}`
          : "Signed in to GitHub",
      );
      return true;
    } catch (err) {
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncError: String(err),
      }));
      return false;
    }
  },

  async unlockCloudSyncWithPassphrase(passphrase: string): Promise<boolean> {
    setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
    try {
      // Seed current local syncable prefs before first unlock so a brand-new vault captures them
      await orbitIpc.cloudSyncUpdatePreferences({
        theme: state.theme,
        enterToSend: state.enterToSend,
        notifyOnApproval: state.notifyOnApproval,
        notifyOnTurnComplete: state.notifyOnTurnComplete,
        coalesceExploration: state.coalesceExploration,
        worktreeBranchPrefix: state.worktreeBranchPrefix,
        draftUseWorktree: state.draftUseWorktree,
        draftMode: state.draftMode,
      });
      const result = await orbitIpc.cloudSyncUnlockWithPassphrase(passphrase);
      const status = await orbitIpc.getCloudSyncStatus();
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncStatus: status,
        cloudSyncLastResult: result,
        cloudSyncError: null,
      }));
      await orbitActions.reloadFleetAndHosts();
      await orbitActions.hydrateSyncedPreferencesFromCore();
      notifyToast(formatCloudSyncToast(result));
      return true;
    } catch (err) {
      const status = await orbitIpc.getCloudSyncStatus().catch(() => state.cloudSyncStatus);
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncStatus: status,
        cloudSyncError: String(err),
      }));
      return false;
    }
  },

  async syncCloudNow(): Promise<boolean> {
    setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
    try {
      const result = await orbitIpc.cloudSyncNow();
      const status = await orbitIpc.getCloudSyncStatus();
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncStatus: status,
        cloudSyncLastResult: result,
        cloudSyncError: null,
      }));
      await orbitActions.reloadFleetAndHosts();
      await orbitActions.hydrateSyncedPreferencesFromCore();
      notifyToast(formatCloudSyncToast(result));
      return true;
    } catch (err) {
      const status = await orbitIpc.getCloudSyncStatus().catch(() => state.cloudSyncStatus);
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncStatus: status,
        cloudSyncError: String(err),
      }));
      return false;
    }
  },

  async setCloudAutoSync(enabled: boolean): Promise<void> {
    try {
      const status = await orbitIpc.cloudSyncSetAutoSync(enabled);
      setState((s) => ({ ...s, cloudSyncStatus: status }));
    } catch (err) {
      notifyToast(`Failed to update auto-sync: ${String(err)}`);
    }
  },

  async setCloudGithubClientId(clientId: string | null): Promise<void> {
    try {
      const status = await orbitIpc.cloudSyncSetGithubClientId(clientId);
      setState((s) => ({ ...s, cloudSyncStatus: status, cloudSyncError: null }));
    } catch (err) {
      setState((s) => ({ ...s, cloudSyncError: String(err) }));
    }
  },

  async logoutCloudSync(clearLocalHosts = false): Promise<void> {
    cloudDeviceFlowPollToken += 1;
    setState((s) => ({ ...s, cloudSyncBusy: true, cloudSyncError: null }));
    try {
      const status = await orbitIpc.cloudSyncLogout(clearLocalHosts);
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncStatus: status,
        cloudSyncDeviceFlow: null,
        cloudSyncLastResult: null,
        cloudSyncError: null,
      }));
      if (clearLocalHosts) {
        await orbitActions.reloadFleetAndHosts();
      }
      notifyToast("Signed out of Cloud Sync");
    } catch (err) {
      setState((s) => ({
        ...s,
        cloudSyncBusy: false,
        cloudSyncError: String(err),
      }));
    }
  },

  getSnapshot(): OrbitUiState {
    return state;
  },
};

let cloudDeviceFlowPollToken = 0;

function formatCloudSyncToast(res: CloudSyncResultView): string {
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

async function pushCloudPrefPatch(patch: Record<string, unknown>): Promise<void> {
  if (state.cloudSyncStatus?.state !== "ready") return;
  await orbitIpc.cloudSyncUpdatePreferences(patch);
}

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

// Re-attach Rust event subscription immediately after a Vite HMR store reload so
// preserved state stays live without waiting for React to remount `<App />`.
if (hmrData?.wasInitialized) {
  initialized = true;
  queueMicrotask(() => {
    if (disposed) return;
    void subscribeCoreEvents();
    void orbitActions.reloadFleetAndHosts(!state.fleetHydrated).catch(() => undefined);
    orbitActions.refreshSidebarView();
  });
}

if (viteHot) {
  viteHot.dispose((data) => {
    draftPersistence.flushNow();
    data.draftUpdatedAtByScope = draftPersistence.exportUpdatedAtByScope();
    draftPersistence.dispose();

    disposed = true;
    data.state = state;
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

function subscribeStore(onStoreChange: () => void): () => void {
  listeners.add(onStoreChange);
  ensureStoreInitialized();
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

function subscribeDraftPresence(onPresenceChange: () => void): () => void {
  draftPresenceListeners.add(onPresenceChange);
  return () => {
    draftPresenceListeners.delete(onPresenceChange);
  };
}

function noopSubscribe(): () => void {
  return () => {};
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

/**
 * Ultra-lightweight boolean subscription for sidebar thread rows:
 * Fires ONLY when a scope transitions between empty and non-empty unsent draft (never per-keystroke),
 * and skips subscription completely while a row is `selected` (`enabled = false`).
 */
export function useHasThreadDraft(scopeKey: string, enabled = true): boolean {
  return useSyncExternalStore(
    enabled ? subscribeDraftPresence : noopSubscribe,
    () => (enabled ? hasNonEmptyScopeDraft(state, scopeKey) : false),
  );
}
