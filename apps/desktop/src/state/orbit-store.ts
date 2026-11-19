import { useSyncExternalStore } from "react";
import {
  getCatalogForProvider,
  resolveActiveModelSpec,
  resolveSupportedEffortForModel,
} from "@/lib/model-catalog";
import {
  type UnifiedProjectEntry,
  buildUnifiedProjects,
  isLocalHostCard,
  matchesProjectDirectory,
  projectPathScopeKey,
} from "@/lib/projects";
import {
  type AgentCardView,
  type AgentTimelineDeltaView,
  type CoreNotification,
  type ExternalEditorInfo,
  type ExternalEditorKind,
  type FleetOverviewView,
  type GitReviewView,
  type HostDetailView,
  type PromptBadge,
  type ProviderKind,
  type ReasoningEffort,
  type SessionMode,
  type SshHostItem,
  type TimelineItemView,
  orbitIpc,
  timelineItemLogicalKey,
} from "@/lib/orbit-ipc";

export type SlashDirective = "plan" | "review" | "compact";

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
  | { id: string; kind: "file"; path: string }
  | { id: string; kind: "git_diff"; staged: boolean; label: string }
  | { id: string; kind: "terminal_output"; label: string; lineCount: number; content: string }
  | {
      id: string;
      kind: "image";
      filename: string;
      path: string;
      previewUrl: string;
      sizeBytes: number;
    };

export interface CachedTimelineEntry {
  latestSeq: number;
  agent: AgentCardView | null;
  items: TimelineItemView[];
}

export interface TerminalTabSession {
  key: string;
  hostId: string;
  terminalId: string | null;
  slot: number;
  cwd: string;
  title: string;
}

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

export interface OrbitUiState {
  fleet: FleetOverviewView;
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

  // Incremental timelines keyed by `hostId::agentId`
  timelines: Record<string, CachedTimelineEntry>;

  // Git & Review Inspector
  inspectorOpen: boolean;
  inspectorWidth: number;
  reviewScope: ReviewScope;
  diffStaged: boolean;
  diffLayout: DiffLayoutMode;
  selectedDiffPath: string | null;
  gitReviews: Record<string, GitReviewView>;

  // Sidebar & Layout
  sidebarOpen: boolean;
  sidebarDesktopOpenPref: boolean;
  isCompactViewport: boolean;
  sidebarWidth: number;
  collapsedHostIds: Record<string, boolean>;
  collapsedProjectIds: Record<string, boolean>;
  hiddenProjectKeys: Record<string, boolean>;
  pinnedThreadKeys: string[];
  threadCustomTitles: Record<string, string>;
  archivedThreads: Record<string, ArchivedThreadEntry>;
  seenAgentSeqs: Record<string, number>;
  sidebarProjectSort: SidebarProjectSort;
  sidebarThreadFilter: SidebarThreadFilter;

