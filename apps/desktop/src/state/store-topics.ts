import type { OrbitUiState } from "./store-types.ts";

/**
 * Domain-scoped notification channels for `OrbitUiState`.
 *
 * Every top-level field of `OrbitUiState` maps to exactly one `StoreTopic` bit.
 * On `setState`, only changed topic bits are emitted, so high-frequency streaming
 * deltas (`stage`) never wake listeners subscribed solely to unrelated domains
 * (`preferences`, `sidebar`, `terminal`, `fleet`, `updater`, `automations`, etc.).
 */
export const STORE_TOPICS = [
  "stage",
  "fleet",
  "composer",
  "workspace",
  "sidebar",
  "terminal",
  "preferences",
  "updater",
  "automations",
  "cloud",
  "ui",
] as const;

export type StoreTopic = (typeof STORE_TOPICS)[number];

export const TOPIC_BIT: Readonly<Record<StoreTopic, number>> = {
  stage: 1 << 0,
  fleet: 1 << 1,
  composer: 1 << 2,
  workspace: 1 << 3,
  sidebar: 1 << 4,
  terminal: 1 << 5,
  preferences: 1 << 6,
  updater: 1 << 7,
  automations: 1 << 8,
  cloud: 1 << 9,
  ui: 1 << 10,
};

export const ALL_TOPICS_MASK: number = (1 << STORE_TOPICS.length) - 1;

/**
 * Exhaustive compile-time mapping from every `OrbitUiState` key to its notification topic.
 * Adding a field to `OrbitUiState` without classifying it here is a TypeScript error.
 */
