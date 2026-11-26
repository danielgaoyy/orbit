import type { HostDetailView } from "../lib/orbit-ipc.ts";
import type {
  OrbitUiState,
  SidePanelOpenSource,
  SidePanelScopeKey,
  SidePanelSnapshot,
  SidePanelSplitState,
  SidePanelTab,
  SidePanelTabKind,
} from "./store-types.ts";

export const SIDE_PANEL_STORAGE_KEY = "orbit.desktop.sidePanel.v1";
export const SIDE_PANEL_SCHEMA_VERSION = 1 as const;
export const SIDE_PANEL_MAX_SCOPES = 100;
export const SIDE_PANEL_MAX_CLOSED_TABS = 10;

export const EMPTY_SIDE_PANEL_TABS: SidePanelTab[] = [];
export const EMPTY_SIDE_PANEL_UNSEEN_IDS: string[] = [];

const FORBIDDEN_KEYS = new Set(["__proto__", "constructor", "prototype"]);

const VALID_TAB_KINDS: ReadonlySet<SidePanelTabKind> = new Set<SidePanelTabKind>([
  "review",
  "files",
  "file",
  "terminal",
  "browser",
  "side_question",
  "subagent",
  "board",
  "peer",
  "artifacts",
]);

export interface ParsedSidePanelScopeKey {
  kind: "draft" | "agent";
  hostId: string;
  agentId: string | null;
}

export interface PersistedSidePanelPayloadV1 {
  version: typeof SIDE_PANEL_SCHEMA_VERSION;
  byScope: Record<SidePanelScopeKey, SidePanelSnapshot>;
}

export interface SwitchSidePanelScopeOptions {
  dropCurrent?: boolean;
  now?: number;
}

export interface OpenSidePanelTabOptions {
  source?: SidePanelOpenSource;
  activate?: boolean;
  updateExisting?: (existing: SidePanelTab, incoming: SidePanelTab) => SidePanelTab;
  upsertTabs?: (existingTabs: readonly SidePanelTab[]) => {
    tabs: SidePanelTab[];
    tabId: string;
  };
  terminalFocused?: boolean | "preserve";
  patch?: Partial<OrbitUiState>;
}

/**
 * Walks `parent_agent_id` / `parent_host_id` links up to the root non-archived parent
 * so child subagent sessions always map to their root parent's side-panel scope.
 */
export function resolveRootAgentTarget(
  hostDetails: Record<string, HostDetailView> | undefined,
  hostId: string | null,
  agentId: string | null,
  archivedThreads?: Readonly<Record<string, unknown>>,
): { hostId: string | null; agentId: string | null } {
  if (!hostId || !agentId) {
    return { hostId, agentId: null };
  }
  if (!hostDetails) {
    return { hostId, agentId };
  }
  let rootHostId = hostId;
  let rootAgentId = agentId;
  let cursor = hostDetails[hostId]?.agents.find((a) => a.agent_id === agentId);
  const visited = new Set<string>([`${hostId}::${agentId}`]);

  while (cursor?.parent_agent_id) {
    const nextHostId = cursor.parent_host_id ?? rootHostId;
    const nextAgentId = cursor.parent_agent_id;
    const visitKey = `${nextHostId}::${nextAgentId}`;
    if (visited.has(visitKey)) break;
    visited.add(visitKey);
    const parentAg = hostDetails[nextHostId]?.agents.find((a) => a.agent_id === nextAgentId);
    if (!parentAg || parentAg.archived_at_rfc3339 || archivedThreads?.[visitKey]) break;
    rootHostId = nextHostId;
    rootAgentId = nextAgentId;
    cursor = parentAg;
  }

  return { hostId: rootHostId, agentId: rootAgentId };
}

export function resolveSidePanelScopeKey(params: {
  selectedHostId: string | null;
  selectedAgentId: string | null;
  isDraftNewSession: boolean;
  hostDetails?: Record<string, HostDetailView>;
  archivedThreads?: Readonly<Record<string, unknown>>;
}): SidePanelScopeKey | null {
  if (!params.selectedHostId) return null;
  if (params.isDraftNewSession || !params.selectedAgentId) {
    return `draft::${params.selectedHostId}`;
  }
  const root = resolveRootAgentTarget(
    params.hostDetails,
    params.selectedHostId,
    params.selectedAgentId,
    params.archivedThreads,
  );
  if (!root.hostId) return null;
  if (!root.agentId) return `draft::${root.hostId}`;
  return `${root.hostId}::${root.agentId}`;
}

export function parseSidePanelScopeKey(
  scopeKey: SidePanelScopeKey | null | undefined,
): ParsedSidePanelScopeKey | null {
  if (!scopeKey || typeof scopeKey !== "string") return null;
  const sepIdx = scopeKey.indexOf("::");
  if (sepIdx <= 0) return null;
  const prefix = scopeKey.slice(0, sepIdx);
  const suffix = scopeKey.slice(sepIdx + 2);
  if (!suffix) return null;
  if (prefix === "draft") {
    return { kind: "draft", hostId: suffix, agentId: null };
  }
  return { kind: "agent", hostId: prefix, agentId: suffix };
}

export function sidePanelTabScopeKey(
  tab: Pick<SidePanelTab, "hostId" | "parentHostId" | "agentId" | "parentAgentId">,
): SidePanelScopeKey {
  const host = tab.parentHostId || tab.hostId || "none";
  const agent = tab.agentId ?? tab.parentAgentId ?? null;
  return agent ? `${host}::${agent}` : `draft::${host}`;
}

/** Tab kinds whose content is "the workspace", not one pinned file or entity. */
const DRAFT_WORKSPACE_FOLLOWING_KINDS: ReadonlySet<SidePanelTabKind> = new Set<SidePanelTabKind>([
  "files",
  "review",
  "terminal",
  "browser",
  "side_question",
  "artifacts",
]);

/**
 * A draft has no agent yet, so its workspace-level tabs must follow the draft's project
 * picker. Session tabs are pinned to the agent's cwd and are left untouched.
 */
export function rebindDraftSidePanelCwd(s: OrbitUiState, nextCwd: string): OrbitUiState {
  if (!s.isDraftNewSession || !nextCwd) return s;
  const rebind = (tabs: SidePanelTab[]): SidePanelTab[] => {
    let changed = false;
    const next = tabs.map((tab) => {
      if (tab.agentId != null || !DRAFT_WORKSPACE_FOLLOWING_KINDS.has(tab.kind)) return tab;
      if (tab.cwd === nextCwd) return tab;
      changed = true;
      return { ...tab, cwd: nextCwd };
    });
    return changed ? next : tabs;
  };
  const nextTabs = rebind(s.sidePanelTabs);
  const nextClosed = rebind(s.sidePanelClosedTabs);
  if (nextTabs === s.sidePanelTabs && nextClosed === s.sidePanelClosedTabs) return s;
  return { ...s, sidePanelTabs: nextTabs, sidePanelClosedTabs: nextClosed };
}

export function stripTransientTabFields(
  tab: SidePanelTab,
  opts?: { stripFocusLines?: boolean },
): SidePanelTab {
  const hasTransient =
    tab.focusNonce !== undefined ||
    (Boolean(opts?.stripFocusLines) &&
      (tab.focusLine !== undefined || tab.focusEndLine !== undefined));
  if (!hasTransient) return tab;
  const next: SidePanelTab = { ...tab };
  delete next.focusNonce;
  if (opts?.stripFocusLines) {
    delete next.focusLine;
    delete next.focusEndLine;
  }
  return next;
}

export function isTabValidForScope(
  tab: SidePanelTab,
  parsed: ParsedSidePanelScopeKey | null,
): boolean {
  if (!parsed) return false;
  if (tab.kind === "subagent") {
    if (parsed.kind !== "agent" || !parsed.agentId) return false;
    if (tab.parentAgentId !== parsed.agentId) return false;
    if (tab.parentHostId && tab.parentHostId !== parsed.hostId) return false;
    return true;
  }
  if (tab.kind === "peer") {
    const ownerHostId = tab.parentHostId ?? tab.hostId;
    if (ownerHostId && ownerHostId !== parsed.hostId) return false;
    return true;
  }
  if (tab.kind === "board") {
    if (tab.hostId && tab.hostId !== parsed.hostId) return false;
    return true;
  }
  if (tab.hostId && tab.hostId !== parsed.hostId) {
    return false;
  }
  return true;
}