  // Persistent Multi-Slot Terminal Dock
  terminalOpen: boolean;
  terminalHeight: number;
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
  terminalHeight?: number;
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
      pinnedThreadKeys: snap.pinnedThreadKeys,
      threadCustomTitles: snap.threadCustomTitles,
      archivedThreads: snap.archivedThreads,
      seenAgentSeqs: snap.seenAgentSeqs,
      sidebarProjectSort: snap.sidebarProjectSort,
      sidebarThreadFilter: snap.sidebarThreadFilter,
      inspectorWidth: snap.inspectorWidth,
      terminalHeight: snap.terminalHeight,
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

const LEGACY_RETIRED_MODEL_SLUGS = new Set([
  "gpt-5.4",
  "gpt-5.3-codex",
  "gpt-5.2-codex",
  "gpt-5.4-mini",
  "gemini-3.1-pro",
  "gemini-3-flash",
  "opusplan",
  "opus",
  "sonnet",
  "haiku",
]);

function sanitizeSavedModel(slug: string | null | undefined): string | null {
  if (!slug || LEGACY_RETIRED_MODEL_SLUGS.has(slug)) {
    return null;
  }
  return slug;
}

const COMPACT_VIEWPORT_COLLAPSE_PX = 800;
const COMPACT_VIEWPORT_EXPAND_PX = 824;

const initialDesktopSidebarOpenPref = initialPrefs.sidebarOpen ?? true;
const initialIsCompactViewport =
  typeof window !== "undefined" ? window.innerWidth < COMPACT_VIEWPORT_COLLAPSE_PX : false;

let state: OrbitUiState = {
  fleet: { revision: 0, total_pending_approvals: 0, hosts: [], projects: [] },
  hostDetails: {},
  selectedHostId: null,
  selectedAgentId: null,
  selectedWorkspaceCwd: "",
  isDraftNewSession: false,
  sessionScrollNonce: 0,

  draftProvider: initialPrefs.draftProvider ?? "codex",
  draftModels: {
    claude_code: sanitizeSavedModel(initialPrefs.draftModels?.claude_code),
    codex: sanitizeSavedModel(initialPrefs.draftModels?.codex),
    antigravity: sanitizeSavedModel(initialPrefs.draftModels?.antigravity),
  },
  draftEffort: initialPrefs.draftEffort ?? "medium",
  draftMode:
    initialPrefs.draftMode && initialPrefs.draftMode !== "plan"
      ? initialPrefs.draftMode
      : "accept_edits",
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

  timelines: {},

  inspectorOpen: false,
  inspectorWidth: initialPrefs.inspectorWidth ?? 440,
  reviewScope: "working_tree",
  diffStaged: false,
  diffLayout: initialPrefs.diffLayout ?? "unified",
  selectedDiffPath: null,
  gitReviews: {},

  sidebarOpen: initialIsCompactViewport ? false : initialDesktopSidebarOpenPref,
  sidebarDesktopOpenPref: initialDesktopSidebarOpenPref,
  isCompactViewport: initialIsCompactViewport,
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
  pinnedThreadKeys: initialPrefs.pinnedThreadKeys ?? [],
  threadCustomTitles: initialPrefs.threadCustomTitles ?? {},
  archivedThreads: initialPrefs.archivedThreads ?? {},
  seenAgentSeqs: initialPrefs.seenAgentSeqs ?? {},
  sidebarProjectSort: initialPrefs.sidebarProjectSort ?? "recent",
  sidebarThreadFilter: initialPrefs.sidebarThreadFilter ?? "all",

  terminalOpen: false,
  terminalHeight: initialPrefs.terminalHeight ?? 260,
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

// Coalesced single-flight gates so daemon bursts never pile up overlapping IPC roundtrips or duplicate renders
interface TimelinePullGate {
  inFlight: boolean;
  queued: boolean;
  forceFull: boolean;
  rafId: number | null;
}
interface SimpleSyncGate {
  inFlight: boolean;
  queued: boolean;
  rafId: number | null;
}
const timelinePullGates = new Map<string, TimelinePullGate>();
const hostSyncGates = new Map<string, SimpleSyncGate>();
const fleetSyncGate: SimpleSyncGate = { inFlight: false, queued: false, rafId: null };

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

function mergeTimelineDelta(
  existing: CachedTimelineEntry | undefined,
  delta: AgentTimelineDeltaView,
  fullReset: boolean,
): CachedTimelineEntry {
  const baseItems = fullReset || !existing ? [] : [...existing.items];
  const indexByKey = new Map<string, number>();
  baseItems.forEach((it, idx) => {
    indexByKey.set(timelineItemLogicalKey(it), idx);
  });

  // Apply newly resolved permission/question states onto historical items in O(n) only when present
  const resolvedPermIds = Object.keys(delta.resolved_permissions);
  const resolvedQuestIds = Object.keys(delta.resolved_questions);
  if (resolvedPermIds.length > 0 || resolvedQuestIds.length > 0) {
    for (let i = 0; i < baseItems.length; i += 1) {
      const it = baseItems[i];
      if (it.item_type === "permission_request" && delta.resolved_permissions[it.request_id]) {
        baseItems[i] = {
          ...it,
          resolved_decision: delta.resolved_permissions[it.request_id],
        };
      } else if (it.item_type === "question_request" && delta.resolved_questions[it.request_id]) {
        baseItems[i] = {
          ...it,
          resolved_answers: delta.resolved_questions[it.request_id],
        };
      }
    }
  }

  for (const incoming of delta.upserted_items) {
    const key = timelineItemLogicalKey(incoming);
    const existingIdx = indexByKey.get(key);
    let canUpdateInPlace = existingIdx !== undefined;

    if (
      canUpdateInPlace &&
      existingIdx !== undefined &&
      incoming.item_type !== "user_prompt" &&
      incoming.seq > 0
    ) {
      // Never allow an assistant/tool update to overwrite an item belonging to an earlier turn
      for (let j = existingIdx + 1; j < baseItems.length; j += 1) {
        const between = baseItems[j];
        if (
          between.item_type === "user_prompt" &&
          between.seq > 0 &&
          between.seq < incoming.seq
        ) {
          canUpdateInPlace = false;
          break;
        }
      }
    }

    if (canUpdateInPlace && existingIdx !== undefined) {
      baseItems[existingIdx] = incoming;
    } else {
      indexByKey.set(key, baseItems.length);
      baseItems.push(incoming);
    }
  }

  return {
    latestSeq: Math.max(existing?.latestSeq ?? 0, delta.latest_seq),
    agent: delta.agent ?? existing?.agent ?? null,
    items: baseItems,
  };
}

let initialized = false;
let toastTimer: ReturnType<typeof setTimeout> | null = null;
const notifiedApprovalAgents = new Set<string>();

function notifyToast(msg: string): void {
  if (toastTimer) clearTimeout(toastTimer);
  setState((s) => ({ ...s, toastNotice: msg }));
  toastTimer = setTimeout(() => {
    setState((s) => ({ ...s, toastNotice: null }));
  }, 3000);
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

function parseLeadingSlashDirective(rawText: string): {
  directive: SlashDirective | null;
  rest: string;
} {
  const m = /^\/(plan|review|compact)(?:\s+([\s\S]*))?$/i.exec(rawText.trim());
  if (!m) {
    return { directive: null, rest: rawText.trim() };
  }
  return {
    directive: m[1].toLowerCase() as SlashDirective,
    rest: (m[2] ?? "").trim(),
  };
}

function buildHiddenWorkflowInstruction(directive: SlashDirective): string {
  switch (directive) {
    case "plan":
      return [
        '<WORKFLOW_INSTRUCTION command="/plan">',
        "You are invoked with the /plan workflow.",
        "First inspect and verify the relevant codebase files, architecture, and constraints.",
        "Clarify any critical ambiguities if needed, then present a concise, structured, step-by-step implementation plan.",
        "Do NOT modify files or run mutating commands until the user approves the plan.",
        "</WORKFLOW_INSTRUCTION>",
      ].join("\n");
    case "review":
      return [
        '<WORKFLOW_INSTRUCTION command="/review">',
        "You are invoked with the /review workflow.",
        "Perform a thorough engineering code review of the attached changes.",
        "Focus on correctness bugs, regressions, edge cases, concurrency/state hazards, security, and adherence to project conventions.",
        "Cite exact file paths and line numbers, prioritize findings by severity, and do NOT modify files unless asked.",
        "</WORKFLOW_INSTRUCTION>",
      ].join("\n");
    case "compact":
      return [
        '<WORKFLOW_INSTRUCTION command="/compact">',
        "Summarize and compact our conversation context into a concise, high-signal checkpoint:",
        "1. Core goal & user preferences",
        "2. Key architectural decisions & completed changes (with file paths)",
        "3. Open issues & immediate next steps",
        "</WORKFLOW_INSTRUCTION>",
      ].join("\n");
  }
}

async function drainPromptQueueIfIdle(hostId: string, detail: HostDetailView): Promise<void> {
  for (const ag of detail.agents) {
    const key = `${hostId}::${ag.agent_id}`;
    const queue = state.queuedPrompts[key];
    if (queue && queue.length > 0 && !ag.is_running && !ag.is_awaiting_permission) {
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

export const orbitActions = {
  async init(): Promise<void> {
    if (initialized) return;
    initialized = true;

    const [editors, sshList] = await Promise.all([
      orbitIpc.listExternalEditors(),
      orbitIpc.listSshHosts(),
    ]);
    setState((s) => ({ ...s, externalEditors: editors, sshHosts: sshList }));

    await orbitActions.reloadFleetAndHosts(true);

    await orbitIpc.subscribe((ev: CoreNotification) => {
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
        // Only stream live timeline deltas for the actively viewed session;
        // background sessions catch up cleanly from `latestSeq` when selected.
        if (
          !state.isDraftNewSession &&
          ev.host_id === state.selectedHostId &&
          ev.agent_id === state.selectedAgentId
        ) {
          const tlKey = `${ev.host_id}::${ev.agent_id}`;
          let gate = timelinePullGates.get(tlKey);
          if (!gate) {
            gate = { inFlight: false, queued: false, forceFull: false, rafId: null };
            timelinePullGates.set(tlKey, gate);
          }
          if (gate.inFlight) {
            gate.queued = true;
          } else if (gate.rafId === null) {
            const hid = ev.host_id;
            const aid = ev.agent_id;
            if (typeof window !== "undefined" && typeof window.requestAnimationFrame === "function") {
              gate.rafId = window.requestAnimationFrame(() => {
                const curGate = timelinePullGates.get(tlKey);
                if (curGate) curGate.rafId = null;
                void orbitActions.pullTimelineDelta(hid, aid);
              });
            } else {
              void orbitActions.pullTimelineDelta(hid, aid);
            }
          }
        }
      } else if (ev.kind === "git_changed") {
        if (ev.host_id === state.selectedHostId && ev.cwd === state.selectedWorkspaceCwd) {
          void orbitActions.loadGitReviewSnapshot(ev.host_id, ev.cwd, state.selectedDiffPath);
          void orbitActions.ensureWorkspaceFiles(ev.host_id, ev.cwd, true);
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
  },

  async reloadFleetAndHosts(initialBootstrap = false): Promise<void> {
    const snap = await orbitIpc.getFleetOverview();
    const targetHostId = state.selectedHostId ?? snap.hosts[0]?.host_id ?? null;

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
      const activeHostId =
        targetHostId && snap.hosts.some((h) => h.host_id === targetHostId)
          ? targetHostId
          : (snap.hosts[0]?.host_id ?? null);
      const activeDetail = activeHostId ? nextDetails[activeHostId] : undefined;

      let nextAgentId = s.selectedAgentId;
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

      const nextSeenSeqs: Record<string, number> = { ...s.seenAgentSeqs };
      for (const [hid, det] of Object.entries(nextDetails)) {
        for (const ag of det.agents) {
          const k = threadScopeKey(hid, ag.agent_id);
          if (nextSeenSeqs[k] === undefined) {
            nextSeenSeqs[k] = ag.latest_seq;
          }
          if (!nextDraft && hid === activeHostId && ag.agent_id === nextAgentId) {
            nextSeenSeqs[k] = Math.max(nextSeenSeqs[k] ?? 0, ag.latest_seq);
          }
        }
      }

      const activeAgent = activeDetail?.agents.find((a) => a.agent_id === nextAgentId);
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
        hostDetails: nextDetails,
        selectedHostId: activeHostId,
        selectedAgentId: nextAgentId,
        selectedWorkspaceCwd: nextCwd,
        isDraftNewSession: nextDraft,
        draftProvider: nextDraftProvider,
        seenAgentSeqs: nextSeenSeqs,
      };
    }, true);

    syncAttentionIndicators(state.fleet, state.hostDetails);

    if (state.selectedHostId) {
      if (initialBootstrap) {
        void orbitIpc.refreshQuotas(state.selectedHostId);
      }
      if (state.selectedAgentId && !state.isDraftNewSession) {
        const tlKey = `${state.selectedHostId}::${state.selectedAgentId}`;
        if (initialBootstrap || !state.timelines[tlKey]) {
          void orbitIpc.openAgentTimeline(state.selectedHostId, state.selectedAgentId);
          void orbitActions.pullTimelineDelta(state.selectedHostId, state.selectedAgentId, true);
        }
      }
    }
  },

  async reloadHostDetail(hostId: string, includeFleetOverview = false): Promise<void> {
    const [detail, overview] = includeFleetOverview
      ? await Promise.all([orbitIpc.getHostDetail(hostId), orbitIpc.getFleetOverview()])
      : [await orbitIpc.getHostDetail(hostId), null];
    if (!detail) return;

    const hadPrevDetail = Boolean(state.hostDetails[hostId]);
    const prevAgents = state.hostDetails[hostId]?.agents ?? [];
    const newestCreated =
      state.selectedHostId === hostId && state.isDraftNewSession
        ? detail.agents.find((a) => !prevAgents.some((p) => p.agent_id === a.agent_id))
        : undefined;

    if (
      state.notifyOnTurnComplete &&
      typeof document !== "undefined" &&
      document.hidden &&
      "Notification" in window &&
      Notification.permission === "granted"
    ) {
      for (const nextAg of detail.agents) {
        const prevAg = prevAgents.find((p) => p.agent_id === nextAg.agent_id);
        if (prevAg?.is_running && !nextAg.is_running && !nextAg.is_awaiting_permission) {
          new Notification(`Orbit · ${detail.host.display_name}`, {
            body: `Completed: ${nextAg.title}`,
          });
        }
      }
    }

    let seenMutated = false;
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

      let nextSeenSeqs = s.seenAgentSeqs;
      for (const ag of detail.agents) {
        const k = threadScopeKey(hostId, ag.agent_id);
        const isCurrentlyViewing =
          s.workspacePanel === "conversation" &&
          !nextDraft &&
          s.selectedHostId === hostId &&
          nextAgentId === ag.agent_id;
        if (isCurrentlyViewing) {
          const prevSeen = nextSeenSeqs[k] ?? 0;
          if (ag.latest_seq > prevSeen || nextSeenSeqs[k] === undefined) {
            if (!seenMutated) {
              nextSeenSeqs = { ...nextSeenSeqs };
              seenMutated = true;
            }
            nextSeenSeqs[k] = Math.max(prevSeen, ag.latest_seq);
          }
        } else if (nextSeenSeqs[k] === undefined) {
          if (!seenMutated) {
            nextSeenSeqs = { ...nextSeenSeqs };
            seenMutated = true;
          }
          // Seed pre-existing historical agents as seen on initial hydration; mark newly spawned background agents as unseen (`0`)
          nextSeenSeqs[k] = hadPrevDetail ? 0 : ag.latest_seq;
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
        seenAgentSeqs: nextSeenSeqs,
      };
    }, seenMutated);

    syncAttentionIndicators(state.fleet, state.hostDetails);
    void drainPromptQueueIfIdle(hostId, detail);

    if (newestCreated) {
      void orbitIpc.openAgentTimeline(hostId, newestCreated.agent_id);
      void orbitActions.pullTimelineDelta(hostId, newestCreated.agent_id, true);
    }
  },

  async pullTimelineDelta(hostId: string, agentId: string, forceFull = false): Promise<void> {
    const tlKey = `${hostId}::${agentId}`;
    let gate = timelinePullGates.get(tlKey);
    if (!gate) {
      gate = { inFlight: false, queued: false, forceFull: false, rafId: null };
      timelinePullGates.set(tlKey, gate);
    }
    if (gate.rafId !== null && typeof window !== "undefined") {
      window.cancelAnimationFrame(gate.rafId);
      gate.rafId = null;
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

        const existing = state.timelines[tlKey];
        const sinceSeq = nextForceFull || !existing ? 0 : existing.latestSeq;
        const delta = await orbitIpc.getAgentTimelineDelta(hostId, agentId, sinceSeq);

        const prevAgent = existing?.agent ?? null;
        const nextAgent = delta.agent;
        const agentChanged =
          nextAgent !== null &&
          (prevAgent === null ||
            prevAgent.status_label !== nextAgent.status_label ||
            prevAgent.latest_seq !== nextAgent.latest_seq ||
            prevAgent.is_running !== nextAgent.is_running ||
            prevAgent.is_awaiting_permission !== nextAgent.is_awaiting_permission ||
            prevAgent.model !== nextAgent.model ||
            prevAgent.effort !== nextAgent.effort ||
            prevAgent.title !== nextAgent.title ||
            prevAgent.cwd !== nextAgent.cwd ||
            prevAgent.updated_at_rfc3339 !== nextAgent.updated_at_rfc3339 ||
            JSON.stringify(prevAgent.mode) !== JSON.stringify(nextAgent.mode));

        const hasChanges =
          sinceSeq === 0 ||
          delta.upserted_items.length > 0 ||
          Object.keys(delta.resolved_permissions).length > 0 ||
          Object.keys(delta.resolved_questions).length > 0 ||
          agentChanged;

        if (hasChanges) {
          setState((s) => {
            const merged = mergeTimelineDelta(s.timelines[tlKey], delta, sinceSeq === 0);
            const isViewing =
              s.workspacePanel === "conversation" &&
              !s.isDraftNewSession &&
              s.selectedHostId === hostId &&
              s.selectedAgentId === agentId;
            const prevSeen = s.seenAgentSeqs[tlKey] ?? 0;
            const nextSeenSeqs =
              isViewing && merged.latestSeq > prevSeen
                ? { ...s.seenAgentSeqs, [tlKey]: merged.latestSeq }
                : s.seenAgentSeqs;
            return {
              ...s,
              timelines: {
                ...s.timelines,
                [tlKey]: merged,
              },
              seenAgentSeqs: nextSeenSeqs,
            };
          });
        }

        if (!gate.queued) break;
        nextForceFull = gate.forceFull;
      }
    } finally {
      gate.inFlight = false;
    }
  },

  selectHost(hostId: string): void {
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
      (s) => {
        const nextSeenSeqs = firstAgent
          ? {
              ...s.seenAgentSeqs,
              [threadScopeKey(hostId, firstAgent.agent_id)]: Math.max(
                s.seenAgentSeqs[threadScopeKey(hostId, firstAgent.agent_id)] ?? 0,
                firstAgent.latest_seq,
              ),
            }
          : s.seenAgentSeqs;
        return {
          ...s,
          activeView: "workspace",
          workspacePanel: "conversation",
          selectedHostId: hostId,
          selectedAgentId: firstAgent?.agent_id ?? null,
          isDraftNewSession: firstAgent === null,
          sessionScrollNonce: s.sessionScrollNonce + 1,
          selectedWorkspaceCwd: nextCwd,
          selectedDiffPath: null,
          seenAgentSeqs: nextSeenSeqs,
          draftProvider:
            avail.length > 0 && !avail.includes(s.draftProvider) ? avail[0] : s.draftProvider,
        };
      },
      Boolean(firstAgent),
    );

    void orbitIpc.refreshQuotas(hostId);
    if (firstAgent) {
      void orbitIpc.openAgentTimeline(hostId, firstAgent.agent_id);
      void orbitActions.pullTimelineDelta(hostId, firstAgent.agent_id, false);
    }
    if (state.inspectorOpen) {
      void orbitActions.refreshGit(hostId, nextCwd, null, state.diffStaged);
    }
  },

  selectAgent(hostId: string, agentId: string): void {
    const detail = state.hostDetails[hostId];
    const ag = detail?.agents.find((a) => a.agent_id === agentId);
    const nextCwd = ag?.cwd ?? state.selectedWorkspaceCwd;
    const tKey = threadScopeKey(hostId, agentId);

    setState(
      (s) => {
        const latestSeq = Math.max(
          s.seenAgentSeqs[tKey] ?? 0,
          ag?.latest_seq ?? 0,
          s.timelines[tKey]?.latestSeq ?? 0,
        );
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
          seenAgentSeqs: {
            ...s.seenAgentSeqs,
            [tKey]: latestSeq,
          },
        };
      },
      true,
    );

    void orbitIpc.openAgentTimeline(hostId, agentId);
    void orbitActions.pullTimelineDelta(hostId, agentId, false);
    if (state.inspectorOpen) {
      void orbitActions.refreshGit(hostId, nextCwd, null, state.diffStaged);
    }
  },

  startNewThreadDraft(
    hostId?: string,
    cwd?: string,
    prefillPrompt?: string,
    prefillSlash?: SlashDirective | null,
  ): void {
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
    if (state.inspectorOpen && state.selectedHostId) {
      void orbitActions.refreshGit(state.selectedHostId, cwd, null, state.diffStaged);
    }
  },

  selectProjectWorkspace(hostId: string, cwd: string): void {
    const detail = state.hostDetails[hostId];
    const avail = detail?.host.available_providers ?? [];
    setState((s) => ({
      ...s,
      selectedHostId: hostId,
      selectedWorkspaceCwd: cwd,
      selectedDiffPath: null,
      draftProvider:
        avail.length > 0 && !avail.includes(s.draftProvider) ? avail[0] : s.draftProvider,
    }));
    void orbitActions.ensureWorkspaceFiles(hostId, cwd);
    if (state.inspectorOpen) {
      void orbitActions.refreshGit(hostId, cwd, null, state.diffStaged);
    }
  },

  setDraftConfig(patch: {
    provider?: ProviderKind;
    model?: string | null;
    effort?: ReasoningEffort;
    mode?: SessionMode;
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
    const files = await orbitIpc.getWorkspaceFiles(hostId, cwd);
    if (files.length > 0) {
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
      if (x.kind === "file" && item.kind === "file") return x.path === item.path;
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
    const isImage = file.type.startsWith("image/");
    if (isImage) {
      const buf = await file.arrayBuffer();
      const bytes = Array.from(new Uint8Array(buf));
      const previewUrl = URL.createObjectURL(file);
      const name = file.name || `screenshot-${Date.now()}.png`;
      try {
        const staged = await orbitIpc.stageAttachment(name, bytes);
        orbitActions.addContextMention({
          id: `img_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
          kind: "image",
          filename: staged.filename,
          path: staged.path,
          previewUrl,
          sizeBytes: staged.size_bytes,
        });
      } catch (err) {
        notifyToast(`Image attach failed: ${String(err)}`);
      }
      return;
    }

    // Text / code files: inline if <= 64 KiB, otherwise stage on disk and mention path
    if (file.size <= 64 * 1024) {
      const text = await file.text();
      const key = composerScopeKey(state);
      const lineCount = text.split("\n").length;
      setState((s) => ({
        ...s,
        pastedAttachments: {
          ...s.pastedAttachments,
          [key]: [
            ...(s.pastedAttachments[key] ?? []),
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
    } else {
      const buf = await file.arrayBuffer();
      const staged = await orbitIpc.stageAttachment(file.name, Array.from(new Uint8Array(buf)));
      orbitActions.addContextMention({
        id: `fref_${Date.now()}`,
        kind: "file",
        path: staged.path,
      });
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
    const cleanFocus = extraFocus.trim();
    const badges: PromptBadge[] = [{ kind: "slash", label: "/compact" }];
    const wirePrompt =
      provider === "claude_code" || provider === "antigravity"
        ? cleanFocus
          ? `/compact ${cleanFocus}`
          : "/compact"
        : cleanFocus
          ? `${buildHiddenWorkflowInstruction("compact")}\n\n<USER_REQUEST>\n${cleanFocus}\n</USER_REQUEST>`
          : buildHiddenWorkflowInstruction("compact");

    await orbitIpc.sendPrompt(hostId, agentId, wirePrompt, {
      displayPrompt: cleanFocus,
      badges,
    });
  },

  async submitComposer(steerImmediately = false): Promise<void> {
    const hostId = state.selectedHostId;
    if (!hostId) return;

    const key = composerScopeKey(state);
    const rawDraft = (state.composerDrafts[key] ?? "").trim();
    const storedDirective = state.slashDirectives[key] ?? null;
    const inlineParsed = storedDirective
      ? { directive: null, rest: rawDraft }
      : parseLeadingSlashDirective(rawDraft);
    const effectiveDirective: SlashDirective | null =
      storedDirective ?? inlineParsed.directive;
    const cleanUserText = storedDirective ? rawDraft : inlineParsed.rest;

    const existingContexts = state.contextItems[key] ?? [];
    const registeredInline = state.inlineMentions[key] ?? [];
    const pastes = state.pastedAttachments[key] ?? [];
    const quotes = state.diffQuotes[key] ?? [];

    // Resolve any inline `@tokens` still present in `cleanUserText` without showing attachment boxes
    const resolvedInlineContexts: ContextMentionItem[] = [];
    for (const m of registeredInline) {
      if (!cleanUserText.includes(m.token)) continue;
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

    // Ensure `/review` automatically includes working tree diff invisibly
    const contexts: ContextMentionItem[] =
      effectiveDirective === "review" &&
      !mergedContexts.some((c) => c.kind === "git_diff")
        ? [
            ...mergedContexts,
            {
              id: `diff_work_${Date.now()}`,
              kind: "git_diff",
              staged: false,
              label: "Working Tree Diff",
            },
          ]
        : mergedContexts;

    if (effectiveDirective === "compact") {
      if (state.isDraftNewSession || !state.selectedAgentId) {
        notifyToast("Start an active session first to compact context");
        return;
      }
      setState((s) => ({
        ...s,
        composerDrafts: { ...s.composerDrafts, [key]: "" },
        slashDirectives: { ...s.slashDirectives, [key]: null },
      }));
      await orbitActions.submitSlashCompact(cleanUserText);
      return;
    }

    const hasSubstantiveTarget =
      cleanUserText.length > 0 ||
      contexts.length > 0 ||
      pastes.length > 0 ||
      quotes.length > 0;

    if (!hasSubstantiveTarget && effectiveDirective !== "review") {
      return;
    }

    // 1. Build clean user-facing badges (never exposing hidden boilerplate or raw diffs)
    const badges: PromptBadge[] = [];
    if (effectiveDirective) {
      badges.push({ kind: "slash", label: `/${effectiveDirective}` });
    }

    const imageItems = contexts.filter(
      (c): c is Extract<ContextMentionItem, { kind: "image" }> => c.kind === "image",
    );
    const fileItems = contexts.filter(
      (c): c is Extract<ContextMentionItem, { kind: "file" }> => c.kind === "file",
    );
    const gitDiffItems = contexts.filter(
      (c): c is Extract<ContextMentionItem, { kind: "git_diff" }> => c.kind === "git_diff",
    );
    const termItems = contexts.filter(
      (c): c is Extract<ContextMentionItem, { kind: "terminal_output" }> =>
        c.kind === "terminal_output",
    );

    for (const img of imageItems) {
      badges.push({ kind: "image", label: img.filename });
    }
    for (const f of fileItems) {
      const base = basenameFromPath(f.path);
      if (!cleanUserText.includes(`@${base}`)) {
        badges.push({ kind: "file", label: base });
      }
    }
    for (const gd of gitDiffItems) {
      const inlineAlias = gd.staged ? "@staged-diff" : "@working-diff";
      if (!cleanUserText.includes(inlineAlias) && effectiveDirective !== "review") {
        badges.push({ kind: "git_diff", label: gd.label });
      }
    }
    for (const tm of termItems) {
      if (!cleanUserText.includes("@terminal")) {
        badges.push({ kind: "terminal", label: tm.label });
      }
    }
    for (const q of quotes) {
      const shortFile = basenameFromPath(q.path);
      badges.push({
        kind: "quote",
        label: q.lineNumber !== null ? `${shortFile}:${q.lineNumber}` : shortFile,
      });
    }
    for (const p of pastes) {
      badges.push({ kind: "paste", label: p.filename ?? p.label });
    }

    // 2. Build hidden structured `wirePrompt` strictly for the Agent CLI
    const wireSections: string[] = [];

    if (effectiveDirective) {
      wireSections.push(buildHiddenWorkflowInstruction(effectiveDirective));
    }

    const contextBlocks: string[] = [];
    if (imageItems.length > 0) {
      contextBlocks.push(
        imageItems.map((img) => `Attached image file: \`${img.path}\``).join("\n"),
      );
    }
    if (fileItems.length > 0) {
      contextBlocks.push(
        `Referenced workspace files: ${fileItems.map((f) => `\`@${f.path}\``).join(", ")}`,
      );
    }
    for (const gd of gitDiffItems) {
      const cwd = state.selectedWorkspaceCwd || "~";
      await orbitIpc.fetchGitDiff(hostId, cwd, null, gd.staged);
      const gr = await orbitIpc.getGitReview(hostId, cwd, null);
      if (gr.raw_diff.trim()) {
        const clipped = gr.raw_diff.slice(0, 14000);
        contextBlocks.push(`${gd.label}:\n\`\`\`diff\n${clipped}\n\`\`\``);
      } else {
        contextBlocks.push(
          `Context: Inspect ${gd.staged ? "staged" : "working tree"} git changes in workspace.`,
        );
      }
    }
    for (const tm of termItems) {
      contextBlocks.push(`${tm.label}:\n\`\`\`text\n${tm.content}\n\`\`\``);
    }
    for (const q of quotes) {
      const loc = q.lineNumber !== null ? `${q.path}:${q.lineNumber}` : q.path;
      contextBlocks.push(`Reference \`${loc}\`:\n\`\`\`\n${q.code}\n\`\`\``);
    }
    for (const p of pastes) {
      contextBlocks.push(`${p.label}:\n\`\`\`text\n${p.content}\n\`\`\``);
    }

    if (contextBlocks.length > 0) {
      wireSections.push(`<ATTACHED_CONTEXT>\n${contextBlocks.join("\n\n")}\n</ATTACHED_CONTEXT>`);
    }

    if (cleanUserText) {
      if (wireSections.length > 0) {
        wireSections.push(`<USER_REQUEST>\n${cleanUserText}\n</USER_REQUEST>`);
      } else {
        wireSections.push(cleanUserText);
      }
    } else if (effectiveDirective === "review") {
      wireSections.push(
        "<USER_REQUEST>\nReview the attached workspace changes thoroughly.\n</USER_REQUEST>",
      );
    }

    const assembledWirePrompt = wireSections.join("\n\n");

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
      const cat = getCatalogForProvider(
        hostDetailForCreate?.provider_catalogs,
        effectiveProvider,
      );
      const explicitDraftModel = state.draftModels[effectiveProvider] ?? null;
      const spec = resolveActiveModelSpec(cat, explicitDraftModel);
      const resolvedModel = explicitDraftModel ?? spec?.id ?? null;
      const resolvedEffort = resolveSupportedEffortForModel(
        spec,
        state.draftEffort,
        cat.configured_default_effort,
      );

      await orbitIpc.createAgent({
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
      });
      setState((s) => ({ ...s, draftProvider: effectiveProvider, draftWorktreeBranch: "" }));
      return;
    }

    const agentId = state.selectedAgentId;
    const detail = state.hostDetails[hostId];
    const activeAgent = detail?.agents.find((a) => a.agent_id === agentId);

    if (activeAgent?.is_running || activeAgent?.is_awaiting_permission) {
      if (steerImmediately) {
        await orbitIpc.interruptAgent(hostId, agentId);
        await orbitIpc.sendPrompt(hostId, agentId, assembledWirePrompt, {
          displayPrompt: cleanUserText,
          badges,
        });
      } else {
        const queuedItem: QueuedPromptItem = {
          id: `q_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
          wirePrompt: assembledWirePrompt,
          displayText: cleanUserText,
          badges,
          slashDirective: effectiveDirective,
          contexts,
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

  removeQueuedPrompt(index: number): void {
    const key = composerScopeKey(state);
    setState((s) => ({
      ...s,
      queuedPrompts: {
        ...s.queuedPrompts,
        [key]: (s.queuedPrompts[key] ?? []).filter((_, idx) => idx !== index),
      },
    }));
  },

  popQueuedToDraft(index: number): void {
    const key = composerScopeKey(state);
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
    orbitActions.focusComposer();
  },

  async flushQueuedPromptImmediately(index: number): Promise<void> {
    const hostId = state.selectedHostId;
    const agentId = state.selectedAgentId;
    if (!hostId || !agentId) return;
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
    await orbitIpc.interruptAgent(hostId, agentId);
    await orbitIpc.sendPrompt(hostId, agentId, target.wirePrompt, {
      displayPrompt: target.displayText,
      badges: target.badges,
    });
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
    const tKey = threadScopeKey(hostId, agentId);
    setState(
      (s) => {
        const exists = s.pinnedThreadKeys.includes(tKey);
        return {
          ...s,
          pinnedThreadKeys: exists
            ? s.pinnedThreadKeys.filter((k) => k !== tKey)
            : [tKey, ...s.pinnedThreadKeys],
        };
      },
      true,
    );
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

    setState(
      (s) => ({
        ...s,
        seenAgentSeqs: {
          ...s.seenAgentSeqs,
          [tKey]: 0,
        },
        ...(isCurrentlySelected ? { isDraftNewSession: true, selectedAgentId: null } : {}),
      }),
      true,
    );
  },

  markThreadRead(hostId: string, agentId: string): void {
    const tKey = threadScopeKey(hostId, agentId);
    const detail = state.hostDetails[hostId];
    const ag =
      detail?.agents.find((a) => a.agent_id === agentId) ??
      state.timelines[tKey]?.agent ??
      null;
    const targetSeq = Math.max(
      ag?.latest_seq ?? 0,
      state.timelines[tKey]?.latestSeq ?? 0,
      1,
    );
    setState(
      (s) => ({
        ...s,
        seenAgentSeqs: {
          ...s.seenAgentSeqs,
          [tKey]: targetSeq,
        },
      }),
      true,
    );
  },

  async copyThread(hostId: string, agentId: string): Promise<void> {
    const tKey = threadScopeKey(hostId, agentId);
    let items = state.timelines[tKey]?.items;
    if (!items || items.length === 0) {
      try {
        const full = await orbitIpc.getAgentTimeline(hostId, agentId);
        items = full.items;
      } catch {
        items = [];
      }
    }

    const sections: string[] = [];
    for (const item of items) {
      if (item.item_type === "user_prompt") {
        const txt = item.text.trim();
        if (txt) sections.push(`## User\n\n${txt}`);
      } else if (item.item_type === "assistant_message") {
        const txt = item.text.trim();
        if (txt) sections.push(`## Assistant\n\n${txt}`);
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

  archiveThread(hostId: string, agentId: string): void {
    const tKey = threadScopeKey(hostId, agentId);
    const wasSelected =
      !state.isDraftNewSession &&
      state.selectedHostId === hostId &&
      state.selectedAgentId === agentId;

    let fallbackAgentId: string | null = null;
    if (wasSelected) {
      const detail = state.hostDetails[hostId];
      const nextCandidate = detail?.agents.find(
        (a) =>
          a.agent_id !== agentId &&
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

    if (wasSelected && fallbackAgentId) {
      orbitActions.selectAgent(hostId, fallbackAgentId);
    }
    notifyToast("Conversation archived");
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
        };
      },
      true,
    );
    orbitActions.selectAgent(hostId, agentId);
    notifyToast("Conversation restored");
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
        const nextSeen = { ...s.seenAgentSeqs };
        delete nextSeen[tKey];
        return {
          ...s,
          archivedThreads: nextArchived,
          threadCustomTitles: nextTitles,
          seenAgentSeqs: nextSeen,
          pinnedThreadKeys: s.pinnedThreadKeys.filter((k) => k !== tKey),
          ...(wasSelected ? { selectedAgentId: null, isDraftNewSession: true } : {}),
        };
      },
      true,
    );

    await orbitIpc.archiveAgent(hostId, agentId);
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
  },

  toggleSidebar(): void {
    const isCompact = state.isCompactViewport;
    setState(
      (s) => {
        const nextOpen = !s.sidebarOpen;
        return {
          ...s,
          sidebarOpen: nextOpen,
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

    if (nextCompact === state.isCompactViewport) return;

    setState((s) => ({
      ...s,
      isCompactViewport: nextCompact,
      sidebarOpen: nextCompact ? false : s.sidebarDesktopOpenPref,
    }));
  },

  setSidebarWidth(w: number): void {
    setState((s) => ({ ...s, sidebarWidth: Math.min(400, Math.max(230, w)) }), true);
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

  async deleteProject(project: UnifiedProjectEntry): Promise<void> {
    const hostId = project.host.host_id;
    try {
      const running = project.conversations.filter(
        ({ agent }) => agent.is_running || agent.is_awaiting_permission,
      );
      if (running.length > 0) {
        await Promise.allSettled(
          running.map(({ agent }) => orbitIpc.interruptAgent(hostId, agent.agent_id)),
        );
      }

      if (project.conversations.length > 0) {
        await Promise.allSettled(
          project.conversations.map(({ agent }) => orbitIpc.archiveAgent(hostId, agent.agent_id)),
        );
      }

      const removedPinnedIds = new Set<string>();
      if (project.pinnedProject?.project_id) {
        removedPinnedIds.add(project.pinnedProject.project_id);
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
        nextHidden[projectPathScopeKey(hostId, project.primaryCwd)] = true;
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
          const remainingPinned = (s.fleet.projects ?? []).filter(
            (p) => !removedPinnedIds.has(p.project_id),
          );
          const hostDetail = s.hostDetails[hostId];
          const remainingHostDetails = hostDetail
            ? {
                ...s.hostDetails,
                [hostId]: {
                  ...hostDetail,
                  agents: hostDetail.agents.filter((a) => !deletedAgentIds.has(a.agent_id)),
                },
              }
            : s.hostDetails;
          const remainingProjects = buildUnifiedProjects(
            s.fleet.hosts,
            remainingPinned,
            remainingHostDetails,
            nextHidden,
          );
          const nextProject =
            remainingProjects.find(
              (p) =>
                p.id !== project.id &&
                !(
                  p.host.host_id === hostId &&
                  matchesProjectDirectory(project.directories, p.primaryCwd)
                ),
            ) ?? null;

          if (nextProject) {
            nextHostId = nextProject.host.host_id;
            nextCwd = nextProject.primaryCwd;
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
          pinnedThreadKeys:
            deletedThreadKeys.size > 0
              ? s.pinnedThreadKeys.filter((k) => !deletedThreadKeys.has(k))
              : s.pinnedThreadKeys,
          archivedThreads: omitRecordKeys(s.archivedThreads, deletedThreadKeys),
          threadCustomTitles: omitRecordKeys(s.threadCustomTitles, deletedThreadKeys),
          seenAgentSeqs: omitRecordKeys(s.seenAgentSeqs, deletedThreadKeys),
          timelines: omitRecordKeys(s.timelines, deletedThreadKeys),
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
      notifyToast("Project deleted");
    } catch (err) {
      notifyToast(`Failed to delete project: ${String(err)}`);
      throw err;
    }
  },

  toggleInspector(openOverride?: boolean): void {
    const nextOpen = openOverride ?? !state.inspectorOpen;
    setState((s) => ({ ...s, inspectorOpen: nextOpen }));
    if (nextOpen && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        state.selectedDiffPath,
        state.diffStaged,
      );
    }
  },

  openInspectorForFile(path: string, scope: ReviewScope = "session_turns"): void {
    setState((s) => ({
      ...s,
      inspectorOpen: true,
      reviewScope: scope,
      selectedDiffPath: path,
    }));
    if (scope === "working_tree" && state.selectedHostId && state.selectedWorkspaceCwd) {
      void orbitActions.refreshGit(
        state.selectedHostId,
        state.selectedWorkspaceCwd,
        path,
        state.diffStaged,
      );
    }
  },

  setInspectorWidth(w: number): void {
    setState((s) => ({ ...s, inspectorWidth: Math.min(760, Math.max(340, w)) }), true);
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

  toggleTerminalDock(openOverride?: boolean): void {
    const nextOpen = openOverride ?? !state.terminalOpen;
    setState((s) => ({ ...s, terminalOpen: nextOpen }));
    if (nextOpen && state.selectedHostId) {
      const hostTabs = state.terminalTabs.filter((t) => t.hostId === state.selectedHostId);
      if (hostTabs.length === 0) {
        orbitActions.spawnNewTerminalTab(state.selectedHostId, state.selectedWorkspaceCwd || "~");
      } else if (!hostTabs.some((t) => t.key === state.activeTerminalTabKey)) {
        setState((s) => ({ ...s, activeTerminalTabKey: hostTabs[0].key }));
      }
    }
  },

  setTerminalHeight(h: number): void {
    setState((s) => ({ ...s, terminalHeight: Math.min(540, Math.max(160, h)) }), true);
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
      setState((s) => ({ ...s, activeTerminalTabKey: existing.key, terminalOpen: true }));
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
      terminalOpen: true,
      terminalTabs: [
        ...s.terminalTabs,
        { key: tabKey, hostId, terminalId, slot, cwd, title },
      ],
      activeTerminalTabKey: tabKey,
    }));
    void orbitIpc.attachTerminal(hostId, terminalId, slot);
  },

  closeTerminalTab(tabKey: string, killRemoteShim: boolean): void {
    const target = state.terminalTabs.find((t) => t.key === tabKey);
    if (target && killRemoteShim && target.terminalId) {
      void orbitIpc.killTerminal(target.hostId, target.terminalId);
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
        terminalOpen: remaining.length === 0 ? false : s.terminalOpen,
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