export const STATE_KEY_TOPIC: Readonly<Record<keyof OrbitUiState, StoreTopic>> = {
  // fleet
  fleet: "fleet",
  fleetHydrated: "fleet",
  bootstrapError: "fleet",
  hostDetails: "fleet",
  selectedHostId: "fleet",
  selectedAgentId: "fleet",
  selectedWorkspaceCwd: "fleet",
  isDraftNewSession: "fleet",
  sidebarView: "fleet",
  stageRunningSubagentMeta: "fleet",
  sshHosts: "fleet",
  externalEditors: "fleet",
  isBrowserOffline: "fleet",
  dismissedVersionMismatchHosts: "fleet",

  // stage
  stages: "stage",
  rewoundSessions: "stage",
  sessionScrollNonce: "stage",
  pendingJumpTarget: "stage",
  stageFindFocusNonce: "stage",
  slashCommands: "stage",

  // composer
  draftProvider: "composer",
  draftAcpAgentId: "composer",
  draftModels: "composer",
  draftEffort: "composer",
  draftFastMode: "composer",
  draftMode: "composer",
  draftProfileId: "composer",
  draftUseWorktree: "composer",
  draftWorktreeBranch: "composer",
  draftWorktreeRepoPath: "composer",
  draftWorkspaceId: "composer",
  composerDrafts: "composer",
  slashDirectives: "composer",
  inlineMentions: "composer",
  contextItems: "composer",
  pastedAttachments: "composer",
  diffQuotes: "composer",
  queuedPrompts: "composer",
  promptHistory: "composer",

  // workspace
  sidePanelOpen: "workspace",
  sidePanelWidth: "workspace",
  sidePanelTabs: "workspace",
  activeSidePanelTabId: "workspace",
  sidePanelSplit: "workspace",
  fileViewCache: "workspace",
  fileEditorDrafts: "workspace",
  pendingUnsavedFileAction: "workspace",
  sideQuestionsByScope: "workspace",
  sideQuestionDrafts: "workspace",
  sideQuestionFocusNonce: "workspace",
  sidePanelLaunchpadFocusNonce: "workspace",
  workspaceFilesCache: "workspace",
  workspaceEmptyDirsCache: "workspace",
  gitReposCache: "workspace",
  reviewScope: "workspace",
  diffStaged: "workspace",
  diffLayout: "workspace",
  selectedDiffPath: "workspace",
  gitReviews: "workspace",
  overviewDesktopOpenPref: "workspace",
  overviewPopoverOpen: "workspace",
  stageCanDockOverview: "workspace",
  overviewCollapsedSections: "workspace",
  sessionCreatedPrs: "workspace",
  quickOpenFilesOpen: "workspace",

  // sidebar
  sidebarOpen: "sidebar",
  sidebarDesktopOpenPref: "sidebar",
  sidebarAutoYieldedForSidePanel: "sidebar",
  isCompactViewport: "sidebar",
  isTightForDualSidebars: "sidebar",
  sidebarWidth: "sidebar",
  collapsedHostIds: "sidebar",
  collapsedProjectIds: "sidebar",
  collapsedWorkspaceIds: "sidebar",
  hiddenProjectKeys: "sidebar",
  threadCustomTitles: "sidebar",
  archivedThreads: "sidebar",
  archivedSubagents: "sidebar",
  threadManualOrder: "sidebar",
  sidebarProjectSort: "sidebar",
  sidebarThreadFilter: "sidebar",
  sidebarFilterFocusNonce: "sidebar",

  // terminal
  terminalTabs: "terminal",
  terminalSplits: "terminal",
  activeTerminalTabKey: "terminal",
  terminalFocused: "terminal",

  // preferences
  locale: "preferences",
  theme: "preferences",
  resolvedTheme: "preferences",
  enterToSend: "preferences",
  activeTurnSendBehavior: "preferences",
  notifyOnApproval: "preferences",
  notifyOnTurnComplete: "preferences",
  notifyOnAgentError: "preferences",
  notifyOnGoalFinished: "preferences",
  notifySoundEnabled: "preferences",
  notifyQuietHoursEnabled: "preferences",
  notifyQuietStartHour: "preferences",
  notifyQuietEndHour: "preferences",
  coalesceExploration: "preferences",
  worktreeBranchPrefix: "preferences",
  preferredEditor: "preferences",
  voiceInputDeviceId: "preferences",
  voiceEnabledByHost: "preferences",
  terminalFontSize: "preferences",
  terminalCursorStyle: "preferences",
  terminalScrollback: "preferences",
  terminalThemeMode: "preferences",
  customKeybindings: "preferences",

  // updater
  appVersion: "updater",
  hostUpgradeStates: "updater",
  releaseUpdateInfo: "updater",
  checkingReleaseUpdate: "updater",
  releaseCheckError: "updater",
  fleetUpdate: "updater",

  // automations
  lspServersByWorkspace: "automations",
  lspDiagnosticsByFile: "automations",
  lifecycleHooksByScope: "automations",
  browserByHost: "automations",
  guardianByHost: "automations",
  permissionRulesByHost: "automations",
  eventTriggersByHost: "automations",
  imChannelsByHost: "automations",
  pushByHost: "automations",
  pathRulesByScope: "automations",
  externalMcpServersByHost: "automations",

  // cloud
  cloudSyncStatus: "cloud",
  cloudSyncBusy: "cloud",
  cloudSyncError: "cloud",
  cloudSyncDeviceFlow: "cloud",
  cloudSyncLastResult: "cloud",

  // ui
  activeView: "ui",
  workspacePanel: "ui",
  overviewPinnedLanes: "ui",
  settingsSection: "ui",
  instructionsTarget: "ui",
  commandPaletteOpen: "ui",
  onboardingOpen: "ui",
  pairDialogOpen: "ui",
  relayDialogOpen: "ui",
  remoteFolderPickerHostId: "ui",
  exportDialogTarget: "ui",
  copyCodeBlocksTarget: "ui",
  handoffDialogTarget: "ui",
  mediaPreviewTarget: "ui",
  toastNotice: "ui",
  toastAction: "ui",
  lastArchivedUndo: "ui",
};

const STATE_KEY_TOPIC_ENTRIES: ReadonlyArray<readonly [keyof OrbitUiState, number]> = (
  Object.keys(STATE_KEY_TOPIC) as Array<keyof OrbitUiState>
).map((key) => [key, TOPIC_BIT[STATE_KEY_TOPIC[key]]] as const);

const STATE_KEY_TOPIC_BIT_MAP = new Map<string, number>(
  STATE_KEY_TOPIC_ENTRIES.map(([key, bit]) => [key as string, bit]),
);

const SINGLE_TOPIC_BITS: readonly number[] = STORE_TOPICS.map((topic) => TOPIC_BIT[topic]);

export function topicsToMask(topics: StoreTopic | readonly StoreTopic[]): number {
  if (typeof topics === "string") {
    return TOPIC_BIT[topics] ?? 0;
  }
  let mask = 0;
  for (let i = 0; i < topics.length; i += 1) {
    mask |= TOPIC_BIT[topics[i]] ?? 0;
  }
  return mask;
}