export function validateTabsForScope(
  tabs: readonly SidePanelTab[],
  scopeKey: SidePanelScopeKey | null,
  opts?: { stripFocusNonce?: boolean },
): SidePanelTab[] {
  const parsed = parseSidePanelScopeKey(scopeKey);
  if (!parsed || tabs.length === 0) return EMPTY_SIDE_PANEL_TABS;
  const seenIds = new Set<string>();
  const valid: SidePanelTab[] = [];
  for (const rawTab of tabs) {
    if (!rawTab || !rawTab.id || seenIds.has(rawTab.id)) continue;
    if (!isTabValidForScope(rawTab, parsed)) continue;
    seenIds.add(rawTab.id);
    valid.push(opts?.stripFocusNonce ? stripTransientTabFields(rawTab) : rawTab);
  }
  return valid.length > 0 ? valid : EMPTY_SIDE_PANEL_TABS;
}

export function normalizeActiveTabId(
  tabs: readonly SidePanelTab[],
  activeTabId: string | null,
  preferredIndex?: number,
): string | null {
  if (tabs.length === 0) return null;
  if (activeTabId === null) return null;
  if (tabs.some((t) => t.id === activeTabId)) return activeTabId;
  if (preferredIndex !== undefined && preferredIndex >= 0) {
    return tabs[Math.min(preferredIndex, tabs.length - 1)]?.id ?? null;
  }
  return tabs[tabs.length - 1]?.id ?? null;
}

export function normalizeSidePanelSplit(
  tabs: readonly SidePanelTab[],
  activeTabId: string | null,
  split: SidePanelSplitState | null,
): SidePanelSplitState | null {
  if (!split || tabs.length < 2) return null;
  if (!tabs.some((t) => t.id === split.secondaryTabId)) return null;
  if (activeTabId !== null && split.secondaryTabId === activeTabId) return null;
  const rawRatio = typeof split.ratio === "number" && Number.isFinite(split.ratio) ? split.ratio : 0.5;
  const ratio = Math.min(0.75, Math.max(0.25, rawRatio));
  if (ratio === split.ratio) return split;
  return { secondaryTabId: split.secondaryTabId, ratio };
}

export function isEmptyDefaultSnapshot(snap: SidePanelSnapshot): boolean {
  return (
    !snap.open &&
    snap.tabs.length === 0 &&
    !snap.userDismissed &&
    snap.closedTabs.length === 0 &&
    (snap.unseenTabIds?.length ?? 0) === 0
  );
}

export function pruneSidePanelByScopeLru(
  byScope: Record<SidePanelScopeKey, SidePanelSnapshot>,
  maxScopes: number = SIDE_PANEL_MAX_SCOPES,
): Record<SidePanelScopeKey, SidePanelSnapshot> {
  const entries = Object.entries(byScope);
  if (entries.length <= maxScopes) return byScope;
  entries.sort((a, b) => (b[1].lastUsedAt || 0) - (a[1].lastUsedAt || 0));
  const kept: Record<SidePanelScopeKey, SidePanelSnapshot> = {};
  for (let i = 0; i < maxScopes && i < entries.length; i += 1) {
    kept[entries[i][0]] = entries[i][1];
  }
  return kept;
}

export function buildSnapshotFromLiveState(
  s: Pick<
    OrbitUiState,
    | "sidePanelOpen"
    | "sidePanelCollapsedByResize"
    | "sidePanelTabs"
    | "activeSidePanelTabId"
    | "sidePanelSplit"
    | "sidePanelUserDismissed"
    | "sidePanelClosedTabs"
    | "sidePanelUnseenTabIds"
  >,
  now: number = Date.now(),
  opts?: { stripFocusLines?: boolean },
): SidePanelSnapshot {
  const tabs = s.sidePanelTabs.map((t) => stripTransientTabFields(t, opts));
  const activeTabId = normalizeActiveTabId(tabs, s.activeSidePanelTabId);
  const split = normalizeSidePanelSplit(tabs, activeTabId, s.sidePanelSplit);
  const closedTabs = s.sidePanelClosedTabs
    .slice(0, SIDE_PANEL_MAX_CLOSED_TABS)
    .map((t) => stripTransientTabFields(t, opts));
  const tabIdSet = new Set(tabs.map((t) => t.id));
  const unseenTabIds = s.sidePanelUnseenTabIds.filter((id) => tabIdSet.has(id));
  return {
    open: s.sidePanelOpen || s.sidePanelCollapsedByResize,
    tabs,
    activeTabId,
    split,
    userDismissed: s.sidePanelUserDismissed,
    closedTabs,
    unseenTabIds: unseenTabIds.length > 0 ? unseenTabIds : EMPTY_SIDE_PANEL_UNSEEN_IDS,
    lastUsedAt: now,
  };
}

export function resolveSidebarYieldForSidePanel(
  s: Pick<
    OrbitUiState,
    | "sidePanelOpen"
    | "sidebarOpen"
    | "sidebarDesktopOpenPref"
    | "sidebarAutoYieldedForSidePanel"
    | "isCompactViewport"
    | "isTightForDualSidebars"
  >,
  nextPanelOpen: boolean,
): { sidebarOpen: boolean; sidebarAutoYieldedForSidePanel: boolean } {
  if (nextPanelOpen) {
    if (!s.sidePanelOpen && !s.isCompactViewport && s.isTightForDualSidebars && s.sidebarOpen) {
      return { sidebarOpen: false, sidebarAutoYieldedForSidePanel: true };
    }
    return {
      sidebarOpen: s.sidebarOpen,
      sidebarAutoYieldedForSidePanel: s.sidebarAutoYieldedForSidePanel,
    };
  }
  if (s.sidebarAutoYieldedForSidePanel && !s.isCompactViewport) {
    return {
      sidebarOpen: s.sidebarDesktopOpenPref,
      sidebarAutoYieldedForSidePanel: false,
    };
  }
  return {
    sidebarOpen: s.sidebarOpen,
    sidebarAutoYieldedForSidePanel: s.sidebarAutoYieldedForSidePanel,
  };
}

export function resolveCurrentTabScope(
  s: Pick<
    OrbitUiState,
    "selectedHostId" | "selectedAgentId" | "isDraftNewSession" | "selectedWorkspaceCwd" | "hostDetails"
  >,
): { hostId: string | undefined; agentId: string | null; cwd: string } {
  const hostId = s.selectedHostId ?? undefined;
  if (!hostId || s.isDraftNewSession || !s.selectedAgentId) {
    return {
      hostId,
      agentId: null,
      cwd: s.selectedWorkspaceCwd || "~",
    };
  }
  const root = resolveRootAgentTarget(s.hostDetails, hostId, s.selectedAgentId);
  const resolvedHost = root.hostId ?? hostId;
  const resolvedAgent = root.agentId;
  const agentCard = resolvedAgent
    ? s.hostDetails[resolvedHost]?.agents.find((a) => a.agent_id === resolvedAgent)
    : undefined;
  return {
    hostId: resolvedHost,
    agentId: resolvedAgent,
    cwd: agentCard?.cwd || s.selectedWorkspaceCwd || "~",
  };
}

export function pushClosedSidePanelTab(
  closedTabs: readonly SidePanelTab[],
  tab: SidePanelTab,
): SidePanelTab[] {
  const cleaned = stripTransientTabFields(tab);
  const deduped = closedTabs.filter((t) => t.id !== cleaned.id);
  return [cleaned, ...deduped].slice(0, SIDE_PANEL_MAX_CLOSED_TABS);
}

/**
 * Single state-transition primitive for switching the active session scope of the right Side Panel.
 * Saves the outgoing scope's snapshot (unless empty or dropped), loads & validates the incoming scope,
 * reconciles left-sidebar auto-yield and compact collapse, clears cross-scope transient state, and
 * bumps `sidePanelScopeSwitchNonce` so `WorkspaceMotionProvider` swaps cleanly without slide flashes.
 */
