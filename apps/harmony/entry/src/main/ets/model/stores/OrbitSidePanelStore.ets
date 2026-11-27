import { orbitLogWarn } from '../OrbitLogger';
import { nonBlankPath } from '../OrbitTypes';

export const ORBIT_SIDE_PANEL_STORAGE_KEY: string = 'orbit.harmony.v2.sidePanel';
export const ORBIT_SIDE_PANEL_SCHEMA_VERSION: number = 1;
export const ORBIT_SIDE_PANEL_DEFAULT_WIDTH_VP: number = 360;
export const ORBIT_SIDE_PANEL_MIN_WIDTH_VP: number = 320;
export const ORBIT_SIDE_PANEL_MAX_WIDTH_MD_VP: number = 420;
export const ORBIT_SIDE_PANEL_MAX_WIDTH_LG_VP: number = 680;
export const ORBIT_SIDE_PANEL_MAX_SCOPES: number = 100;
export const ORBIT_SIDE_PANEL_MAX_CLOSED_TABS: number = 10;
export const ORBIT_SIDE_PANEL_DEBOUNCE_MS: number = 220;

export type OrbitSidePanelKind =
  | 'review'
  | 'files'
  | 'file'
  | 'terminal'
  | 'board'
  | 'side_question'
  | 'subagent'
  | 'peer'
  | 'briefing'
  | 'automations';

export interface OrbitSidePanelTab {
  tabId: string;
  kind: OrbitSidePanelKind;
  hostId: string;
  agentId: string | null;
  cwd: string | null;
  path: string | null;
  parentHostId: string | null;
  title: string;
  closable: boolean;
  // Optional scoped entity metadata (never resolved from global selection)
  parentAgentId?: string | null;
  directParentHostId?: string | null;
  directParentAgentId?: string | null;
  childAgentId?: string | null;
  providerSubagentId?: string | null;
  subagentCallId?: string | null;
  workspaceId?: string | null;
  terminalSlot?: number | null;
  gitStaged?: boolean;
}

export interface OrbitOpenSidePanelTabOptions {
  source: 'user' | 'auto';
  activate: boolean;
}

export interface OrbitAgentParentEdge {
  parentHostId: string | null;
  parentAgentId: string | null;
  archived: boolean;
}

export interface OrbitAgentParentLookup {
  getParentEdge(hostId: string, agentId: string): OrbitAgentParentEdge | null;
  hasCachedHostDetail?(hostId: string): boolean;
  hasAgentCard?(hostId: string, agentId: string): boolean;
}

export interface OrbitResolvedRootScope {
  hostId: string;
  rootAgentId: string | null;
  unresolvedHostId?: string | null;
  missingAgentCard?: boolean;
}

export interface OrbitSidePanelStorage {
  getSidePanelStateJson(): string;
  setSidePanelStateJson(payloadJson: string, onAsyncFailure?: () => void): boolean;
}

export interface OrbitSidePanelVisibilityListener {
  onSidePanelVisibilityChanged(isOpen: boolean): void;
  onSidePanelTabClosed?(scopeKey: string, closedTab: OrbitSidePanelTab): void;
}

interface PersistedSnapshotRecord {
  open: boolean;
  tabs: OrbitSidePanelTab[];
  activeTabId: string;
  unseenTabIds: string[];
  widthVp: number;
  closedTabs: OrbitSidePanelTab[];
  userDismissed: boolean;
  lastUsedAt: number;
  lastUsedOrder: number;
}

interface PersistedSidePanelEnvelopeV1 {
  version: number;
  byScope: Record<string, PersistedSnapshotRecord>;
}

const VALID_PANEL_KINDS: string[] = [
  'review',
  'files',
  'file',
  'terminal',
  'board',
  'side_question',
  'subagent',
  'peer',
  'briefing',
  'automations'
];

const DRAFT_CWD_FOLLOWING_KINDS: string[] = [
  'review',
  'files',
  'terminal',
  'board',
  'side_question',
  'automations'
];

const FORBIDDEN_SCOPE_KEYS: string[] = ['__proto__', 'constructor', 'prototype'];

function isValidPanelKind(raw: string): boolean {
  return VALID_PANEL_KINDS.indexOf(raw) >= 0;
}

function isDraftCwdFollowingKind(kind: OrbitSidePanelKind): boolean {
  return DRAFT_CWD_FOLLOWING_KINDS.indexOf(kind) >= 0;
}

function isForbiddenScopeKey(key: string): boolean {
  return FORBIDDEN_SCOPE_KEYS.indexOf(key) >= 0;
}

/**
 * Unambiguous tuple encoder for tab IDs, cache keys, and composite entity identities (R2-P0-1).
 * Uses JSON array encoding so delimiters like `::`, `/`, or `|` inside paths/IDs can NEVER collide
 * (e.g. `["/repo","x::y"]` vs `["/repo::x","y"]` produce distinct strings).
 */
export function encodeSidePanelKeyTuple(parts: string[]): string {
  return JSON.stringify(parts);
}

export function canonicalFileTabId(hostId: string, cwd: string, path: string): string {
  return `file:${encodeSidePanelKeyTuple([hostId.trim(), nonBlankPath(cwd), nonBlankPath(path)])}`;
}

export function canonicalCwdToolTabId(kind: OrbitSidePanelKind, hostId: string, cwd: string): string {
  return `${kind}:${encodeSidePanelKeyTuple([hostId.trim(), nonBlankPath(cwd)])}`;
}

export function canonicalManagedSubagentTabId(childHostId: string, childAgentId: string): string {
  return `subagent:${encodeSidePanelKeyTuple(['managed', childHostId.trim(), childAgentId.trim()])}`;
}

export function canonicalProviderSubagentTabId(
  directParentHostId: string,
  directParentAgentId: string,
  providerSubagentId: string
): string {
  return `subagent:${encodeSidePanelKeyTuple([
    'provider',
    directParentHostId.trim(),
    directParentAgentId.trim(),
    providerSubagentId.trim()
  ])}`;
}

export function canonicalPeerTabId(peerHostId: string, peerAgentId: string): string {
  return `peer:${encodeSidePanelKeyTuple([peerHostId.trim(), peerAgentId.trim()])}`;
}

export function canonicalSideQuestionTabId(scopeKey: string): string {
  return encodeSidePanelKeyTuple(['side_question', scopeKey.trim()]);
}

export function isSameFileTarget(
  aHost: string,
  aCwd: string,
  aPath: string,
  bHost: string,
  bCwd: string,
  bPath: string
): boolean {
  return (
    aHost.trim() === bHost.trim() &&
    nonBlankPath(aCwd) === nonBlankPath(bCwd) &&
    nonBlankPath(aPath) === nonBlankPath(bPath)
  );
}

export function isSameFileTab(a: OrbitSidePanelTab, b: OrbitSidePanelTab): boolean {
  if (a.kind !== 'file' || b.kind !== 'file') {
    return false;
  }
  return isSameFileTarget(
    a.hostId,
    a.cwd !== null ? a.cwd : '',
    a.path !== null ? a.path : '',
    b.hostId,
    b.cwd !== null ? b.cwd : '',
    b.path !== null ? b.path : ''
  );
}

export function orbitSidePanelScopeKeyFor(host: string, rootAgent: string | null): string {
  const cleanHost = host.trim();
  const cleanAgent = rootAgent !== null ? rootAgent.trim() : '';
  if (cleanAgent.length > 0) {
    return `${cleanHost}::${cleanAgent}`;
  }
  return `draft::${cleanHost}`;
}

export function resolveRootAgentScope(
  hostId: string,
  agentId: string | null,
  lookup: OrbitAgentParentLookup | null,
  requireStartCardExists: boolean = false
): OrbitResolvedRootScope {
  const cleanHost = hostId.trim();
  const cleanAgent = agentId !== null ? agentId.trim() : '';
  if (cleanHost.length === 0 || cleanAgent.length === 0 || lookup === null) {
    return {
      hostId: cleanHost,
      rootAgentId: cleanAgent.length > 0 ? cleanAgent : null,
      unresolvedHostId: null,
      missingAgentCard: false
    };
  }
  let rootHost = cleanHost;
  let rootAgent = cleanAgent;
  let followedParent = false;
  const visited: Set<string> = new Set<string>();
  visited.add(encodeSidePanelKeyTuple([rootHost, rootAgent]));

  let reachedHopLimit = false;
  for (let depth = 0; depth < 16; depth++) {
    if (lookup.hasCachedHostDetail !== undefined && !lookup.hasCachedHostDetail(rootHost)) {
      return {
        hostId: rootHost,
        rootAgentId: rootAgent,
        unresolvedHostId: rootHost,
        missingAgentCard: false
      };
    }
    // Distinguish "agent card missing from cached host detail" from "verified root with no parent" (R3-P1-2).
    if (
      (followedParent || requireStartCardExists) &&
      lookup.hasAgentCard !== undefined &&
      !lookup.hasAgentCard(rootHost, rootAgent)
    ) {
      return {
        hostId: rootHost,
        rootAgentId: rootAgent,
        unresolvedHostId: rootHost,
        missingAgentCard: true
      };
    }
    const edge = lookup.getParentEdge(rootHost, rootAgent);
    if (edge === null || edge.parentAgentId === null || edge.parentAgentId.trim().length === 0) {
      break;
    }
    const nextHost = (edge.parentHostId !== null && edge.parentHostId.trim().length > 0)
      ? edge.parentHostId.trim()
      : rootHost;
    const nextAgent = edge.parentAgentId.trim();
    const visitKey = encodeSidePanelKeyTuple([nextHost, nextAgent]);
    if (visited.has(visitKey)) {
      break;
    }
    visited.add(visitKey);
    if (lookup.hasCachedHostDetail !== undefined && !lookup.hasCachedHostDetail(nextHost)) {
      return {
        hostId: nextHost,
        rootAgentId: nextAgent,
        unresolvedHostId: nextHost,
        missingAgentCard: false
      };
    }
    if (lookup.hasAgentCard !== undefined && !lookup.hasAgentCard(nextHost, nextAgent)) {
      return {
        hostId: nextHost,
        rootAgentId: nextAgent,
        unresolvedHostId: nextHost,
        missingAgentCard: true
      };
    }
    const parentEdge = lookup.getParentEdge(nextHost, nextAgent);
    if (parentEdge !== null && parentEdge.archived) {
      break;
    }
    rootHost = nextHost;
    rootAgent = nextAgent;
    followedParent = true;
    if (depth === 15) {
      reachedHopLimit = true;
    }
  }
  if (reachedHopLimit) {
    const trailingEdge = lookup.getParentEdge(rootHost, rootAgent);
    if (
      trailingEdge !== null &&
      trailingEdge.parentAgentId !== null &&
      trailingEdge.parentAgentId.trim().length > 0 &&
      !trailingEdge.archived
    ) {
      const nextHost = (trailingEdge.parentHostId !== null && trailingEdge.parentHostId.trim().length > 0)
        ? trailingEdge.parentHostId.trim()
        : rootHost;
      return {
        hostId: rootHost,
        rootAgentId: rootAgent,
        unresolvedHostId: nextHost,
        missingAgentCard: false
      };
    }
  }

  return {
    hostId: rootHost,
    rootAgentId: rootAgent,
    unresolvedHostId: null,
    missingAgentCard: false
  };
}

