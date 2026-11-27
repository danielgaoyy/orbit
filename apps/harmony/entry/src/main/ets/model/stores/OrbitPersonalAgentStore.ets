import {
  BriefingDecisionDto,
  BriefingHostDto,
  BriefingMissionDto,
  BriefingWaitItemDto,
  BriefingWorkerDto,
  OrbitMeshPeerRow,
  PersonalAgentActivityEntryDto,
  PersonalAgentBriefingDto,
  RouterHostCapabilitiesDto,
  RouterStatusSnapshotDto
} from '../OrbitTypes';
import { OrbitPersonalAgentBridge } from '../OrbitCoreBridge';
import { orbitLogWarn } from '../OrbitLogger';

const BRIEFING_THROTTLE_MS: number = 160;
const ACTIVITY_THROTTLE_MS: number = 750;

export interface OrbitBriefingAppliedCallback {
  onBriefingApplied(briefing: PersonalAgentBriefingDto): void;
}

export interface OrbitPersonalAgentVisitStorage {
  getLastVisitMs(hostId: string, agentId: string): number;
  setLastVisitMs(hostId: string, agentId: string, tsMs: number): void;
}

function paVisitCompositeKey(hostId: string, agentId: string): string {
  return `${hostId.trim()}::${agentId.trim()}`;
}

/**
 * Personal Agent Briefing, Activity, Router & Fleet Mesh domain store.
 * - Rendered collections use `@Trace T[]`; lookup indices use non-`@Trace` plain `Map`.
 * - `refreshBriefing` is throttled at 160 ms (trailing-edge guaranteed).
 * - `refreshActivity` is throttled at 750 ms, coalesces concurrent identical-scope calls,
 *   and discards stale out-of-order responses via generation counters.
 * - Last-visit timestamps are scoped per `(hostId, agentId)` and persisted via `OrbitPersonalAgentVisitStorage`.
 */
@ObservedV2
export class OrbitPersonalAgentStore {
  @Trace public briefing: PersonalAgentBriefingDto | null = null;
  @Trace public needsYou: BriefingDecisionDto[] = [];
  @Trace public activity: PersonalAgentActivityEntryDto[] = [];
  @Trace public routerHosts: RouterHostCapabilitiesDto[] = [];
  @Trace public activeRouterStatus: RouterStatusSnapshotDto | null = null;
  @Trace public primaryRouterHostId: string = '';
  @Trace public fleetMeshPeers: OrbitMeshPeerRow[] = [];
  @Trace public busy: boolean = false;
  @Trace public sinceLastVisitMs: number = 0;

  // Non-@Trace O(1) lookup indices synchronized alongside rendered arrays.
  private readonly decisionByKey: Map<string, BriefingDecisionDto> = new Map<string, BriefingDecisionDto>();
  private readonly routerHostById: Map<string, RouterHostCapabilitiesDto> =
    new Map<string, RouterHostCapabilitiesDto>();
  private readonly lastVisitByPaKey: Map<string, number> = new Map<string, number>();

  private bridge: OrbitPersonalAgentBridge | null = null;
  private briefingCallback: OrbitBriefingAppliedCallback | null = null;
  private visitStorage: OrbitPersonalAgentVisitStorage | null = null;
  private activeVisitPaKey: string = '';
  private currentVisitToken: number = 0;
  private loadedVisitToken: number = -1;
  private briefingFetchedWhileIdle: boolean = false;

  private lastBriefingFetchMs: number = 0;
  private briefingInvalidated: boolean = true;
  private briefingTimerId: number = -1;
  private lastActivityFetchMs: number = 0;
  private activityTimerId: number = -1;
  private activityGen: number = 0;
  private pendingActivityHostId: string = '';
  private pendingActivityLimit: number = 50;
  private pendingActivitySinceMs: number | null = null;
  private inFlightActivityScopeKey: string = '';
  private inFlightActivityPromise: Promise<void> | null = null;