export function switchSidePanelScope(
  s: OrbitUiState,
  nextKey: SidePanelScopeKey | null,
  opts?: SwitchSidePanelScopeOptions,
): OrbitUiState {
  const now = opts?.now ?? Date.now();
  const currentKey =
    s.sidePanelScopeKey ??
    resolveSidePanelScopeKey({
      selectedHostId: s.selectedHostId,
      selectedAgentId: s.selectedAgentId,
      isDraftNewSession: s.isDraftNewSession,
      hostDetails: s.hostDetails,
    });

  // Same-scope call: if `sidePanelScopeKey` was unset (cold startup hydration), lazily load persisted snapshot
  // only when the live panel is still in its clean initial empty state.
  if (currentKey === nextKey && !opts?.dropCurrent) {
    if (s.sidePanelScopeKey === nextKey) {
      return s;
    }
    if (nextKey !== null) {
      const liveEmpty =
        !s.sidePanelOpen &&
        s.sidePanelTabs.length === 0 &&
        !s.sidePanelUserDismissed &&
        s.sidePanelClosedTabs.length === 0 &&
        s.sidePanelUnseenTabIds.length === 0;
      const persisted = liveEmpty ? s.sidePanelByScope[nextKey] : undefined;
      if (persisted) {
        return loadSnapshotIntoLive(s, nextKey, persisted, s.sidePanelByScope, true);
      }
    }
    return {
      ...s,
      sidePanelScopeKey: nextKey,
    };
  }

  let nextByScope = s.sidePanelByScope;
  if (currentKey) {
    const currentIsArchived = Boolean(s.archivedThreads[currentKey]);
    if (opts?.dropCurrent || currentIsArchived) {
      if (currentKey in nextByScope) {
        nextByScope = { ...nextByScope };
        delete nextByScope[currentKey];
      }
    } else {
      const outgoing = buildSnapshotFromLiveState(s, now);
      if (isEmptyDefaultSnapshot(outgoing)) {
        if (currentKey in nextByScope) {
          nextByScope = { ...nextByScope };
          delete nextByScope[currentKey];
        }
      } else {
        nextByScope = pruneSidePanelByScopeLru({
          ...nextByScope,
          [currentKey]: outgoing,
        });
      }
    }
  }

  const incoming = nextKey ? nextByScope[nextKey] : undefined;
  return loadSnapshotIntoLive(s, nextKey, incoming, nextByScope, true);
}

function loadSnapshotIntoLive(
  s: OrbitUiState,
  nextKey: SidePanelScopeKey | null,
  incoming: SidePanelSnapshot | undefined,
  byScope: Record<SidePanelScopeKey, SidePanelSnapshot>,
  bumpSwitchNonce: boolean,
): OrbitUiState {
  let nextOpen = false;
  let nextTabs: SidePanelTab[] = EMPTY_SIDE_PANEL_TABS;
  let nextActiveTabId: string | null = null;
  let nextSplit: SidePanelSplitState | null = null;
  let nextUserDismissed = false;
  let nextClosedTabs: SidePanelTab[] = EMPTY_SIDE_PANEL_TABS;
  let nextUnseenTabIds: string[] = EMPTY_SIDE_PANEL_UNSEEN_IDS;
  let nextCollapsedByResize = false;

  if (incoming && nextKey) {
    nextTabs = validateTabsForScope(incoming.tabs, nextKey, { stripFocusNonce: true });
    nextActiveTabId = normalizeActiveTabId(nextTabs, incoming.activeTabId);
    nextSplit = normalizeSidePanelSplit(nextTabs, nextActiveTabId, incoming.split);
    nextClosedTabs = validateTabsForScope(incoming.closedTabs, nextKey, { stripFocusNonce: true })
      .slice(0, SIDE_PANEL_MAX_CLOSED_TABS);
    const validTabIds = new Set(nextTabs.map((t) => t.id));
    const filteredUnseen = (incoming.unseenTabIds ?? []).filter((id) => validTabIds.has(id));
    nextUnseenTabIds = filteredUnseen.length > 0 ? filteredUnseen : EMPTY_SIDE_PANEL_UNSEEN_IDS;
    nextUserDismissed = Boolean(incoming.userDismissed);

    // If a snapshot previously had tabs and every single one became invalid on load, keep it closed.
    const allTabsInvalidated = incoming.tabs.length > 0 && nextTabs.length === 0;
    const desiredOpen = Boolean(incoming.open) && !allTabsInvalidated;
    if (desiredOpen && s.isCompactViewport) {
      nextOpen = false;
      nextCollapsedByResize = true;
    } else {
      nextOpen = desiredOpen;
      nextCollapsedByResize = false;
    }
    if (nextOpen) {
      nextUnseenTabIds = EMPTY_SIDE_PANEL_UNSEEN_IDS;
    }
  }

  const sidebarPatch = resolveSidebarYieldForSidePanel(s, nextOpen);

  return {
    ...s,
    sidePanelByScope: byScope,
    sidePanelScopeKey: nextKey,
    sidePanelOpen: nextOpen,
    sidePanelTabs: nextTabs,
    activeSidePanelTabId: nextActiveTabId,
    sidePanelSplit: nextSplit,
    sidePanelUserDismissed: nextUserDismissed,
    sidePanelClosedTabs: nextClosedTabs,
    sidePanelUnseenTabIds: nextUnseenTabIds,
    sidePanelCollapsedByResize: nextCollapsedByResize,
    sidePanelScopeSwitchNonce: bumpSwitchNonce
      ? s.sidePanelScopeSwitchNonce + 1
      : s.sidePanelScopeSwitchNonce,
    sidebarOpen: sidebarPatch.sidebarOpen,
    sidebarAutoYieldedForSidePanel: sidebarPatch.sidebarAutoYieldedForSidePanel,
    terminalFocused: false,
    overviewPopoverOpen: false,
    pendingUnsavedFileAction: null,
  };
}

/**
 * Moves the live draft scope (`draft::hostId`) cleanly onto the newly promoted real session
 * (`hostId::newAgentId`) so any side panel opened while composing the first prompt stays seamlessly
 * open, while removing `draft::hostId` so the next New Conversation starts closed.
 */
export function promoteDraftSidePanelScope(
  s: OrbitUiState,
  hostId: string,
  newAgentId: string,
  newCwd?: string | null,
  now: number = Date.now(),
): OrbitUiState {
  const draftKey: SidePanelScopeKey = `draft::${hostId}`;
  const targetKey: SidePanelScopeKey = `${hostId}::${newAgentId}`;

  const rebindDraftTab = (tab: SidePanelTab): SidePanelTab => {
    const wasDraftTab = tab.agentId == null;
    const nextAgentId = wasDraftTab ? newAgentId : tab.agentId;
    const shouldUpdateCwd =
      Boolean(newCwd) &&
      (!tab.cwd ||
        tab.cwd === "~" ||
        (wasDraftTab && DRAFT_WORKSPACE_FOLLOWING_KINDS.has(tab.kind)));
    const nextCwd = shouldUpdateCwd && newCwd ? newCwd : tab.cwd;
    if (nextAgentId === tab.agentId && nextCwd === tab.cwd) return tab;
    return {
      ...tab,
      agentId: nextAgentId,
      ...(nextCwd !== undefined ? { cwd: nextCwd } : {}),
    };
  };

  let nextByScope = s.sidePanelByScope;
  if (draftKey in nextByScope) {
    nextByScope = { ...nextByScope };
    delete nextByScope[draftKey];
  }

  const promotedTabs = validateTabsForScope(s.sidePanelTabs.map(rebindDraftTab), targetKey);
  const promotedActive = normalizeActiveTabId(promotedTabs, s.activeSidePanelTabId);
  const promotedSplit = normalizeSidePanelSplit(promotedTabs, promotedActive, s.sidePanelSplit);
  const promotedClosed = validateTabsForScope(
    s.sidePanelClosedTabs.map(rebindDraftTab),
    targetKey,
  ).slice(0, SIDE_PANEL_MAX_CLOSED_TABS);

  // Also migrate any draft side-question entries/draft text to the promoted session scope
  // so an open Side Question tab keeps its Q&A seamlessly after promotion.
  let nextSideQuestionsByScope = s.sideQuestionsByScope;
  if (s.sideQuestionsByScope[draftKey]?.length && !s.sideQuestionsByScope[targetKey]?.length) {
    nextSideQuestionsByScope = {
      ...s.sideQuestionsByScope,
      [targetKey]: s.sideQuestionsByScope[draftKey],
    };
    delete nextSideQuestionsByScope[draftKey];
  }
  let nextSideQuestionDrafts = s.sideQuestionDrafts;
  if (s.sideQuestionDrafts[draftKey] && !s.sideQuestionDrafts[targetKey]) {
    nextSideQuestionDrafts = {
      ...s.sideQuestionDrafts,
      [targetKey]: s.sideQuestionDrafts[draftKey],
    };
    delete nextSideQuestionDrafts[draftKey];
  }

  void now;
  return {
    ...s,
    sidePanelByScope: nextByScope,
    sidePanelScopeKey: targetKey,
    sidePanelTabs: promotedTabs,
    activeSidePanelTabId: promotedActive,
    sidePanelSplit: promotedSplit,
    sidePanelClosedTabs: promotedClosed,
    sideQuestionsByScope: nextSideQuestionsByScope,
    sideQuestionDrafts: nextSideQuestionDrafts,
  };
}