function cloneTab(tab: OrbitSidePanelTab): OrbitSidePanelTab {
  const copy: OrbitSidePanelTab = {
    tabId: tab.tabId,
    kind: tab.kind,
    hostId: tab.hostId,
    agentId: tab.agentId,
    cwd: tab.cwd,
    path: tab.path,
    parentHostId: tab.parentHostId,
    title: tab.title,
    closable: tab.closable
  };
  if (tab.parentAgentId !== undefined) {
    copy.parentAgentId = tab.parentAgentId;
  }
  if (tab.directParentHostId !== undefined) {
    copy.directParentHostId = tab.directParentHostId;
  }
  if (tab.directParentAgentId !== undefined) {
    copy.directParentAgentId = tab.directParentAgentId;
  }
  if (tab.childAgentId !== undefined) {
    copy.childAgentId = tab.childAgentId;
  }
  if (tab.providerSubagentId !== undefined) {
    copy.providerSubagentId = tab.providerSubagentId;
  }
  if (tab.subagentCallId !== undefined) {
    copy.subagentCallId = tab.subagentCallId;
  }
  if (tab.workspaceId !== undefined) {
    copy.workspaceId = tab.workspaceId;
  }
  if (tab.terminalSlot !== undefined) {
    copy.terminalSlot = tab.terminalSlot;
  }
  if (tab.gitStaged !== undefined) {
    copy.gitStaged = tab.gitStaged;
  }
  return copy;
}

function sanitizePersistedTab(rawObj: Object | null | undefined): OrbitSidePanelTab | null {
  if (rawObj === null || rawObj === undefined || typeof rawObj !== 'object' || Array.isArray(rawObj)) {
    return null;
  }
  const rec = rawObj as Record<string, Object | null | undefined>;
  const rawTabId = rec['tabId'];
  const rawKind = rec['kind'];
  const rawHostId = rec['hostId'];
  const rawTitle = rec['title'];
  if (
    typeof rawTabId !== 'string' ||
    rawTabId.trim().length === 0 ||
    typeof rawKind !== 'string' ||
    !isValidPanelKind(rawKind) ||
    typeof rawHostId !== 'string' ||
    rawHostId.trim().length === 0 ||
    typeof rawTitle !== 'string'
  ) {
    return null;
  }
  const rawAgentId = rec['agentId'];
  const rawCwd = rec['cwd'];
  const rawPath = rec['path'];
  const rawParentHostId = rec['parentHostId'];
  const rawClosable = rec['closable'];

  const tab: OrbitSidePanelTab = {
    tabId: rawTabId.trim(),
    kind: rawKind as OrbitSidePanelKind,
    hostId: rawHostId.trim(),
    agentId: typeof rawAgentId === 'string' && rawAgentId.trim().length > 0 ? rawAgentId.trim() : null,
    cwd: typeof rawCwd === 'string' && rawCwd.trim().length > 0 ? rawCwd : null,
    path: typeof rawPath === 'string' && rawPath.trim().length > 0 ? rawPath : null,
    parentHostId: typeof rawParentHostId === 'string' && rawParentHostId.trim().length > 0
      ? rawParentHostId.trim()
      : null,
    title: rawTitle.trim().length > 0 ? rawTitle : rawKind,
    closable: typeof rawClosable === 'boolean' ? rawClosable : true
  };

  const rawParentAgentId = rec['parentAgentId'];
  if (typeof rawParentAgentId === 'string' && rawParentAgentId.trim().length > 0) {
    tab.parentAgentId = rawParentAgentId.trim();
  }
  const rawDirectParentHost = rec['directParentHostId'];
  if (typeof rawDirectParentHost === 'string' && rawDirectParentHost.trim().length > 0) {
    tab.directParentHostId = rawDirectParentHost.trim();
  }
  const rawDirectParentAgent = rec['directParentAgentId'];
  if (typeof rawDirectParentAgent === 'string' && rawDirectParentAgent.trim().length > 0) {
    tab.directParentAgentId = rawDirectParentAgent.trim();
  }
  const rawChildAgentId = rec['childAgentId'];
  if (typeof rawChildAgentId === 'string' && rawChildAgentId.trim().length > 0) {
    tab.childAgentId = rawChildAgentId.trim();
  }
  const rawProviderSubId = rec['providerSubagentId'];
  if (typeof rawProviderSubId === 'string' && rawProviderSubId.trim().length > 0) {
    tab.providerSubagentId = rawProviderSubId.trim();
  }
  const rawSubCallId = rec['subagentCallId'];
  if (typeof rawSubCallId === 'string' && rawSubCallId.trim().length > 0) {
    tab.subagentCallId = rawSubCallId.trim();
  }
  const rawWsId = rec['workspaceId'];
  if (typeof rawWsId === 'string' && rawWsId.trim().length > 0) {
    tab.workspaceId = rawWsId.trim();
  }
  const rawSlot = rec['terminalSlot'];
  if (typeof rawSlot === 'number' && Number.isFinite(rawSlot) && rawSlot >= 1) {
    tab.terminalSlot = Math.floor(rawSlot);
  }
  const rawGitStaged = rec['gitStaged'];
  if (typeof rawGitStaged === 'boolean') {
    tab.gitStaged = rawGitStaged;
  }
  // Canonicalize file tab IDs with unambiguous tuple encoding on hydrate (R2-P0-1).
  if (tab.kind === 'file' && tab.cwd !== null && tab.path !== null) {
    tab.tabId = canonicalFileTabId(tab.hostId, tab.cwd, tab.path);
  }
  return tab;
}

function clampSidePanelWidth(widthVp: number): number {
  if (!Number.isFinite(widthVp) || widthVp <= 0) {
    return ORBIT_SIDE_PANEL_DEFAULT_WIDTH_VP;
  }
  return Math.max(
    ORBIT_SIDE_PANEL_MIN_WIDTH_VP,
    Math.min(ORBIT_SIDE_PANEL_MAX_WIDTH_LG_VP, Math.round(widthVp))
  );
}

@ObservedV2
export class OrbitSidePanelSnapshot {
  @Trace public open: boolean = false;
  @Trace public tabs: OrbitSidePanelTab[] = [];
  @Trace public activeTabId: string = '';
  @Trace public unseenTabIds: string[] = [];
  @Trace public widthVp: number = ORBIT_SIDE_PANEL_DEFAULT_WIDTH_VP;
  public closedTabs: OrbitSidePanelTab[] = [];
  public userDismissed: boolean = false;
  public lastUsedAt: number = 0;
  public lastUsedOrder: number = 0;

  constructor(defaultWidthVp: number = ORBIT_SIDE_PANEL_DEFAULT_WIDTH_VP) {
    this.widthVp = clampSidePanelWidth(defaultWidthVp);
  }

  public isEmptyDefault(): boolean {
    return (
      !this.open &&
      this.tabs.length === 0 &&
      this.closedTabs.length === 0 &&
      !this.userDismissed &&
      this.unseenTabIds.length === 0
    );
  }

  public findTabIndex(tabId: string): number {
    for (let i = 0; i < this.tabs.length; i++) {
      if (this.tabs[i].tabId === tabId) {
        return i;
      }
    }
    return -1;
  }

  public getActiveTab(): OrbitSidePanelTab | null {
    if (this.activeTabId.length === 0) {
      return null;
    }
    const idx = this.findTabIndex(this.activeTabId);
    return idx >= 0 ? this.tabs[idx] : null;
  }

  public hasUnseen(tabId: string): boolean {
    return this.unseenTabIds.indexOf(tabId) >= 0;
  }

  public clearUnseen(tabId: string): boolean {
    if (this.unseenTabIds.indexOf(tabId) < 0) {
      return false;
    }
    const next: string[] = [];
    for (let i = 0; i < this.unseenTabIds.length; i++) {
      if (this.unseenTabIds[i] !== tabId) {
        next.push(this.unseenTabIds[i]);
      }
    }
    this.unseenTabIds = next;
    return true;
  }

  public addUnseen(tabId: string): boolean {
    if (tabId.length === 0 || this.unseenTabIds.indexOf(tabId) >= 0) {
      return false;
    }
    const next: string[] = [];
    for (let i = 0; i < this.unseenTabIds.length; i++) {
      next.push(this.unseenTabIds[i]);
    }
    next.push(tabId);
    this.unseenTabIds = next;
    return true;
  }

  public pruneUnseenToLiveTabs(): void {
    if (this.unseenTabIds.length === 0) {
      return;
    }
    const liveIds: Set<string> = new Set<string>();
    for (let i = 0; i < this.tabs.length; i++) {
      liveIds.add(this.tabs[i].tabId);
    }
    const next: string[] = [];
    for (let i = 0; i < this.unseenTabIds.length; i++) {
      const id = this.unseenTabIds[i];
      if (liveIds.has(id) && next.indexOf(id) < 0) {
        next.push(id);
      }
    }
    if (next.length !== this.unseenTabIds.length) {
      this.unseenTabIds = next;
    }
  }

  public pushClosedTab(tab: OrbitSidePanelTab): void {
    const next: OrbitSidePanelTab[] = [cloneTab(tab)];
    for (let i = 0; i < this.closedTabs.length && next.length < ORBIT_SIDE_PANEL_MAX_CLOSED_TABS; i++) {
      if (this.closedTabs[i].tabId !== tab.tabId) {
        next.push(this.closedTabs[i]);
      }
    }
    this.closedTabs = next;
  }

  public removeClosedTabById(tabId: string): void {
    let found = false;
    for (let i = 0; i < this.closedTabs.length; i++) {
      if (this.closedTabs[i].tabId === tabId) {
        found = true;
        break;
      }
    }
    if (!found) {
      return;
    }
    const next: OrbitSidePanelTab[] = [];
    for (let i = 0; i < this.closedTabs.length; i++) {
      if (this.closedTabs[i].tabId !== tabId) {
        next.push(this.closedTabs[i]);
      }
    }
    this.closedTabs = next;
  }

  public normalizeActiveTabId(preferredIndex: number = -1): void {
    if (this.tabs.length === 0) {
      this.activeTabId = '';
      return;
    }
    if (this.activeTabId.length > 0 && this.findTabIndex(this.activeTabId) >= 0) {
      return;
    }
    if (this.activeTabId.length === 0 && preferredIndex < 0) {
      return;
    }
    if (preferredIndex >= 0) {
      const clampedIdx = Math.min(preferredIndex, this.tabs.length - 1);
      this.activeTabId = this.tabs[clampedIdx].tabId;
      return;
    }
    this.activeTabId = this.tabs[this.tabs.length - 1].tabId;
  }
}

interface ScopeEvictionEntry {
  key: string;
  order: number;
  lastUsedAt: number;
}

/**
 * Per-session independent Right Side Panel state machine.
 *
 * Invariants:
 * 1. Every session scope (`${hostId}::${rootAgentId}` or `draft::${hostId}`) owns an isolated
 *    `OrbitSidePanelSnapshot` with independent tabs, active tab, unseen badges, LIFO-10 closed stack,
 *    and open/dismissed state.
 * 2. Child subagents ALWAYS route to their root agent's scope and open as a `subagent` tab inside it.
 * 3. Every tab carries explicit `hostId`, `agentId`, `cwd`, `path`, and `parentHostId`; panel views
 *    strictly read `tab.*` and NEVER fall back to global `selectedHostId` or `activeCwd`.
 * 4. Persistence writes to Preferences key `orbit.harmony.v2.sidePanel` with 220 ms debounce and
 *    LRU-100 scope eviction.
 */
