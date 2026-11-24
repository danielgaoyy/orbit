import type { ComposerInlineDirective } from "@/lib/slash-commands";
import type {
  HostSettingsTab,
  SettingsCoreSectionId,
  SettingsSection,
  SettingsSectionTarget,
  SettingsSubTabId,
} from "@/components/settings/settings-nav";
import type { OrbitLocale } from "@/lib/i18n";
import type { RichMediaKind } from "@/lib/media-utils";
import type {
  AgentStageView,
  BrowserElementRef,
  BrowserPolicyConfig,
  BrowserSessionSummary,
  CloudSyncResultView,
  CloudSyncStatusView,
  EventTriggerDelivery,
  EventTriggerRecord,
  ExternalEditorInfo,
  ExternalEditorKind,
  ExternalMcpServerRecord,
  ExtractedCodeBlock,
  FleetOverviewView,
  FleetSidebarView,
  GitHubDeviceFlowStartView,
  GitReposInspectionView,
  GitReviewView,
  GuardianAuditEntry,
  GuardianPolicyConfig,
  HookExecutionRecord,
  HostDetailView,
  ImChannelRecord,
  ImThreadBinding,
  LifecycleHookRule,
  LspDiagnostic,
  LspServerStatus,
  PathRulesSnapshotView,
  PromptBadge,
  ProviderKind,
  PushDeviceRecord,
  PushPreferences,
  ReasoningEffort,
  ReleaseUpdateInfo,
  RichBlockView,
  SessionMode,
  SessionPullRequestView,
  SlashCommandView,
  SlashDirective,
  SshHostItem,
  WorkspaceFileConflict,
  WorkspaceFileView,
  WorktreeHandoffDirection,
  WorktreeHandoffMode,
} from "@/lib/orbit-ipc";

export type {
  HostSettingsTab,
  SettingsCoreSectionId,
  SettingsSection,
  SettingsSectionTarget,
  SettingsSubTabId,
  SlashDirective,
};

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

export interface UndoArchiveEntry {
  hostId: string;
  agentId: string;
  title: string;
  draft?: string;
  slashDirective?: ComposerInlineDirective | null;
  inlineMentions?: InlineMentionTarget[];
  contexts?: ContextMentionItem[];
  pastes?: PastedAttachment[];
  quotes?: DiffQuoteSnippet[];
  sideQuestionDraft?: string;
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
    }
  | {
      token: string;
      kind: "folder";
      path: string;
      fileCount: number;
    }
  | {
      token: string;
      kind: "pr";
      number: number;
      title: string;
      url?: string | null;
      state?: string | null;
      headRef?: string | null;
      baseRef?: string | null;
      summary?: string | null;
    }
  | {
      token: string;
      kind: "web";
      url: string;
      label: string;
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
    }
  | {
      id: string;
      kind: "skill";
      slug: string;
      label: string;
      description?: string;
    }
  | {
      id: string;
      kind: "dom_element";
      selector: string;
      tag: string;
      textPreview: string;
      attributes: Record<string, string>;
      computedBox?: { x: number; y: number; width: number; height: number } | null;
      outerHtmlExcerpt: string;
      pageUrl: string;
    }
  | {
      id: string;
      kind: "folder";
      path: string;
      fileCount: number;
      treeSummary?: string | null;
    }
  | {
      id: string;
      kind: "pr";
      number: number;
      title: string;
      url?: string | null;
      state?: string | null;
      headRef?: string | null;
      baseRef?: string | null;
      summary?: string | null;
    }
  | {
      id: string;
      kind: "web";
      url: string;
      label: string;
    };

export type TerminalSplitDirection = "horizontal" | "vertical";

export interface TerminalSplitGroup {
  id: string;
  hostId: string;
  direction: TerminalSplitDirection;
  paneKeys: string[];
  ratios: number[];
}

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
  sideQuestionEntryId?: string;
}