/**
 * Remaps optimistic agent IDs (`opt_*` -> `ag_*`) across `sidePanelByScope`, `sidePanelScopeKey`,
 * and live/saved tab metadata when the daemon confirms the real agent ID.
 */
export function remapOptimisticSidePanelScopes(
  s: OrbitUiState,
  hostId: string,
  replacedMap: Record<string, string>,
): OrbitUiState {
  const entries = Object.entries(replacedMap);
  if (entries.length === 0) return s;

  const resolveId = (id: string | null | undefined): string | null | undefined => {
    if (!id) return id;
    let cur = id;
    for (let i = 0; i < 4 && replacedMap[cur]; i += 1) {
      cur = replacedMap[cur];
    }
    return cur;
  };

  const remapTab = (tab: SidePanelTab): SidePanelTab => {
    const tabHost = tab.parentHostId ?? tab.hostId;
    if (tabHost && tabHost !== hostId && tab.hostId !== hostId) return tab;
    const nextAgentId = resolveId(tab.agentId);
    const nextParentId = resolveId(tab.parentAgentId);
    const nextChildId = tab.hostId === hostId ? resolveId(tab.childAgentId) : tab.childAgentId;
    const nextPeerId = tab.hostId === hostId ? resolveId(tab.peerAgentId) : tab.peerAgentId;
    if (
      nextAgentId === tab.agentId &&
      nextParentId === tab.parentAgentId &&
      nextChildId === tab.childAgentId &&
      nextPeerId === tab.peerAgentId
    ) {
      return tab;
    }
    const nextId =
      tab.kind === "peer" && nextPeerId && tab.id === `tab:peer:${hostId}::${tab.peerAgentId}`
        ? `tab:peer:${hostId}::${nextPeerId}`
        : tab.kind === "subagent" &&
            nextChildId &&
            tab.id === `tab:subagent:agent:${hostId}:${tab.childAgentId}`
          ? `tab:subagent:agent:${hostId}:${nextChildId}`
          : tab.id;
    return {
      ...tab,
      id: nextId,
      agentId: nextAgentId,
      parentAgentId: nextParentId,
      childAgentId: nextChildId,
      peerAgentId: nextPeerId ?? undefined,
    };
  };

  const remapTabList = (
    tabs: readonly SidePanelTab[],
    activeTabId: string | null,
    split: SidePanelSplitState | null,
    unseenTabIds: readonly string[],
  ) => {
    let changed = false;
    const idMap = new Map<string, string>();
    const nextTabs = tabs.map((t) => {
      const mapped = remapTab(t);
      if (mapped !== t) {
        changed = true;
        if (mapped.id !== t.id) idMap.set(t.id, mapped.id);
      }
      return mapped;
    });
    if (!changed) {
      return {
        changed: false,
        tabs: tabs as SidePanelTab[],
        activeTabId,
        split,
        unseenTabIds: unseenTabIds as string[],
      };
    }
    const nextActive = activeTabId ? (idMap.get(activeTabId) ?? activeTabId) : null;
    const nextSplit =
      split && idMap.has(split.secondaryTabId)
        ? { ...split, secondaryTabId: idMap.get(split.secondaryTabId)! }
        : split;
    const nextUnseen = unseenTabIds.map((id) => idMap.get(id) ?? id);
    return {
      changed: true,
      tabs: nextTabs,
      activeTabId: nextActive,
      split: nextSplit,
      unseenTabIds: nextUnseen,
    };
  };

  let byScopeChanged = false;
  const nextByScope: Record<SidePanelScopeKey, SidePanelSnapshot> = {};
  for (const [scopeKey, snap] of Object.entries(s.sidePanelByScope)) {
    const parsed = parseSidePanelScopeKey(scopeKey);
    let nextScopeKey = scopeKey;
    if (parsed?.kind === "agent" && parsed.hostId === hostId && parsed.agentId) {
      const mappedAgent = resolveId(parsed.agentId);
      if (mappedAgent && mappedAgent !== parsed.agentId) {
        nextScopeKey = `${hostId}::${mappedAgent}`;
        byScopeChanged = true;
      }
    }
    const tabRes = remapTabList(snap.tabs, snap.activeTabId, snap.split, snap.unseenTabIds ?? []);
    const closedRes = remapTabList(snap.closedTabs, null, null, []);
    if (tabRes.changed || closedRes.changed) {
      byScopeChanged = true;
    }
    if (!(nextScopeKey in nextByScope) || nextScopeKey === scopeKey) {
      nextByScope[nextScopeKey] =
        tabRes.changed || closedRes.changed
          ? {
              ...snap,
              tabs: tabRes.tabs,
              activeTabId: tabRes.activeTabId,
              split: tabRes.split,
              closedTabs: closedRes.tabs,
              unseenTabIds: tabRes.unseenTabIds,
            }
          : snap;
    }
  }

  let nextLiveScopeKey = s.sidePanelScopeKey;
  const parsedLive = parseSidePanelScopeKey(s.sidePanelScopeKey);
  if (parsedLive?.kind === "agent" && parsedLive.hostId === hostId && parsedLive.agentId) {
    const mappedAgent = resolveId(parsedLive.agentId);
    if (mappedAgent && mappedAgent !== parsedLive.agentId) {
      nextLiveScopeKey = `${hostId}::${mappedAgent}`;
    }
  }

  const liveRes = remapTabList(
    s.sidePanelTabs,
    s.activeSidePanelTabId,
    s.sidePanelSplit,
    s.sidePanelUnseenTabIds,
  );
  const liveClosedRes = remapTabList(s.sidePanelClosedTabs, null, null, []);

  if (
    !byScopeChanged &&
    nextLiveScopeKey === s.sidePanelScopeKey &&
    !liveRes.changed &&
    !liveClosedRes.changed
  ) {
    return s;
  }

  return {
    ...s,
    sidePanelByScope: byScopeChanged ? nextByScope : s.sidePanelByScope,
    sidePanelScopeKey: nextLiveScopeKey,
    sidePanelTabs: liveRes.changed ? liveRes.tabs : s.sidePanelTabs,
    activeSidePanelTabId: liveRes.changed ? liveRes.activeTabId : s.activeSidePanelTabId,
    sidePanelSplit: liveRes.changed ? liveRes.split : s.sidePanelSplit,
    sidePanelUnseenTabIds: liveRes.changed ? liveRes.unseenTabIds : s.sidePanelUnseenTabIds,
    sidePanelClosedTabs: liveClosedRes.changed ? liveClosedRes.tabs : s.sidePanelClosedTabs,
  };
}