@ObservedV2
export class OrbitSidePanelStore {
  @Trace public activeScopeKey: string = '';
  @Trace public activeSnapshot: OrbitSidePanelSnapshot = new OrbitSidePanelSnapshot();
  @Trace public isCompactViewport: boolean = false;
  @Trace public stateRevision: number = 0;

  // Non-@Trace O(1) scope table + LRU clock
  private readonly snapshotsByScope: Map<string, OrbitSidePanelSnapshot> =
    new Map<string, OrbitSidePanelSnapshot>();
  private lruClock: number = 0;
  private preferredWidthVp: number = ORBIT_SIDE_PANEL_DEFAULT_WIDTH_VP;
  private storage: OrbitSidePanelStorage | null = null;
  private parentLookup: OrbitAgentParentLookup | null = null;
  private visibilityListener: OrbitSidePanelVisibilityListener | null = null;
  private persistTimerId: number = -1;
  private dirtyPersist: boolean = false;
  private flushSeq: number = 0;
  private readonly debounceMs: number;

  constructor(debounceMs: number = ORBIT_SIDE_PANEL_DEBOUNCE_MS) {
    this.debounceMs = Math.max(0, debounceMs);
  }

  public bindStorage(storage: OrbitSidePanelStorage | null): void {
    this.storage = storage;
  }

  public bindParentLookup(lookup: OrbitAgentParentLookup | null): void {
    this.parentLookup = lookup;
  }

  public setVisibilityListener(listener: OrbitSidePanelVisibilityListener | null): void {
    this.visibilityListener = listener;
  }

  public setCompactViewport(compact: boolean): void {
    if (this.isCompactViewport === compact) {
      return;
    }
    this.isCompactViewport = compact;
  }

  public setDefaultWidthVp(widthVp: number): void {
    this.preferredWidthVp = clampSidePanelWidth(widthVp);
    if (this.activeSnapshot.widthVp !== this.preferredWidthVp) {
      this.activeSnapshot.widthVp = this.preferredWidthVp;
      this.touchAndPersist(this.activeScopeKey, this.activeSnapshot);
    }
  }

  public get isOpen(): boolean {
    return this.activeSnapshot.open;
  }

  public get tabs(): OrbitSidePanelTab[] {
    return this.activeSnapshot.tabs;
  }

  public get activeTabId(): string {
    return this.activeSnapshot.activeTabId;
  }

  public get activeTab(): OrbitSidePanelTab | null {
    return this.activeSnapshot.getActiveTab();
  }

  public get unseenTabIds(): string[] {
    return this.activeSnapshot.unseenTabIds;
  }

  public get hasUnseen(): boolean {
    return this.activeSnapshot.unseenTabIds.length > 0;
  }

  public get widthVp(): number {
    return this.activeSnapshot.widthVp;
  }

  public get scopeCount(): number {
    return this.snapshotsByScope.size;
  }

  public get hasPendingPersist(): boolean {
    return this.dirtyPersist;
  }

  public getSnapshotForScope(scopeKey: string): OrbitSidePanelSnapshot | null {
    const found = this.snapshotsByScope.get(scopeKey);
    return found !== undefined ? found : null;
  }

  public scopeKeyFor(host: string, rootAgent: string | null): string {
    return orbitSidePanelScopeKeyFor(host, rootAgent);
  }

  public resolveRootScope(
    hostId: string,
    agentId: string | null,
    requireStartCardExists: boolean = false
  ): OrbitResolvedRootScope {
    return resolveRootAgentScope(hostId, agentId, this.parentLookup, requireStartCardExists);
  }

  public switchScope(
    hostId: string,
    agentId: string | null,
    nowMs: number = Date.now()
  ): OrbitSidePanelSnapshot {
    const resolved = this.resolveRootScope(hostId, agentId);
    const nextKey = resolved.hostId.length > 0
      ? this.scopeKeyFor(resolved.hostId, resolved.rootAgentId)
      : '';
    return this.switchScopeByKey(nextKey, nowMs);
  }

  public switchScopeByKey(nextKey: string, nowMs: number = Date.now()): OrbitSidePanelSnapshot {
    const prevKey = this.activeScopeKey;
    const prevOpen = this.activeSnapshot.open;

    if (prevKey === nextKey && nextKey.length > 0) {
      this.touchScope(nextKey, this.activeSnapshot, nowMs);
      return this.activeSnapshot;
    }

    // Finalize outgoing scope: retain in LRU table only if non-empty.
    if (prevKey.length > 0) {
      if (this.activeSnapshot.isEmptyDefault()) {
        this.snapshotsByScope.delete(prevKey);
      } else {
        this.snapshotsByScope.set(prevKey, this.activeSnapshot);
        this.touchScope(prevKey, this.activeSnapshot, nowMs);
      }
    }

    if (nextKey.length === 0 || isForbiddenScopeKey(nextKey)) {
      this.activeScopeKey = '';
      this.activeSnapshot = new OrbitSidePanelSnapshot(this.preferredWidthVp);
      this.stateRevision += 1;
      this.notifyVisibilityChangeIfNeeded(prevOpen, false);
      return this.activeSnapshot;
    }

    let incoming = this.snapshotsByScope.get(nextKey);
    if (incoming === undefined) {
      incoming = new OrbitSidePanelSnapshot(this.preferredWidthVp);
    } else {
      this.touchScope(nextKey, incoming, nowMs);
    }

    this.activeScopeKey = nextKey;
    this.activeSnapshot = incoming;
    if (!incoming.isEmptyDefault()) {
      this.snapshotsByScope.set(nextKey, incoming);
      this.pruneLruScopes();
    }
    this.stateRevision += 1;
    this.notifyVisibilityChangeIfNeeded(prevOpen, incoming.open);
    return incoming;
  }

  /**
   * Forces a subagent click/open to route to the root agent's scope and open a `subagent` tab
   * inside that root scope without ever making the child subagent itself the active main scope.
   * Preserves the child's actual `hostId`, `childAgentId`, and direct parent identity separately
   * from the root scope `(parentHostId, parentAgentId)`.
   */
  public openSubagentTab(
    rootHostId: string,
    rootAgentId: string,
    tab: OrbitSidePanelTab,
    opts: OrbitOpenSidePanelTabOptions = { source: 'user', activate: true }
  ): OrbitSidePanelTab {
    const resolved = this.resolveRootScope(rootHostId, rootAgentId);
    const effectiveRootHost = resolved.hostId.length > 0 ? resolved.hostId : rootHostId.trim();
    const effectiveRootAgent = resolved.rootAgentId !== null ? resolved.rootAgentId : rootAgentId.trim();
    this.switchScopeByKey(this.scopeKeyFor(effectiveRootHost, effectiveRootAgent));

    const scopedTab = cloneTab(tab);
    scopedTab.kind = 'subagent';
    if (
      scopedTab.directParentHostId === undefined ||
      scopedTab.directParentHostId === null ||
      scopedTab.directParentHostId.trim().length === 0
    ) {
      scopedTab.directParentHostId = (tab.parentHostId !== null && tab.parentHostId.trim().length > 0)
        ? tab.parentHostId.trim()
        : effectiveRootHost;
    }
    if (
      scopedTab.directParentAgentId === undefined ||
      scopedTab.directParentAgentId === null ||
      scopedTab.directParentAgentId.trim().length === 0
    ) {
      scopedTab.directParentAgentId = (
        tab.parentAgentId !== undefined &&
        tab.parentAgentId !== null &&
        tab.parentAgentId.trim().length > 0
      )
        ? tab.parentAgentId.trim()
        : effectiveRootAgent;
    }
    scopedTab.parentHostId = effectiveRootHost;
    scopedTab.parentAgentId = effectiveRootAgent;
    this.openTab(scopedTab, opts);
    return scopedTab;
  }

  /**
   * Opens or updates a tab inside a specific `targetScopeKey` without forcing a scope switch
   * if the user has already navigated to another conversation.
   */
  public openTabInScope(
    targetScopeKey: string,
    tab: OrbitSidePanelTab,
    opts: OrbitOpenSidePanelTabOptions
  ): void {
    const cleanScopeKey = targetScopeKey.trim();
    if (cleanScopeKey.length === 0 || isForbiddenScopeKey(cleanScopeKey) || tab.tabId.trim().length === 0) {
      return;
    }
    if (cleanScopeKey === this.activeScopeKey) {
      this.openTab(tab, opts);
      return;
    }
    const incoming = cloneTab(tab);
    let snap = this.snapshotsByScope.get(cleanScopeKey);
    if (snap === undefined) {
      snap = new OrbitSidePanelSnapshot(this.preferredWidthVp);
    }
    this.applyOpenTabToSnapshot(snap, incoming, opts);
    this.touchAndPersist(cleanScopeKey, snap);
    this.stateRevision += 1;
  }

  private applyOpenTabToSnapshot(
    snap: OrbitSidePanelSnapshot,
    incoming: OrbitSidePanelTab,
    opts: OrbitOpenSidePanelTabOptions
  ): void {
    // Enforce unambiguous file tabId and strict per-field `(hostId, cwd, path)` matching (R2-P0-1).
    if (incoming.kind === 'file' && incoming.cwd !== null && incoming.path !== null) {
      incoming.tabId = canonicalFileTabId(incoming.hostId, incoming.cwd, incoming.path);
    }
    let existingIdx = -1;
    if (incoming.kind === 'file') {
      for (let i = 0; i < snap.tabs.length; i++) {
        if (isSameFileTab(snap.tabs[i], incoming)) {
          existingIdx = i;
          break;
        }
      }
    } else {
      existingIdx = snap.findTabIndex(incoming.tabId);
    }
    const nextTabs: OrbitSidePanelTab[] = [];
    for (let i = 0; i < snap.tabs.length; i++) {
      nextTabs.push(i === existingIdx ? incoming : snap.tabs[i]);
    }
    if (existingIdx < 0) {
      nextTabs.push(incoming);
    }
    snap.tabs = nextTabs;
    snap.removeClosedTabById(incoming.tabId);

    if (opts.source === 'user') {
      snap.open = true;
      snap.userDismissed = false;
      if (opts.activate || snap.activeTabId.length === 0) {
        snap.activeTabId = incoming.tabId;
        snap.clearUnseen(incoming.tabId);
      }
    } else {
      // source === 'auto'
      if (this.isCompactViewport) {
        // sm (< 680vp): NEVER hijack screen on auto open; light unseen indicator only.
        if (!snap.open) {
          snap.addUnseen(incoming.tabId);
          if (snap.activeTabId.length === 0) {
            snap.activeTabId = incoming.tabId;
          }
        } else if (snap.activeTabId === incoming.tabId) {
          snap.clearUnseen(incoming.tabId);
        } else {
          snap.addUnseen(incoming.tabId);
        }
      } else if (!opts.activate) {
        // Background auto-add: never force open or steal focus; mark unseen.
        if (snap.activeTabId.length === 0) {
          snap.activeTabId = incoming.tabId;
        }
        if (!snap.open || snap.activeTabId !== incoming.tabId) {
          snap.addUnseen(incoming.tabId);
        }
      } else if (snap.open) {
        // Already open: never steal focus from another open tab; mark unseen if inactive.
        if (snap.activeTabId.length === 0) {
          snap.activeTabId = incoming.tabId;
          snap.clearUnseen(incoming.tabId);
        } else if (snap.activeTabId === incoming.tabId) {
          snap.clearUnseen(incoming.tabId);
        } else {
          snap.addUnseen(incoming.tabId);
        }
      } else if (snap.userDismissed) {
        // Respect explicit user dismissal: keep closed and surface unseen dot.
        if (snap.activeTabId.length === 0) {
          snap.activeTabId = incoming.tabId;
        }
        snap.addUnseen(incoming.tabId);
      } else {
        // Wide/md, closed, not dismissed, activate=true: open & activate.
        snap.open = true;
        snap.activeTabId = incoming.tabId;
        snap.clearUnseen(incoming.tabId);
      }
    }
  }

