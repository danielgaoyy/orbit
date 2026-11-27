import {
  ClientProjectDto,
  FleetOverviewDto,
  FleetSidebarDto,
  FleetSidebarRequestDto,
  HostCardDto,
  HostDetailDto
} from '../OrbitTypes';
import { OrbitFleetBridge } from '../OrbitCoreBridge';
import { orbitLogWarn } from '../OrbitLogger';

/**
 * Multi-host Fleet & Sidebar domain store.
 * Strictly obeys the state collection rule:
 * - Rendered collections use `@Trace T[]` arrays.
 * - O(1) lookup indices use non-`@Trace` plain `Map` instances synchronized whenever arrays mutate.
 */
@ObservedV2
export class OrbitFleetStore {
  @Trace public hosts: HostCardDto[] = [];
  @Trace public projects: ClientProjectDto[] = [];
  @Trace public totalPendingApprovals: number = 0;
  @Trace public fleetRevision: number = 0;
  @Trace public selectedHostId: string = '';
  @Trace public currentHostDetail: HostDetailDto | null = null;
  @Trace public sidebarView: FleetSidebarDto | null = null;
  @Trace public sidebarProjectSort: string = 'recent';
  @Trace public sidebarThreadFilter: string = 'all';
  @Trace public sidebarSearchQuery: string = '';
  @Trace public dirtyConversationKeys: string[] = [];
  @Trace public hostDetailRevision: number = 0;

  // Non-@Trace O(1) lookup indices synchronized alongside rendered arrays.
  private readonly hostById: Map<string, HostCardDto> = new Map<string, HostCardDto>();
  private readonly hostDetailById: Map<string, HostDetailDto> = new Map<string, HostDetailDto>();
  private readonly hostGenById: Map<string, number> = new Map<string, number>();
  private readonly hostDetailRevisionByHost: Map<string, number> = new Map<string, number>();
  private readonly inFlightHostDetailGenById: Map<string, number> = new Map<string, number>();
  private readonly inFlightHostDetailPromiseById: Map<string, Promise<HostDetailDto | null>> =
    new Map<string, Promise<HostDetailDto | null>>();
  private readonly removedHostIds: Set<string> = new Set<string>();
  private readonly dirtyConversationIndex: Map<string, boolean> = new Map<string, boolean>();

  private bridge: OrbitFleetBridge | null = null;
  private fleetGen: number = 0;
  private hostGen: number = 0;
  private sidebarGen: number = 0;

  private bumpHostGen(hostId: string): number {
    const next = (this.hostGenById.get(hostId) ?? 0) + 1;
    this.hostGenById.set(hostId, next);
    return next;
  }

  private bumpHostDetailRevision(hostId: string): number {
    const clean = hostId.trim();
    this.hostDetailRevision += 1;
    if (clean.length === 0) {
      return this.hostDetailRevision;
    }
    const next = (this.hostDetailRevisionByHost.get(clean) ?? 0) + 1;
    this.hostDetailRevisionByHost.set(clean, next);
    return next;
  }

  public getHostDetailRevision(hostId: string): number {
    const clean = hostId.trim();
    if (clean.length === 0) {
      return 0;
    }
    return this.hostDetailRevisionByHost.get(clean) ?? 0;
  }

  public isHostDetailInFlight(hostId: string): boolean {
    const clean = hostId.trim();
    return clean.length > 0 && this.inFlightHostDetailGenById.has(clean);
  }

  public getAllCachedHostIds(): string[] {
    const out: string[] = [];
    this.hostDetailById.forEach((_detail: HostDetailDto, hid: string): void => {
      out.push(hid);
    });
    return out;
  }

  public setCachedHostDetailForTest(hostId: string, detail: HostDetailDto | null): void {
    const clean = hostId.trim();
    if (clean.length === 0) {
      return;
    }
    this.bumpHostGen(clean);
    this.removedHostIds.delete(clean);
    this.inFlightHostDetailGenById.delete(clean);
    this.inFlightHostDetailPromiseById.delete(clean);
    if (detail !== null) {
      this.hostDetailById.set(clean, detail);
    } else {
      this.hostDetailById.delete(clean);
    }
    if (clean === this.selectedHostId) {
      this.currentHostDetail = detail;
    }
    this.hostDetailRevision += 1;
  }

  public ensureHostDetailAsync(hostId: string): void {
    const clean = hostId.trim();
    if (
      this.bridge === null ||
      clean.length === 0 ||
      this.hostDetailById.has(clean) ||
      this.inFlightHostDetailGenById.has(clean)
    ) {
      return;
    }
    this.reloadHostAsync(clean).catch((err: Object) => {
      orbitLogWarn('OrbitFleetStore', 'ensureHostDetailAsync failed', err);
    });
  }