function pruneTabsFromList(
  tabs: readonly SidePanelTab[],
  activeTabId: string | null,
  split: SidePanelSplitState | null,
  closedTabs: readonly SidePanelTab[],
  unseenTabIds: readonly string[],
  shouldRemove: (tab: SidePanelTab) => boolean,
): {
  changed: boolean;
  tabs: SidePanelTab[];
  activeTabId: string | null;
  split: SidePanelSplitState | null;
  closedTabs: SidePanelTab[];
  unseenTabIds: string[];
  closedAllTabs: boolean;
} {
  let removedActiveIdx = -1;
  const remaining: SidePanelTab[] = [];
  for (let i = 0; i < tabs.length; i += 1) {
    const tab = tabs[i];
    if (shouldRemove(tab)) {
      if (tab.id === activeTabId) {
        removedActiveIdx = remaining.length;
      }
    } else {
      remaining.push(tab);
    }
  }
  const remainingClosed = closedTabs.filter((t) => !shouldRemove(t));
  const tabsRemoved = remaining.length !== tabs.length;
  const closedRemoved = remainingClosed.length !== closedTabs.length;
  if (!tabsRemoved && !closedRemoved) {
    return {
      changed: false,
      tabs: tabs as SidePanelTab[],
      activeTabId,
      split,
      closedTabs: closedTabs as SidePanelTab[],
      unseenTabIds: unseenTabIds as string[],
      closedAllTabs: false,
    };
  }
  const nextActive = tabsRemoved
    ? normalizeActiveTabId(
        remaining,
        activeTabId,
        removedActiveIdx >= 0 ? removedActiveIdx : undefined,
      )
    : activeTabId;
  const nextSplit = tabsRemoved ? normalizeSidePanelSplit(remaining, nextActive, split) : split;
  const remainingIds = new Set(remaining.map((t) => t.id));
  const nextUnseen = unseenTabIds.filter((id) => remainingIds.has(id));
  return {
    changed: true,
    tabs: remaining.length > 0 ? remaining : EMPTY_SIDE_PANEL_TABS,
    activeTabId: nextActive,
    split: nextSplit,
    closedTabs: remainingClosed.length > 0 ? remainingClosed : EMPTY_SIDE_PANEL_TABS,
    unseenTabIds: nextUnseen.length > 0 ? nextUnseen : EMPTY_SIDE_PANEL_UNSEEN_IDS,
    closedAllTabs: tabsRemoved && remaining.length === 0,
  };
}

/**
 * Drops snapshots for archived/deleted threads and prunes any subagent/peer tabs referencing
 * those threads from live state and all other cached snapshots (without setting `userDismissed`).
 */
export function dropSidePanelScopes(
  s: OrbitUiState,
  removedThreadKeys: ReadonlySet<string>,
  removedAgents?: ReadonlyArray<{ hostId: string; agentId: string }>,
): OrbitUiState {
  if (removedThreadKeys.size === 0 && (!removedAgents || removedAgents.length === 0)) {
    return s;
  }
  const removedAgentKeySet = new Set<string>(removedThreadKeys);
  if (removedAgents) {
    for (const a of removedAgents) {
      removedAgentKeySet.add(`${a.hostId}::${a.agentId}`);
    }
  }
  const tabReferencesRemovedAgent = (tab: SidePanelTab): boolean => {
    if (tab.kind === "subagent" && tab.childAgentId) {
      const childHost = tab.hostId ?? tab.parentHostId;
      if (childHost && removedAgentKeySet.has(`${childHost}::${tab.childAgentId}`)) {
        return true;
      }
    }
    if (tab.kind === "peer" && tab.peerAgentId && tab.hostId) {
      if (removedAgentKeySet.has(`${tab.hostId}::${tab.peerAgentId}`)) {
        return true;
      }
    }
    return false;
  };

  let byScopeChanged = false;
  const nextByScope: Record<SidePanelScopeKey, SidePanelSnapshot> = {};
  for (const [scopeKey, snap] of Object.entries(s.sidePanelByScope)) {
    if (removedThreadKeys.has(scopeKey)) {
      byScopeChanged = true;
      continue;
    }
    const pruned = pruneTabsFromList(
      snap.tabs,
      snap.activeTabId,
      snap.split,
      snap.closedTabs,
      snap.unseenTabIds ?? EMPTY_SIDE_PANEL_UNSEEN_IDS,
      tabReferencesRemovedAgent,
    );
    if (pruned.changed) {
      byScopeChanged = true;
      const nextSnap: SidePanelSnapshot = {
        ...snap,
        open: pruned.closedAllTabs ? false : snap.open,
        tabs: pruned.tabs,
        activeTabId: pruned.activeTabId,
        split: pruned.split,
        closedTabs: pruned.closedTabs,
        unseenTabIds: pruned.unseenTabIds,
      };
      if (!isEmptyDefaultSnapshot(nextSnap)) {
        nextByScope[scopeKey] = nextSnap;
      }
    } else {
      nextByScope[scopeKey] = snap;
    }
  }

  const livePruned = pruneTabsFromList(
    s.sidePanelTabs,
    s.activeSidePanelTabId,
    s.sidePanelSplit,
    s.sidePanelClosedTabs,
    s.sidePanelUnseenTabIds,
    tabReferencesRemovedAgent,
  );

  if (!byScopeChanged && !livePruned.changed) {
    return s;
  }

  const nextOpen = livePruned.closedAllTabs ? false : s.sidePanelOpen;
  const sidebarPatch = livePruned.closedAllTabs
    ? resolveSidebarYieldForSidePanel(s, false)
    : {
        sidebarOpen: s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: s.sidebarAutoYieldedForSidePanel,
      };

  return {
    ...s,
    sidePanelByScope: byScopeChanged ? nextByScope : s.sidePanelByScope,
    sidePanelOpen: nextOpen,
    sidePanelTabs: livePruned.changed ? livePruned.tabs : s.sidePanelTabs,
    activeSidePanelTabId: livePruned.changed ? livePruned.activeTabId : s.activeSidePanelTabId,
    sidePanelSplit: livePruned.changed ? livePruned.split : s.sidePanelSplit,
    sidePanelClosedTabs: livePruned.changed ? livePruned.closedTabs : s.sidePanelClosedTabs,
    sidePanelUnseenTabIds: livePruned.changed ? livePruned.unseenTabIds : s.sidePanelUnseenTabIds,
    sidebarOpen: sidebarPatch.sidebarOpen,
    sidebarAutoYieldedForSidePanel: sidebarPatch.sidebarAutoYieldedForSidePanel,
    terminalFocused: nextOpen ? s.terminalFocused : false,
  };
}

/**
 * Removes archived subagent tabs from both live state (if currently on the parent's scope) and
 * the parent's stored snapshot, normalizing active tab, split state, and left-sidebar auto-yield.
 */