  public openTab(tab: OrbitSidePanelTab, opts: OrbitOpenSidePanelTabOptions): void {
    const incoming = cloneTab(tab);
    if (incoming.tabId.trim().length === 0) {
      return;
    }

    // Enforce subagent root-scope routing invariant if parent scope metadata is attached.
    if (
      incoming.kind === 'subagent' &&
      incoming.parentAgentId !== undefined &&
      incoming.parentAgentId !== null &&
      incoming.parentAgentId.trim().length > 0
    ) {
      const parentHost = (incoming.parentHostId !== null && incoming.parentHostId.trim().length > 0)
        ? incoming.parentHostId.trim()
        : incoming.hostId.trim();
      const root = this.resolveRootScope(parentHost, incoming.parentAgentId);
      if (root.hostId.length > 0) {
        const rootKey = this.scopeKeyFor(root.hostId, root.rootAgentId);
        if (
          incoming.directParentHostId === undefined ||
          incoming.directParentHostId === null ||
          incoming.directParentHostId.trim().length === 0
        ) {
          incoming.directParentHostId = parentHost;
        }
        if (
          incoming.directParentAgentId === undefined ||
          incoming.directParentAgentId === null ||
          incoming.directParentAgentId.trim().length === 0
        ) {
          incoming.directParentAgentId = incoming.parentAgentId.trim();
        }
        incoming.parentHostId = root.hostId;
        incoming.parentAgentId = root.rootAgentId;
        if (this.activeScopeKey !== rootKey) {
          this.switchScopeByKey(rootKey);
        }
      }
    } else if (this.activeScopeKey.length === 0 && incoming.hostId.trim().length > 0) {
      const targetHost = (incoming.parentHostId !== null && incoming.parentHostId.trim().length > 0)
        ? incoming.parentHostId.trim()
        : incoming.hostId.trim();
      const root = this.resolveRootScope(targetHost, incoming.agentId);
      const inferredKey = this.scopeKeyFor(root.hostId, root.rootAgentId);
      this.switchScopeByKey(inferredKey);
    }

    const snap = this.activeSnapshot;
    const prevOpen = snap.open;
    this.applyOpenTabToSnapshot(snap, incoming, opts);
    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
    this.notifyVisibilityChangeIfNeeded(prevOpen, snap.open);
  }

  public updateActiveTabTerminalSlot(hostId: string, tabId: string, slot: number): void {
    const clampedSlot = Math.max(1, Math.min(3, Math.floor(slot)));
    const snap = this.activeSnapshot;
    const idx = snap.findTabIndex(tabId);
    if (idx < 0) {
      return;
    }
    const existing = snap.tabs[idx];
    if (existing.kind !== 'terminal' || existing.hostId !== hostId || existing.terminalSlot === clampedSlot) {
      return;
    }
    const updated = cloneTab(existing);
    updated.terminalSlot = clampedSlot;
    const nextTabs: OrbitSidePanelTab[] = [];
    for (let i = 0; i < snap.tabs.length; i++) {
      nextTabs.push(i === idx ? updated : snap.tabs[i]);
    }
    snap.tabs = nextTabs;
    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
  }

  public updateTabGitStaged(targetScopeKey: string, tabId: string, staged: boolean): void {
    const cleanScope = targetScopeKey.trim().length > 0 ? targetScopeKey.trim() : this.activeScopeKey;
    if (cleanScope.length === 0 || tabId.trim().length === 0) {
      return;
    }
    const snap = cleanScope === this.activeScopeKey
      ? this.activeSnapshot
      : (this.snapshotsByScope.get(cleanScope) ?? null);
    if (snap === null) {
      return;
    }
    const idx = snap.findTabIndex(tabId);
    if (idx < 0) {
      return;
    }
    const existing = snap.tabs[idx];
    if (existing.kind !== 'review' || (existing.gitStaged === true) === staged) {
      return;
    }
    const updated = cloneTab(existing);
    updated.gitStaged = staged;
    const nextTabs: OrbitSidePanelTab[] = [];
    for (let i = 0; i < snap.tabs.length; i++) {
      nextTabs.push(i === idx ? updated : snap.tabs[i]);
    }
    snap.tabs = nextTabs;
    this.touchAndPersist(cleanScope, snap);
    this.stateRevision += 1;
  }

  /**
   * Closes a tab strictly within `targetScopeKey` without touching whichever other scope is
   * currently active (P1-1: guards late async Rename/Delete completions).
   */
  public closeTabInScope(targetScopeKey: string, tabId: string): void {
    const cleanScope = targetScopeKey.trim();
    if (cleanScope.length === 0 || tabId.trim().length === 0) {
      return;
    }
    if (cleanScope === this.activeScopeKey) {
      this.close(tabId);
      return;
    }
    const snap = this.snapshotsByScope.get(cleanScope);
    if (snap === undefined) {
      return;
    }
    const idx = snap.findTabIndex(tabId);
    if (idx < 0) {
      return;
    }
    const target = snap.tabs[idx];
    if (!target.closable) {
      return;
    }
    snap.pushClosedTab(target);
    const remaining: OrbitSidePanelTab[] = [];
    for (let i = 0; i < snap.tabs.length; i++) {
      if (i !== idx) {
        remaining.push(snap.tabs[i]);
      }
    }
    snap.tabs = remaining;
    snap.clearUnseen(tabId);
    if (remaining.length === 0) {
      snap.activeTabId = '';
      snap.open = false;
    } else if (snap.activeTabId === tabId) {
      const nextIdx = Math.min(idx, remaining.length - 1);
      snap.activeTabId = remaining[nextIdx].tabId;
      snap.clearUnseen(snap.activeTabId);
    }
    this.touchAndPersist(cleanScope, snap);
    this.stateRevision += 1;
    if (this.visibilityListener !== null && this.visibilityListener.onSidePanelTabClosed !== undefined) {
      this.visibilityListener.onSidePanelTabClosed(cleanScope, target);
    }
  }

  public activate(tabId: string): void {
    const snap = this.activeSnapshot;
    const idx = snap.findTabIndex(tabId);
    if (idx < 0) {
      return;
    }
    const prevOpen = snap.open;
    snap.activeTabId = tabId;
    snap.clearUnseen(tabId);
    snap.open = true;
    snap.userDismissed = false;
    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
    this.notifyVisibilityChangeIfNeeded(prevOpen, snap.open);
  }

  public close(tabId: string): void {
    const snap = this.activeSnapshot;
    const idx = snap.findTabIndex(tabId);
    if (idx < 0) {
      return;
    }
    const target = snap.tabs[idx];
    if (!target.closable) {
      return;
    }
    const prevOpen = snap.open;
    snap.pushClosedTab(target);

    const remaining: OrbitSidePanelTab[] = [];
    for (let i = 0; i < snap.tabs.length; i++) {
      if (i !== idx) {
        remaining.push(snap.tabs[i]);
      }
    }
    snap.tabs = remaining;
    snap.clearUnseen(tabId);

    if (remaining.length === 0) {
      snap.activeTabId = '';
      snap.open = false;
      // Explicit user close of the final tab marks this scope dismissed so subsequent auto-events
      // do not immediately pop the panel back open (see docs/design/side-panel.md §4).
      snap.userDismissed = true;
    } else if (snap.activeTabId === tabId) {
      const nextIdx = Math.min(idx, remaining.length - 1);
      snap.activeTabId = remaining[nextIdx].tabId;
      snap.clearUnseen(snap.activeTabId);
    }

    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
    if (this.visibilityListener !== null && this.visibilityListener.onSidePanelTabClosed !== undefined) {
      this.visibilityListener.onSidePanelTabClosed(this.activeScopeKey, target);
    }
    this.notifyVisibilityChangeIfNeeded(prevOpen, snap.open);
  }

  public reopenLast(): OrbitSidePanelTab | null {
    const snap = this.activeSnapshot;
    if (snap.closedTabs.length === 0) {
      return null;
    }
    const popped = snap.closedTabs[0];
    const remainingClosed: OrbitSidePanelTab[] = [];
    for (let i = 1; i < snap.closedTabs.length; i++) {
      remainingClosed.push(snap.closedTabs[i]);
    }
    snap.closedTabs = remainingClosed;

    const reopened = cloneTab(popped);
    const prevOpen = snap.open;
    const existingIdx = snap.findTabIndex(reopened.tabId);
    if (existingIdx < 0) {
      const nextTabs: OrbitSidePanelTab[] = [];
      for (let i = 0; i < snap.tabs.length; i++) {
        nextTabs.push(snap.tabs[i]);
      }
      nextTabs.push(reopened);
      snap.tabs = nextTabs;
    }
    snap.activeTabId = reopened.tabId;
    snap.clearUnseen(reopened.tabId);
    snap.open = true;
    snap.userDismissed = false;

    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
    this.notifyVisibilityChangeIfNeeded(prevOpen, snap.open);
    return reopened;
  }

  public openPanel(): void {
    const snap = this.activeSnapshot;
    const prevOpen = snap.open;
    snap.open = true;
    snap.userDismissed = false;
    if (snap.activeTabId.length === 0 && snap.tabs.length > 0) {
      snap.activeTabId = snap.tabs[snap.tabs.length - 1].tabId;
    }
    if (snap.activeTabId.length > 0) {
      snap.clearUnseen(snap.activeTabId);
    }
    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
    this.notifyVisibilityChangeIfNeeded(prevOpen, snap.open);
  }

  public openLaunchpad(): void {
    const snap = this.activeSnapshot;
    const prevOpen = snap.open;
    snap.open = true;
    snap.userDismissed = false;
    snap.activeTabId = '';
    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
    this.notifyVisibilityChangeIfNeeded(prevOpen, snap.open);
  }

  public dismissPanel(): void {
    const snap = this.activeSnapshot;
    const prevOpen = snap.open;
    snap.open = false;
    snap.userDismissed = true;
    this.touchAndPersist(this.activeScopeKey, snap);
    this.stateRevision += 1;
    this.notifyVisibilityChangeIfNeeded(prevOpen, snap.open);
  }

  public togglePanel(): void {
    if (this.activeSnapshot.open) {
      this.dismissPanel();
    } else {
      this.openPanel();
    }
  }

  public setWidthVp(widthVp: number): void {
    const clamped = clampSidePanelWidth(widthVp);
    this.preferredWidthVp = clamped;
    if (this.activeSnapshot.widthVp === clamped) {
      return;
    }
    this.activeSnapshot.widthVp = clamped;
    this.touchAndPersist(this.activeScopeKey, this.activeSnapshot);
    this.stateRevision += 1;
  }