  /**
   * Awaits an authoritative `HostDetailDto` for `hostId`, explicitly distinguishing a request
   * superseded by a newer generation for the same host (e.g. concurrent `host_changed(hostId)`)
   * from a removed host or genuine bridge failure (R3-P1-2).
   * - When `forceFresh` is `false` and a cached detail already exists, returns it immediately.
   * - When an awaited in-flight promise returns `null` solely because a newer generation bumped
   *   `hostGenById[hostId]`, re-checks the newer in-flight promise or freshly populated cache
   *   (bounded to 16 generation handoffs) instead of falsely failing navigation.
   */
  public async awaitLatestHostDetailAsync(
    hostId: string,
    forceFresh: boolean = false
  ): Promise<HostDetailDto | null> {
    const clean = hostId.trim();
    if (clean.length === 0) {
      return null;
    }
    for (let attempt = 0; attempt < 16; attempt++) {
      if (this.removedHostIds.has(clean)) {
        return null;
      }
      if (forceFresh && attempt === 0) {
        if (this.bridge === null) {
          return null;
        }
        const expectedGen = (this.hostGenById.get(clean) ?? 0) + 1;
        const loaded = await this.reloadHostAsync(clean);
        if (loaded !== null) {
          return loaded;
        }
        if (this.removedHostIds.has(clean)) {
          return null;
        }
        if ((this.hostGenById.get(clean) ?? 0) > expectedGen) {
          continue;
        }
        return null;
      }

      if (!forceFresh && attempt === 0) {
        const initialCached = this.hostDetailById.get(clean);
        if (initialCached !== undefined) {
          return initialCached;
        }
      }

      const existingPromise = this.inFlightHostDetailPromiseById.get(clean);
      if (existingPromise !== undefined) {
        const observedGen =
          this.inFlightHostDetailGenById.get(clean) ?? (this.hostGenById.get(clean) ?? 0);
        const loaded = await existingPromise;
        if (loaded !== null) {
          return loaded;
        }
        if (this.removedHostIds.has(clean)) {
          return null;
        }
        if ((this.hostGenById.get(clean) ?? 0) > observedGen) {
          continue;
        }
        return null;
      }

      const cachedAfterSupersede = this.hostDetailById.get(clean);
      if (cachedAfterSupersede !== undefined) {
        return cachedAfterSupersede;
      }

      if (this.bridge === null) {
        return null;
      }
      const expectedGen = (this.hostGenById.get(clean) ?? 0) + 1;
      const loaded = await this.reloadHostAsync(clean);
      if (loaded !== null) {
        return loaded;
      }
      if (this.removedHostIds.has(clean)) {
        return null;
      }
      if ((this.hostGenById.get(clean) ?? 0) > expectedGen) {
        continue;
      }
      return null;
    }
    if (this.removedHostIds.has(clean)) {
      return null;
    }
    const fallbackCached = this.hostDetailById.get(clean);
    return fallbackCached !== undefined ? fallbackCached : null;
  }

  public async ensureHostDetailLoadedAsync(hostId: string): Promise<HostDetailDto | null> {
    return await this.awaitLatestHostDetailAsync(hostId, false);
  }

  public bindBridge(bridge: OrbitFleetBridge | null): void {
    this.bridge = bridge;
  }

  public getHostById(hostId: string): HostCardDto | null {
    const found = this.hostById.get(hostId);
    return found !== undefined ? found : null;
  }

  public getCachedHostDetail(hostId: string): HostDetailDto | null {
    const found = this.hostDetailById.get(hostId);
    return found !== undefined ? found : null;
  }

  public isConversationDirty(hostId: string, agentId: string): boolean {
    return this.dirtyConversationIndex.get(`${hostId}::${agentId}`) === true;
  }

  public markConversationDirty(hostId: string, agentId: string): void {
    if (hostId.length === 0 || agentId.length === 0) {
      return;
    }
    const key = `${hostId}::${agentId}`;
    if (this.dirtyConversationIndex.get(key) === true) {
      return;
    }
    this.dirtyConversationIndex.set(key, true);
    this.dirtyConversationKeys.push(key);
  }

  public clearConversationDirty(hostId: string, agentId: string): void {
    const key = `${hostId}::${agentId}`;
    if (!this.dirtyConversationIndex.has(key)) {
      return;
    }
    this.dirtyConversationIndex.delete(key);
    const next: string[] = [];
    for (let i = 0; i < this.dirtyConversationKeys.length; i++) {
      if (this.dirtyConversationKeys[i] !== key) {
        next.push(this.dirtyConversationKeys[i]);
      }
    }
    this.dirtyConversationKeys = next;
  }

