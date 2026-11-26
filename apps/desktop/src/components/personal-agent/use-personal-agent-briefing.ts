import { useCallback, useEffect, useMemo, useRef, useState, useSyncExternalStore } from "react";
import {
  type HostDetailView,
  type PersonalAgentActivityEntry,
  type PersonalAgentBriefingView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { orbitActions, subscribeOrbitTopics, useOrbitSelector } from "@/state/orbit-store";
import { selectPersonalAgentRef } from "@/state/selectors/personal-agent-selectors";
import {
  type SinceLastVisitSummary,
  computeSinceLastVisitSummary,
  filterActivityForPersonalAgent,
} from "./personal-agent-briefing-model";

const BRIEFING_THROTTLE_MS = 160;
const ACTIVITY_THROTTLE_MS = 750;
const ACTIVITY_FETCH_LIMIT = 50;
const LAST_VISIT_STORAGE_KEY = "orbit.desktop.pa.last_visit_ms.v1";

interface BriefingStoreSnapshot {
  briefing: PersonalAgentBriefingView | null;
  activity: PersonalAgentActivityEntry[];
  loading: boolean;
  activityLoading: boolean;
}

const EMPTY_ACTIVITY: PersonalAgentActivityEntry[] = [];

let currentSnapshot: BriefingStoreSnapshot = {
  briefing: null,
  activity: EMPTY_ACTIVITY,
  loading: false,
  activityLoading: false,
};

const listeners = new Set<() => void>();
let unsubscribeOrbitFleet: (() => void) | null = null;
let activityConsumerCount = 0;

let briefingTimerId: number | null = null;
let briefingReqSeq = 0;
let activityTimerId: number | null = null;
let activityReqSeq = 0;
let lastActivitySignature = "";
let lastActivityHostId: string | null = null;

function emitChange(): void {
  for (const listener of listeners) {
    listener();
  }
}

function updateSnapshot(patch: Partial<BriefingStoreSnapshot>): void {
  currentSnapshot = { ...currentSnapshot, ...patch };
  emitChange();
}

function computeActivityTriggerSignature(hostDetails: Record<string, HostDetailView>): string {
  const parts: string[] = [];
  for (const [hostId, detail] of Object.entries(hostDetails)) {
    const permIds = (detail.pending_permissions ?? [])
      .map((p) => `${p.agent_id}:${p.request_id}:${p.router_recommendation?.decision ?? ""}`)
      .join(",");
    let maxSeq = 0;
    let runningCount = 0;
    let awaitingCount = 0;
    for (const ag of detail.agents) {
      if (ag.latest_seq > maxSeq) maxSeq = ag.latest_seq;
      if (ag.is_running) runningCount += 1;
      if (ag.is_awaiting_permission) awaitingCount += 1;
    }
    parts.push(`${hostId}|${permIds}|${detail.agents.length}|${maxSeq}|${runningCount}|${awaitingCount}`);
  }
  return parts.sort().join(";");
}

async function fetchBriefingNow(): Promise<void> {
  const reqId = ++briefingReqSeq;
  const snap = orbitActions.getSnapshot();
  const primaryHostId = snap.primaryRouterHostId;
  if (currentSnapshot.briefing === null && !currentSnapshot.loading) {
    updateSnapshot({ loading: true });
  }
  try {
    const view = await orbitIpc.getPersonalAgentBriefing(primaryHostId);
    if (reqId !== briefingReqSeq) return;
    updateSnapshot({ briefing: view, loading: false });
  } catch {
    if (reqId !== briefingReqSeq) return;
    updateSnapshot({ loading: false });
  }
}

async function fetchActivityNow(explicitHostId?: string | null): Promise<void> {
  const snap = orbitActions.getSnapshot();
  const pa = selectPersonalAgentRef(snap);
  const hostId = explicitHostId ?? pa?.hostId ?? currentSnapshot.briefing?.personal_agent?.host_id ?? null;
  if (!hostId) {
    if (currentSnapshot.activity.length > 0 || currentSnapshot.activityLoading) {
      updateSnapshot({ activity: EMPTY_ACTIVITY, activityLoading: false });
    }
    return;
  }

  const reqId = ++activityReqSeq;
  lastActivityHostId = hostId;
  if (currentSnapshot.activity.length === 0 && !currentSnapshot.activityLoading) {
    updateSnapshot({ activityLoading: true });
  }
  try {
    const entries = await orbitIpc.listPersonalAgentActivity(hostId, ACTIVITY_FETCH_LIMIT);
    if (reqId !== activityReqSeq) return;
    updateSnapshot({ activity: entries ?? EMPTY_ACTIVITY, activityLoading: false });
  } catch {
    if (reqId !== activityReqSeq) return;
    updateSnapshot({ activityLoading: false });
  }
}

function scheduleBriefingRefresh(immediate = false): void {
  if (typeof window === "undefined") {
    void fetchBriefingNow();
    return;
  }
  if (immediate) {
    if (briefingTimerId !== null) {
      window.clearTimeout(briefingTimerId);
      briefingTimerId = null;
    }
    void fetchBriefingNow();
    return;
  }
  if (briefingTimerId !== null) return;
  briefingTimerId = window.setTimeout(() => {
    briefingTimerId = null;
    void fetchBriefingNow();
  }, BRIEFING_THROTTLE_MS);
}

function scheduleActivityRefresh(immediate = false): void {
  if (activityConsumerCount <= 0) return;
  if (typeof window === "undefined") {
    void fetchActivityNow();
    return;
  }
  if (immediate) {
    if (activityTimerId !== null) {
      window.clearTimeout(activityTimerId);
      activityTimerId = null;
    }
    void fetchActivityNow();
    return;
  }
  if (activityTimerId !== null) return;
  activityTimerId = window.setTimeout(() => {
    activityTimerId = null;
    void fetchActivityNow();
  }, ACTIVITY_THROTTLE_MS);
}

function onFleetStoreChanged(): void {
  scheduleBriefingRefresh(false);
  if (activityConsumerCount > 0) {
    const snap = orbitActions.getSnapshot();
    const pa = selectPersonalAgentRef(snap);
    const nextSig = computeActivityTriggerSignature(snap.hostDetails);
    const hostChanged = (pa?.hostId ?? null) !== lastActivityHostId;
    if (hostChanged || nextSig !== lastActivitySignature) {
      lastActivitySignature = nextSig;
      scheduleActivityRefresh(hostChanged);
    }
  }
}

function ensureFleetSubscription(): void {
  if (unsubscribeOrbitFleet !== null) return;
  unsubscribeOrbitFleet = subscribeOrbitTopics("fleet", onFleetStoreChanged);
  scheduleBriefingRefresh(true);
}

function releaseFleetSubscriptionIfIdle(): void {
  if (listeners.size > 0) return;
  unsubscribeOrbitFleet?.();
  unsubscribeOrbitFleet = null;
  if (briefingTimerId !== null && typeof window !== "undefined") {
    window.clearTimeout(briefingTimerId);
    briefingTimerId = null;
  }
  if (activityTimerId !== null && typeof window !== "undefined") {
    window.clearTimeout(activityTimerId);
    activityTimerId = null;
  }
}

function subscribeBriefingStore(listener: () => void): () => void {
  listeners.add(listener);
  ensureFleetSubscription();
  return () => {
    listeners.delete(listener);
    releaseFleetSubscriptionIfIdle();
  };
}

function getBriefingSnapshot(): BriefingStoreSnapshot {
  return currentSnapshot;
}

/**
 * Triggers an immediate refresh of the Personal Agent briefing projection and (when active)
 * its automated decision activity log.
 */
export function refreshPersonalAgentBriefing(opts?: { includeActivity?: boolean }): void {
  scheduleBriefingRefresh(true);
  if (opts?.includeActivity) {
    void fetchActivityNow();
  } else if (activityConsumerCount > 0) {
    scheduleActivityRefresh(true);
  }
}

/**
 * Subscribes to the throttled `getPersonalAgentBriefing` projection and, when `includeActivity`
 * is true (i.e. Personal Agent home is open), refreshes `listPersonalAgentActivity` on open
 * and on permission/agent updates.
 */
export function usePersonalAgentBriefing(opts?: {
  enabled?: boolean;
  includeActivity?: boolean;
}): {
  briefing: PersonalAgentBriefingView | null;
  activity: PersonalAgentActivityEntry[];
  loading: boolean;
  activityLoading: boolean;
  refresh: (refreshOpts?: { includeActivity?: boolean }) => void;
} {
  const enabled = opts?.enabled ?? true;
  const includeActivity = Boolean(enabled && opts?.includeActivity);

  const paRef = useOrbitSelector(selectPersonalAgentRef);
  const paHostId = paRef?.hostId ?? null;
  const paAgentId = paRef?.agentId ?? null;

  const snap = useSyncExternalStore(
    enabled ? subscribeBriefingStore : noopSubscribe,
    getBriefingSnapshot,
    getBriefingSnapshot,
  );

  useEffect(() => {
    if (!includeActivity) return;
    activityConsumerCount += 1;
    lastActivitySignature = computeActivityTriggerSignature(orbitActions.getSnapshot().hostDetails);
    void fetchActivityNow(paHostId);
    return () => {
      activityConsumerCount = Math.max(0, activityConsumerCount - 1);
      if (activityConsumerCount === 0 && activityTimerId !== null && typeof window !== "undefined") {
        window.clearTimeout(activityTimerId);
        activityTimerId = null;
      }
    };
  }, [includeActivity, paHostId, paAgentId]);

  const filteredActivity = useMemo(
    () =>
      includeActivity
        ? filterActivityForPersonalAgent(
            snap.activity,
            snap.briefing,
            paHostId && paAgentId ? { hostId: paHostId, agentId: paAgentId } : null,
          )
        : EMPTY_ACTIVITY,
    [includeActivity, snap.activity, snap.briefing, paHostId, paAgentId],
  );

  const refresh = useCallback((refreshOpts?: { includeActivity?: boolean }) => {
    refreshPersonalAgentBriefing({
      includeActivity: refreshOpts?.includeActivity ?? includeActivity,
    });
  }, [includeActivity]);

  return {
    briefing: enabled ? snap.briefing : null,
    activity: filteredActivity,
    loading: enabled ? snap.loading : false,
    activityLoading: includeActivity ? snap.activityLoading : false,
    refresh,
  };
}

function noopSubscribe(): () => void {
  return () => {};
}

function readStoredLastVisitMap(): Record<string, number> {
  if (typeof window === "undefined") return {};
  try {
    const raw = window.localStorage.getItem(LAST_VISIT_STORAGE_KEY);
    if (!raw) return {};
    const parsed = JSON.parse(raw) as unknown;
    return parsed && typeof parsed === "object" ? (parsed as Record<string, number>) : {};
  } catch {
    return {};
  }
}

function writeStoredLastVisit(scopeKey: string, tsMs: number): void {
  if (typeof window === "undefined" || !scopeKey) return;
  try {
    const map = readStoredLastVisitMap();
    map[scopeKey] = tsMs;
    window.localStorage.setItem(LAST_VISIT_STORAGE_KEY, JSON.stringify(map));
  } catch {
    // Ignore storage quota / privacy errors
  }
}

/**
 * Tracks the user's previous Personal Agent visit timestamp so the conversation can show a
 * dismissible "While you were away" summary (`completed` + `auto_handled`).
 */
export function usePersonalAgentAwayBanner(
  scopeKey: string | null,
  briefing: PersonalAgentBriefingView | null,
  filteredActivity: readonly PersonalAgentActivityEntry[],
): {
  summary: SinceLastVisitSummary;
  dismissed: boolean;
  dismiss: () => void;
} {
  const [dismissedScopeKey, setDismissedScopeKey] = useState<string | null>(null);
  const previousVisitRef = useRef<{ scopeKey: string | null; tsMs: number | null }>({
    scopeKey: null,
    tsMs: null,
  });

  if (scopeKey !== previousVisitRef.current.scopeKey) {
    const stored = scopeKey ? (readStoredLastVisitMap()[scopeKey] ?? null) : null;
    previousVisitRef.current = { scopeKey, tsMs: stored };
  }

  useEffect(() => {
    if (!scopeKey) return;
    const recordNow = () => writeStoredLastVisit(scopeKey, Date.now());
    const onVisibility = () => {
      if (document.visibilityState !== "visible") {
        recordNow();
      }
    };
    window.addEventListener("pagehide", recordNow);
    document.addEventListener("visibilitychange", onVisibility);
    return () => {
      recordNow();
      window.removeEventListener("pagehide", recordNow);
      document.removeEventListener("visibilitychange", onVisibility);
    };
  }, [scopeKey]);

  const summary = useMemo(
    () =>
      computeSinceLastVisitSummary(
        briefing,
        filteredActivity,
        previousVisitRef.current.tsMs,
      ),
    [briefing, filteredActivity],
  );

  const dismiss = useCallback(() => {
    if (!scopeKey) return;
    setDismissedScopeKey(scopeKey);
    const now = Date.now();
    previousVisitRef.current = { scopeKey, tsMs: now };
    writeStoredLastVisit(scopeKey, now);
  }, [scopeKey]);

  return {
    summary,
    dismissed: scopeKey !== null && dismissedScopeKey === scopeKey,
    dismiss,
  };
}
