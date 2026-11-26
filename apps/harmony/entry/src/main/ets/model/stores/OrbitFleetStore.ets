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

  // Non-@Trace O(1) lookup indices synchronized alongside rendered arrays.
  private readonly hostById: Map<string, HostCardDto> = new Map<string, HostCardDto>();
  private readonly hostDetailById: Map<string, HostDetailDto> = new Map<string, HostDetailDto>();
  private readonly dirtyConversationIndex: Map<string, boolean> = new Map<string, boolean>();

  private bridge: OrbitFleetBridge | null = null;
  private fleetGen: number = 0;
  private hostGen: number = 0;
  private sidebarGen: number = 0;

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
    }
    this.hosts = snap.hosts;
    this.projects = snap.projects;
    this.totalPendingApprovals = snap.total_pending_approvals;
    this.fleetRevision = snap.revision;
  }

  public injectSyntheticHostIfEmpty(hostId: string, displayName: string): void {
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
      return detail;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'hostDetailJson failed', err as Object);
      return null;
    }
  }

  public async reloadHostAsync(hostId: string): Promise<HostDetailDto | null> {
    if (this.bridge === null || hostId.length === 0) {
      return null;
    }
    const isSelected = hostId === this.selectedHostId;
    if (isSelected) {
      this.hostGen += 1;
    }
    const reqGen = this.hostGen;
    try {
      const raw = await this.bridge.hostDetailJsonAsync(hostId);
      const detail = JSON.parse(raw) as HostDetailDto | null;
      if (detail !== null) {
        this.hostDetailById.set(hostId, detail);
      } else {
        this.hostDetailById.delete(hostId);
      }
      if (isSelected && hostId === this.selectedHostId && reqGen === this.hostGen) {
        this.currentHostDetail = detail;
      }
      return detail;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'hostDetailJsonAsync failed', err as Object);
      return null;
    }
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
    this.hostDetailById.delete(hostId);
    this.hostById.delete(hostId);
  }
}