export function removeSubagentTabsFromScopes(
  s: OrbitUiState,
  hostId: string,
  parentAgentId: string,
  targets: ReadonlyArray<{
    id?: string | null;
    providerSubagentId?: string | null;
    childAgentId?: string | null;
  }>,
): OrbitUiState {
  if (targets.length === 0) return s;

  const matchesTarget = (tab: SidePanelTab): boolean => {
    if (tab.kind !== "subagent") return false;
    if (tab.parentAgentId && tab.parentAgentId !== parentAgentId) return false;
    if (tab.parentHostId && tab.parentHostId !== hostId) return false;
    return targets.some(
      (target) =>
        (Boolean(target.id) && tab.subagentCallId === target.id) ||
        (Boolean(target.providerSubagentId) &&
          tab.providerSubagentId === target.providerSubagentId) ||
        (Boolean(target.childAgentId) && tab.childAgentId === target.childAgentId),
    );
  };

  const parentScopeKey = `${hostId}::${parentAgentId}`;
  let nextByScope = s.sidePanelByScope;
  const storedParent = s.sidePanelByScope[parentScopeKey];
  if (storedParent) {
    const pruned = pruneTabsFromList(
      storedParent.tabs,
      storedParent.activeTabId,
      storedParent.split,
      storedParent.closedTabs,
      storedParent.unseenTabIds ?? EMPTY_SIDE_PANEL_UNSEEN_IDS,
      matchesTarget,
    );
    if (pruned.changed) {
      nextByScope = { ...s.sidePanelByScope };
      const nextSnap: SidePanelSnapshot = {
        ...storedParent,
        open: pruned.closedAllTabs ? false : storedParent.open,
        tabs: pruned.tabs,
        activeTabId: pruned.activeTabId,
        split: pruned.split,
        closedTabs: pruned.closedTabs,
        unseenTabIds: pruned.unseenTabIds,
      };
      if (isEmptyDefaultSnapshot(nextSnap)) {
        delete nextByScope[parentScopeKey];
      } else {
        nextByScope[parentScopeKey] = nextSnap;
      }
    }
  }

  const livePruned = pruneTabsFromList(
    s.sidePanelTabs,
    s.activeSidePanelTabId,
    s.sidePanelSplit,
    s.sidePanelClosedTabs,
    s.sidePanelUnseenTabIds,
    matchesTarget,
  );

  if (nextByScope === s.sidePanelByScope && !livePruned.changed) {
    return s;
  }

  const nextOpen = livePruned.closedAllTabs ? false : s.sidePanelOpen;
  const sidebarPatch = livePruned.closedAllTabs
    ? resolveSidebarYieldForSidePanel(s, false)
    : {
        sidebarOpen: s.sidebarOpen,
        sidebarAutoYieldedForSidePanel: s.sidebarAutoYieldedForSidePanel,
      };

  return {
    ...s,
    sidePanelByScope: nextByScope,
    sidePanelOpen: nextOpen,
    sidePanelTabs: livePruned.changed ? livePruned.tabs : s.sidePanelTabs,
    activeSidePanelTabId: livePruned.changed ? livePruned.activeTabId : s.activeSidePanelTabId,
    sidePanelSplit: livePruned.changed ? livePruned.split : s.sidePanelSplit,
    sidePanelClosedTabs: livePruned.changed ? livePruned.closedTabs : s.sidePanelClosedTabs,
    sidePanelUnseenTabIds: livePruned.changed ? livePruned.unseenTabIds : s.sidePanelUnseenTabIds,
    sidebarOpen: sidebarPatch.sidebarOpen,
    sidebarAutoYieldedForSidePanel: sidebarPatch.sidebarAutoYieldedForSidePanel,
    terminalFocused: nextOpen ? s.terminalFocused : false,
  };
}

/**
 * Single open entrypoint for all `open*Tab` actions. Enforces explicit tab scope binding,
 * unified left-sidebar auto-yield, `terminalFocused` consistency, and the `user` vs `auto`
 * auto-open & unseen-activity policy.
 */
export function applyOpenSidePanelTab(
  s: OrbitUiState,
  rawTab: SidePanelTab,
  opts?: OpenSidePanelTabOptions,
): OrbitUiState {
  const source: SidePanelOpenSource = opts?.source ?? "user";
  const scopeKey =
    s.sidePanelScopeKey ??
    resolveSidePanelScopeKey({
      selectedHostId: s.selectedHostId,
      selectedAgentId: s.selectedAgentId,
      isDraftNewSession: s.isDraftNewSession,
      hostDetails: s.hostDetails,
    });
  const currentScope = resolveCurrentTabScope(s);

  const boundTab: SidePanelTab = {
    ...rawTab,
    hostId: rawTab.hostId ?? currentScope.hostId,
    agentId: rawTab.agentId !== undefined ? rawTab.agentId : currentScope.agentId,
    ...(rawTab.cwd === undefined && currentScope.cwd ? { cwd: currentScope.cwd } : {}),
  };

  let nextTabs: SidePanelTab[];
  let resolvedTabId: string;

  if (opts?.upsertTabs) {
    const upserted = opts.upsertTabs(s.sidePanelTabs);
    nextTabs = upserted.tabs;
    resolvedTabId = upserted.tabId;
  } else {
    const existingIdx = s.sidePanelTabs.findIndex((t) => t.id === boundTab.id);
    resolvedTabId = boundTab.id;
    if (existingIdx >= 0) {
      const existing = s.sidePanelTabs[existingIdx];
      const updated = opts?.updateExisting ? opts.updateExisting(existing, boundTab) : existing;
      if (updated === existing) {
        nextTabs = s.sidePanelTabs;
      } else {
        nextTabs = s.sidePanelTabs.slice();
        nextTabs[existingIdx] = updated;
      }
    } else {
      nextTabs = [...s.sidePanelTabs, boundTab];
    }
  }

  const nextClosedTabs = s.sidePanelClosedTabs.some((t) => t.id === resolvedTabId)
    ? s.sidePanelClosedTabs.filter((t) => t.id !== resolvedTabId)
    : s.sidePanelClosedTabs;

  let nextOpen: boolean;
  let nextActiveTabId: string | null;
  let nextUserDismissed: boolean;
  let nextCollapsedByResize: boolean;
  let nextUnseenTabIds: string[];
  let nextOverviewPopoverOpen = s.overviewPopoverOpen;

  if (source === "user") {
    nextOpen = true;
    nextActiveTabId = resolvedTabId;
    nextUserDismissed = false;
    nextCollapsedByResize = false;
    nextUnseenTabIds = EMPTY_SIDE_PANEL_UNSEEN_IDS;
    nextOverviewPopoverOpen = false;
  } else if (s.sidePanelOpen) {
    // Auto while panel is already open: never steal focus from another open tab;
    // only activate when the active surface is the empty launchpad AND activate=true.
    nextOpen = true;
    nextActiveTabId =
      s.activeSidePanelTabId === resolvedTabId
        ? resolvedTabId
        : s.activeSidePanelTabId === null && opts?.activate === true
          ? resolvedTabId
          : s.activeSidePanelTabId;
    nextUserDismissed = false;
    nextCollapsedByResize = false;
    nextUnseenTabIds = EMPTY_SIDE_PANEL_UNSEEN_IDS;
  } else if (s.sidePanelUserDismissed) {
    // Auto while user explicitly closed the panel: add in background and show unseen dot.
    nextOpen = false;
    nextActiveTabId = s.activeSidePanelTabId ?? resolvedTabId;
    nextUserDismissed = true;
    nextCollapsedByResize = s.sidePanelCollapsedByResize;
    nextUnseenTabIds = s.sidePanelUnseenTabIds.includes(resolvedTabId)
      ? s.sidePanelUnseenTabIds
      : [...s.sidePanelUnseenTabIds, resolvedTabId];
  } else if (s.isCompactViewport) {
    // Auto on compact viewport: do not pop open over the narrow stage; mark collapsedByResize & unseen.
    nextOpen = false;
    nextActiveTabId = s.activeSidePanelTabId ?? resolvedTabId;
    nextUserDismissed = false;
    nextCollapsedByResize = true;
    nextUnseenTabIds = s.sidePanelUnseenTabIds.includes(resolvedTabId)
      ? s.sidePanelUnseenTabIds
      : [...s.sidePanelUnseenTabIds, resolvedTabId];
  } else {
    // Auto & panel closed & not dismissed: open & activate.
    nextOpen = true;
    nextActiveTabId = resolvedTabId;
    nextUserDismissed = false;
    nextCollapsedByResize = false;
    nextUnseenTabIds = EMPTY_SIDE_PANEL_UNSEEN_IDS;
    nextOverviewPopoverOpen = false;
  }

  const nextSplit = normalizeSidePanelSplit(nextTabs, nextActiveTabId, s.sidePanelSplit);
  const sidebarPatch =
    nextOpen !== s.sidePanelOpen
      ? resolveSidebarYieldForSidePanel(s, nextOpen)
      : {
          sidebarOpen: s.sidebarOpen,
          sidebarAutoYieldedForSidePanel: s.sidebarAutoYieldedForSidePanel,
        };

  let nextTerminalFocused: boolean;
  if (opts?.terminalFocused === "preserve") {
    nextTerminalFocused = nextOpen ? s.terminalFocused : false;
  } else if (opts?.terminalFocused === true) {
    nextTerminalFocused = nextOpen && nextActiveTabId === resolvedTabId ? true : s.terminalFocused;
  } else {
    // Reset terminal focus when switching to a non-terminal tab or closing; preserve if background-added while user is typing in terminal.
    nextTerminalFocused =
      nextOpen && nextActiveTabId !== resolvedTabId ? s.terminalFocused : false;
  }

  return {
    ...s,
    ...(opts?.patch ?? {}),
    sidePanelScopeKey: scopeKey,
    sidePanelOpen: nextOpen,
    sidePanelTabs: nextTabs,
    activeSidePanelTabId: nextActiveTabId,
    sidePanelSplit: nextSplit,
    sidePanelUserDismissed: nextUserDismissed,
    sidePanelClosedTabs: nextClosedTabs,
    sidePanelUnseenTabIds: nextUnseenTabIds,
    sidePanelCollapsedByResize: nextCollapsedByResize,
    overviewPopoverOpen: nextOverviewPopoverOpen,
    pendingUnsavedFileAction: source === "user" ? null : s.pendingUnsavedFileAction,
    terminalFocused: nextTerminalFocused,
    sidebarOpen: sidebarPatch.sidebarOpen,
    sidebarAutoYieldedForSidePanel: sidebarPatch.sidebarAutoYieldedForSidePanel,
  };
}