  public rebindDraftCwd(
    hostId: string,
    cwd: string,
    resolvedWorkspaceId?: string | null
  ): void {
    const cleanHost = hostId.trim();
    const cleanCwd = nonBlankPath(cwd);
    if (cleanHost.length === 0 || cleanCwd.length === 0) {
      return;
    }
    const draftKey = this.scopeKeyFor(cleanHost, null);
    const snap = this.activeScopeKey === draftKey
      ? this.activeSnapshot
      : (this.snapshotsByScope.get(draftKey) ?? null);
    if (snap === null) {
      return;
    }

    const hasExplicitWs = resolvedWorkspaceId !== undefined;
    const cleanWsId = (resolvedWorkspaceId !== undefined && resolvedWorkspaceId !== null && resolvedWorkspaceId.trim().length > 0)
      ? resolvedWorkspaceId.trim()
      : null;

    let changed = false;
    const rebindTab = (t: OrbitSidePanelTab): OrbitSidePanelTab => {
      if (t.agentId !== null || !isDraftCwdFollowingKind(t.kind)) {
        return t;
      }
      const cwdChanged = t.cwd !== cleanCwd;
      const nextWsId: string | null | undefined = t.kind === 'board'
        ? (hasExplicitWs ? cleanWsId : (cwdChanged ? null : t.workspaceId))
        : t.workspaceId;
      const wsChanged = t.kind === 'board' && nextWsId !== t.workspaceId;
      if (!cwdChanged && !wsChanged) {
        return t;
      }
      const updated = cloneTab(t);
      updated.cwd = cleanCwd;
      if (t.kind === 'board') {
        updated.workspaceId = nextWsId ?? null;
      }
      changed = true;
      return updated;
    };

    const nextTabs: OrbitSidePanelTab[] = [];
    for (let i = 0; i < snap.tabs.length; i++) {
      nextTabs.push(rebindTab(snap.tabs[i]));
    }

    const nextClosed: OrbitSidePanelTab[] = [];
    for (let i = 0; i < snap.closedTabs.length; i++) {
      nextClosed.push(rebindTab(snap.closedTabs[i]));
    }

    if (!changed) {
      return;
    }
    snap.tabs = nextTabs;
    snap.closedTabs = nextClosed;
    this.touchAndPersist(draftKey, snap);
    this.stateRevision += 1;
  }

  private mergeSnapshots(target: OrbitSidePanelSnapshot, incoming: OrbitSidePanelSnapshot): void {
    const mergedTabs: OrbitSidePanelTab[] = [];
    const seenIds: Set<string> = new Set<string>();
    for (let i = 0; i < target.tabs.length; i++) {
      seenIds.add(target.tabs[i].tabId);
      mergedTabs.push(target.tabs[i]);
    }
    for (let i = 0; i < incoming.tabs.length; i++) {
      if (!seenIds.has(incoming.tabs[i].tabId)) {
        seenIds.add(incoming.tabs[i].tabId);
        mergedTabs.push(incoming.tabs[i]);
      }
    }
    target.tabs = mergedTabs;

    const mergedClosed: OrbitSidePanelTab[] = [];
    const seenClosed: Set<string> = new Set<string>();
    const addClosed = (list: OrbitSidePanelTab[]): void => {
      for (let i = 0; i < list.length && mergedClosed.length < ORBIT_SIDE_PANEL_MAX_CLOSED_TABS; i++) {
        const c = list[i];
        if (!seenIds.has(c.tabId) && !seenClosed.has(c.tabId)) {
          seenClosed.add(c.tabId);
          mergedClosed.push(c);
        }
      }
    };
    addClosed(incoming.closedTabs);
    addClosed(target.closedTabs);
    target.closedTabs = mergedClosed;

    const mergedUnseen: string[] = [];
    const addUnseen = (list: string[]): void => {
      for (let i = 0; i < list.length; i++) {
        const id = list[i];
        if (seenIds.has(id) && mergedUnseen.indexOf(id) < 0) {
          mergedUnseen.push(id);
        }
      }
    };
    addUnseen(target.unseenTabIds);
    addUnseen(incoming.unseenTabIds);
    target.unseenTabIds = mergedUnseen;

    if (target.activeTabId.length === 0 && incoming.activeTabId.length > 0 && seenIds.has(incoming.activeTabId)) {
      target.activeTabId = incoming.activeTabId;
    }
    target.open = target.open || incoming.open;
    if (target.open) {
      target.userDismissed = false;
    }
    target.normalizeActiveTabId();
    target.pruneUnseenToLiveTabs();
  }

  public promoteDraftScope(
    hostId: string,
    agentId: string,
    newCwd?: string | null,
    resolvedWorkspaceId?: string | null
  ): void {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      return;
    }
    const draftKey = this.scopeKeyFor(cleanHost, null);
    const targetKey = this.scopeKeyFor(cleanHost, cleanAgent);
    const draftSnap = this.activeScopeKey === draftKey
      ? this.activeSnapshot
      : (this.snapshotsByScope.get(draftKey) ?? null);

    if (draftSnap === null) {
      if (this.activeScopeKey === draftKey) {
        this.activeScopeKey = targetKey;
        this.stateRevision += 1;
      }
      return;
    }

    const cleanCwd = newCwd !== undefined && newCwd !== null && newCwd.trim().length > 0
      ? newCwd
      : null;
    const hasExplicitWs = resolvedWorkspaceId !== undefined;
    const cleanWsId = (resolvedWorkspaceId !== undefined && resolvedWorkspaceId !== null && resolvedWorkspaceId.trim().length > 0)
      ? resolvedWorkspaceId.trim()
      : null;

    const remapCanonicalDraftTabId = (t: OrbitSidePanelTab): string => {
      if (t.kind === 'file') {
        return t.tabId;
      }
      if (t.kind === 'side_question') {
        return canonicalSideQuestionTabId(targetKey);
      }
      if (t.tabId === `${t.kind}:${draftKey}`) {
        return `${t.kind}:${targetKey}`;
      }
      if (t.kind === 'subagent' && t.tabId === `crew:${draftKey}`) {
        return `crew:${targetKey}`;
      }
      return t.tabId;
    };

    const rebindList = (list: OrbitSidePanelTab[], idMap: Map<string, string>): OrbitSidePanelTab[] => {
      const out: OrbitSidePanelTab[] = [];
      const seen: Set<string> = new Set<string>();
      for (let i = 0; i < list.length; i++) {
        const t = cloneTab(list[i]);
        const wasDraft = t.agentId === null;
        if (wasDraft && t.hostId === cleanHost) {
          t.agentId = cleanAgent;
        }
        if (
          (t.parentAgentId === undefined || t.parentAgentId === null || t.parentAgentId.length === 0) &&
          (t.parentHostId === cleanHost || t.hostId === cleanHost)
        ) {
          t.parentAgentId = cleanAgent;
        }
        const prevCwd = t.cwd;
        if (
          cleanCwd !== null &&
          (t.cwd === null || t.cwd.length === 0 || t.cwd === '~' || (wasDraft && isDraftCwdFollowingKind(t.kind)))
        ) {
          t.cwd = cleanCwd;
        }
        if (t.kind === 'board') {
          if (hasExplicitWs) {
            t.workspaceId = cleanWsId;
          } else if (t.cwd !== prevCwd) {
            t.workspaceId = null;
          }
        }
        const nextId = remapCanonicalDraftTabId(t);
        if (nextId !== t.tabId) {
          idMap.set(t.tabId, nextId);
          t.tabId = nextId;
        }
        if (!seen.has(t.tabId)) {
          seen.add(t.tabId);
          out.push(t);
        }
      }
      return out;
    };

    const activeIdMap: Map<string, string> = new Map<string, string>();
    const closedIdMap: Map<string, string> = new Map<string, string>();
    draftSnap.tabs = rebindList(draftSnap.tabs, activeIdMap);
    draftSnap.closedTabs = rebindList(draftSnap.closedTabs, closedIdMap);

    if (draftSnap.activeTabId.length > 0) {
      const mappedActive = activeIdMap.get(draftSnap.activeTabId);
      if (mappedActive !== undefined) {
        draftSnap.activeTabId = mappedActive;
      }
    }
    if (draftSnap.unseenTabIds.length > 0) {
      const nextUnseen: string[] = [];
      for (let i = 0; i < draftSnap.unseenTabIds.length; i++) {
        const orig = draftSnap.unseenTabIds[i];
        const mapped = activeIdMap.get(orig) ?? orig;
        if (nextUnseen.indexOf(mapped) < 0) {
          nextUnseen.push(mapped);
        }
      }
      draftSnap.unseenTabIds = nextUnseen;
    }

    draftSnap.normalizeActiveTabId();
    draftSnap.pruneUnseenToLiveTabs();