  public bindBridge(bridge: OrbitPersonalAgentBridge | null): void {
    this.bridge = bridge;
  }

  public setBriefingAppliedCallback(callback: OrbitBriefingAppliedCallback | null): void {
    this.briefingCallback = callback;
  }

  public bindVisitStorage(storage: OrbitPersonalAgentVisitStorage | null): void {
    this.visitStorage = storage;
  }

  public getDecisionByKey(key: string): BriefingDecisionDto | null {
    const found = this.decisionByKey.get(key);
    return found !== undefined ? found : null;
  }

  public getRouterHostById(hostId: string): RouterHostCapabilitiesDto | null {
    const found = this.routerHostById.get(hostId);
    return found !== undefined ? found : null;
  }

  public setRouterHosts(hosts: RouterHostCapabilitiesDto[]): void {
    this.routerHostById.clear();
    for (let i = 0; i < hosts.length; i++) {
      this.routerHostById.set(hosts[i].host_id, hosts[i]);
    }
    this.routerHosts = hosts;
  }

  /**
   * Loads the persisted previous-visit timestamp for `(hostId, agentId)` into `sinceLastVisitMs`
   * WITHOUT overwriting storage yet, so the "While you were away" banner accurately summarizes
   * events since the previous visit for this exact Personal Agent.
   */
  public enterPersonalAgentVisit(hostId: string, agentId: string): number {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      this.activeVisitPaKey = '';
      this.sinceLastVisitMs = 0;
      return 0;
    }
    const key = paVisitCompositeKey(cleanHost, cleanAgent);
    const isNewVisit = this.activeVisitPaKey !== key;
    let stored = this.lastVisitByPaKey.get(key) ?? 0;
    if (stored <= 0 && this.visitStorage !== null) {
      stored = this.visitStorage.getLastVisitMs(cleanHost, cleanAgent);
      if (stored > 0) {
        this.lastVisitByPaKey.set(key, stored);
      }
    }
    this.activeVisitPaKey = key;
    this.sinceLastVisitMs = stored;
    if (isNewVisit) {
      this.currentVisitToken += 1;
      // If the entry flow (e.g., `openPersonalAgentHome` or cold-start bootstrap) fetched a fresh
      // non-invalidated briefing while idle before entering this PA visit, adopt it deterministically
      // with zero wall-clock heuristic so panel mount never triggers a second RPC.
      if (this.briefing !== null && !this.briefingInvalidated && this.briefingFetchedWhileIdle) {
        this.loadedVisitToken = this.currentVisitToken;
        this.briefingFetchedWhileIdle = false;
      } else {
        this.loadedVisitToken = -1;
        this.briefingFetchedWhileIdle = false;
      }
    }
    return stored;
  }

  /**
   * Records and persists the current visit timestamp for `(hostId, agentId)` (called when
   * leaving the PA session, backgrounding the app, or dismissing the away banner).
   */
  public recordPersonalAgentVisitNow(
    hostId: string,
    agentId: string,
    nowMs: number = Date.now()
  ): void {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0 || !Number.isFinite(nowMs) || nowMs <= 0) {
      return;
    }
    const stamped = Math.floor(nowMs);
    const key = paVisitCompositeKey(cleanHost, cleanAgent);
    this.lastVisitByPaKey.set(key, stamped);
    if (this.visitStorage !== null) {
      this.visitStorage.setLastVisitMs(cleanHost, cleanAgent, stamped);
    }
    if (this.activeVisitPaKey === key) {
      this.sinceLastVisitMs = stamped;
    }
  }

  public getPersistedVisitMs(hostId: string, agentId: string): number {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      return 0;
    }
    const key = paVisitCompositeKey(cleanHost, cleanAgent);
    const cached = this.lastVisitByPaKey.get(key) ?? 0;
    if (cached > 0) {
      return cached;
    }
    if (this.visitStorage !== null) {
      const persisted = this.visitStorage.getLastVisitMs(cleanHost, cleanAgent);
      if (persisted > 0) {
        this.lastVisitByPaKey.set(key, persisted);
      }
      return persisted;
    }
    return 0;
  }

  public activeVisitKeyMatches(hostId: string, agentId: string): boolean {
    const cleanHost = hostId.trim();
    const cleanAgent = agentId.trim();
    if (cleanHost.length === 0 || cleanAgent.length === 0) {
      return false;
    }
    return this.activeVisitPaKey === paVisitCompositeKey(cleanHost, cleanAgent);
  }

  public clearActiveVisitKey(): void {
    this.activeVisitPaKey = '';
    this.loadedVisitToken = -1;
    this.briefingFetchedWhileIdle = false;
    this.briefingInvalidated = true;
  }

  public markVisitedNow(): void {
    const pa = this.briefing?.personal_agent ?? null;
    if (pa !== null && pa.host_id.length > 0 && pa.agent_id.length > 0) {
      this.recordPersonalAgentVisitNow(pa.host_id, pa.agent_id, Date.now());
      return;
    }
    this.sinceLastVisitMs = Date.now();
  }

  /**
   * Reuses the already-loaded non-invalidated briefing snapshot for the active PA visit token
   * (zero extra RPCs and zero trailing timers regardless of mount delay, e.g. 1650 ms+).
   * Fetches only when briefing is missing, explicitly invalidated by a backend/user event,
   * or belongs to a previous visit.
   */
  public ensureFreshBriefing(): PersonalAgentBriefingDto | null {
    if (this.bridge === null) {
      return this.briefing;
    }
    if (
      this.briefing !== null &&
      !this.briefingInvalidated &&
      this.loadedVisitToken === this.currentVisitToken
    ) {
      return this.briefing;
    }
    const isNewVisitFetch = this.loadedVisitToken !== this.currentVisitToken;
    return this.refreshBriefing(isNewVisitFetch);
  }

  /**
   * Marks the cached briefing as invalidated by a real backend event or user action and performs
   * a throttled refresh (trailing-edge guaranteed at `BRIEFING_THROTTLE_MS`).
   */
  public refreshBriefing(force: boolean = false): PersonalAgentBriefingDto | null {
    if (this.bridge === null) {
      return this.briefing;
    }
    this.briefingInvalidated = true;
    this.loadedVisitToken = -1;
    this.briefingFetchedWhileIdle = false;
    const nowMs = Date.now();
    const elapsed = nowMs - this.lastBriefingFetchMs;
    if (!force && this.lastBriefingFetchMs > 0 && elapsed < BRIEFING_THROTTLE_MS) {
      if (this.briefingTimerId === -1) {
        const waitMs = Math.max(1, BRIEFING_THROTTLE_MS - elapsed);
        this.briefingTimerId = setTimeout((): void => {
          this.briefingTimerId = -1;
          this.executeBriefingFetch();
        }, waitMs);
      }
      return this.briefing;
    }
    if (this.briefingTimerId !== -1) {
      clearTimeout(this.briefingTimerId);
      this.briefingTimerId = -1;
    }
    return this.executeBriefingFetch();
  }

  public isBriefingDirty(): boolean {
    return this.briefingInvalidated;
  }

  private executeBriefingFetch(): PersonalAgentBriefingDto | null {
    if (this.bridge === null) {
      return null;
    }
    try {
      const hostArg = this.primaryRouterHostId.trim().length > 0
        ? this.primaryRouterHostId.trim()
        : null;
      const raw = this.bridge.personalAgentBriefingJson(hostArg);
      const parsed = JSON.parse(raw) as PersonalAgentBriefingDto;
      // Only mark the visit snapshot loaded and clear dirty AFTER parse succeeds so a failed
      // bridge call preserves the last good UI snapshot while remaining retryable on next ensure.
      this.lastBriefingFetchMs = Date.now();
      this.briefingInvalidated = false;
      this.loadedVisitToken = this.currentVisitToken;
      this.briefingFetchedWhileIdle = this.activeVisitPaKey.length === 0;
      this.applyBriefing(parsed);
      return parsed;
    } catch (err) {
      this.briefingInvalidated = true;
      this.loadedVisitToken = -1;
      this.briefingFetchedWhileIdle = false;
      orbitLogWarn('OrbitPersonalAgentStore', 'personalAgentBriefingJson failed', err as Object);
      return this.briefing;
    }
  }

  public applyBriefing(next: PersonalAgentBriefingDto): void {
    this.decisionByKey.clear();
    const items = next.needs_you ?? [];
    for (let i = 0; i < items.length; i++) {
      this.decisionByKey.set(items[i].key, items[i]);
    }
    this.needsYou = items;
    this.briefing = next;
    if (this.briefingCallback !== null) {
      this.briefingCallback.onBriefingApplied(next);
    }
  }

  /**
   * Projects an isolated briefing bound strictly to `(hostId, agentId)` for a Side Panel Briefing tab,
   * without mutating the fleet-wide `PersonalOverviewView` briefing snapshot.
   */
  public fetchScopedBriefing(
    hostId: string,
    agentId: string | null = null
  ): PersonalAgentBriefingDto | null {
    const cleanHost = hostId.trim();
    if (cleanHost.length === 0) {
      return null;
    }
    let rawBriefing: PersonalAgentBriefingDto | null = null;
    if (this.bridge !== null) {
      try {
        const raw = this.bridge.personalAgentBriefingJson(cleanHost);
        rawBriefing = JSON.parse(raw) as PersonalAgentBriefingDto;
      } catch (err) {
        orbitLogWarn('OrbitPersonalAgentStore', 'Scoped personalAgentBriefingJson failed', err as Object);
        rawBriefing = this.briefing;
      }
    } else {
      rawBriefing = this.briefing;
    }
    if (rawBriefing === null) {
      return null;
    }
    const cleanAgent = agentId !== null ? agentId.trim() : '';
    const matchesTarget = (entryHostId: string, entryAgentId: string): boolean => {
      if (entryHostId !== cleanHost) {
        return false;
      }
      return cleanAgent.length === 0 || entryAgentId === cleanAgent;
    };

    const needsYou: BriefingDecisionDto[] = [];
    for (let i = 0; i < (rawBriefing.needs_you ?? []).length; i++) {
      const dec = rawBriefing.needs_you[i];
      if (matchesTarget(dec.host_id, dec.agent_id)) {
        needsYou.push(dec);
      }
    }

    // Strictly project missions to `(cleanHost, cleanAgent)` so sibling workers on the same mission
    // or same-ID workers on other hosts NEVER leak into `workers`, `state`, `done`, or summary counts (R3-P1-5).
    const countedWorkersByKey: Map<string, BriefingWorkerDto> = new Map<string, BriefingWorkerDto>();
    const missions: BriefingMissionDto[] = [];
    for (let i = 0; i < (rawBriefing.missions ?? []).length; i++) {
      const m = rawBriefing.missions[i];
      const rootMatches = matchesTarget(m.root.host_id, m.root.agent_id);
      const matchedWorkers: BriefingWorkerDto[] = [];
      for (let j = 0; j < (m.workers ?? []).length; j++) {
        const w = m.workers[j];
        if (matchesTarget(w.host_id, w.agent_id)) {
          matchedWorkers.push(w);
        }
      }
      if (!rootMatches && matchedWorkers.length === 0) {
        continue;
      }

      // Build the exact deduplicated participant set represented by this projected mission.
      const projectedParticipants: BriefingWorkerDto[] = [];
      const seenParticipantKeys: Set<string> = new Set<string>();
      const pushParticipant = (entry: BriefingWorkerDto): void => {
        const key = `${entry.host_id}::${entry.agent_id}`;
        if (!seenParticipantKeys.has(key)) {
          seenParticipantKeys.add(key);
          projectedParticipants.push(entry);
        }
        if (!countedWorkersByKey.has(key)) {
          countedWorkersByKey.set(key, entry);
        }
      };
      if (rootMatches) {
        pushParticipant(m.root);
      }
      for (let j = 0; j < matchedWorkers.length; j++) {
        pushParticipant(matchedWorkers[j]);
      }

      const projectedRoot: BriefingWorkerDto = rootMatches ? m.root : matchedWorkers[0];
      const projectedHostNames: string[] = [];
      let missionDone = 0;
      let hasAwaiting = false;
      let hasRunning = false;
      let hasError = false;
      let allDone = projectedParticipants.length > 0;
      let maxUpdatedMs = 0;
      for (let j = 0; j < projectedParticipants.length; j++) {
        const p = projectedParticipants[j];
        if (p.host_name.length > 0 && projectedHostNames.indexOf(p.host_name) < 0) {
          projectedHostNames.push(p.host_name);
        }
        if (p.updated_at_ms > maxUpdatedMs) {
          maxUpdatedMs = p.updated_at_ms;
        }
        if (p.state === 'awaiting') {
          hasAwaiting = true;
          allDone = false;
        } else if (p.state === 'running') {
          hasRunning = true;
          allDone = false;
        } else if (p.state === 'error') {
          hasError = true;
          allDone = false;
        } else if (p.state === 'done') {
          missionDone += 1;
        } else {
          allDone = false;
        }
      }
      let projectedState = projectedParticipants.length > 0 ? projectedParticipants[0].state : 'idle';
      if (hasAwaiting) {
        projectedState = 'awaiting';
      } else if (hasRunning) {
        projectedState = 'running';
      } else if (hasError) {
        projectedState = 'error';
      } else if (allDone) {
        projectedState = 'done';
      }

      missions.push({
        key: m.key,
        root: projectedRoot,
        workers: matchedWorkers,
        host_names: projectedHostNames,
        done: missionDone,
        total: projectedParticipants.length,
        state: projectedState,
        updated_at_ms: maxUpdatedMs > 0 ? maxUpdatedMs : m.updated_at_ms
      });
    }

    const otherActive: BriefingWorkerDto[] = [];
    for (let i = 0; i < (rawBriefing.other_active ?? []).length; i++) {
      const w = rawBriefing.other_active[i];
      if (matchesTarget(w.host_id, w.agent_id)) {
        otherActive.push(w);
        const key = `${w.host_id}::${w.agent_id}`;
        if (!countedWorkersByKey.has(key)) {
          countedWorkersByKey.set(key, w);
        }
      }
    }
    const waiting: BriefingWaitItemDto[] = [];
    for (let i = 0; i < (rawBriefing.waiting ?? []).length; i++) {
      const wi = rawBriefing.waiting[i];
      if (matchesTarget(wi.host_id, wi.agent_id)) {
        waiting.push(wi);
      }
    }
    const hosts: BriefingHostDto[] = [];
    let onlineCount = 0;
    for (let i = 0; i < (rawBriefing.hosts ?? []).length; i++) {
      const h = rawBriefing.hosts[i];
      if (h.host_id === cleanHost) {
        hosts.push(h);
        if (h.online) {
          onlineCount += 1;
        }
      }
    }
    let runningCount = 0;
    let doneRecentCount = 0;
    countedWorkersByKey.forEach((w: BriefingWorkerDto): void => {
      if (w.state === 'running' || w.state === 'awaiting') {
        runningCount += 1;
      } else if (w.state === 'done') {
        doneRecentCount += 1;
      }
    });
    const scopedPa = (
      rawBriefing.personal_agent !== null &&
      rawBriefing.personal_agent.host_id === cleanHost &&
      (cleanAgent.length === 0 || rawBriefing.personal_agent.agent_id === cleanAgent)
    )
      ? rawBriefing.personal_agent
      : null;
    return {
      personal_agent: scopedPa,
      needs_you: needsYou,
      missions: missions,
      other_active: otherActive,
      waiting: waiting,
      hosts: hosts,
      counts: {
        needs_you: needsYou.length,
        running: runningCount,
        done_recent: doneRecentCount,
        hosts_online: onlineCount,
        hosts_total: hosts.length
      },
      since_last_visit: {
        completed: doneRecentCount,
        auto_handled: 0
      }
    };
  }

  public async refreshActivity(
    hostId: string,
    limit: number = 50,
    sinceMs: number | null = null,
    force: boolean = false
  ): Promise<void> {
    const cleanHost = hostId.trim();
    if (this.bridge === null || cleanHost.length === 0) {
      return;
    }
    const clampedLimit = Math.max(1, Math.min(200, Math.floor(limit)));
    const scopeKey = `${cleanHost}|${clampedLimit}|${sinceMs !== null ? sinceMs : ''}`;
    if (!force && this.inFlightActivityPromise !== null && this.inFlightActivityScopeKey === scopeKey) {
      return this.inFlightActivityPromise;
    }
    const nowMs = Date.now();
    const elapsed = nowMs - this.lastActivityFetchMs;
    if (!force && this.lastActivityFetchMs > 0 && elapsed < ACTIVITY_THROTTLE_MS) {
      this.pendingActivityHostId = cleanHost;
      this.pendingActivityLimit = clampedLimit;
      this.pendingActivitySinceMs = sinceMs;
      if (this.activityTimerId === -1) {
        const waitMs = Math.max(1, ACTIVITY_THROTTLE_MS - elapsed);
        this.activityTimerId = setTimeout((): void => {
          this.activityTimerId = -1;
          this.executeActivityFetch(
            this.pendingActivityHostId,
            this.pendingActivityLimit,
            this.pendingActivitySinceMs
          ).catch((err: Object) => {
            orbitLogWarn('OrbitPersonalAgentStore', 'Deferred activity fetch failed', err);
          });
        }, waitMs);
      }
      return;
    }
    if (this.activityTimerId !== -1) {
      clearTimeout(this.activityTimerId);
      this.activityTimerId = -1;
    }
    return this.executeActivityFetch(cleanHost, clampedLimit, sinceMs);
  }

  private executeActivityFetch(
    hostId: string,
    limit: number,
    sinceMs: number | null
  ): Promise<void> {
    if (this.bridge === null || hostId.trim().length === 0) {
      return Promise.resolve();
    }
    const clampedLimit = Math.max(1, Math.min(200, Math.floor(limit)));
    const scopeKey = `${hostId}|${clampedLimit}|${sinceMs !== null ? sinceMs : ''}`;
    this.lastActivityFetchMs = Date.now();
    this.activityGen += 1;
    const reqGen = this.activityGen;
    const promise: Promise<void> = this.bridge
      .listPersonalAgentActivityJson(hostId, clampedLimit, sinceMs)
      .then((raw: string): void => {
        if (reqGen !== this.activityGen) {
          return;
        }
        const parsed = JSON.parse(raw) as PersonalAgentActivityEntryDto[];
        this.activity = Array.isArray(parsed) ? parsed : [];
      })
      .catch((err: Object): void => {
        orbitLogWarn('OrbitPersonalAgentStore', 'listPersonalAgentActivityJson failed', err);
      })
      .finally((): void => {
        if (this.inFlightActivityPromise === promise) {
          this.inFlightActivityPromise = null;
          this.inFlightActivityScopeKey = '';
        }
      });
    this.inFlightActivityScopeKey = scopeKey;
    this.inFlightActivityPromise = promise;
    return promise;
  }

  public dispose(): void {
    if (this.briefingTimerId !== -1) {
      clearTimeout(this.briefingTimerId);
      this.briefingTimerId = -1;
    }
    if (this.activityTimerId !== -1) {
      clearTimeout(this.activityTimerId);
      this.activityTimerId = -1;
    }
    this.activityGen += 1;
    this.inFlightActivityPromise = null;
    this.inFlightActivityScopeKey = '';
  }
}