function isClosedTabTargetAlive(s: OrbitUiState, candidate: SidePanelTab): boolean {
  if (candidate.kind === "peer") {
    if (!candidate.hostId || !candidate.peerAgentId) return false;
    const targetKey = `${candidate.hostId}::${candidate.peerAgentId}`;
    if (s.archivedThreads[targetKey]) return false;
    const detail = s.hostDetails[candidate.hostId];
    if (detail) {
      const ag = detail.agents.find((a) => a.agent_id === candidate.peerAgentId);
      if (!ag || ag.archived_at_rfc3339) return false;
    }
    return true;
  }
  if (candidate.kind === "subagent") {
    const parentHostId = candidate.parentHostId ?? candidate.hostId;
    const parentAgentId = candidate.parentAgentId;
    if (parentHostId && parentAgentId) {
      const archived = s.archivedSubagents;
      if (
        (candidate.subagentCallId &&
          archived[`${parentHostId}::${parentAgentId}::${candidate.subagentCallId}`]) ||
        (candidate.providerSubagentId &&
          archived[`${parentHostId}::${parentAgentId}::${candidate.providerSubagentId}`]) ||
        (candidate.childAgentId &&
          archived[`${parentHostId}::${parentAgentId}::${candidate.childAgentId}`])
      ) {
        return false;
      }
    }
    if (candidate.childAgentId) {
      const childHostId = candidate.hostId ?? candidate.parentHostId;
      if (childHostId) {
        const childKey = `${childHostId}::${candidate.childAgentId}`;
        if (s.archivedThreads[childKey]) return false;
        const detail = s.hostDetails[childHostId];
        if (detail) {
          const ag = detail.agents.find((a) => a.agent_id === candidate.childAgentId);
          if (!ag || ag.archived_at_rfc3339) return false;
        }
      }
    }
    return true;
  }
  if (candidate.kind === "board") {
    if (!candidate.hostId || !candidate.workspaceId) return false;
    const detail = s.hostDetails[candidate.hostId];
    if (detail) {
      const ws = detail.workspaces.find((w) => w.workspace_id === candidate.workspaceId);
      if (!ws || ws.archived_at) return false;
    }
    return true;
  }
  return true;
}

/**
 * Reopens the most recently closed valid tab for the current session scope (⌘⇧T).
 */
export function applyReopenClosedSidePanelTab(s: OrbitUiState): {
  state: OrbitUiState;
  reopenedTab: SidePanelTab | null;
} {
  if (s.sidePanelClosedTabs.length === 0) {
    return { state: s, reopenedTab: null };
  }
  const scopeKey =
    s.sidePanelScopeKey ??
    resolveSidePanelScopeKey({
      selectedHostId: s.selectedHostId,
      selectedAgentId: s.selectedAgentId,
      isDraftNewSession: s.isDraftNewSession,
      hostDetails: s.hostDetails,
      archivedThreads: s.archivedThreads,
    });
  const parsed = parseSidePanelScopeKey(scopeKey);
  const openIds = new Set(s.sidePanelTabs.map((t) => t.id));

  const deadIndices = new Set<number>();
  let targetIdx = -1;
  for (let i = 0; i < s.sidePanelClosedTabs.length; i += 1) {
    const candidate = s.sidePanelClosedTabs[i];
    if (!candidate) {
      deadIndices.add(i);
      continue;
    }
    if (parsed && !isTabValidForScope(candidate, parsed)) {
      deadIndices.add(i);
      continue;
    }
    if (!isClosedTabTargetAlive(s, candidate)) {
      deadIndices.add(i);
      continue;
    }
    if (openIds.has(candidate.id)) continue;
    targetIdx = i;
    break;
  }

  if (targetIdx < 0) {
    if (deadIndices.size === 0) {
      return { state: s, reopenedTab: null };
    }
    const prunedClosed = s.sidePanelClosedTabs.filter((_, idx) => !deadIndices.has(idx));
    return {
      state: {
        ...s,
        sidePanelClosedTabs: prunedClosed.length > 0 ? prunedClosed : EMPTY_SIDE_PANEL_TABS,
      },
      reopenedTab: null,
    };
  }

  const reopenedTab = stripTransientTabFields(s.sidePanelClosedTabs[targetIdx]);
  const remainingClosed = s.sidePanelClosedTabs.filter(
    (_, idx) => idx !== targetIdx && !deadIndices.has(idx),
  );
  const baseState: OrbitUiState = {
    ...s,
    sidePanelClosedTabs: remainingClosed.length > 0 ? remainingClosed : EMPTY_SIDE_PANEL_TABS,
  };
  const nextState = applyOpenSidePanelTab(baseState, reopenedTab, {
    source: "user",
    terminalFocused: reopenedTab.kind === "terminal" ? true : false,
  });
  return { state: nextState, reopenedTab };
}

function sanitizePersistedTab(raw: unknown): SidePanelTab | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const obj = raw as Record<string, unknown>;
  if (typeof obj.id !== "string" || !obj.id.trim()) return null;
  if (typeof obj.kind !== "string" || !VALID_TAB_KINDS.has(obj.kind as SidePanelTabKind)) {
    return null;
  }
  if (typeof obj.title !== "string" || !obj.title.trim()) return null;

  const tab: SidePanelTab = {
    id: obj.id,
    kind: obj.kind as SidePanelTabKind,
    title: obj.title,
  };

  if (typeof obj.filePath === "string") tab.filePath = obj.filePath;
  if (typeof obj.hostId === "string" && obj.hostId) tab.hostId = obj.hostId;
  if (typeof obj.parentHostId === "string" && obj.parentHostId) tab.parentHostId = obj.parentHostId;
  if (typeof obj.agentId === "string" || obj.agentId === null) tab.agentId = obj.agentId;
  if (typeof obj.cwd === "string") tab.cwd = obj.cwd;
  if (typeof obj.url === "string") tab.url = obj.url;
  if (typeof obj.subagentCallId === "string") tab.subagentCallId = obj.subagentCallId;
  if (typeof obj.subagentRole === "string") tab.subagentRole = obj.subagentRole;
  if (typeof obj.subagentPrompt === "string") tab.subagentPrompt = obj.subagentPrompt;
  if (typeof obj.childAgentId === "string" || obj.childAgentId === null) {
    tab.childAgentId = obj.childAgentId;
  }
  if (typeof obj.providerSubagentId === "string" || obj.providerSubagentId === null) {
    tab.providerSubagentId = obj.providerSubagentId;
  }
  if (typeof obj.parentAgentId === "string" || obj.parentAgentId === null) {
    tab.parentAgentId = obj.parentAgentId;
  }
  if (typeof obj.workspaceId === "string") tab.workspaceId = obj.workspaceId;
  if (typeof obj.peerAgentId === "string") tab.peerAgentId = obj.peerAgentId;
  if (typeof obj.artifactId === "string") tab.artifactId = obj.artifactId;
  if (typeof obj.sideQuestionEntryId === "string") {
    tab.sideQuestionEntryId = obj.sideQuestionEntryId;
  }

  return tab;
}