  public applyFleetOverview(snap: FleetOverviewDto): void {
    this.hostById.clear();
    for (let i = 0; i < snap.hosts.length; i++) {
      this.hostById.set(snap.hosts[i].host_id, snap.hosts[i]);
      this.removedHostIds.delete(snap.hosts[i].host_id);
    }
    this.hosts = snap.hosts;
    this.projects = snap.projects;
    this.totalPendingApprovals = snap.total_pending_approvals;
    this.fleetRevision = snap.revision;
  }

  public injectSyntheticHostIfEmpty(hostId: string, displayName: string): void {
    this.removedHostIds.delete(hostId);
    if (this.hostById.has(hostId)) {
      return;
    }
    const synthetic: HostCardDto = {
      host_id: hostId,
      display_name: displayName,
      hostname: displayName,
      custom_name: null,
      ssh_alias: null,
      route_mode: 'auto',
      remote_daemon_port: 6868,
      custom_direct_endpoint: null,
      via_ssh_tunnel: false,
      last_ping_ms: 1,
      daemon_version: 'perf-replay',
      os_arch: 'ohos',
      daemon_pubkey_b64: '',
      direct_endpoints: [],
      relay_url: null,
      connection_state: 'online_direct',
      last_error: null,
      available_providers: ['antigravity'],
      workspace_count: 1,
      agent_count: 1,
      pending_approval_count: 0,
      is_local: true
    };
    this.hostById.set(hostId, synthetic);
    const nextHosts: HostCardDto[] = [];
    for (let i = 0; i < this.hosts.length; i++) {
      nextHosts.push(this.hosts[i]);
    }
    nextHosts.push(synthetic);
    this.hosts = nextHosts;
  }