    this.snapshotsByScope.delete(draftKey);
    const existingTarget = this.snapshotsByScope.get(targetKey);
    let finalSnap: OrbitSidePanelSnapshot = draftSnap;
    if (existingTarget !== undefined && existingTarget !== draftSnap) {
      this.mergeSnapshots(existingTarget, draftSnap);
      finalSnap = existingTarget;
    } else if (!draftSnap.isEmptyDefault()) {
      this.snapshotsByScope.set(targetKey, draftSnap);
    }
    if (this.activeScopeKey === draftKey) {
      this.activeScopeKey = targetKey;
      this.activeSnapshot = finalSnap;
    }
    this.touchAndPersist(targetKey, finalSnap);
    this.stateRevision += 1;
  }

  public remapOptimisticScopes(hostId: string, optId: string, realId: string): void {
    const cleanHost = hostId.trim();
    const cleanOpt = optId.trim();
    const cleanReal = realId.trim();
    if (cleanHost.length === 0 || cleanOpt.length === 0 || cleanReal.length === 0 || cleanOpt === cleanReal) {
      return;
    }

    const oldScopeKey = this.scopeKeyFor(cleanHost, cleanOpt);
    const newScopeKey = this.scopeKeyFor(cleanHost, cleanReal);

    const remapTab = (tab: OrbitSidePanelTab, idMap: Map<string, string>): OrbitSidePanelTab => {
      const ownerHost = (tab.parentHostId !== null && tab.parentHostId.length > 0)
        ? tab.parentHostId
        : tab.hostId;
      const directParentHost = (
        tab.directParentHostId !== undefined &&
        tab.directParentHostId !== null &&
        tab.directParentHostId.length > 0
      )
        ? tab.directParentHostId
        : ownerHost;
      if (ownerHost !== cleanHost && tab.hostId !== cleanHost && directParentHost !== cleanHost) {
        return tab;
      }
      let changed = false;
      const next = cloneTab(tab);

      if (next.hostId === cleanHost && next.agentId === cleanOpt) {
        next.agentId = cleanReal;
        changed = true;
      }
      if (ownerHost === cleanHost && next.parentAgentId === cleanOpt) {
        next.parentAgentId = cleanReal;
        changed = true;
      }
      if (directParentHost === cleanHost && next.directParentAgentId === cleanOpt) {
        next.directParentAgentId = cleanReal;
        changed = true;
      }
      if (next.hostId === cleanHost && next.childAgentId === cleanOpt) {
        next.childAgentId = cleanReal;
        changed = true;
      }

      // Structured canonical tabId migration by kind + entity fields.
      // NEVER perform arbitrary substring replacement (which corrupts file paths like `opt_42.txt`).
      let remappedId = next.tabId;
      if (next.kind === 'file') {
        remappedId = next.tabId;
      } else if (next.kind === 'subagent') {
        if (next.tabId === `crew:${oldScopeKey}`) {
          remappedId = `crew:${newScopeKey}`;
        } else if (
          next.hostId === cleanHost &&
          (next.tabId === `subagent:${cleanHost}::${cleanOpt}` ||
            next.tabId === canonicalManagedSubagentTabId(cleanHost, cleanOpt))
        ) {
          remappedId = next.tabId === canonicalManagedSubagentTabId(cleanHost, cleanOpt)
            ? canonicalManagedSubagentTabId(cleanHost, cleanReal)
            : `subagent:${cleanHost}::${cleanReal}`;
        } else if (
          directParentHost === cleanHost &&
          next.providerSubagentId !== undefined &&
          next.providerSubagentId !== null &&
          next.providerSubagentId.length > 0 &&
          (next.tabId === `subagent:provider:${cleanHost}::${cleanOpt}::${next.providerSubagentId}` ||
            next.tabId === canonicalProviderSubagentTabId(cleanHost, cleanOpt, next.providerSubagentId))
        ) {
          remappedId = canonicalProviderSubagentTabId(cleanHost, cleanReal, next.providerSubagentId);
        } else if (
          directParentHost === cleanHost &&
          next.subagentCallId !== undefined &&
          next.subagentCallId !== null &&
          next.subagentCallId.length > 0 &&
          (next.tabId === `subagent:provider:${cleanHost}::${cleanOpt}::${next.subagentCallId}` ||
            next.tabId === canonicalProviderSubagentTabId(cleanHost, cleanOpt, next.subagentCallId))
        ) {
          remappedId = canonicalProviderSubagentTabId(cleanHost, cleanReal, next.subagentCallId);
        }
      } else if (next.kind === 'peer') {
        if (
          next.hostId === cleanHost &&
          (next.tabId === `peer:${cleanHost}::${cleanOpt}` ||
            next.tabId === canonicalPeerTabId(cleanHost, cleanOpt))
        ) {
          remappedId = next.tabId === canonicalPeerTabId(cleanHost, cleanOpt)
            ? canonicalPeerTabId(cleanHost, cleanReal)
            : `peer:${cleanHost}::${cleanReal}`;
        }
      } else if (next.kind === 'side_question') {
        if (
          next.tabId === canonicalSideQuestionTabId(oldScopeKey) ||
          next.tabId === `side_question:${oldScopeKey}` ||
          next.tabId === canonicalSideQuestionTabId(`draft::${cleanHost}`) ||
          next.tabId === `side_question:draft::${cleanHost}`
        ) {
          remappedId = canonicalSideQuestionTabId(newScopeKey);
        }
      } else if (next.tabId === `${next.kind}:${oldScopeKey}`) {
        remappedId = `${next.kind}:${newScopeKey}`;
      }

      if (remappedId !== next.tabId) {
        idMap.set(next.tabId, remappedId);
        next.tabId = remappedId;
        changed = true;
      }
      return changed ? next : tab;
    };

    const remapSnapshot = (snap: OrbitSidePanelSnapshot): boolean => {
      let changed = false;
      const idMap: Map<string, string> = new Map<string, string>();
      const seenTabs: Set<string> = new Set<string>();
      const nextTabs: OrbitSidePanelTab[] = [];
      for (let i = 0; i < snap.tabs.length; i++) {
        const mapped = remapTab(snap.tabs[i], idMap);
        if (mapped !== snap.tabs[i]) {
          changed = true;
        }
        if (!seenTabs.has(mapped.tabId)) {
          seenTabs.add(mapped.tabId);
          nextTabs.push(mapped);
        } else {
          changed = true;
        }
      }
      const nextClosed: OrbitSidePanelTab[] = [];
      const seenClosed: Set<string> = new Set<string>();
      const closedIdMap: Map<string, string> = new Map<string, string>();
      for (let i = 0; i < snap.closedTabs.length; i++) {
        const mapped = remapTab(snap.closedTabs[i], closedIdMap);
        if (mapped !== snap.closedTabs[i]) {
          changed = true;
        }
        if (!seenTabs.has(mapped.tabId) && !seenClosed.has(mapped.tabId)) {
          seenClosed.add(mapped.tabId);
          nextClosed.push(mapped);
        } else {
          changed = true;
        }
      }
      if (!changed) {
        return false;
      }
      snap.tabs = nextTabs;
      snap.closedTabs = nextClosed;
      if (snap.activeTabId.length > 0) {
        const mappedActive = idMap.get(snap.activeTabId);
        if (mappedActive !== undefined) {
          snap.activeTabId = mappedActive;
        }
      }
      if (snap.unseenTabIds.length > 0) {
        const nextUnseen: string[] = [];
        for (let i = 0; i < snap.unseenTabIds.length; i++) {
          const orig = snap.unseenTabIds[i];
          const mapped = idMap.get(orig) ?? orig;
          if (seenTabs.has(mapped) && nextUnseen.indexOf(mapped) < 0) {
            nextUnseen.push(mapped);
          }
        }
        snap.unseenTabIds = nextUnseen;
      }
      snap.normalizeActiveTabId();
      return true;
    };

    let anyChanged = false;
    const keys: string[] = [];
    this.snapshotsByScope.forEach((_v: OrbitSidePanelSnapshot, k: string): void => {
      keys.push(k);
    });

    for (let i = 0; i < keys.length; i++) {
      const k = keys[i];
      const snap = this.snapshotsByScope.get(k);
      if (snap === undefined) {
        continue;
      }
      if (remapSnapshot(snap)) {
        anyChanged = true;
      }
      if (k === oldScopeKey) {
        this.snapshotsByScope.delete(oldScopeKey);
        const existingTarget = this.snapshotsByScope.get(newScopeKey);
        if (existingTarget !== undefined && existingTarget !== snap) {
          this.mergeSnapshots(existingTarget, snap);
        } else {
          this.snapshotsByScope.set(newScopeKey, snap);
        }
        anyChanged = true;
      }
    }

    if (this.activeScopeKey === oldScopeKey) {
      if (remapSnapshot(this.activeSnapshot)) {
        anyChanged = true;
      }
      this.activeScopeKey = newScopeKey;
      const existingTarget = this.snapshotsByScope.get(newScopeKey);
      if (existingTarget !== undefined && existingTarget !== this.activeSnapshot) {
        this.mergeSnapshots(existingTarget, this.activeSnapshot);
        this.activeSnapshot = existingTarget;
      } else if (!this.activeSnapshot.isEmptyDefault()) {
        this.snapshotsByScope.set(newScopeKey, this.activeSnapshot);
      }
      anyChanged = true;
    } else if (!this.snapshotsByScope.has(this.activeScopeKey)) {
      if (remapSnapshot(this.activeSnapshot)) {
        anyChanged = true;
      }
    }

    if (anyChanged) {
      this.schedulePersist();
      this.stateRevision += 1;
    }
  }

  public dropScope(scopeKey: string): void {
    const cleanKey = scopeKey.trim();
    if (cleanKey.length === 0) {
      return;
    }
    let changed = false;
    const prevOpen = this.activeSnapshot.open;
    const droppedTabs: OrbitSidePanelTab[] = [];
    const existingDropSnap = this.snapshotsByScope.get(cleanKey);
    if (existingDropSnap !== undefined) {
      for (let i = 0; i < existingDropSnap.tabs.length; i++) {
        droppedTabs.push(existingDropSnap.tabs[i]);
      }
    }

    if (this.snapshotsByScope.delete(cleanKey)) {
      changed = true;
    }

    if (this.activeScopeKey === cleanKey) {
      if (droppedTabs.length === 0) {
        for (let i = 0; i < this.activeSnapshot.tabs.length; i++) {
          droppedTabs.push(this.activeSnapshot.tabs[i]);
        }
      }
      this.activeScopeKey = '';
      this.activeSnapshot = new OrbitSidePanelSnapshot(this.preferredWidthVp);
      changed = true;
    }
    if (
      droppedTabs.length > 0 &&
      this.visibilityListener !== null &&
      this.visibilityListener.onSidePanelTabClosed !== undefined
    ) {
      for (let i = 0; i < droppedTabs.length; i++) {
        this.visibilityListener.onSidePanelTabClosed(cleanKey, droppedTabs[i]);
      }
    }

    // If scopeKey is `${hostId}::${agentId}` (not draft::), prune any subagent/peer tabs referencing it.
    const sep = cleanKey.indexOf('::');
    if (sep > 0) {
      const prefix = cleanKey.substring(0, sep);
      const suffix = cleanKey.substring(sep + 2);
      if (prefix !== 'draft' && suffix.length > 0) {
        if (this.pruneMatchingTabsAcrossAllScopes((tab: OrbitSidePanelTab, _scopeKey: string): boolean => {
          if (tab.kind === 'subagent') {
            const isProvider = (
              (tab.providerSubagentId !== undefined && tab.providerSubagentId !== null && tab.providerSubagentId.length > 0) ||
              tab.tabId.startsWith('subagent:provider:')
            );
            if (!isProvider) {
              return tab.hostId === prefix && (tab.agentId === suffix || tab.childAgentId === suffix);
            }
            const directParentHost = (tab.directParentHostId !== undefined && tab.directParentHostId !== null && tab.directParentHostId.length > 0)
              ? tab.directParentHostId
              : (tab.parentHostId ?? tab.hostId);
            const directParentAgent = (tab.directParentAgentId !== undefined && tab.directParentAgentId !== null && tab.directParentAgentId.length > 0)
              ? tab.directParentAgentId
              : (tab.parentAgentId ?? '');
            return directParentHost === prefix && directParentAgent === suffix;
          }
          if (tab.kind === 'peer') {
            return tab.hostId === prefix && tab.agentId === suffix;
          }
          return false;
        })) {
          changed = true;
        }
      }
    }

    if (changed) {
      this.schedulePersist();
      this.stateRevision += 1;
      this.notifyVisibilityChangeIfNeeded(prevOpen, this.activeSnapshot.open);
    }
  }

  public removeSubagentTabs(
    hostId: string,
    agentId: string,
    parentScopeKey?: string,
    isProviderSubagent: boolean = false,
    directParentHostId?: string,
    directParentAgentId?: string
  ): void {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    const cleanParentScope = parentScopeKey !== undefined ? parentScopeKey.trim() : '';
    const cleanDirectHost = directParentHostId !== undefined ? directParentHostId.trim() : '';
    const cleanDirectAgent = directParentAgentId !== undefined ? directParentAgentId.trim() : '';
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      return;
    }
    const prevOpen = this.activeSnapshot.open;
    const changed = this.pruneMatchingTabsAcrossAllScopes((tab: OrbitSidePanelTab, scopeKey: string): boolean => {
      if (tab.kind !== 'subagent' || tab.tabId.startsWith('crew:')) {
        return false;
      }
      const isProviderTab = (
        (tab.providerSubagentId !== undefined && tab.providerSubagentId !== null && tab.providerSubagentId.length > 0) ||
        tab.tabId.startsWith('subagent:provider:') ||
        tab.tabId.startsWith('subagent:["provider",')
      );
      if (isProviderSubagent) {
        if (!isProviderTab) {
          return false;
        }
        const tabDirectHost = (tab.directParentHostId !== undefined && tab.directParentHostId !== null && tab.directParentHostId.length > 0)
          ? tab.directParentHostId.trim()
          : (tab.parentHostId !== null && tab.parentHostId.trim().length > 0 ? tab.parentHostId.trim() : tab.hostId.trim());
        const tabDirectAgent = (tab.directParentAgentId !== undefined && tab.directParentAgentId !== null && tab.directParentAgentId.length > 0)
          ? tab.directParentAgentId.trim()
          : (tab.parentAgentId !== undefined && tab.parentAgentId !== null ? tab.parentAgentId.trim() : '');

        // P1-7: When direct parent `(directParentHostId, directParentAgentId)` is specified,
        // require EXACT match so two direct parents p1 & p2 under the same root with the same
        // provider subagent ID NEVER clobber each other (across both open tabs & closedTabs).
        if (cleanDirectHost.length > 0 || cleanDirectAgent.length > 0) {
          const expectedHost = cleanDirectHost.length > 0 ? cleanDirectHost : cleanHost;
          if (tabDirectHost !== expectedHost) {
            return false;
          }
          if (cleanDirectAgent.length > 0 && tabDirectAgent !== cleanDirectAgent) {
            return false;
          }
        } else if (cleanParentScope.length > 0) {
          const tabDirectScope = this.scopeKeyFor(tabDirectHost, tabDirectAgent.length > 0 ? tabDirectAgent : null);
          if (tabDirectScope !== cleanParentScope && scopeKey !== cleanParentScope) {
            return false;
          }
          if (tabDirectHost !== cleanHost && tab.hostId !== cleanHost) {
            return false;
          }
        } else if (tabDirectHost !== cleanHost && tab.hostId !== cleanHost) {
          return false;
        }
        return (
          tab.providerSubagentId === cleanAgent ||
          tab.subagentCallId === cleanAgent ||
          tab.agentId === cleanAgent
        );
      }
      // Managed child subagent: never remove provider tabs; match strictly on child's ACTUAL host.
      if (isProviderTab || tab.hostId !== cleanHost) {
        return false;
      }
      return tab.childAgentId === cleanAgent || tab.agentId === cleanAgent;
    });

    if (changed) {
      this.schedulePersist();
      this.stateRevision += 1;
      this.notifyVisibilityChangeIfNeeded(prevOpen, this.activeSnapshot.open);
    }
  }

  private pruneMatchingTabsAcrossAllScopes(
    predicate: (tab: OrbitSidePanelTab, scopeKey: string) => boolean
  ): boolean {
    let anyChanged = false;
    const pruneSnap = (snap: OrbitSidePanelSnapshot, scopeKey: string): boolean => {
      let removedActiveIdx = -1;
      const nextTabs: OrbitSidePanelTab[] = [];
      for (let i = 0; i < snap.tabs.length; i++) {
        const t = snap.tabs[i];
        if (predicate(t, scopeKey)) {
          if (t.tabId === snap.activeTabId) {
            removedActiveIdx = nextTabs.length;
          }
        } else {
          nextTabs.push(t);
        }
      }
      const nextClosed: OrbitSidePanelTab[] = [];
      for (let i = 0; i < snap.closedTabs.length; i++) {
        if (!predicate(snap.closedTabs[i], scopeKey)) {
          nextClosed.push(snap.closedTabs[i]);
        }
      }
      const tabsRemoved = nextTabs.length !== snap.tabs.length;
      const closedRemoved = nextClosed.length !== snap.closedTabs.length;
      if (!tabsRemoved && !closedRemoved) {
        return false;
      }
      snap.tabs = nextTabs;
      snap.closedTabs = nextClosed;
      if (tabsRemoved) {
        if (nextTabs.length === 0) {
          snap.activeTabId = '';
          snap.open = false;
          // Auto-removal (archive/delete) does NOT mark userDismissed = true.
        } else {
          snap.normalizeActiveTabId(removedActiveIdx);
          if (snap.open && snap.activeTabId.length > 0) {
            snap.clearUnseen(snap.activeTabId);
          }
        }
      }
      snap.pruneUnseenToLiveTabs();
      return true;
    };

    const visitedSnaps: Set<OrbitSidePanelSnapshot> = new Set<OrbitSidePanelSnapshot>();
    const emptyKeys: string[] = [];
    this.snapshotsByScope.forEach((snap: OrbitSidePanelSnapshot, key: string): void => {
      visitedSnaps.add(snap);
      if (pruneSnap(snap, key)) {
        anyChanged = true;
        if (snap.isEmptyDefault() && key !== this.activeScopeKey) {
          emptyKeys.push(key);
        }
      }
    });
    for (let i = 0; i < emptyKeys.length; i++) {
      this.snapshotsByScope.delete(emptyKeys[i]);
    }
    if (!visitedSnaps.has(this.activeSnapshot)) {
      if (pruneSnap(this.activeSnapshot, this.activeScopeKey)) {
        anyChanged = true;
      }
    }
    return anyChanged;
  }

  // Persistence (Preferences key: `orbit.harmony.v2.sidePanel`, 220ms debounce, LRU 100)

  public hydrate(): boolean {
    if (this.storage === null) {
      return false;
    }
    try {
      const raw = this.storage.getSidePanelStateJson();
      if (raw.trim().length === 0) {
        return true;
      }
      return this.hydrateFromJson(raw);
    } catch (err) {
      orbitLogWarn('OrbitSidePanelStore', 'hydrate failed; falling back to empty state', err as Object);
      this.resetToEmpty();
      return false;
    }
  }

  public hydrateFromJson(raw: string): boolean {
    try {
      const parsedObj = JSON.parse(raw) as Object;
      if (parsedObj === null || typeof parsedObj !== 'object' || Array.isArray(parsedObj)) {
        this.resetToEmpty();
        return false;
      }
      const env = parsedObj as Record<string, Object | null | undefined>;
      if (env['version'] !== ORBIT_SIDE_PANEL_SCHEMA_VERSION) {
        this.resetToEmpty();
        return false;
      }
      const rawByScope = env['byScope'];
      if (rawByScope === null || rawByScope === undefined || typeof rawByScope !== 'object' || Array.isArray(rawByScope)) {
        this.resetToEmpty();
        return false;
      }

      this.snapshotsByScope.clear();
      this.lruClock = 0;
      const scopeMap = rawByScope as Record<string, Object | null | undefined>;
      const scopeKeys = Object.keys(scopeMap);
      let maxOrder = 0;
      for (let i = 0; i < scopeKeys.length; i++) {
        const key = scopeKeys[i];
        if (key.length === 0 || isForbiddenScopeKey(key) || key.indexOf('::') <= 0) {
          continue;
        }
        const rawSnap = scopeMap[key];
        if (rawSnap === null || rawSnap === undefined || typeof rawSnap !== 'object' || Array.isArray(rawSnap)) {
          continue;
        }
        const snap = this.deserializeSnapshot(rawSnap as Record<string, Object | null | undefined>, i + 1, key);
        if (snap !== null && !snap.isEmptyDefault()) {
          this.snapshotsByScope.set(key, snap);
          if (snap.lastUsedOrder > maxOrder) {
            maxOrder = snap.lastUsedOrder;
          }
        }
      }
      this.lruClock = maxOrder;
      this.pruneLruScopes();

      if (this.activeScopeKey.length > 0) {
        const restored = this.snapshotsByScope.get(this.activeScopeKey);
        this.activeSnapshot = restored !== undefined
          ? restored
          : new OrbitSidePanelSnapshot(this.preferredWidthVp);
      } else {
        this.activeSnapshot = new OrbitSidePanelSnapshot(this.preferredWidthVp);
      }
      this.stateRevision += 1;
      return true;
    } catch (err) {
      orbitLogWarn('OrbitSidePanelStore', 'Corrupt sidePanel JSON; falling back to empty', err as Object);
      this.resetToEmpty();
      return false;
    }
  }

  public serializeState(): string {
    const byScope: Record<string, PersistedSnapshotRecord> = {};
    this.pruneLruScopes();

    if (this.activeScopeKey.length > 0 && !this.activeSnapshot.isEmptyDefault()) {
      this.snapshotsByScope.set(this.activeScopeKey, this.activeSnapshot);
      this.pruneLruScopes();
    }

    this.snapshotsByScope.forEach((snap: OrbitSidePanelSnapshot, key: string): void => {
      if (key.length === 0 || isForbiddenScopeKey(key) || snap.isEmptyDefault()) {
        return;
      }
      const tabsCopy: OrbitSidePanelTab[] = [];
      for (let i = 0; i < snap.tabs.length; i++) {
        tabsCopy.push(cloneTab(snap.tabs[i]));
      }
      const closedCopy: OrbitSidePanelTab[] = [];
      for (let i = 0; i < snap.closedTabs.length && i < ORBIT_SIDE_PANEL_MAX_CLOSED_TABS; i++) {
        closedCopy.push(cloneTab(snap.closedTabs[i]));
      }
      const unseenCopy: string[] = [];
      for (let i = 0; i < snap.unseenTabIds.length; i++) {
        unseenCopy.push(snap.unseenTabIds[i]);
      }
      byScope[key] = {
        open: snap.open,
        tabs: tabsCopy,
        activeTabId: snap.activeTabId,
        unseenTabIds: unseenCopy,
        widthVp: clampSidePanelWidth(snap.widthVp),
        closedTabs: closedCopy,
        userDismissed: snap.userDismissed,
        lastUsedAt: snap.lastUsedAt,
        lastUsedOrder: snap.lastUsedOrder
      };
    });

    const envelope: PersistedSidePanelEnvelopeV1 = {
      version: ORBIT_SIDE_PANEL_SCHEMA_VERSION,
      byScope: byScope
    };
    return JSON.stringify(envelope);
  }

  public flushNow(): boolean {
    if (this.persistTimerId !== -1) {
      clearTimeout(this.persistTimerId);
      this.persistTimerId = -1;
    }
    if (this.storage === null) {
      this.dirtyPersist = true;
      return false;
    }
    this.flushSeq += 1;
    const myFlushSeq = this.flushSeq;
    let failedInCallback = false;
    // Clear dirty BEFORE invoking storage so whether onAsyncFailure fires synchronously
    // inside `setSidePanelStateJson` OR asynchronously in a later microtask, dirty stays `true` (N-A).
    this.dirtyPersist = false;
    try {
      const payload = this.serializeState();
      const persisted = this.storage.setSidePanelStateJson(payload, (): void => {
        failedInCallback = true;
        if (myFlushSeq === this.flushSeq) {
          this.dirtyPersist = true;
        }
      });
      if (!persisted || failedInCallback) {
        this.dirtyPersist = true;
        return false;
      }
      return true;
    } catch (err) {
      this.dirtyPersist = true;
      orbitLogWarn('OrbitSidePanelStore', 'Failed to persist sidePanel state', err as Object);
      return false;
    }
  }

  public dispose(): void {
    if (this.dirtyPersist || this.persistTimerId !== -1) {
      this.flushNow();
    }
  }

  private resetToEmpty(): void {
    this.snapshotsByScope.clear();
    this.lruClock = 0;
    this.activeSnapshot = new OrbitSidePanelSnapshot(this.preferredWidthVp);
    this.stateRevision += 1;
  }

  private deserializeSnapshot(
    rec: Record<string, Object | null | undefined>,
    fallbackOrder: number,
    scopeKey: string = ''
  ): OrbitSidePanelSnapshot | null {
    const rawWidth = rec['widthVp'];
    const widthVal = typeof rawWidth === 'number' && Number.isFinite(rawWidth)
      ? clampSidePanelWidth(rawWidth)
      : this.preferredWidthVp;
    const snap = new OrbitSidePanelSnapshot(widthVal);
    const cleanScopeKey = scopeKey.trim();

    // Extract authoritative scope agent for non-draft scopes (`H::real` -> `real`, `draft::H` -> `null`).
    let scopeAgentId: string | null = null;
    const scopeSep = cleanScopeKey.indexOf('::');
    if (scopeSep > 0 && !cleanScopeKey.startsWith('draft::')) {
      const parsedAgent = cleanScopeKey.substring(scopeSep + 2).trim();
      if (parsedAgent.length > 0) {
        scopeAgentId = parsedAgent;
      }
    }

    const sideQuestionMetadataPriority = (tab: OrbitSidePanelTab, alreadyCanonical: boolean): number => {
      let score = 0;
      if (alreadyCanonical) {
        score += 4;
      }
      if (tab.agentId !== null && tab.agentId.trim().length > 0) {
        score += 2;
        if (scopeAgentId !== null && tab.agentId.trim() === scopeAgentId) {
          score += 2;
        }
      }
      if (tab.cwd !== null && tab.cwd.trim().length > 0) {
        score += 1;
      }
      return score;
    };

    const canonicalizeHydratedTab = (tab: OrbitSidePanelTab): OrbitSidePanelTab => {
      // Collapse ANY legacy or stale-draft side_question tab inside `cleanScopeKey` to the single
      // canonical scope-bound tabId (e.g. both `["side_question","draft::H"]` and `side_question:H::real`
      // normalize to `canonicalSideQuestionTabId("H::real")`, R3-P1-3).
      if (tab.kind === 'side_question' && cleanScopeKey.length > 0) {
        tab.tabId = canonicalSideQuestionTabId(cleanScopeKey);
        if ((tab.agentId === null || tab.agentId.trim().length === 0) && scopeAgentId !== null) {
          tab.agentId = scopeAgentId;
        }
      }
      return tab;
    };

    const rawTabs = rec['tabs'];
    const seenIds: Set<string> = new Set<string>();
    const tabIndexById: Map<string, number> = new Map<string, number>();
    const tabScoreById: Map<string, number> = new Map<string, number>();
    const migratedIdMap: Map<string, string> = new Map<string, string>();
    const tabs: OrbitSidePanelTab[] = [];
    if (Array.isArray(rawTabs)) {
      for (let i = 0; i < rawTabs.length; i++) {
        const rawEntry = rawTabs[i] as Object;
        const origRawId = (rawEntry !== null && rawEntry !== undefined && typeof rawEntry === 'object' && !Array.isArray(rawEntry))
          ? ((rawEntry as Record<string, Object | null | undefined>)['tabId'] as string | undefined)
          : undefined;
        const origTrimmed = typeof origRawId === 'string' ? origRawId.trim() : '';
        const sanitizedRaw = sanitizePersistedTab(rawEntry);
        if (sanitizedRaw !== null) {
          const canonicalTargetId = (sanitizedRaw.kind === 'side_question' && cleanScopeKey.length > 0)
            ? canonicalSideQuestionTabId(cleanScopeKey)
            : sanitizedRaw.tabId;
          const alreadyCanonical = origTrimmed === canonicalTargetId;
          const priorityScore = sideQuestionMetadataPriority(sanitizedRaw, alreadyCanonical);
          const sanitized = canonicalizeHydratedTab(sanitizedRaw);
          if (origTrimmed.length > 0 && !alreadyCanonical) {
            migratedIdMap.set(origTrimmed, sanitized.tabId);
          }
          if (!seenIds.has(sanitized.tabId)) {
            seenIds.add(sanitized.tabId);
            tabIndexById.set(sanitized.tabId, tabs.length);
            tabScoreById.set(sanitized.tabId, priorityScore);
            tabs.push(sanitized);
          } else {
            const prevScore = tabScoreById.get(sanitized.tabId) ?? -1;
            if (priorityScore > prevScore) {
              // Prefer canonical/non-null-agent metadata over an earlier migrated draft/legacy duplicate.
              const existingIdx = tabIndexById.get(sanitized.tabId);
              if (existingIdx !== undefined && existingIdx >= 0 && existingIdx < tabs.length) {
                tabs[existingIdx] = sanitized;
                tabScoreById.set(sanitized.tabId, priorityScore);
              }
            }
          }
        }
      }
    }
    snap.tabs = tabs;

    const rawClosed = rec['closedTabs'];
    const closedTabs: OrbitSidePanelTab[] = [];
    const seenClosed: Set<string> = new Set<string>();
    const closedIndexById: Map<string, number> = new Map<string, number>();
    const closedScoreById: Map<string, number> = new Map<string, number>();
    if (Array.isArray(rawClosed)) {
      for (let i = 0; i < rawClosed.length; i++) {
        const rawEntry = rawClosed[i] as Object;
        const origRawId = (rawEntry !== null && rawEntry !== undefined && typeof rawEntry === 'object' && !Array.isArray(rawEntry))
          ? ((rawEntry as Record<string, Object | null | undefined>)['tabId'] as string | undefined)
          : undefined;
        const origTrimmed = typeof origRawId === 'string' ? origRawId.trim() : '';
        const sanitizedRaw = sanitizePersistedTab(rawEntry);
        if (sanitizedRaw !== null) {
          const canonicalTargetId = (sanitizedRaw.kind === 'side_question' && cleanScopeKey.length > 0)
            ? canonicalSideQuestionTabId(cleanScopeKey)
            : sanitizedRaw.tabId;
          const alreadyCanonical = origTrimmed === canonicalTargetId;
          const priorityScore = sideQuestionMetadataPriority(sanitizedRaw, alreadyCanonical);
          const sanitized = canonicalizeHydratedTab(sanitizedRaw);
          if (origTrimmed.length > 0 && !alreadyCanonical) {
            migratedIdMap.set(origTrimmed, sanitized.tabId);
          }
          if (seenIds.has(sanitized.tabId)) {
            continue;
          }
          if (!seenClosed.has(sanitized.tabId)) {
            seenClosed.add(sanitized.tabId);
            closedIndexById.set(sanitized.tabId, closedTabs.length);
            closedScoreById.set(sanitized.tabId, priorityScore);
            closedTabs.push(sanitized);
          } else {
            // Apply the EXACT SAME canonical metadata priority rule to closedTabs while preserving LIFO slot (R3-P1-3).
            const prevScore = closedScoreById.get(sanitized.tabId) ?? -1;
            if (priorityScore > prevScore) {
              const existingIdx = closedIndexById.get(sanitized.tabId);
              if (existingIdx !== undefined && existingIdx >= 0 && existingIdx < closedTabs.length) {
                closedTabs[existingIdx] = sanitized;
                closedScoreById.set(sanitized.tabId, priorityScore);
              }
            }
          }
        }
      }
    }
    snap.closedTabs = closedTabs.length > ORBIT_SIDE_PANEL_MAX_CLOSED_TABS
      ? closedTabs.slice(0, ORBIT_SIDE_PANEL_MAX_CLOSED_TABS)
      : closedTabs;

    const rawActive = rec['activeTabId'];
    const rawActiveStr = typeof rawActive === 'string' ? rawActive.trim() : '';
    snap.activeTabId = migratedIdMap.get(rawActiveStr) ?? rawActiveStr;
    snap.normalizeActiveTabId();

    const rawUnseen = rec['unseenTabIds'];
    const unseen: string[] = [];
    if (Array.isArray(rawUnseen)) {
      for (let i = 0; i < rawUnseen.length; i++) {
        const item = rawUnseen[i] as Object;
        if (typeof item === 'string') {
          const cleanItem = item.trim();
          const mapped = migratedIdMap.get(cleanItem) ?? cleanItem;
          if (seenIds.has(mapped) && unseen.indexOf(mapped) < 0) {
            unseen.push(mapped);
          }
        }
      }
    }
    snap.unseenTabIds = unseen;

    const rawOpen = rec['open'];
    const allTabsDropped = Array.isArray(rawTabs) && rawTabs.length > 0 && tabs.length === 0;
    snap.open = rawOpen === true && !allTabsDropped;
    snap.userDismissed = rec['userDismissed'] === true;

    const rawLastUsedAt = rec['lastUsedAt'];
    snap.lastUsedAt = typeof rawLastUsedAt === 'number' && Number.isFinite(rawLastUsedAt)
      ? rawLastUsedAt
      : Date.now();
    const rawOrder = rec['lastUsedOrder'];
    snap.lastUsedOrder = typeof rawOrder === 'number' && Number.isFinite(rawOrder) && rawOrder > 0
      ? rawOrder
      : fallbackOrder;
    return snap;
  }

  private touchScope(scopeKey: string, snap: OrbitSidePanelSnapshot, nowMs: number = Date.now()): void {
    this.lruClock += 1;
    snap.lastUsedAt = nowMs;
    snap.lastUsedOrder = this.lruClock;
    if (scopeKey.length > 0 && !snap.isEmptyDefault()) {
      this.snapshotsByScope.set(scopeKey, snap);
      this.pruneLruScopes();
    }
  }

  private touchAndPersist(scopeKey: string, snap: OrbitSidePanelSnapshot): void {
    if (scopeKey.length > 0) {
      if (snap.isEmptyDefault()) {
        this.snapshotsByScope.delete(scopeKey);
      } else {
        this.touchScope(scopeKey, snap, Date.now());
      }
    }
    this.schedulePersist();
  }

  private pruneLruScopes(): void {
    if (this.snapshotsByScope.size <= ORBIT_SIDE_PANEL_MAX_SCOPES) {
      return;
    }
    const entries: ScopeEvictionEntry[] = [];
    this.snapshotsByScope.forEach((snap: OrbitSidePanelSnapshot, key: string): void => {
      if (key !== this.activeScopeKey) {
        entries.push({
          key: key,
          order: snap.lastUsedOrder,
          lastUsedAt: snap.lastUsedAt
        });
      }
    });
    entries.sort((a: ScopeEvictionEntry, b: ScopeEvictionEntry): number => {
      if (a.order !== b.order) {
        return a.order - b.order;
      }
      return a.lastUsedAt - b.lastUsedAt;
    });
    const toEvict = this.snapshotsByScope.size - ORBIT_SIDE_PANEL_MAX_SCOPES;
    for (let i = 0; i < toEvict && i < entries.length; i++) {
      this.snapshotsByScope.delete(entries[i].key);
    }
  }

  private schedulePersist(): void {
    this.dirtyPersist = true;
    if (this.storage === null) {
      return;
    }
    if (this.persistTimerId !== -1) {
      clearTimeout(this.persistTimerId);
      this.persistTimerId = -1;
    }
    if (this.debounceMs === 0) {
      this.flushNow();
      return;
    }
    this.persistTimerId = setTimeout((): void => {
      this.persistTimerId = -1;
      this.flushNow();
    }, this.debounceMs);
  }

  private notifyVisibilityChangeIfNeeded(prevOpen: boolean, nextOpen: boolean): void {
    if (prevOpen !== nextOpen && this.visibilityListener !== null) {
      this.visibilityListener.onSidePanelVisibilityChanged(nextOpen);
    }
  }
}