/**
 * Zero-allocation bitmask diff of top-level `OrbitUiState` keys between `prev` and `next`.
 */
export function computeChangedTopicMask(prev: OrbitUiState, next: OrbitUiState): number {
  if (prev === next) return 0;
  let dirtyMask = 0;
  for (let i = 0; i < STATE_KEY_TOPIC_ENTRIES.length; i += 1) {
    const entry = STATE_KEY_TOPIC_ENTRIES[i];
    const key = entry[0];
    if (!Object.is(prev[key], next[key])) {
      dirtyMask |= entry[1];
    }
  }
  return dirtyMask;
}

const trackingProxyCache = new WeakMap<OrbitUiState, OrbitUiState>();
const activeMaskStack: number[] = [];

const TRACKING_PROXY_HANDLER: ProxyHandler<OrbitUiState> = {
  get(target, prop, receiver) {
    const depth = activeMaskStack.length;
    if (depth > 0 && typeof prop === "string") {
      const bit = STATE_KEY_TOPIC_BIT_MAP.get(prop);
      if (bit !== undefined) {
        activeMaskStack[depth - 1] |= bit;
      } else if (prop in target) {
        activeMaskStack[depth - 1] = ALL_TOPICS_MASK;
      }
    }
    return Reflect.get(target, prop, receiver);
  },
  has(target, prop) {
    const depth = activeMaskStack.length;
    if (depth > 0 && typeof prop === "string") {
      const bit = STATE_KEY_TOPIC_BIT_MAP.get(prop);
      activeMaskStack[depth - 1] |= bit !== undefined ? bit : ALL_TOPICS_MASK;
    }
    return Reflect.has(target, prop);
  },
  ownKeys(target) {
    const depth = activeMaskStack.length;
    if (depth > 0) {
      activeMaskStack[depth - 1] = ALL_TOPICS_MASK;
    }
    return Reflect.ownKeys(target);
  },
  getOwnPropertyDescriptor(target, prop) {
    const depth = activeMaskStack.length;
    if (depth > 0 && typeof prop === "string") {
      const bit = STATE_KEY_TOPIC_BIT_MAP.get(prop);
      if (bit !== undefined) {
        activeMaskStack[depth - 1] |= bit;
      }
    }
    return Reflect.getOwnPropertyDescriptor(target, prop);
  },
};

/**
 * Evaluates a selector against a cached thin top-level property-tracking Proxy around `state`
 * to automatically infer the exact topic bitmask read by the selector.
 * Nested values are returned unwrapped so reference equality (`Object.is`) is 100% preserved,
 * and an active-mask stack keeps nested/re-entrant selector evaluations isolated.
 */
export function evaluateSelectorWithTopicTracking<T>(
  state: OrbitUiState,
  selector: (s: OrbitUiState) => T,
): { value: T; mask: number } {
  let proxy = trackingProxyCache.get(state);
  if (!proxy) {
    proxy = new Proxy(state, TRACKING_PROXY_HANDLER);
    trackingProxyCache.set(state, proxy);
  }

  activeMaskStack.push(0);
  let value: T;
  let mask: number;
  try {
    value = selector(proxy);
    mask = activeMaskStack[activeMaskStack.length - 1];
  } finally {
    activeMaskStack.pop();
  }

  if ((value as unknown) === proxy) {
    value = state as unknown as T;
    mask = ALL_TOPICS_MASK;
  }
  return { value, mask };
}

interface TopicSubscriptionEntry {
  listener: () => void;
  mask: number;
  lastNotifiedEpoch: number;
  active: boolean;
}

export interface TopicSubscriptionHandle {
  unsubscribe(): void;
  updateMask(nextMask: number): void;
  getMask(): number;
}

/**
 * Per-topic bucketed listener registry.
 * Emitting a single hot topic (e.g. `TOPIC_BIT.stage`) only iterates listeners registered
 * for that topic bucket and performs zero allocations on the hot path.
 */
export class TopicNotificationHub {
  private readonly buckets: ReadonlyArray<Set<TopicSubscriptionEntry>> = STORE_TOPICS.map(
    () => new Set<TopicSubscriptionEntry>(),
  );
  private epoch = 0;