export interface FileViewCacheEntry {
  status: "loading" | "ready" | "error";
  data?: WorkspaceFileView;
  error?: string;
  /** Marked stale after a workspace Git change; rendered immediately via stale-while-revalidate. */
  stale?: boolean;
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

export interface PendingUnsavedFileAction {
  fileKey: string;
  label: string;
  action:
    | { kind: "close_tab"; tabId: string }
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

export type OverviewSectionId =
  | "pull_requests"
  | "subagents"
  | "side_questions"
  | "goals"
  | "tasks"
  | "artifacts"
  | "links"
  | "touched_files";

export type ReviewScope = "working_tree" | "session_turns";
export type DiffLayoutMode = "unified" | "split";
export type ThemeMode = "system" | "light" | "dark";
export type ResolvedTheme = "light" | "dark";
export type ActiveViewMode = "workspace" | "settings";
export type WorkspacePanelMode = "conversation" | "automations";
export type TerminalCursorStyle = "bar" | "block" | "underline";
export type TerminalThemeMode = "harmonized" | "high_contrast";
export type SidebarProjectSort = "recent" | "alphabetical" | "manual";
export type SidebarThreadFilter = "all" | "active_unread";

export interface ArchivedThreadEntry {
  key: string;
  hostId: string;
  agentId: string;
  archivedAtMs: number;
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

export interface ToastAction {
  label: string;
  url?: string;
  onClick?: () => void;
}

export interface ComposerFocusOptions {
  onlyIfIdle?: boolean;
}

export type WindowedAgentStageView = AgentStageView & {
  window_start_turn_index?: number;
  total_turn_count?: number;
  has_older_turns?: boolean;
  has_older_history?: boolean;
  oldest_loaded_seq?: number;
};

export interface StageRunningSubagentRef {
  child_agent_id?: string | null;
  provider_subagent_id?: string | null;
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
  slashDirectives: Record<string, ComposerInlineDirective | null>;
  inlineMentions: Record<string, InlineMentionTarget[]>;
  contextItems: Record<string, ContextMentionItem[]>;
  pastedAttachments: Record<string, PastedAttachment[]>;
  diffQuotes: Record<string, DiffQuoteSnippet[]>;
  queuedPrompts: Record<string, QueuedPromptItem[]>;
  workspaceFilesCache: Record<string, string[]>;
  workspaceEmptyDirsCache: Record<string, string[]>;
  promptHistory: string[];

  // Incremental stage projections keyed by `hostId::agentId` + Rust sidebar tree + slash commands
  stages: Record<string, WindowedAgentStageView>;
  sidebarView: FleetSidebarView | null;
  stageRunningSubagentMeta: Record<string, readonly StageRunningSubagentRef[]>;
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
  sessionCreatedPrs: Record<string, SessionPullRequestView[]>;
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
  hiddenProjectKeys: Record<string, boolean>;
  threadCustomTitles: Record<string, string>;
  archivedThreads: Record<string, ArchivedThreadEntry>;
  archivedSubagents: Record<string, true>;
  threadManualOrder: Record<string, string[]>;
  sidebarProjectSort: SidebarProjectSort;
  sidebarThreadFilter: SidebarThreadFilter;
  sidebarFilterFocusNonce: number;
  stageFindFocusNonce: number;

  // Persistent Multi-Slot PTY Sessions (rendered inside Side Panel Terminal tab)
  terminalTabs: TerminalTabSession[];
  terminalSplits: TerminalSplitGroup[];
  activeTerminalTabKey: string | null;
  terminalFocused: boolean;

  // Modals, Views & Overlays
  activeView: ActiveViewMode;
  workspacePanel: WorkspacePanelMode;
  settingsSection: SettingsSection;
  commandPaletteOpen: boolean;
  onboardingOpen: boolean;
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
  toastAction: ToastAction | null;
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
  pathRulesByScope: Record<string, PathRulesSnapshotView>;
  cloudSyncBusy: boolean;
  cloudSyncError: string | null;
  cloudSyncDeviceFlow: GitHubDeviceFlowStartView | null;
  cloudSyncLastResult: CloudSyncResultView | null;