  public reloadFleetSync(): FleetOverviewDto | null {
    if (this.bridge === null) {
      return null;
    }
    this.fleetGen += 1;
    try {
      const snap = JSON.parse(this.bridge.fleetOverviewJson()) as FleetOverviewDto;
      this.applyFleetOverview(snap);
      return snap;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'fleetOverviewJson failed', err as Object);
      return null;
    }
  }

  public async reloadFleetAsync(): Promise<FleetOverviewDto | null> {
    if (this.bridge === null) {
      return null;
    }
    this.fleetGen += 1;
    const reqGen = this.fleetGen;
    try {
      const raw = await this.bridge.fleetOverviewJsonAsync();
      if (reqGen !== this.fleetGen) {
        return null;
      }
      const snap = JSON.parse(raw) as FleetOverviewDto;
      this.applyFleetOverview(snap);
      return snap;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'fleetOverviewJsonAsync failed', err as Object);
      return null;
    }
  }

  public setSelectedHostId(hostId: string): boolean {
    const changed = this.selectedHostId !== hostId;
    if (changed) {
      this.hostGen += 1;
      this.selectedHostId = hostId;
      const cached = this.hostDetailById.get(hostId);
      this.currentHostDetail = cached !== undefined ? cached : null;
    }
    return changed;
  }

  public reloadHostSync(hostId: string): HostDetailDto | null {
    if (this.bridge === null || hostId.length === 0) {
      return null;
    }
    this.removedHostIds.delete(hostId);
    this.bumpHostGen(hostId);
    // Clear any stale in-flight async marker superseded by this synchronous reload (P1-12).
    this.inFlightHostDetailGenById.delete(hostId);
    this.inFlightHostDetailPromiseById.delete(hostId);
    if (hostId === this.selectedHostId) {
      this.hostGen += 1;
    }
    try {
      const detail = JSON.parse(this.bridge.hostDetailJson(hostId)) as HostDetailDto | null;
      if (detail !== null) {
        this.hostDetailById.set(hostId, detail);
      } else {
        this.hostDetailById.delete(hostId);
      }
      if (hostId === this.selectedHostId) {
        this.currentHostDetail = detail;
      }
      this.bumpHostDetailRevision(hostId);
      return detail;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'hostDetailJson failed', err as Object);
      return null;
    }
  }

  public reloadHostAsync(hostId: string): Promise<HostDetailDto | null> {
    if (this.bridge === null || hostId.length === 0) {
      return Promise.resolve(null);
    }
    const bridge = this.bridge;
    this.removedHostIds.delete(hostId);
    const reqPerHostGen = this.bumpHostGen(hostId);
    const isSelected = hostId === this.selectedHostId;
    if (isSelected) {
      this.hostGen += 1;
    }
    const reqSelectedGen = this.hostGen;
    this.inFlightHostDetailGenById.set(hostId, reqPerHostGen);

    const runPromise: Promise<HostDetailDto | null> = (async (): Promise<HostDetailDto | null> => {
      try {
        const raw = await bridge.hostDetailJsonAsync(hostId);
        // Drop out-of-order or removed-host response strictly BEFORE writing cache or notifying consumers.
        if (reqPerHostGen !== this.hostGenById.get(hostId)) {
          return null;
        }
        const detail = JSON.parse(raw) as HostDetailDto | null;
        if (detail !== null) {
          this.hostDetailById.set(hostId, detail);
        } else {
          this.hostDetailById.delete(hostId);
        }
        if (isSelected && hostId === this.selectedHostId && reqSelectedGen === this.hostGen) {
          this.currentHostDetail = detail;
        }
        this.bumpHostDetailRevision(hostId);
        return detail;
      } catch (err) {
        orbitLogWarn('OrbitFleetStore', 'hostDetailJsonAsync failed', err as Object);
        return null;
      } finally {
        if (this.inFlightHostDetailGenById.get(hostId) === reqPerHostGen) {
          this.inFlightHostDetailGenById.delete(hostId);
          this.inFlightHostDetailPromiseById.delete(hostId);
        }
      }
    })();

    this.inFlightHostDetailPromiseById.set(hostId, runPromise);
    return runPromise;
  }

  public buildSidebarRequest(
    activeAgentId: string | null,
    activeCwd: string | null
  ): FleetSidebarRequestDto {
    return {
      sort: (this.sidebarProjectSort === 'name' || this.sidebarProjectSort === 'alphabetical')
        ? 'alphabetical'
        : 'recent',
      filter: this.sidebarThreadFilter === 'active_unread' ? 'active_unread' : 'all',
      query: this.sidebarSearchQuery,
      active_host_id: this.selectedHostId.length > 0 ? this.selectedHostId : null,
      active_agent_id: (activeAgentId !== null && activeAgentId.length > 0) ? activeAgentId : null,
      active_cwd: (activeCwd !== null && activeCwd.length > 0) ? activeCwd : null
    };
  }

  public refreshSidebarSync(
    activeAgentId: string | null,
    activeCwd: string | null
  ): FleetSidebarDto | null {
    if (this.bridge === null) {
      return null;
    }
    this.sidebarGen += 1;
    try {
      const req = this.buildSidebarRequest(activeAgentId, activeCwd);
      const parsed = JSON.parse(
        this.bridge.fleetSidebarByRequestJson(JSON.stringify(req))
      ) as FleetSidebarDto;
      this.sidebarView = parsed;
      return parsed;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'fleetSidebarByRequestJson failed', err as Object);
      return null;
    }
  }

  public async refreshSidebarAsync(
    activeAgentId: string | null,
    activeCwd: string | null
  ): Promise<FleetSidebarDto | null> {
    if (this.bridge === null) {
      return null;
    }
    this.sidebarGen += 1;
    const reqGen = this.sidebarGen;
    try {
      const req = this.buildSidebarRequest(activeAgentId, activeCwd);
      const raw = await this.bridge.fleetSidebarByRequestJsonAsync(JSON.stringify(req));
      if (reqGen !== this.sidebarGen) {
        return null;
      }
      const parsed = JSON.parse(raw) as FleetSidebarDto;
      this.sidebarView = parsed;
      return parsed;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'fleetSidebarByRequestJsonAsync failed', err as Object);
      return null;
    }
  }

  public otherHostsPendingApprovals(): number {
    let total = 0;
    for (let i = 0; i < this.hosts.length; i++) {
      if (this.hosts[i].host_id !== this.selectedHostId) {
        total += this.hosts[i].pending_approval_count;
      }
    }
    return total;
  }

  public removeCachedHost(hostId: string): void {
    const clean = hostId.trim();
    if (clean.length === 0) {
      return;
    }
    // Bump per-host generation and mark removed so any in-flight `reloadHostAsync` or
    // `awaitLatestHostDetailAsync` terminates immediately without writing cache (P1-12 + R3-P1-2).
    this.removedHostIds.add(clean);
    this.bumpHostGen(clean);
    this.inFlightHostDetailGenById.delete(clean);
    this.inFlightHostDetailPromiseById.delete(clean);
    this.hostDetailById.delete(clean);
    this.hostById.delete(clean);
    if (this.selectedHostId === clean) {
      this.hostGen += 1;
      this.currentHostDetail = null;
    }
    this.bumpHostDetailRevision(clean);
  }
}
