import {
  BriefingDecisionDto,
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
