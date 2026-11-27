import {
  ClientProjectDto,
  FleetOverviewDto,
  FleetSidebarDto,
  FleetSidebarRequestDto,
  HostCardDto,
  HostDetailDto,
  SidebarConversationDto,
  SidebarProjectGroupDto
} from '../OrbitTypes';
import { OrbitFleetBridge } from '../OrbitCoreBridge';
import { orbitLogWarn } from '../OrbitLogger';

export const ORBIT_MAX_VISIBLE_THREADS_PER_PROJECT: number = 5;

export type OrbitSidebarRowKind =
  | 'hdr_attention'
  | 'hdr_pinned'
  | 'hdr_projects'
  | 'hdr_empty_projects'
  | 'proj'
  | 'proj_more'
  | 'conv';

export interface OrbitSidebarRow {
  readonly key: string;
  readonly kind: OrbitSidebarRowKind;
  readonly section: string;
  readonly conv: SidebarConversationDto | null;
  readonly project: SidebarProjectGroupDto | null;
  readonly showWorkspaceHint: boolean;
  readonly indentLeft: number;
  readonly hiddenCount: number;
  readonly isMoreExpanded: boolean;
  readonly isProjectCollapsed: boolean;
}

function conversationCompositeKey(hostId: string, agentId: string): string {
  return `${hostId}::${agentId}`;
}

function filterConversationsForSidebar(
  items: SidebarConversationDto[] | undefined,
  selectedHostId: string,
  excludedAgentKey: string
): SidebarConversationDto[] {
  if (items === undefined || items.length === 0) {
    return [];
  }
  const out: SidebarConversationDto[] = [];
  for (let i = 0; i < items.length; i += 1) {
    const conv = items[i];
    if (selectedHostId.length > 0 && conv.host_id !== selectedHostId) {
      continue;
    }
    if (
      excludedAgentKey.length > 0 &&
      conversationCompositeKey(conv.host_id, conv.agent.agent_id) === excludedAgentKey
    ) {
      continue;
    }
    out.push(conv);
  }
  return out;
}

/**
 * Pure deterministic builder flattening `FleetSidebarDto` into virtualized `OrbitSidebarRow[]`
 * with stable `hdr:` / `conv:` / `proj:` keys.
 *
 * Rules:
 * - Filters out `is_home_workspace === true` groups that have zero visible conversations.
 * - Excludes the Personal Agent conversation from Pinned and Projects (defense-in-depth alongside core).
 */