  // Pure-Rust LSP Language Servers & File Diagnostics
  lspServersByWorkspace: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      servers: LspServerStatus[];
      error?: string | null;
    }
  >;
  lspDiagnosticsByFile: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      diagnostics: LspDiagnostic[];
    }
  >;
  // Deterministic Lifecycle Hooks (UserGlobal + ProjectOrbit)
  lifecycleHooksByScope: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      hooks: LifecycleHookRule[];
      recentExecutions: HookExecutionRecord[];
      projectHooksTrusted: boolean;
      error?: string | null;
    }
  >;
  browserByHost: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      sessions: BrowserSessionSummary[];
      policy: BrowserPolicyConfig | null;
      chromeBinary: string | null;
      activeSessionId: string | null;
      elementsBySession: Record<string, BrowserElementRef[]>;
      textExcerptBySession: Record<string, string | null>;
      busy: boolean;
      error?: string | null;
    }
  >;
  guardianByHost: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      config: GuardianPolicyConfig | null;
      recentAudits: GuardianAuditEntry[];
      saving?: boolean;
      error?: string | null;
    }
  >;
  // External Event Triggers (GitHub, Slack, Linear, Generic Webhook + GitHub PR Polling)
  eventTriggersByHost: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      triggers: EventTriggerRecord[];
      recentDeliveries: EventTriggerDelivery[];
      baseWebhookUrl: string;
      error?: string | null;
    }
  >;
  imChannelsByHost: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      channels: ImChannelRecord[];
      bindings: ImThreadBinding[];
      baseWebhookUrl: string;
      saving?: boolean;
      error?: string | null;
    }
  >;
  pushByHost: Record<
    string,
    {
      status: "idle" | "loading" | "ready" | "error";
      devices: PushDeviceRecord[];
      preferences: PushPreferences | null;
      saving?: boolean;
      testing?: boolean;
      error?: string | null;
    }
  >;

  // User ergonomic & ADE preferences
  locale: OrbitLocale;
  theme: ThemeMode;
  resolvedTheme: ResolvedTheme;
  enterToSend: boolean;
  notifyOnApproval: boolean;
  notifyOnTurnComplete: boolean;
  notifyOnAgentError: boolean;
  notifyOnGoalFinished: boolean;
  notifySoundEnabled: boolean;
  notifyQuietHoursEnabled: boolean;
  notifyQuietStartHour: number;
  notifyQuietEndHour: number;
  coalesceExploration: boolean;
  worktreeBranchPrefix: string;
  preferredEditor: ExternalEditorKind | null;
  terminalFontSize: number;
  terminalCursorStyle: TerminalCursorStyle;
  terminalScrollback: number;
  terminalThemeMode: TerminalThemeMode;
}

export interface PersistedPrefs {
  locale?: OrbitLocale;
  theme?: ThemeMode;
  enterToSend?: boolean;
  notifyOnApproval?: boolean;
  notifyOnTurnComplete?: boolean;
  notifyOnAgentError?: boolean;
  notifyOnGoalFinished?: boolean;
  notifySoundEnabled?: boolean;
  notifyQuietHoursEnabled?: boolean;
  notifyQuietStartHour?: number;
  notifyQuietEndHour?: number;
  coalesceExploration?: boolean;
  worktreeBranchPrefix?: string;
  preferredEditor?: ExternalEditorKind | null;
  terminalFontSize?: number;
  terminalCursorStyle?: TerminalCursorStyle;
  terminalScrollback?: number;
  terminalThemeMode?: TerminalThemeMode;
  sidebarOpen?: boolean;
  sidebarWidth?: number;
  hiddenProjectKeys?: Record<string, boolean>;
  pinnedThreadKeys?: string[];
  threadCustomTitles?: Record<string, string>;
  archivedThreads?: Record<string, ArchivedThreadEntry>;
  archivedSubagents?: Record<string, true>;
  collapsedProjectIds?: Record<string, boolean>;
  collapsedWorkspaceIds?: Record<string, boolean>;
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

export interface StagePullGate {
  inFlight: boolean;
  queued: boolean;
  forceFull: boolean;
}

export interface SimpleSyncGate {
  inFlight: boolean;
  queued: boolean;
  rafId: number | null;
}

export interface AutoGitRefreshGate {
  timerId: number | null;
  timerDueAt: number;
  trailingTimerId: number | null;
  inFlight: boolean;
  queued: boolean;
  lastStartedAt: number;
  lastFocusTriggerAt: number;
  cwd: string;
}

export interface TerminalStreamChannel {
  chunks: Uint8Array[];
  subs: Set<(bytes: Uint8Array) => void>;
}

export interface PendingDraftCreateState {
  hostId: string;
  optimisticAgentId: string | null;
  startedAt: number;
}

export interface OrbitStoreHmrData {
  state?: OrbitUiState;
  terminalChannels?: Map<string, TerminalStreamChannel>;
  wasInitialized?: boolean;
  draftUpdatedAtByScope?: Record<string, number>;
}
