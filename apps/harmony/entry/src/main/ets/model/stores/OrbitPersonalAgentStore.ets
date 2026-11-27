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

/**
 * Personal Agent Briefing, Activity, Router & Fleet Mesh domain store.
 * - Rendered collections use `@Trace T[]`; lookup indices use non-`@Trace` plain `Map`.
 * - `refreshBriefing` is throttled at 160 ms (trailing-edge guaranteed).
 * - `refreshActivity` is throttled at 750 ms with generation-based stale response rejection.
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

  private bridge: OrbitPersonalAgentBridge | null = null;
  private lastBriefingFetchMs: number = 0;
  private briefingTimerId: number = -1;
  private lastActivityFetchMs: number = 0;
  private activityTimerId: number = -1;
  private activityGen: number = 0;
  private pendingActivityHostId: string = '';
  private pendingActivityLimit: number = 50;
  private pendingActivitySinceMs: number | null = null;

  public bindBridge(bridge: OrbitPersonalAgentBridge | null): void {
    this.bridge = bridge;
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

  public markVisitedNow(): void {
    this.sinceLastVisitMs = Date.now();
  }

  public refreshBriefing(force: boolean = false): PersonalAgentBriefingDto | null {
    if (this.bridge === null) {
      return this.briefing;
    }
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

  private executeBriefingFetch(): PersonalAgentBriefingDto | null {
    if (this.bridge === null) {
      return null;
    }
    this.lastBriefingFetchMs = Date.now();
    try {
      const hostArg = this.primaryRouterHostId.trim().length > 0
        ? this.primaryRouterHostId.trim()
        : null;
      const raw = this.bridge.personalAgentBriefingJson(hostArg);
      const parsed = JSON.parse(raw) as PersonalAgentBriefingDto;
      this.applyBriefing(parsed);
      return parsed;
    } catch (err) {
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
    if (this.bridge === null || hostId.trim().length === 0) {
      return;
    }
    const nowMs = Date.now();
    const elapsed = nowMs - this.lastActivityFetchMs;
    if (!force && this.lastActivityFetchMs > 0 && elapsed < ACTIVITY_THROTTLE_MS) {
      this.pendingActivityHostId = hostId;
      this.pendingActivityLimit = limit;
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
    await this.executeActivityFetch(hostId, limit, sinceMs);
  }

  private async executeActivityFetch(
    hostId: string,
    limit: number,
    sinceMs: number | null
  ): Promise<void> {
    if (this.bridge === null || hostId.trim().length === 0) {
      return;
    }
    this.lastActivityFetchMs = Date.now();
    this.activityGen += 1;
    const reqGen = this.activityGen;
    try {
      const clampedLimit = Math.max(1, Math.min(200, Math.floor(limit)));
      const raw = await this.bridge.listPersonalAgentActivityJson(hostId, clampedLimit, sinceMs);
      if (reqGen !== this.activityGen) {
        return;
      }
      const parsed = JSON.parse(raw) as PersonalAgentActivityEntryDto[];
      this.activity = Array.isArray(parsed) ? parsed : [];
    } catch (err) {
      orbitLogWarn('OrbitPersonalAgentStore', 'listPersonalAgentActivityJson failed', err as Object);
    }
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
  }
}