export function buildOrbitSidebarRows(
  sidebar: FleetSidebarDto | null,
  selectedHostId: string,
  searchQuery: string,
  collapsedProjectIds: ReadonlySet<string>,
  expandedMoreProjectIds: ReadonlySet<string>,
  personalAgentKey: string = ''
): OrbitSidebarRow[] {
  const rows: OrbitSidebarRow[] = [];
  const normalizedQuery = searchQuery.trim().toLowerCase();

  if (normalizedQuery.length === 0 && sidebar !== null) {
    // Needs Attention still surfaces a Personal Agent when it needs approval (matches core).
    const needsAttention = filterConversationsForSidebar(
      sidebar.needs_attention,
      selectedHostId,
      ''
    );
    if (needsAttention.length > 0) {
      rows.push({
        key: 'hdr:attention',
        kind: 'hdr_attention',
        section: 'attention',
        conv: null,
        project: null,
        showWorkspaceHint: false,
        indentLeft: 0,
        hiddenCount: 0,
        isMoreExpanded: false,
        isProjectCollapsed: false
      });
      for (let i = 0; i < needsAttention.length; i += 1) {
        const conv = needsAttention[i];
        rows.push({
          key: `conv:attn:${conv.host_id}:${conv.agent.agent_id}`,
          kind: 'conv',
          section: 'attention',
          conv: conv,
          project: null,
          showWorkspaceHint: true,
          indentLeft: 4,
          hiddenCount: 0,
          isMoreExpanded: false,
          isProjectCollapsed: false
        });
      }
    }

    const pinned = filterConversationsForSidebar(
      sidebar.pinned_conversations,
      selectedHostId,
      personalAgentKey
    );
    if (pinned.length > 0) {
      rows.push({
        key: 'hdr:pinned',
        kind: 'hdr_pinned',
        section: 'pinned',
        conv: null,
        project: null,
        showWorkspaceHint: false,
        indentLeft: 0,
        hiddenCount: 0,
        isMoreExpanded: false,
        isProjectCollapsed: false
      });
      for (let i = 0; i < pinned.length; i += 1) {
        const conv = pinned[i];
        rows.push({
          key: `conv:pin:${conv.host_id}:${conv.agent.agent_id}`,
          kind: 'conv',
          section: 'pinned',
          conv: conv,
          project: null,
          showWorkspaceHint: true,
          indentLeft: 4,
          hiddenCount: 0,
          isMoreExpanded: false,
          isProjectCollapsed: false
        });
      }
    }
  }

  rows.push({
    key: 'hdr:projects',
    kind: 'hdr_projects',
    section: 'projects',
    conv: null,
    project: null,
    showWorkspaceHint: false,
    indentLeft: 0,
    hiddenCount: 0,
    isMoreExpanded: false,
    isProjectCollapsed: false
  });

  const visibleProjects: SidebarProjectGroupDto[] = [];
  const rawProjects = sidebar !== null ? sidebar.projects : [];
  for (let i = 0; i < rawProjects.length; i += 1) {
    const group = rawProjects[i];
    if (selectedHostId.length > 0 && group.host_id !== selectedHostId) {
      continue;
    }
    const filteredConvs = filterConversationsForSidebar(
      group.conversations,
      selectedHostId,
      personalAgentKey
    );
    if (group.is_home_workspace && filteredConvs.length === 0) {
      continue;
    }
    if (filteredConvs.length === group.conversations.length) {
      visibleProjects.push(group);
    } else {
      visibleProjects.push({
        id: group.id,
        host_id: group.host_id,
        name: group.name,
        primary_cwd: group.primary_cwd,
        directories: group.directories,
        git_branch: group.git_branch,
        pinned_project_id: group.pinned_project_id,
        is_home_workspace: group.is_home_workspace,
        sort_weight_ms: group.sort_weight_ms,
        conversations: filteredConvs
      });
    }
  }

  if (visibleProjects.length === 0) {
    rows.push({
      key: 'hdr:empty_projects',
      kind: 'hdr_empty_projects',
      section: 'projects',
      conv: null,
      project: null,
      showWorkspaceHint: false,
      indentLeft: 0,
      hiddenCount: 0,
      isMoreExpanded: false,
      isProjectCollapsed: false
    });
    return rows;
  }

  for (let i = 0; i < visibleProjects.length; i += 1) {
    const group = visibleProjects[i];
    const isCollapsed = normalizedQuery.length === 0 && collapsedProjectIds.has(group.id);
    rows.push({
      key: `proj:${group.id}`,
      kind: 'proj',
      section: 'projects',
      conv: null,
      project: group,
      showWorkspaceHint: false,
      indentLeft: 0,
      hiddenCount: 0,
      isMoreExpanded: false,
      isProjectCollapsed: isCollapsed
    });

    if (!isCollapsed) {
      const moreExpanded = normalizedQuery.length > 0 || expandedMoreProjectIds.has(group.id);
      const total = group.conversations.length;
      const limit = moreExpanded ? total : Math.min(total, ORBIT_MAX_VISIBLE_THREADS_PER_PROJECT);
      for (let j = 0; j < limit; j += 1) {
        const conv = group.conversations[j];
        rows.push({
          key: `conv:${group.id}:${conv.host_id}:${conv.agent.agent_id}`,
          kind: 'conv',
          section: 'projects',
          conv: conv,
          project: group,
          showWorkspaceHint: false,
          indentLeft: 12,
          hiddenCount: 0,
          isMoreExpanded: false,
          isProjectCollapsed: false
        });
      }

      if (normalizedQuery.length === 0 && total > ORBIT_MAX_VISIBLE_THREADS_PER_PROJECT) {
        rows.push({
          key: `proj:more:${group.id}`,
          kind: 'proj_more',
          section: 'projects',
          conv: null,
          project: group,
          showWorkspaceHint: false,
          indentLeft: 12,
          hiddenCount: total - ORBIT_MAX_VISIBLE_THREADS_PER_PROJECT,
          isMoreExpanded: moreExpanded,
          isProjectCollapsed: false
        });
      }
    }
  }

  return rows;
}