function sanitizePersistedSnapshot(
  scopeKey: SidePanelScopeKey,
  raw: unknown,
  fallbackNow: number,
): SidePanelSnapshot | null {
  if (!raw || typeof raw !== "object" || Array.isArray(raw)) return null;
  const obj = raw as Record<string, unknown>;
  const rawTabs = Array.isArray(obj.tabs)
    ? obj.tabs.map(sanitizePersistedTab).filter((t): t is SidePanelTab => t !== null)
    : [];
  const tabs = validateTabsForScope(rawTabs, scopeKey, { stripFocusNonce: true });
  const rawActive = typeof obj.activeTabId === "string" ? obj.activeTabId : null;
  const activeTabId = normalizeActiveTabId(tabs, rawActive);

  let rawSplit: SidePanelSplitState | null = null;
  if (obj.split && typeof obj.split === "object" && !Array.isArray(obj.split)) {
    const sp = obj.split as Record<string, unknown>;
    if (typeof sp.secondaryTabId === "string") {
      rawSplit = {
        secondaryTabId: sp.secondaryTabId,
        ratio: typeof sp.ratio === "number" && Number.isFinite(sp.ratio) ? sp.ratio : 0.5,
      };
    }
  }
  const split = normalizeSidePanelSplit(tabs, activeTabId, rawSplit);

  const rawClosed = Array.isArray(obj.closedTabs)
    ? obj.closedTabs.map(sanitizePersistedTab).filter((t): t is SidePanelTab => t !== null)
    : [];
  const closedTabs = validateTabsForScope(rawClosed, scopeKey, { stripFocusNonce: true }).slice(
    0,
    SIDE_PANEL_MAX_CLOSED_TABS,
  );

  const validTabIds = new Set(tabs.map((t) => t.id));
  const rawUnseen = Array.isArray(obj.unseenTabIds)
    ? obj.unseenTabIds.filter((id): id is string => typeof id === "string" && validTabIds.has(id))
    : [];
  const unseenTabIds =
    rawUnseen.length > 0 ? Array.from(new Set(rawUnseen)) : EMPTY_SIDE_PANEL_UNSEEN_IDS;

  const allTabsInvalidated = rawTabs.length > 0 && tabs.length === 0;
  const open = Boolean(obj.open) && !allTabsInvalidated;
  const userDismissed = Boolean(obj.userDismissed);
  const lastUsedAt =
    typeof obj.lastUsedAt === "number" && Number.isFinite(obj.lastUsedAt) && obj.lastUsedAt > 0
      ? obj.lastUsedAt
      : fallbackNow;

  const snap: SidePanelSnapshot = {
    open,
    tabs,
    activeTabId,
    split,
    userDismissed,
    closedTabs,
    unseenTabIds: open ? EMPTY_SIDE_PANEL_UNSEEN_IDS : unseenTabIds,
    lastUsedAt,
  };
  return isEmptyDefaultSnapshot(snap) ? null : snap;
}

export function serializeSidePanelPersistence(
  s: Pick<
    OrbitUiState,
    | "sidePanelByScope"
    | "sidePanelScopeKey"
    | "selectedHostId"
    | "selectedAgentId"
    | "isDraftNewSession"
    | "hostDetails"
    | "sidePanelOpen"
    | "sidePanelCollapsedByResize"
    | "sidePanelTabs"
    | "activeSidePanelTabId"
    | "sidePanelSplit"
    | "sidePanelUserDismissed"
    | "sidePanelClosedTabs"
    | "sidePanelUnseenTabIds"
  >,
  now: number = Date.now(),
): PersistedSidePanelPayloadV1 {
  const merged: Record<SidePanelScopeKey, SidePanelSnapshot> = {};

  for (const [scopeKey, snap] of Object.entries(s.sidePanelByScope)) {
    if (FORBIDDEN_KEYS.has(scopeKey) || !parseSidePanelScopeKey(scopeKey)) continue;
    const cleanedTabs = snap.tabs.map((t) => stripTransientTabFields(t, { stripFocusLines: true }));
    const cleanedClosed = snap.closedTabs
      .slice(0, SIDE_PANEL_MAX_CLOSED_TABS)
      .map((t) => stripTransientTabFields(t, { stripFocusLines: true }));
    const activeTabId = normalizeActiveTabId(cleanedTabs, snap.activeTabId);
    const split = normalizeSidePanelSplit(cleanedTabs, activeTabId, snap.split);
    const cleanedSnap: SidePanelSnapshot = {
      open: snap.open,
      tabs: cleanedTabs,
      activeTabId,
      split,
      userDismissed: snap.userDismissed,
      closedTabs: cleanedClosed,
      unseenTabIds: snap.open
        ? EMPTY_SIDE_PANEL_UNSEEN_IDS
        : (snap.unseenTabIds ?? EMPTY_SIDE_PANEL_UNSEEN_IDS),
      lastUsedAt: snap.lastUsedAt,
    };
    if (!isEmptyDefaultSnapshot(cleanedSnap)) {
      merged[scopeKey] = cleanedSnap;
    }
  }

  const liveKey =
    s.sidePanelScopeKey ??
    resolveSidePanelScopeKey({
      selectedHostId: s.selectedHostId,
      selectedAgentId: s.selectedAgentId,
      isDraftNewSession: s.isDraftNewSession,
      hostDetails: s.hostDetails,
    });

  if (liveKey && !FORBIDDEN_KEYS.has(liveKey) && parseSidePanelScopeKey(liveKey)) {
    const liveSnap = buildSnapshotFromLiveState(s, now, { stripFocusLines: true });
    if (isEmptyDefaultSnapshot(liveSnap)) {
      delete merged[liveKey];
    } else {
      merged[liveKey] = liveSnap;
    }
  }

  return {
    version: SIDE_PANEL_SCHEMA_VERSION,
    byScope: pruneSidePanelByScopeLru(merged, SIDE_PANEL_MAX_SCOPES),
  };
}

export function loadPersistedSidePanelSnapshots(
  storage?: Pick<Storage, "getItem"> | null,
): Record<SidePanelScopeKey, SidePanelSnapshot> {
  const store =
    storage !== undefined
      ? storage
      : typeof window !== "undefined"
        ? window.localStorage
        : null;
  if (!store) return {};
  try {
    const raw = store.getItem(SIDE_PANEL_STORAGE_KEY);
    if (!raw) return {};
    const parsed: unknown = JSON.parse(raw);
    if (!parsed || typeof parsed !== "object" || Array.isArray(parsed)) return {};
    const envelope = parsed as Record<string, unknown>;
    if (envelope.version !== SIDE_PANEL_SCHEMA_VERSION) return {};
    if (
      !envelope.byScope ||
      typeof envelope.byScope !== "object" ||
      Array.isArray(envelope.byScope)
    ) {
      return {};
    }

    const now = Date.now();
    const out: Record<SidePanelScopeKey, SidePanelSnapshot> = {};
    for (const [scopeKey, rawSnap] of Object.entries(
      envelope.byScope as Record<string, unknown>,
    )) {
      if (FORBIDDEN_KEYS.has(scopeKey) || !parseSidePanelScopeKey(scopeKey)) continue;
      const sanitized = sanitizePersistedSnapshot(scopeKey, rawSnap, now);
      if (sanitized) {
        out[scopeKey] = sanitized;
      }
    }
    return pruneSidePanelByScopeLru(out, SIDE_PANEL_MAX_SCOPES);
  } catch {
    return {};
  }
}

export function savePersistedSidePanelSnapshots(
  s: Pick<
    OrbitUiState,
    | "sidePanelByScope"
    | "sidePanelScopeKey"
    | "selectedHostId"
    | "selectedAgentId"
    | "isDraftNewSession"
    | "hostDetails"
    | "sidePanelOpen"
    | "sidePanelCollapsedByResize"
    | "sidePanelTabs"
    | "activeSidePanelTabId"
    | "sidePanelSplit"
    | "sidePanelUserDismissed"
    | "sidePanelClosedTabs"
    | "sidePanelUnseenTabIds"
  >,
  storage?: Pick<Storage, "setItem"> | null,
  now: number = Date.now(),
): void {
  const store =
    storage !== undefined
      ? storage
      : typeof window !== "undefined"
        ? window.localStorage
        : null;
  if (!store) return;
  try {
    const payload = serializeSidePanelPersistence(s, now);
    store.setItem(SIDE_PANEL_STORAGE_KEY, JSON.stringify(payload));
  } catch {
    // Ignore storage quota or serialization errors
  }
}