  subscribe(mask: number, listener: () => void): TopicSubscriptionHandle {
    const entry: TopicSubscriptionEntry = {
      listener,
      mask: 0,
      lastNotifiedEpoch: 0,
      active: true,
    };
    this.applyEntryMask(entry, mask);

    return {
      unsubscribe: () => {
        if (!entry.active) return;
        entry.active = false;
        this.applyEntryMask(entry, 0);
      },
      updateMask: (nextMask: number) => {
        if (!entry.active) return;
        const normalized = nextMask & ALL_TOPICS_MASK;
        if (entry.mask === normalized) return;
        this.applyEntryMask(entry, normalized);
      },
      getMask: () => entry.mask,
    };
  }

  emit(dirtyMask: number): number {
    const effectiveMask = dirtyMask & ALL_TOPICS_MASK;
    if (effectiveMask === 0) return 0;

    this.epoch += 1;
    const currentEpoch = this.epoch;
    let notified = 0;

    for (let i = 0; i < SINGLE_TOPIC_BITS.length; i += 1) {
      if ((effectiveMask & SINGLE_TOPIC_BITS[i]) === 0) continue;
      const bucket = this.buckets[i];
      if (bucket.size === 0) continue;
      for (const entry of bucket) {
        if (!entry.active || entry.lastNotifiedEpoch === currentEpoch) continue;
        entry.lastNotifiedEpoch = currentEpoch;
        notified += 1;
        entry.listener();
      }
    }
    return notified;
  }

  getBucketSize(topic: StoreTopic): number {
    const idx = STORE_TOPICS.indexOf(topic);
    return idx >= 0 ? this.buckets[idx].size : 0;
  }

  private applyEntryMask(entry: TopicSubscriptionEntry, nextMask: number): void {
    const normalized = nextMask & ALL_TOPICS_MASK;
    const diff = entry.mask ^ normalized;
    if (diff === 0) return;

    for (let i = 0; i < SINGLE_TOPIC_BITS.length; i += 1) {
      const bit = SINGLE_TOPIC_BITS[i];
      if ((diff & bit) === 0) continue;
      if ((normalized & bit) !== 0) {
        this.buckets[i].add(entry);
      } else {
        this.buckets[i].delete(entry);
      }
    }
    entry.mask = normalized;
  }
}

export interface TopicStoreKernel<S extends OrbitUiState = OrbitUiState> {
  getState(): S;
  setState(updater: (prev: S) => S): { prev: S; next: S; dirtyMask: number };
  replaceStateSilent(next: S): void;
  subscribeMask(mask: number, listener: () => void): TopicSubscriptionHandle;
  subscribeTopics(topics: StoreTopic | readonly StoreTopic[], listener: () => void): () => void;
  subscribeAll(listener: () => void): () => void;
  emitMask(dirtyMask: number): number;
  getBucketSize(topic: StoreTopic): number;
}

export function createTopicStoreKernel<S extends OrbitUiState>(
  initialState: S,
  isDisposed: () => boolean = () => false,
): TopicStoreKernel<S> {
  let current = initialState;
  const hub = new TopicNotificationHub();

  return {
    getState(): S {
      return current;
    },
    setState(updater: (prev: S) => S): { prev: S; next: S; dirtyMask: number } {
      const prev = current;
      if (isDisposed()) return { prev, next: prev, dirtyMask: 0 };
      const next = updater(prev);
      if (next === prev) return { prev, next: prev, dirtyMask: 0 };
      const dirtyMask = computeChangedTopicMask(prev, next);
      current = next;
      if (dirtyMask !== 0) {
        hub.emit(dirtyMask);
      }
      return { prev, next, dirtyMask };
    },
    replaceStateSilent(next: S): void {
      if (isDisposed()) return;
      current = next;
    },
    subscribeMask(mask: number, listener: () => void): TopicSubscriptionHandle {
      return hub.subscribe(mask, listener);
    },
    subscribeTopics(
      topics: StoreTopic | readonly StoreTopic[],
      listener: () => void,
    ): () => void {
      const handle = hub.subscribe(topicsToMask(topics), listener);
      return () => handle.unsubscribe();
    },
    subscribeAll(listener: () => void): () => void {
      const handle = hub.subscribe(ALL_TOPICS_MASK, listener);
      return () => handle.unsubscribe();
    },
    emitMask(dirtyMask: number): number {
      if (isDisposed()) return 0;
      return hub.emit(dirtyMask);
    },
    getBucketSize(topic: StoreTopic): number {
      return hub.getBucketSize(topic);
    },
  };
}