/**
 * Multi-host Fleet & Sidebar domain store.
 * Strictly obeys the state collection rule:
 * - Rendered collections use `@Trace T[]` arrays (`hosts`, `projects`, `sidebarRows`).
 * - O(1) lookup indices use non-`@Trace` plain `Map` / `Set` instances synchronized whenever arrays mutate.
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
  @Trace public sidebarRows: OrbitSidebarRow[] = [];
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
  private readonly collapsedProjectIds: Set<string> = new Set<string>();
  private readonly expandedMoreProjectIds: Set<string> = new Set<string>();
  private personalAgentKey: string = '';

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

  public setPersonalAgentKey(hostId: string, agentId: string): void {
    const next = hostId.length > 0 && agentId.length > 0 ? `${hostId}::${agentId}` : '';
    if (this.personalAgentKey === next) {
      return;
    }
    this.personalAgentKey = next;
    this.rebuildSidebarRows();
  }

  public isProjectCollapsed(projectId: string): boolean {
    return this.collapsedProjectIds.has(projectId);
  }

  public toggleProjectCollapsed(projectId: string): void {
    if (this.collapsedProjectIds.has(projectId)) {
      this.collapsedProjectIds.delete(projectId);
    } else {
      this.collapsedProjectIds.add(projectId);
    }
    this.rebuildSidebarRows();
  }

  public isProjectMoreExpanded(projectId: string): boolean {
    return this.expandedMoreProjectIds.has(projectId);
  }

  public toggleProjectMoreExpanded(projectId: string): void {
    if (this.expandedMoreProjectIds.has(projectId)) {
      this.expandedMoreProjectIds.delete(projectId);
    } else {
      this.expandedMoreProjectIds.add(projectId);
    }
    this.rebuildSidebarRows();
  }

  public setSidebarSearchQuery(query: string): void {
    if (this.sidebarSearchQuery === query) {
      return;
    }
    this.sidebarSearchQuery = query;
    this.rebuildSidebarRows();
  }

  public applySidebarView(next: FleetSidebarDto | null): void {
    this.sidebarView = next;
    this.rebuildSidebarRows();
  }

  public rebuildSidebarRows(): void {
    this.sidebarRows = buildOrbitSidebarRows(
      this.sidebarView,
      this.selectedHostId,
      this.sidebarSearchQuery,
      this.collapsedProjectIds,
      this.expandedMoreProjectIds,
      this.personalAgentKey
    );
  }

  public getHostById(hostId: string): HostCardDto | null {
    const found = this.hostById.get(hostId);
    return found !== undefined ? found : null;
  }

  public getCachedHostDetail(hostId: string): HostDetailDto | null {
    const found = this.hostDetailById.get(hostId);
    return found !== undefined ? found : null;
  }

  public getAllCachedHostDetails(): Map<string, HostDetailDto> {
    return this.hostDetailById;
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
    for (let i = 0; i < this.dirtyConversationKeys.length; i += 1) {
      if (this.dirtyConversationKeys[i] !== key) {
        next.push(this.dirtyConversationKeys[i]);
      }
    }
    this.dirtyConversationKeys = next;
  }

  public applyFleetOverview(snap: FleetOverviewDto): void {
    this.hostById.clear();
    for (let i = 0; i < snap.hosts.length; i += 1) {
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
    for (let i = 0; i < this.hosts.length; i += 1) {
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
      this.rebuildSidebarRows();
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
          const prev = this.hostDetailById.get(hostId);
          if (prev !== undefined && prev.revision > detail.revision) {
            return null;
          }
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
      this.applySidebarView(parsed);
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
      this.applySidebarView(parsed);
      return parsed;
    } catch (err) {
      orbitLogWarn('OrbitFleetStore', 'fleetSidebarByRequestJsonAsync failed', err as Object);
      return null;
    }
  }

  public otherHostsPendingApprovals(): number {
    let total = 0;
    for (let i = 0; i < this.hosts.length; i += 1) {
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
