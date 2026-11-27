import { describe, expect, it } from '@ohos/hypium';
import {
  PersonalAgentBriefingDto,
  SessionSubagentEntryDto,
  WorkspaceFileViewDto,
  cwdPathsEquivalent,
  gitStatusMayHaveDiffForSlot,
  isSubagentActiveStatus,
  isSubagentSettledStatus,
  isWorkspaceFileEditable,
  normalizeAbsoluteCwd,
  normalizeWorkspaceRelPath,
  orbitTextHashSig,
  parseProviderSubagentTimelineJson,
  resolveGitDiffRowActionPath,
  resolveSubagentActualHostId,
  resolveSubagentDirectParentAgentId,
  resolveSubagentDirectParentHostId
} from '../../../main/ets/model/OrbitTypes';
import {
  OrbitCoreBridge,
  OrbitFleetBridge,
  OrbitPersonalAgentBridge
} from '../../../main/ets/model/OrbitCoreBridge';
import { OrbitHarmonyStore } from '../../../main/ets/model/OrbitHarmonyStore';
import { OrbitFleetStore } from '../../../main/ets/model/stores/OrbitFleetStore';
import { OrbitPersonalAgentStore } from '../../../main/ets/model/stores/OrbitPersonalAgentStore';
import {
  ORBIT_SIDE_PANEL_MAX_CLOSED_TABS,
  ORBIT_SIDE_PANEL_MAX_SCOPES,
  OrbitAgentParentEdge,
  OrbitAgentParentLookup,
  OrbitSidePanelKind,
  OrbitSidePanelStorage,
  OrbitSidePanelStore,
  OrbitSidePanelTab,
  canonicalCwdToolTabId,
  canonicalFileTabId,
  canonicalProviderSubagentTabId,
  canonicalSideQuestionTabId,
  isSameFileTarget
} from '../../../main/ets/model/stores/OrbitSidePanelStore';
import {
  ORBIT_KEYCODE_K,
  ORBIT_KEYCODE_T,
  ORBIT_KEYCODE_W,
  ORBIT_SIDEBAR_MD_WIDTH_VP,
  ORBIT_STAGE_MIN_WIDTH_VP,
  OrbitSerializedSheetHost,
  OrbitShellStore,
  OrbitShellWidthStorage,
  OrbitShortcutContext,
  computeOrbitShellLayout,
  resolveOrbitBreakpoint,
  resolveOrbitKeyboardShortcut
} from '../../../main/ets/model/stores/OrbitShellStore';

class FakeSidePanelStorage implements OrbitSidePanelStorage {
  public rawJson: string = '';
  public writeCount: number = 0;
  public failNextSyncWrite: boolean = false;
  public failNextSyncCallback: boolean = false;
  public failNextAsyncMicrotask: boolean = false;

  getSidePanelStateJson(): string {
    return this.rawJson;
  }

  setSidePanelStateJson(payloadJson: string, onAsyncFailure?: () => void): boolean {
    this.writeCount += 1;
    if (this.failNextSyncWrite) {
      this.failNextSyncWrite = false;
      return false;
    }
    this.rawJson = payloadJson;
    if (this.failNextSyncCallback) {
      this.failNextSyncCallback = false;
      if (onAsyncFailure !== undefined) {
        onAsyncFailure();
      }
    } else if (this.failNextAsyncMicrotask) {
      this.failNextAsyncMicrotask = false;
      if (onAsyncFailure !== undefined) {
        const cb = onAsyncFailure;
        Promise.resolve().then(() => {
          cb();
        });
      }
    }
    return true;
  }
}

class FakeShellWidthStorage implements OrbitShellWidthStorage {
  public sidebarW: number = 0;
  public panelW: number = 0;

  getSidebarWidthVp(): number {
    return this.sidebarW;
  }

  setSidebarWidthVp(widthVp: number): void {
    this.sidebarW = widthVp;
  }

  getSidePanelWidthVp(): number {
    return this.panelW;
  }

  setSidePanelWidthVp(widthVp: number): void {
    this.panelW = widthVp;
  }
}

class FakeParentLookup implements OrbitAgentParentLookup {
  private readonly edges: Map<string, OrbitAgentParentEdge> = new Map<string, OrbitAgentParentEdge>();
  private readonly cachedHosts: Set<string> = new Set<string>();
  private enforceCachedHosts: boolean = false;
  private enforceAgentCards: boolean = false;

  public setEnforceCachedHosts(enabled: boolean): void {
    this.enforceCachedHosts = enabled;
  }

  public setEnforceAgentCards(enabled: boolean): void {
    this.enforceAgentCards = enabled;
  }

  public setHostCached(hostId: string, cached: boolean): void {
    if (cached) {
      this.cachedHosts.add(hostId);
    } else {
      this.cachedHosts.delete(hostId);
    }
  }

  public addEdge(
    hostId: string,
    agentId: string,
    parentHostId: string | null,
    parentAgentId: string | null,
    archived: boolean = false
  ): void {
    this.cachedHosts.add(hostId);
    this.edges.set(`${hostId}::${agentId}`, {
      parentHostId: parentHostId,
      parentAgentId: parentAgentId,
      archived: archived
    });
  }

  hasCachedHostDetail(hostId: string): boolean {
    if (!this.enforceCachedHosts) {
      return true;
    }
    return this.cachedHosts.has(hostId);
  }

  hasAgentCard(hostId: string, agentId: string): boolean {
    if (!this.enforceAgentCards) {
      return true;
    }
    if (this.enforceCachedHosts && !this.cachedHosts.has(hostId)) {
      return false;
    }
    return this.edges.has(`${hostId}::${agentId}`);
  }

  getParentEdge(hostId: string, agentId: string): OrbitAgentParentEdge | null {
    if (this.enforceCachedHosts && !this.cachedHosts.has(hostId)) {
      return null;
    }
    const found = this.edges.get(`${hostId}::${agentId}`);
    return found !== undefined ? found : null;
  }
}

class FakeOutOfOrderFleetBridge implements OrbitFleetBridge {
  public pendingResolvers: Array<(payload: string) => void> = [];
  public syncDetailByHost: Map<string, string> = new Map<string, string>();

  fleetOverviewJson(): string {
    return '{}';
  }

  fleetOverviewJsonAsync(): Promise<string> {
    return Promise.resolve('{}');
  }

  hostDetailJson(hostId: string): string {
    const val = this.syncDetailByHost.get(hostId);
    return val !== undefined ? val : 'null';
  }

  hostDetailJsonAsync(_hostId: string): Promise<string> {
    return new Promise<string>((resolve: (val: string) => void) => {
      this.pendingResolvers.push(resolve);
    });
  }

  fleetSidebarByRequestJson(_reqJson: string): string {
    return '{}';
  }

  fleetSidebarByRequestJsonAsync(_reqJson: string): Promise<string> {
    return Promise.resolve('{}');
  }
}

class FakePersonalAgentBridge implements OrbitPersonalAgentBridge {
  public briefingPayload: string = '{}';

  personalAgentBriefingJson(_selectedHostId: string | null): string {
    return this.briefingPayload;
  }

  listPersonalAgentActivityJson(_hostId: string, _limit: number): Promise<string> {
    return Promise.resolve('[]');
  }
}

function makeMinimalHostDetailJson(hostId: string, displayName: string): string {
  return `{"host":{"host_id":"${hostId}","display_name":"${displayName}","hostname":"${displayName}","os":"linux","arch":"x86_64","connection_state":"online_direct","route_mode":"auto","available_providers":["claude"],"running_agents":0,"pending_permissions":0,"total_agents":0,"total_terminals":0,"last_ping_ms":12,"last_error":null,"via_ssh_tunnel":false,"ssh_alias":null,"remote_port":6868,"custom_direct_endpoint":null,"relay_url":null},"agents":[],"archived_agents":[],"workspaces":[],"terminals":[],"active_terminal_id":null,"discovered_projects":[],"directory_listing":null,"quotas":[],"provider_models":[],"schedules":[],"last_created_agent_id":null,"last_created_agent_host_id":null}`;
}

function makeTab(
  tabId: string,
  kind: OrbitSidePanelKind,
  hostId: string,
  agentId: string | null,
  cwd: string | null,
  path: string | null = null,
  title: string = ''
): OrbitSidePanelTab {
  return {
    tabId: tabId,
    kind: kind,
    hostId: hostId,
    agentId: agentId,
    cwd: cwd,
    path: path,
    parentHostId: hostId,
    title: title.length > 0 ? title : tabId,
    closable: true
  };
}

interface DeferredHostDetailEntry {
  hostId: string;
  resolve: (json: string) => void;
}

class FakeOrbitCoreBridgeForStore extends OrbitCoreBridge {
  public fleetJson: string = '{"revision":1,"hosts":[],"projects":[],"total_pending_approvals":0}';
  public sidebarJson: string = '{"revision":1,"hosts":[],"projects":[],"attention_items":[],"running_items":[]}';
  public detailJsonByHost: Map<string, string> = new Map<string, string>();
  public deferHostDetailForHosts: Set<string> = new Set<string>();
  public deferredHostDetailQueue: DeferredHostDetailEntry[] = [];
  public openedTimelines: string[] = [];
  public gitReviewJsonCalls: number = 0;
  public workspaceFilesJsonCalls: number = 0;
  public fetchGitDiffCalls: string[] = [];
  public stagePathsCalls: string[] = [];
  public unstagePathsCalls: string[] = [];
  public gitReviewJsonBySlot: Map<string, string> = new Map<string, string>();
  public defaultGitReviewJson: string = '';
  public workspaceFilesJsonPayload: string = '["a.ts","b.ts","f"]';
  public deferNextGitMutation: boolean = false;
  public rejectNextGitMutationError: string | null = null;
  public deferredGitMutationResolvers: Array<(payload: string) => void> = [];
  private eventCallback: ((err: Error | null, payloadJson: string) => void) | null = null;
  private readonly drafts: Map<string, string> = new Map<string, string>();

  constructor() {
    super(null, false);
  }

  public emitCoreEvent(payloadJson: string): void {
    if (this.eventCallback !== null) {
      this.eventCallback(null, payloadJson);
    }
  }

  public override subscribeEvents(callback: (err: Error | null, payloadJson: string) => void): void {
    this.eventCallback = callback;
  }

  public override fallbackProviderCatalogsJson(): string {
    return '[]';
  }

  public override slashCommandsJson(_platform?: string | null, _query?: string | null): string {
    return '[]';
  }

  public override cloudSyncStatusJson(): string {
    return '{"enabled":false,"configured":false,"state":"disabled","device_id":"","last_sync_at":null,"last_error":null}';
  }

  public override fleetOverviewJson(): string {
    return this.fleetJson;
  }

  public override async fleetOverviewJsonAsync(): Promise<string> {
    return this.fleetJson;
  }

  public override fleetSidebarByRequestJson(_reqJson: string): string {
    return this.sidebarJson;
  }

  public override async fleetSidebarByRequestJsonAsync(_reqJson: string): Promise<string> {
    return this.sidebarJson;
  }

  public override hostDetailJson(hostId: string): string {
    return this.detailJsonByHost.get(hostId) ?? 'null';
  }

  public override async hostDetailJsonAsync(hostId: string): Promise<string> {
    if (this.deferHostDetailForHosts.has(hostId)) {
      return await new Promise<string>((resolve: (v: string) => void): void => {
        this.deferredHostDetailQueue.push({ hostId: hostId, resolve: resolve });
      });
    }
    return this.detailJsonByHost.get(hostId) ?? 'null';
  }

  public override async agentStageDeltaByRequestJsonAsync(_reqJson: string): Promise<string> {
    return '{"changed":true,"full_replace":true,"agent":null,"turns":[],"total_turns":0,"window_start":0,"has_older":false,"oldest_seq":0,"latest_seq":0,"running":false,"state_hash":"h0"}';
  }

  public override agentStageTurnsSliceJson(
    _hostId: string,
    _agentId: string,
    _startTurnIndex: number,
    _count: number
  ): string {
    return '[]';
  }

  public override loadOlderAgentTimeline(_hostId: string, _agentId: string, _limitEvents: number): boolean {
    return false;
  }

  public override personalAgentBriefingJson(_primaryRouterHostId?: string | null): string {
    return '{"enabled":false,"primary_router_host_id":null,"router_hosts":[],"needs_you":[],"missions":[],"counts":{"needs_you":0,"running":0,"done_recent":0}}';
  }

  public override async listPersonalAgentActivityJson(
    _hostId: string,
    _limit: number,
    _sinceMs?: number | null
  ): Promise<string> {
    return '[]';
  }

  public override getComposerDraftJson(draftKey: string): string | null {
    const found = this.drafts.get(draftKey);
    return found !== undefined ? found : null;
  }

  public override setComposerDraft(draftKey: string, draftJson: string): void {
    this.drafts.set(draftKey, draftJson);
  }

  public override deleteComposerDraft(draftKey: string): void {
    this.drafts.delete(draftKey);
  }

  public override connectAllHosts(): void {}
  public override connectHost(_hostId: string): void {}
  public override refreshHost(_hostId: string): void {}
  public override refreshQuotas(_hostId: string): void {}
  public override refreshProviderModels(_hostId: string, _forceRefresh: boolean): void {}
  public override removeHost(_hostId: string): void {}
  public override markAgentSeen(_hostId: string, _agentId: string, _seq: number): void {}
  public override listProviderSubagents(_hostId: string, _parentAgentId: string): void {}

  public override async getSessionUsageJson(_hostId: string, _agentId: string): Promise<string> {
    return 'null';
  }

  public override openAgentTimeline(hostId: string, agentId: string): void {
    this.openedTimelines.push(`${hostId}::${agentId}`);
  }

  public override fetchGitStatus(_hostId: string, _cwd: string): void {}

  public override fetchGitDiff(
    hostId: string,
    cwd: string,
    path: string | null | undefined,
    staged: boolean
  ): void {
    const p = (path !== undefined && path !== null) ? path : '__all__';
    this.fetchGitDiffCalls.push(`${hostId}|${cwd}|${p}|${staged ? 'S' : 'U'}`);
  }

  public override gitReviewJson(
    hostId: string,
    cwd: string,
    path?: string | null,
    staged?: boolean | null
  ): string {
    this.gitReviewJsonCalls += 1;
    const pathKey = (path !== undefined && path !== null && path.length > 0) ? `file:${path}` : 'all:';
    const modeKey = staged === true ? 'S' : 'U';
    const keyed = this.gitReviewJsonBySlot.get(`${hostId}|${cwd}|${pathKey}|${modeKey}`);
    if (keyed !== undefined) {
      return keyed;
    }
    const legacyPath = (path !== undefined && path !== null && path.length > 0) ? path : '__all__';
    const legacyKeyed = this.gitReviewJsonBySlot.get(`${hostId}|${cwd}|${legacyPath}`);
    if (legacyKeyed !== undefined) {
      return legacyKeyed;
    }
    return this.defaultGitReviewJson;
  }

  public override fetchWorkspaceFiles(_hostId: string, _cwd: string): void {}

  public override workspaceFilesJson(_hostId: string, _cwd: string): string {
    this.workspaceFilesJsonCalls += 1;
    return this.workspaceFilesJsonPayload;
  }

  private async resolveGitMutationJson(defaultPayload: string): Promise<string> {
    if (this.rejectNextGitMutationError !== null) {
      const msg = this.rejectNextGitMutationError;
      this.rejectNextGitMutationError = null;
      throw new Error(msg);
    }
    if (this.deferNextGitMutation) {
      this.deferNextGitMutation = false;
      return await new Promise<string>((resolve: (v: string) => void): void => {
        this.deferredGitMutationResolvers.push(resolve);
      });
    }
    return defaultPayload;
  }

  public override async gitStagePathsJson(
    hostId: string,
    cwd: string,
    paths: string[],
    all: boolean
  ): Promise<string> {
    this.stagePathsCalls.push(`${hostId}|${cwd}|${all ? 'all' : JSON.stringify(paths)}`);
    return await this.resolveGitMutationJson('{"ok":true}');
  }

  public override async gitUnstagePathsJson(
    hostId: string,
    cwd: string,
    paths: string[],
    all: boolean
  ): Promise<string> {
    this.unstagePathsCalls.push(`${hostId}|${cwd}|${all ? 'all' : JSON.stringify(paths)}`);
    return await this.resolveGitMutationJson('{"ok":true}');
  }

  public override async gitCommitJson(
    _hostId: string,
    _cwd: string,
    _message: string,
    _amend: boolean
  ): Promise<string> {
    return await this.resolveGitMutationJson('{"short_sha":"abc1234"}');
  }

  public override async gitCreatePrJson(
    _hostId: string,
    _cwd: string,
    title: string,
    _body: string,
    _base: string | null,
    _draft: boolean
  ): Promise<string> {
    return await this.resolveGitMutationJson(`{"title":${JSON.stringify(title)},"url":"https://example.local/pr/1"}`);
  }

  public override async gitStashSaveJson(
    _hostId: string,
    _cwd: string,
    _message: string | null,
    _includeUntracked: boolean
  ): Promise<string> {
    return await this.resolveGitMutationJson('{"ok":true}');
  }

  public override async gitStashPopJson(
    _hostId: string,
    _cwd: string,
    _stashRef: string,
    _expectedOid: string | null
  ): Promise<string> {
    return await this.resolveGitMutationJson('{"ok":true}');
  }

  public override async gitRebaseContinueJson(_hostId: string, _cwd: string): Promise<string> {
    return await this.resolveGitMutationJson('{"ok":true}');
  }

  public override async gitRebaseAbortJson(_hostId: string, _cwd: string): Promise<string> {
    return await this.resolveGitMutationJson('{"ok":true}');
  }

  public override async gitPrStatusJson(_hostId: string, _cwd: string): Promise<string> {
    return 'null';
  }

  public override async gitStashListJson(_hostId: string, _cwd: string): Promise<string> {
    return '[]';
  }

  public override async gitConflictsJson(_hostId: string, _cwd: string): Promise<string> {
    return '[]';
  }

  public override async gitRebaseStatusJson(_hostId: string, _cwd: string): Promise<string> {
    return '{"in_progress":false}';
  }
}

function makeTestHostCardJson(hostId: string): string {
  return `{"host_id":"${hostId}","display_name":"${hostId}","hostname":"${hostId}","custom_name":null,"ssh_alias":null,"route_mode":"auto","remote_daemon_port":6868,"custom_direct_endpoint":null,"via_ssh_tunnel":false,"last_ping_ms":1,"daemon_version":"1.0","os_arch":"linux","daemon_pubkey_b64":"","direct_endpoints":[],"relay_url":null,"connection_state":"online_direct","last_error":null,"available_providers":["claude"],"workspace_count":0,"agent_count":2,"pending_approval_count":0,"is_local":false}`;
}

function makeTestAgentCardJson(hostId: string, agentId: string, cwd: string): string {
  return `{"host_id":"${hostId}","agent_id":"${agentId}","title":"${agentId}","provider":"claude","model":"claude-sonnet","mode":"auto","cwd":"${cwd}","status":{"state":"idle"},"is_running":false,"unread_count":0,"attention":false,"latest_seq":1,"updated_at":"2026-11-27T00:00:00Z","parent_agent_id":null,"parent_host_id":null,"workspace_id":""}`;
}

function makeTestHostDetailJson(hostId: string, agentsJson: string[]): string {
  return `{"host":${makeTestHostCardJson(hostId)},"agents":[${agentsJson.join(',')}],"workspaces":[],"terminals":[],"provider_catalogs":[],"models_by_provider":{"antigravity":[],"codex":[],"claude":[],"pi":[],"acp":[]},"discovered_projects":[],"recent_cwds":["/repo"]}`;
}

function makeTestGitReviewJson(
  revision: number,
  hostId: string,
  cwd: string,
  selectedPath: string | null,
  rawDiff: string,
  diffPaths: string[],
  statusFilesJson: string[],
  staged: boolean = false,
  diffRevision: number = 0,
  repoPrefix?: string | null,
  diffInFlight?: boolean,
  repoRoot?: string | null,
  statusRevision: number = 0,
  cwdIdentityEpoch: number = 0,
  lastDiffErrorRevision: number = 0,
  statusIdentityEpoch: number = cwdIdentityEpoch,
  lastDiffErrorStatusRevision: number = statusRevision,
  statusReverifyPending: boolean = false
): string {
  const diffFilesJson: string[] = [];
  for (let i = 0; i < diffPaths.length; i++) {
    diffFilesJson.push(`{"path":${JSON.stringify(diffPaths[i])},"additions":1,"deletions":0,"lines":[]}`);
  }
  const repoPrefixPart = repoPrefix !== undefined ? `,"repo_prefix":${JSON.stringify(repoPrefix)}` : '';
  const repoRootPart = repoRoot !== undefined ? `,"repo_root":${JSON.stringify(repoRoot)}` : '';
  const inFlightPart = diffInFlight !== undefined ? `,"diff_in_flight":${diffInFlight ? 'true' : 'false'}` : '';
  return `{"revision":${revision},"host_id":${JSON.stringify(hostId)},"cwd":${JSON.stringify(cwd)},"status":{"cwd":${JSON.stringify(cwd)},"is_git_repo":true,"branch":"main","upstream":null,"ahead":0,"behind":0,"files":[${statusFilesJson.join(',')}]${repoPrefixPart}${repoRootPart}},"selected_path":${JSON.stringify(selectedPath)},"staged":${staged ? 'true' : 'false'},"diff_revision":${diffRevision}${inFlightPart},"status_revision":${statusRevision},"status_identity_epoch":${statusIdentityEpoch},"status_reverify_pending":${statusReverifyPending ? 'true' : 'false'},"cwd_identity_epoch":${cwdIdentityEpoch},"last_diff_error_revision":${lastDiffErrorRevision},"last_diff_error_status_revision":${lastDiffErrorStatusRevision},"raw_diff":${JSON.stringify(rawDiff)},"diff_files":[${diffFilesJson.join(',')}]}`;
}

async function flushAsyncTurns(count: number = 10): Promise<void> {
  for (let i = 0; i < count; i++) {
    await new Promise<void>((resolve: () => void): void => {
      setTimeout(resolve, 0);
    });
  }
}

export default function orbitPr4Test(): void {
  describe('OrbitPr4', () => {
    it('fiftySessionSwitchesRestoreIndependentScopes', 0, () => {
      const store = new OrbitSidePanelStore(0);
      const totalScopes = 50;

      for (let i = 0; i < totalScopes; i++) {
        const agentId = `agent-${i}`;
        const cwd = `/work/repo-${i}`;
        store.switchScope('host-a', agentId, 1700000000000 + i);
        expect(store.activeScopeKey).assertEqual(`host-a::${agentId}`);

        if (i % 2 === 0) {
          store.openTab(
            makeTab(`rev-${i}`, 'review', 'host-a', agentId, cwd, null, `Review ${i}`),
            { source: 'user', activate: true }
          );
          store.setWidthVp(360 + (i % 5) * 20);
        } else {
          store.openTab(
            makeTab(`files-${i}`, 'files', 'host-a', agentId, cwd, null, `Files ${i}`),
            { source: 'user', activate: true }
          );
          store.openTab(
            makeTab(
              canonicalFileTabId('host-a', cwd, `src/file_${i}.ets`),
              'file',
              'host-a',
              agentId,
              cwd,
              `src/file_${i}.ets`,
              `file_${i}.ets`
            ),
            { source: 'user', activate: true }
          );
          if (i % 3 === 0) {
            store.dismissPanel();
          }
        }
      }

      for (let i = totalScopes - 1; i >= 0; i--) {
        const agentId = `agent-${i}`;
        const cwd = `/work/repo-${i}`;
        const snap = store.switchScope('host-a', agentId, 1700001000000 + (totalScopes - i));
        if (i % 2 === 0) {
          expect(snap.open).assertTrue();
          expect(snap.tabs.length).assertEqual(1);
          expect(snap.activeTabId).assertEqual(`rev-${i}`);
          expect(snap.tabs[0].cwd).assertEqual(cwd);
          expect(snap.tabs[0].hostId).assertEqual('host-a');
          expect(snap.widthVp).assertEqual(360 + (i % 5) * 20);
        } else {
          expect(snap.open).assertEqual(i % 3 !== 0);
          expect(snap.tabs.length).assertEqual(2);
          expect(snap.activeTabId).assertEqual(canonicalFileTabId('host-a', cwd, `src/file_${i}.ets`));
          expect(snap.tabs[1].path).assertEqual(`src/file_${i}.ets`);
          expect(snap.tabs[1].cwd).assertEqual(cwd);
        }
      }

      store.dispose();
    });

    it('autoOpenUnseenAndActivateClearsUnread', 0, () => {
      const store = new OrbitSidePanelStore(0);
      store.setCompactViewport(true);
      store.switchScope('host-1', 'agent-1');

      store.openTab(
        makeTab('auto-rev', 'review', 'host-1', 'agent-1', '/repo'),
        { source: 'auto', activate: true }
      );
      expect(store.isOpen).assertFalse();
      expect(store.tabs.length).assertEqual(1);
      expect(store.unseenTabIds.length).assertEqual(1);
      expect(store.unseenTabIds[0]).assertEqual('auto-rev');

      store.activate('auto-rev');
      expect(store.isOpen).assertTrue();
      expect(store.activeTabId).assertEqual('auto-rev');
      expect(store.unseenTabIds.length).assertEqual(0);

      store.setCompactViewport(false);
      store.openTab(
        makeTab('auto-term', 'terminal', 'host-1', 'agent-1', '/repo'),
        { source: 'auto', activate: false }
      );
      expect(store.activeTabId).assertEqual('auto-rev');
      expect(store.activeSnapshot.hasUnseen('auto-term')).assertTrue();

      store.activate('auto-term');
      expect(store.activeTabId).assertEqual('auto-term');
      expect(store.activeSnapshot.hasUnseen('auto-term')).assertFalse();

      store.dispose();
    });

    it('closeAndReopenLastLifoTenStackAndAutoCollapse', 0, () => {
      const store = new OrbitSidePanelStore(0);
      store.switchScope('host-1', 'agent-1');

      for (let i = 1; i <= 12; i++) {
        store.openTab(
          makeTab(canonicalFileTabId('host-1', '/repo', `f${i}.ets`), 'file', 'host-1', 'agent-1', '/repo', `f${i}.ets`, `f${i}.ets`),
          { source: 'user', activate: true }
        );
      }
      expect(store.tabs.length).assertEqual(12);

      for (let i = 1; i <= 12; i++) {
        store.close(canonicalFileTabId('host-1', '/repo', `f${i}.ets`));
      }

      expect(store.tabs.length).assertEqual(0);
      expect(store.isOpen).assertFalse();
      expect(store.activeTabId).assertEqual('');
      expect(store.activeSnapshot.userDismissed).assertTrue();
      expect(store.activeSnapshot.closedTabs.length).assertEqual(ORBIT_SIDE_PANEL_MAX_CLOSED_TABS);

      for (let expected = 12; expected >= 3; expected--) {
        const expectedId = canonicalFileTabId('host-1', '/repo', `f${expected}.ets`);
        const reopened = store.reopenLast();
        expect(reopened !== null).assertTrue();
        expect(reopened?.tabId).assertEqual(expectedId);
        expect(store.isOpen).assertTrue();
        expect(store.activeTabId).assertEqual(expectedId);
      }
      expect(store.reopenLast() === null).assertTrue();

      store.dispose();
    });

    it('fileTargetIdentityEliminatesDelimiterCollision', 0, () => {
      // R2-P0-1: `/repo` + `x::y` vs `/repo::x` + `y` MUST NEVER collide in tabId or field identity.
      const idA = canonicalFileTabId('host-1', '/repo', 'x::y');
      const idB = canonicalFileTabId('host-1', '/repo::x', 'y');
      expect(idA !== idB).assertTrue();
      expect(isSameFileTarget('host-1', '/repo', 'x::y', 'host-1', '/repo::x', 'y')).assertFalse();
      expect(isSameFileTarget('host-1', '/repo', 'x::y', 'host-1', '/repo', 'x::y')).assertTrue();

      const store = new OrbitSidePanelStore(0);
      store.switchScope('host-1', 'ag-1');
      store.openTab(
        makeTab(idA, 'file', 'host-1', 'ag-1', '/repo', 'x::y', 'x::y'),
        { source: 'user', activate: true }
      );
      store.openTab(
        makeTab(idB, 'file', 'host-1', 'ag-1', '/repo::x', 'y', 'y'),
        { source: 'user', activate: true }
      );

      expect(store.tabs.length).assertEqual(2);
      expect(store.tabs[0].cwd).assertEqual('/repo');
      expect(store.tabs[0].path).assertEqual('x::y');
      expect(store.tabs[1].cwd).assertEqual('/repo::x');
      expect(store.tabs[1].path).assertEqual('y');
      expect(store.activeTabId).assertEqual(idB);

      // Re-opening A reuses A cleanly without duplicating or overwriting B.
      store.openTab(
        makeTab('legacy:file:host-1::/repo::x::y', 'file', 'host-1', 'ag-1', '/repo', 'x::y', 'x::y'),
        { source: 'user', activate: true }
      );
      expect(store.tabs.length).assertEqual(2);
      expect(store.activeTabId).assertEqual(idA);

      // Persistence roundtrip preserves both distinct file tabs without collision.
      const storage = new FakeSidePanelStorage();
      store.bindStorage(storage);
      expect(store.flushNow()).assertTrue();
      const restored = new OrbitSidePanelStore(0);
      restored.bindStorage(storage);
      expect(restored.hydrate()).assertTrue();
      const snap = restored.switchScope('host-1', 'ag-1');
      expect(snap.tabs.length).assertEqual(2);
      expect(snap.tabs[0].path).assertEqual('x::y');
      expect(snap.tabs[1].path).assertEqual('y');

      store.dispose();
      restored.dispose();
    });

    it('subagentForceRoutesToRootAgentScope', 0, () => {
      const lookup = new FakeParentLookup();
      lookup.addEdge('host-1', 'root-1', null, null);
      lookup.addEdge('host-1', 'child-1', 'host-1', 'root-1');
      lookup.addEdge('host-1', 'grandchild-1', 'host-1', 'child-1');

      const store = new OrbitSidePanelStore(0);
      store.bindParentLookup(lookup);

      const resolved = store.resolveRootScope('host-1', 'grandchild-1');
      expect(resolved.hostId).assertEqual('host-1');
      expect(resolved.rootAgentId).assertEqual('root-1');
      expect(resolved.unresolvedHostId === null).assertTrue();

      store.switchScope('host-1', 'other-root');
      const subTab: OrbitSidePanelTab = {
        tabId: 'subagent:host-1::grandchild-1',
        kind: 'subagent',
        hostId: 'host-1',
        agentId: 'grandchild-1',
        cwd: '/repo',
        path: null,
        parentHostId: 'host-1',
        title: 'Grandchild Researcher',
        closable: true,
        directParentHostId: 'host-1',
        directParentAgentId: 'child-1',
        childAgentId: 'grandchild-1'
      };
      store.openSubagentTab('host-1', 'grandchild-1', subTab);

      expect(store.activeScopeKey).assertEqual('host-1::root-1');
      expect(store.isOpen).assertTrue();
      expect(store.activeTab?.kind).assertEqual('subagent');
      expect(store.activeTab?.agentId).assertEqual('grandchild-1');
      expect(store.activeTab?.parentAgentId).assertEqual('root-1');
      expect(store.activeTab?.directParentAgentId).assertEqual('child-1');
      expect(store.getSnapshotForScope('host-1::grandchild-1') === null).assertTrue();

      store.dispose();
    });

    it('crossHostSubagentColdCacheHydrationAndDualParentProviderIsolation', 0, () => {
      const lookup = new FakeParentLookup();
      lookup.addEdge('host-a', 'root-1', null, null);
      lookup.addEdge('host-b', 'child-mid', 'host-a', 'root-1');
      lookup.addEdge('host-c', 'grandchild-remote', 'host-b', 'child-mid');

      const store = new OrbitSidePanelStore(0);
      store.bindParentLookup(lookup);

      // P0-4: Removing intermediate host-b from cache MUST report unresolvedHostId='host-b'
      // instead of misidentifying host-b::child-mid as the root!
      lookup.setEnforceCachedHosts(true);
      lookup.setHostCached('host-b', false);
      const coldResolved = store.resolveRootScope('host-c', 'grandchild-remote');
      expect(coldResolved.unresolvedHostId).assertEqual('host-b');

      // Once host-b is hydrated back into cache, resolution succeeds to host-a::root-1.
      lookup.setHostCached('host-b', true);
      const hotResolved = store.resolveRootScope('host-c', 'grandchild-remote');
      expect(hotResolved.unresolvedHostId === null).assertTrue();
      expect(hotResolved.hostId).assertEqual('host-a');
      expect(hotResolved.rootAgentId).assertEqual('root-1');

      const remoteTab: OrbitSidePanelTab = {
        tabId: 'subagent:host-c::grandchild-remote',
        kind: 'subagent',
        hostId: 'host-c',
        agentId: 'grandchild-remote',
        cwd: '/remote-repo',
        path: null,
        parentHostId: 'host-a',
        title: 'Remote Grandchild',
        closable: true,
        parentAgentId: 'root-1',
        directParentHostId: 'host-b',
        directParentAgentId: 'child-mid',
        childAgentId: 'grandchild-remote'
      };
      store.openSubagentTab('host-c', 'grandchild-remote', remoteTab);
      expect(store.activeScopeKey).assertEqual('host-a::root-1');
      expect(store.activeTab?.hostId).assertEqual('host-c');
      expect(store.activeTab?.parentHostId).assertEqual('host-a');
      expect(store.activeTab?.directParentHostId).assertEqual('host-b');
      expect(store.activeTab?.directParentAgentId).assertEqual('child-mid');

      // Same managed child ID on two different hosts: removing host-b leaves host-a intact.
      store.openTab({
        tabId: 'subagent:host-a::worker-shared',
        kind: 'subagent',
        hostId: 'host-a',
        agentId: 'worker-shared',
        cwd: '/repo-a',
        path: null,
        parentHostId: 'host-a',
        title: 'Worker A',
        closable: true,
        parentAgentId: 'root-1',
        childAgentId: 'worker-shared'
      }, { source: 'user', activate: true });
      store.openTab({
        tabId: 'subagent:host-b::worker-shared',
        kind: 'subagent',
        hostId: 'host-b',
        agentId: 'worker-shared',
        cwd: '/repo-b',
        path: null,
        parentHostId: 'host-a',
        title: 'Worker B',
        closable: true,
        parentAgentId: 'root-1',
        childAgentId: 'worker-shared'
      }, { source: 'user', activate: true });

      store.removeSubagentTabs('host-b', 'worker-shared', undefined, false);
      expect(store.tabs.some((t: OrbitSidePanelTab) => t.tabId === 'subagent:host-b::worker-shared')).assertFalse();
      expect(store.tabs.some((t: OrbitSidePanelTab) => t.tabId === 'subagent:host-a::worker-shared')).assertTrue();

      // P1-7: Same root scope (`host-a::root-1`), TWO direct parents (`p1`, `p2`) on the SAME host (`host-a`)
      // sharing identical `providerSubagentId='sameNative'` across BOTH open tabs and closedTabs!
      store.switchScope('host-a', 'root-1');
      const p1TabId = canonicalProviderSubagentTabId('host-a', 'p1', 'sameNative');
      const p2TabId = canonicalProviderSubagentTabId('host-a', 'p2', 'sameNative');
      const p1ClosedId = `${p1TabId}_closed`;
      const p2ClosedId = `${p2TabId}_closed`;
      expect(p1TabId !== p2TabId).assertTrue();

      store.openTab({
        tabId: p1TabId,
        kind: 'subagent',
        hostId: 'host-a',
        agentId: 'p1',
        cwd: '/repo',
        path: null,
        parentHostId: 'host-a',
        parentAgentId: 'root-1',
        directParentHostId: 'host-a',
        directParentAgentId: 'p1',
        title: 'P1 SameNative',
        closable: true,
        providerSubagentId: 'sameNative'
      }, { source: 'user', activate: true });
      store.openTab({
        tabId: p2TabId,
        kind: 'subagent',
        hostId: 'host-a',
        agentId: 'p2',
        cwd: '/repo',
        path: null,
        parentHostId: 'host-a',
        parentAgentId: 'root-1',
        directParentHostId: 'host-a',
        directParentAgentId: 'p2',
        title: 'P2 SameNative',
        closable: true,
        providerSubagentId: 'sameNative'
      }, { source: 'user', activate: true });
      store.openTab({
        tabId: p1ClosedId,
        kind: 'subagent',
        hostId: 'host-a',
        agentId: 'p1',
        cwd: '/repo',
        path: null,
        parentHostId: 'host-a',
        parentAgentId: 'root-1',
        directParentHostId: 'host-a',
        directParentAgentId: 'p1',
        title: 'P1 Closed',
        closable: true,
        providerSubagentId: 'sameNative'
      }, { source: 'user', activate: false });
      store.openTab({
        tabId: p2ClosedId,
        kind: 'subagent',
        hostId: 'host-a',
        agentId: 'p2',
        cwd: '/repo',
        path: null,
        parentHostId: 'host-a',
        parentAgentId: 'root-1',
        directParentHostId: 'host-a',
        directParentAgentId: 'p2',
        title: 'P2 Closed',
        closable: true,
        providerSubagentId: 'sameNative'
      }, { source: 'user', activate: false });
      store.close(p1ClosedId);
      store.close(p2ClosedId);

      // Removing p1's sameNative MUST delete ONLY p1's open & closed tabs and preserve p2's open & closed tabs!
      store.removeSubagentTabs('host-a', 'sameNative', 'host-a::root-1', true, 'host-a', 'p1');
      expect(store.tabs.some((t: OrbitSidePanelTab) => t.tabId === p1TabId)).assertFalse();
      expect(store.tabs.some((t: OrbitSidePanelTab) => t.tabId === p2TabId)).assertTrue();
      expect(store.activeSnapshot.closedTabs.some((t: OrbitSidePanelTab) => t.tabId === p1ClosedId)).assertFalse();
      expect(store.activeSnapshot.closedTabs.some((t: OrbitSidePanelTab) => t.tabId === p2ClosedId)).assertTrue();

      // Status & cross-host Crew helper assertions (P0-4, P1-13)
      expect(isSubagentSettledStatus('denied')).assertTrue();
      expect(isSubagentActiveStatus('denied')).assertFalse();
      expect(isSubagentActiveStatus('awaiting_permission')).assertTrue();
      const sampleEntry: SessionSubagentEntryDto = {
        id: 'sub-1',
        role: 'Worker',
        prompt_preview: 'Inspect',
        status: 'running',
        child_agent_id: 'grandchild-remote',
        provider_subagent_id: null,
        last_seq: 12,
        host_id: 'host-c',
        parent_host_id: 'host-b',
        parent_agent_id: 'child-mid'
      };
      expect(resolveSubagentActualHostId(sampleEntry, 'host-a')).assertEqual('host-c');
      expect(resolveSubagentDirectParentHostId(sampleEntry, 'host-a')).assertEqual('host-b');
      expect(resolveSubagentDirectParentAgentId(sampleEntry, 'root-1')).assertEqual('child-mid');

      store.dispose();
    });

    it('fileEditableGuardStrictWritableAndProviderNestedPayloadParser', 0, () => {
      const fullWritableText: WorkspaceFileViewDto = {
        host_id: 'host-1',
        cwd: '/repo',
        path: 'src/app.ets',
        resolved_path: '/repo/src/app.ets',
        language: 'typescript',
        mime_type: 'text/plain',
        size_bytes: 120,
        truncated: false,
        content_kind: 'text',
        content: 'export const x = 1;',
        highlighted_lines: [],
        markdown_blocks: [],
        etag: 'etag-1',
        writable: true
      };
      const missingWritableText: WorkspaceFileViewDto = {
        host_id: 'host-1',
        cwd: '/repo',
        path: 'src/unknown_perm.ets',
        resolved_path: '/repo/src/unknown_perm.ets',
        language: 'typescript',
        mime_type: 'text/plain',
        size_bytes: 40,
        truncated: false,
        content_kind: 'text',
        content: 'const y = 2;',
        highlighted_lines: [],
        markdown_blocks: [],
        etag: 'etag-u'
      };
      const truncatedText: WorkspaceFileViewDto = {
        host_id: 'host-1',
        cwd: '/repo',
        path: 'big.log',
        resolved_path: '/repo/big.log',
        language: 'text',
        mime_type: 'text/plain',
        size_bytes: 2000000,
        truncated: true,
        content_kind: 'text',
        content: 'partial...',
        highlighted_lines: [],
        markdown_blocks: [],
        etag: 'etag-2',
        writable: true
      };
      const binaryFile: WorkspaceFileViewDto = {
        host_id: 'host-1',
        cwd: '/repo',
        path: 'bin.dat',
        resolved_path: '/repo/bin.dat',
        language: 'binary',
        mime_type: 'application/octet-stream',
        size_bytes: 512,
        truncated: false,
        content_kind: 'binary',
        content: '',
        highlighted_lines: [],
        markdown_blocks: [],
        writable: true
      };
      const readOnlyText: WorkspaceFileViewDto = {
        host_id: 'host-1',
        cwd: '/repo',
        path: 'locked.txt',
        resolved_path: '/repo/locked.txt',
        language: 'text',
        mime_type: 'text/plain',
        size_bytes: 24,
        truncated: false,
        content_kind: 'text',
        content: 'locked',
        highlighted_lines: [],
        markdown_blocks: [],
        writable: false
      };

      // P0-2 strict writable === true check
      expect(isWorkspaceFileEditable(fullWritableText)).assertTrue();
      expect(isWorkspaceFileEditable(missingWritableText)).assertFalse();
      expect(isWorkspaceFileEditable(truncatedText)).assertFalse();
      expect(isWorkspaceFileEditable(binaryFile)).assertFalse();
      expect(isWorkspaceFileEditable(readOnlyText)).assertFalse();

      // P1-4: Real Rust Vec<ProviderSubagentEvent> shape with nested `payload: { type, ... }`.
      const rawEvents = '[' +
        '{"seq":1,"parent_agent_id":"ag-root","subagent_id":"prov-1","timestamp":"2026-11-26T10:00:00Z","payload":{"type":"thought","text":"Checking files"}},' +
        '{"seq":2,"parent_agent_id":"ag-root","subagent_id":"prov-1","timestamp":"2026-11-26T10:00:01Z","payload":{"type":"tool_call","title":"read_file","status":"running","text":"read src/main.rs"}},' +
        '{"seq":3,"parent_agent_id":"ag-root","subagent_id":"prov-1","timestamp":"2026-11-26T10:00:02Z","payload":{"type":"assistant_message","text":"Found root cause.","status":"completed"}}' +
        ']';
      const parsed = parseProviderSubagentTimelineJson(rawEvents);
      expect(parsed.length).assertEqual(3);
      expect(parsed[0].item_type).assertEqual('thought');
      expect(parsed[0].text).assertEqual('Checking files');
      expect(parsed[1].item_type).assertEqual('tool_call');
      expect(parsed[1].summary_title).assertEqual('read_file');
      expect(parsed[2].item_type).assertEqual('assistant_message');
      expect(parsed[2].text).assertEqual('Found root cause.');
      expect(parseProviderSubagentTimelineJson('not-json').length).assertEqual(0);
      expect(orbitTextHashSig('abcd') !== orbitTextHashSig('efgh')).assertTrue();
      expect(orbitTextHashSig('same') === orbitTextHashSig('same')).assertTrue();
    });

    it('boardDraftRebindUpdatesAndClearsOpenAndClosedTabs', 0, () => {
      const store = new OrbitSidePanelStore(0);
      store.switchScope('host-1', null);

      const openBoard: OrbitSidePanelTab = {
        tabId: 'board:draft::host-1',
        kind: 'board',
        hostId: 'host-1',
        agentId: null,
        cwd: '/repo-a',
        path: null,
        parentHostId: 'host-1',
        title: 'Board',
        closable: true,
        workspaceId: 'ws-a'
      };
      const extraBoard: OrbitSidePanelTab = {
        tabId: 'board:draft-extra',
        kind: 'board',
        hostId: 'host-1',
        agentId: null,
        cwd: '/repo-a',
        path: null,
        parentHostId: 'host-1',
        title: 'Board 2',
        closable: true,
        workspaceId: 'ws-a'
      };
      store.openTab(openBoard, { source: 'user', activate: true });
      store.openTab(extraBoard, { source: 'user', activate: true });
      store.close('board:draft-extra');
      expect(store.activeSnapshot.closedTabs.length).assertEqual(1);

      store.rebindDraftCwd('host-1', '/repo-b', 'ws-b');
      expect(store.tabs[0].cwd).assertEqual('/repo-b');
      expect(store.tabs[0].workspaceId).assertEqual('ws-b');
      expect(store.activeSnapshot.closedTabs[0].cwd).assertEqual('/repo-b');
      expect(store.activeSnapshot.closedTabs[0].workspaceId).assertEqual('ws-b');

      store.rebindDraftCwd('host-1', '/repo-unmapped', '');
      expect(store.tabs[0].cwd).assertEqual('/repo-unmapped');
      expect(store.tabs[0].workspaceId === null).assertTrue();
      expect(store.activeSnapshot.closedTabs[0].workspaceId === null).assertTrue();

      store.dispose();
    });

    it('structuredRemapPreservesFilePathAndMergesExistingScope', 0, () => {
      const store = new OrbitSidePanelStore(0);
      store.switchScope('host-1', null);
      const expectedFileId = canonicalFileTabId('host-1', '/repo', 'opt_42.txt');

      store.openTab({
        tabId: 'board:draft::host-1',
        kind: 'board',
        hostId: 'host-1',
        agentId: null,
        cwd: '/repo',
        path: null,
        parentHostId: 'host-1',
        title: 'Board',
        closable: true,
        workspaceId: 'ws-1'
      }, { source: 'user', activate: true });
      store.openTab(
        makeTab(expectedFileId, 'file', 'host-1', null, '/repo', 'opt_42.txt', 'opt_42.txt'),
        { source: 'user', activate: true }
      );

      store.promoteDraftScope('host-1', 'opt_42', '/repo');
      expect(store.activeScopeKey).assertEqual('host-1::opt_42');
      expect(store.tabs[0].tabId).assertEqual('board:host-1::opt_42');
      expect(store.tabs[1].tabId).assertEqual(expectedFileId);
      expect(store.tabs[1].path).assertEqual('opt_42.txt');

      store.openTabInScope(
        'host-1::ag_real_42',
        makeTab('terminal:host-1::ag_real_42', 'terminal', 'host-1', 'ag_real_42', '/repo'),
        { source: 'user', activate: true }
      );

      store.remapOptimisticScopes('host-1', 'opt_42', 'ag_real_42');
      expect(store.activeScopeKey).assertEqual('host-1::ag_real_42');
      expect(store.tabs.length).assertEqual(3);
      const fileTab = store.tabs.find((t: OrbitSidePanelTab) => t.kind === 'file');
      expect(fileTab !== undefined).assertTrue();
      expect(fileTab?.path).assertEqual('opt_42.txt');
      expect(fileTab?.tabId).assertEqual(expectedFileId);
      expect(store.tabs.some((t: OrbitSidePanelTab) => t.tabId === 'board:host-1::ag_real_42')).assertTrue();
      expect(store.tabs.some((t: OrbitSidePanelTab) => t.tabId === 'terminal:host-1::ag_real_42')).assertTrue();

      store.dispose();
    });

    it('emptyHostTabRejectedOnRealV1Hydration', 0, () => {
      // Build real v1 persisted payload via production serializer, then inject a corrupt blank-host tab.
      const seedStore = new OrbitSidePanelStore(0);
      seedStore.switchScope('host-1', 'ag-1');
      seedStore.openTab(
        makeTab('good-tab', 'terminal', 'host-1', 'ag-1', '/x', null, 'Term'),
        { source: 'user', activate: true }
      );
      const parsedPayload = JSON.parse(seedStore.serializeState()) as Record<string, Object>;
      const byScope = parsedPayload['byScope'] as Record<string, Record<string, Object>>;
      const scopeObj = byScope['host-1::ag-1'];
      const rawTabs = scopeObj['tabs'] as OrbitSidePanelTab[];
      const badTab: OrbitSidePanelTab = {
        tabId: 'bad-tab',
        kind: 'files',
        hostId: '   ',
        agentId: null,
        cwd: '/x',
        path: null,
        parentHostId: null,
        title: 'Bad',
        closable: true
      };
      rawTabs.unshift(badTab);
      scopeObj['activeTabId'] = 'bad-tab';
      seedStore.dispose();

      const storage = new FakeSidePanelStorage();
      storage.rawJson = JSON.stringify(parsedPayload);
      const store = new OrbitSidePanelStore(0);
      store.bindStorage(storage);
      expect(store.hydrate()).assertTrue();
      const snap = store.switchScope('host-1', 'ag-1');
      expect(snap.tabs.length).assertEqual(1);
      expect(snap.tabs[0].tabId).assertEqual('good-tab');
      expect(snap.activeTabId).assertEqual('good-tab');
      store.dispose();
    });

    it('terminalSlotGitStagedAndCloseTabInScopeIsolation', 0, () => {
      const store = new OrbitSidePanelStore(0);
      store.switchScope('host-1', 'ag-1');
      store.openTab(makeTab('term-1', 'terminal', 'host-1', 'ag-1', '/repo'), { source: 'user', activate: true });
      store.openTab(makeTab('rev-1', 'review', 'host-1', 'ag-1', '/repo'), { source: 'user', activate: true });

      store.updateActiveTabTerminalSlot('host-1', 'term-1', 3);
      expect(store.tabs[0].terminalSlot).assertEqual(3);

      // P1-3: staged mode persists on review tab metadata.
      store.updateTabGitStaged('host-1::ag-1', 'rev-1', true);
      expect(store.tabs[1].gitStaged).assertTrue();

      // P1-1: closeTabInScope closes strictly inside scope A even while scope B is active.
      store.switchScope('host-2', 'ag-2');
      store.openTab(makeTab('b-term', 'terminal', 'host-2', 'ag-2', '/b'), { source: 'user', activate: true });
      store.closeTabInScope('host-1::ag-1', 'rev-1');

      expect(store.activeScopeKey).assertEqual('host-2::ag-2');
      expect(store.tabs.length).assertEqual(1);
      expect(store.tabs[0].tabId).assertEqual('b-term');
      expect(store.getSnapshotForScope('host-1::ag-1')?.tabs.length).assertEqual(1);
      expect(store.getSnapshotForScope('host-1::ag-1')?.tabs[0].tabId).assertEqual('term-1');

      store.dispose();
    });

    it('flushRetryPreservesDirtyOnSyncAndAsyncRejection', 0, async () => {
      // Nit N-A: Verify sync return false, sync failure callback before return true, AND async microtask rejection.
      const storage = new FakeSidePanelStorage();
      const store = new OrbitSidePanelStore(220);
      store.bindStorage(storage);
      store.switchScope('host-1', 'ag-1');
      store.openTab(makeTab('t-1', 'terminal', 'host-1', 'ag-1', '/repo'), { source: 'user', activate: true });

      // 1. Synchronous put failure (returns false)
      storage.failNextSyncWrite = true;
      expect(store.flushNow()).assertFalse();
      expect(store.hasPendingPersist).assertTrue();
      expect(store.flushNow()).assertTrue();
      expect(store.hasPendingPersist).assertFalse();

      // 2. Synchronous failure callback fired BEFORE setSidePanelStateJson returns true
      store.setWidthVp(420);
      storage.failNextSyncCallback = true;
      expect(store.flushNow()).assertFalse();
      expect(store.hasPendingPersist).assertTrue();
      expect(store.flushNow()).assertTrue();
      expect(store.hasPendingPersist).assertFalse();

      // 3. Delayed microtask asynchronous rejection callback (real Preferences.flush behavior)
      store.setWidthVp(460);
      storage.failNextAsyncMicrotask = true;
      expect(store.flushNow()).assertTrue();
      expect(store.hasPendingPersist).assertFalse();
      await Promise.resolve();
      expect(store.hasPendingPersist).assertTrue();
      expect(store.flushNow()).assertTrue();
      expect(store.hasPendingPersist).assertFalse();

      store.dispose();
    });

    it('closingLastTabDismissesSoAutoOpenStaysClosedWithUnread', 0, () => {
      const store = new OrbitSidePanelStore(0);
      store.setCompactViewport(false);
      store.switchScope('host-1', 'ag-1');
      store.openTab(makeTab('only-tab', 'terminal', 'host-1', 'ag-1', '/x'), { source: 'user', activate: true });
      store.close('only-tab');
      expect(store.isOpen).assertFalse();
      expect(store.activeSnapshot.userDismissed).assertTrue();

      store.openTabInScope(
        'host-1::ag-1',
        makeTab('side_question:host-1::ag-1', 'side_question', 'host-1', 'ag-1', '/x'),
        { source: 'auto', activate: true }
      );
      expect(store.isOpen).assertFalse();
      expect(store.unseenTabIds.indexOf('side_question:host-1::ag-1') >= 0).assertTrue();
      store.dispose();
    });

    it('lru100EvictionPersistenceAndCorruptFallback', 0, () => {
      const storage = new FakeSidePanelStorage();
      const store1 = new OrbitSidePanelStore(0);
      store1.bindStorage(storage);

      for (let i = 0; i < 112; i++) {
        store1.switchScope('host-1', `lru-${i}`, 1700000000000 + i);
        store1.openTab(
          makeTab(`t-${i}`, 'review', 'host-1', `lru-${i}`, `/repo-${i}`),
          { source: 'user', activate: true }
        );
      }
      store1.switchScope('host-1', 'lru-100', 1700009999999);
      expect(store1.flushNow()).assertTrue();
      expect(store1.scopeCount <= ORBIT_SIDE_PANEL_MAX_SCOPES).assertTrue();
      expect(store1.scopeCount).assertEqual(ORBIT_SIDE_PANEL_MAX_SCOPES);
      expect(store1.getSnapshotForScope('host-1::lru-0') === null).assertTrue();
      expect(store1.getSnapshotForScope('host-1::lru-111') !== null).assertTrue();
      expect(store1.getSnapshotForScope('host-1::lru-100') !== null).assertTrue();
      expect(storage.rawJson.length > 0).assertTrue();
      store1.dispose();

      const store2 = new OrbitSidePanelStore(0);
      store2.bindStorage(storage);
      expect(store2.hydrate()).assertTrue();
      expect(store2.scopeCount).assertEqual(ORBIT_SIDE_PANEL_MAX_SCOPES);
      const restored = store2.switchScope('host-1', 'lru-111');
      expect(restored.open).assertTrue();
      expect(restored.tabs.length).assertEqual(1);
      expect(restored.tabs[0].cwd).assertEqual('/repo-111');
      store2.dispose();

      const corruptStorage = new FakeSidePanelStorage();
      corruptStorage.rawJson = '{invalid-json';
      const store3 = new OrbitSidePanelStore(0);
      store3.bindStorage(corruptStorage);
      expect(store3.hydrate()).assertFalse();
      expect(store3.scopeCount).assertEqual(0);
      expect(store3.isOpen).assertFalse();
      store3.dispose();
    });

    it('breakpointsSafeInsetDegradeShortcutsAndSheetPresentationTracker', 0, () => {
      // 1. Exact breakpoint boundaries (<680 / 680..1023 / >=1024 + folded override)
      expect(resolveOrbitBreakpoint(390)).assertEqual('sm');
      expect(resolveOrbitBreakpoint(679)).assertEqual('sm');
      expect(resolveOrbitBreakpoint(680)).assertEqual('md');
      expect(resolveOrbitBreakpoint(1023)).assertEqual('md');
      expect(resolveOrbitBreakpoint(1024)).assertEqual('lg');
      expect(resolveOrbitBreakpoint(1280, 'folded')).assertEqual('sm');

      // 2. P1-10: Horizontal safe insets deducted BEFORE column budget -> degrades inline panel to sheet
      const mdTightInset = computeOrbitShellLayout(680, 'unknown', false, 280, true, 360, 24);
      expect(mdTightInset.effectiveContentWidthVp).assertEqual(656);
      expect(mdTightInset.sidePanelInline).assertFalse();
      expect(mdTightInset.stageWidthVp >= ORBIT_STAGE_MIN_WIDTH_VP).assertTrue();

      const lgTightInset = computeOrbitShellLayout(1024, 'unknown', true, 280, true, 360, 120);
      expect(lgTightInset.effectiveContentWidthVp).assertEqual(904);
      expect(lgTightInset.sidebarVisible).assertFalse();
      expect(lgTightInset.sidePanelInline).assertTrue();
      expect(lgTightInset.stageWidthVp >= ORBIT_STAGE_MIN_WIDTH_VP).assertTrue();

      // 3. md auto-yield + reducedMotion setter
      const shell = new OrbitShellStore();
      const widthStorage = new FakeShellWidthStorage();
      shell.bindWidthStorage(widthStorage);
      shell.setReducedMotion(true);
      expect(shell.reducedMotion).assertTrue();

      shell.updateWindowSize(800, 1000, false);
      expect(shell.breakpoint).assertEqual('md');
      const mdBefore = shell.resolveLayout(false);
      expect(mdBefore.sidebarVisible).assertTrue();
      expect(mdBefore.sidebarWidthVp).assertEqual(ORBIT_SIDEBAR_MD_WIDTH_VP);

      shell.onSidePanelOpenChanged(true);
      expect(shell.isSidebarCollapsed).assertTrue();
      expect(shell.sidebarAutoYieldedForSidePanel).assertTrue();
      shell.onSidePanelOpenChanged(false);
      expect(shell.isSidebarCollapsed).assertFalse();

      // 4. R3-P1-8: Serialized wait-for-disappear modal & compact panel sheet lifecycle.
      // Sequence A: Mounted A -> closeModalByPresentationId(A) -> open B -> delayed A appear -> native A disappear -> B mounts -> duplicate A disappear ignored!
      const idA = shell.openModal({ kind: 'global_search', hostId: 'host-1' });
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.activeSheetPresentationId).assertEqual(idA);
      expect(shell.onModalSheetAppear(idA)).assertTrue();
      expect(shell.mountedSheetPresentationId).assertEqual(idA);
      // Close request enters `dismissing` without pretending native `onDisappear` already finished!
      expect(shell.closeModalByPresentationId(idA)).assertTrue();
      expect(shell.isSheetModalPresented).assertFalse();
      expect(shell.sheetLifecycleState).assertEqual('dismissing');
      const idB = shell.openModal({ kind: 'archived', hostId: 'host-1' });
      // B is queued while mounted A exits — activeSheetPresentationId stays idA so A's callbacks cannot touch B.
      expect(shell.isSheetModalPresented).assertFalse();
      expect(shell.activeSheetPresentationId).assertEqual(idA);
      expect(shell.isModalKindOpen('archived')).assertTrue();
      // Delayed A onAppear arrives AFTER B was requested: ignored!
      expect(shell.onModalSheetAppear(idA)).assertFalse();
      // A native onDisappear arrives: promotes B cleanly without clearing B!
      expect(shell.onModalSheetDisappear(idA)).assertFalse();
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.activeSheetPresentationId).assertEqual(idB);
      expect(shell.modalRoute?.kind).assertEqual('archived');
      expect(shell.onModalSheetAppear(idB)).assertTrue();
      // Duplicate late onDisappear(idA) after B mounted MUST NOT dismiss B!
      expect(shell.onModalSheetDisappear(idA)).assertFalse();
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.modalRoute?.kind).assertEqual('archived');
      // User swipes B down -> closes B cleanly!
      expect(shell.onModalSheetDisappear(idB)).assertTrue();
      expect(shell.isModalSheetPresented).assertFalse();
      expect(shell.modalRoute === null).assertTrue();

      // Sequence B: Mounted C -> open D serializes through C exit; unmounted close vs mounted close
      const idC = shell.openModal({ kind: 'global_search', hostId: 'host-1' });
      expect(shell.onModalSheetAppear(idC)).assertTrue();
      const idD = shell.openModal({ kind: 'host_settings', hostId: 'host-1' });
      expect(idC !== idD).assertTrue();
      expect(shell.sheetLifecycleState).assertEqual('dismissing');
      expect(shell.onModalSheetAppear(idC)).assertFalse();
      expect(shell.onModalSheetDisappear(idC)).assertFalse();
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.activeSheetPresentationId).assertEqual(idD);
      expect(shell.modalRoute?.kind).assertEqual('host_settings');
      expect(shell.onModalSheetAppear(idD)).assertTrue();
      expect(shell.closeModalByPresentationId(idC)).assertFalse();
      expect(shell.closeModalByPresentationId(idD)).assertTrue();
      expect(shell.isSheetModalPresented).assertFalse();
      expect(shell.sheetLifecycleState).assertEqual('dismissing');
      expect(shell.onModalSheetDisappear(idD)).assertFalse();
      expect(shell.sheetLifecycleState).assertEqual('idle');

      // Sequence B2: Consecutive open(U1) -> open(U2) BEFORE first render (U1 never mounted) replaces U1 directly!
      const idU1 = shell.openModal({ kind: 'global_search', hostId: 'host-1' });
      expect(shell.mountedSheetPresentationId).assertEqual(0);
      const idU2 = shell.openModal({ kind: 'host_settings', hostId: 'host-1' });
      expect(idU1 !== idU2).assertTrue();
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.sheetLifecycleState).assertEqual('presenting');
      expect(shell.pendingModalRoute === null).assertTrue();
      expect(shell.activeSheetPresentationId).assertEqual(idU2);
      expect(shell.modalRoute?.kind).assertEqual('host_settings');
      // Late phantom callbacks from superseded unmounted U1 are strictly ignored.
      expect(shell.onModalSheetAppear(idU1)).assertFalse();
      expect(shell.onModalSheetDisappear(idU1)).assertFalse();
      expect(shell.isSheetModalPresented).assertTrue();
      // Closing unmounted U2 transitions directly to idle without stalling.
      expect(shell.closeModalByPresentationId(idU2)).assertTrue();
      expect(shell.sheetLifecycleState).assertEqual('idle');
      expect(shell.isSheetModalPresented).assertFalse();

      // Sequence C: Mounted E0 -> close -> E -> F before E0 exits -> E0 disappears -> F mounts directly
      const idE0 = shell.openModal({ kind: 'global_search', hostId: 'host-1' });
      expect(shell.onModalSheetAppear(idE0)).assertTrue();
      shell.closeModal();
      shell.openModal({ kind: 'archived', hostId: 'host-1' });
      const idF = shell.openModal({ kind: 'host_settings', hostId: 'host-1' });
      expect(shell.onModalSheetDisappear(idE0)).assertFalse();
      expect(shell.activeSheetPresentationId).assertEqual(idF);
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.modalRoute?.kind).assertEqual('host_settings');
      expect(shell.onModalSheetAppear(idF)).assertTrue();
      expect(shell.onModalSheetDisappear(idF)).assertTrue();
      expect(shell.isModalSheetPresented).assertFalse();

      // Sequence D: Compact side panel OrbitSerializedSheetHost yield -> delayed appear -> restore -> duplicate old disappear ignored!
      const smHost = new OrbitSerializedSheetHost();
      const smId1 = smHost.requestOpen();
      expect(smHost.boundOpen).assertTrue();
      expect(smHost.onAppear(smId1)).assertTrue();
      // Modal opens -> mounted panel yields (requestClose enters dismissing)
      smHost.requestClose();
      expect(smHost.boundOpen).assertFalse();
      expect(smHost.lifecycle).assertEqual('dismissing');
      // Modal closes BEFORE panel sheet 1 finishes disappearing -> requestOpen queues reopen
      smHost.requestOpen();
      expect(smHost.boundOpen).assertFalse();
      expect(smHost.wantReopen).assertTrue();
      // Delayed onAppear from sheet 1 is ignored
      expect(smHost.onAppear(smId1)).assertFalse();
      // Sheet 1 finishes disappearing -> promotes fresh sheet 2 and returns userDismissed = false!
      expect(smHost.onDisappear(smId1)).assertFalse();
      expect(smHost.boundOpen).assertTrue();
      const smId2 = smHost.activePresentationId;
      expect(smId2 > smId1).assertTrue();
      expect(smHost.onAppear(smId2)).assertTrue();
      // Duplicate late onDisappear(smId1) after sheet 2 restored MUST NOT dismiss sheet 2!
      expect(smHost.onDisappear(smId1)).assertFalse();
      expect(smHost.boundOpen).assertTrue();
      expect(smHost.lifecycle).assertEqual('presenting');
      // User swipes sheet 2 down -> returns userDismissed = true!
      expect(smHost.onDisappear(smId2)).assertTrue();
      expect(smHost.boundOpen).assertFalse();
      expect(smHost.lifecycle).assertEqual('idle');
      // Unmounted compact sheet close returns immediately to idle (never stalls)
      const smId3 = smHost.requestOpen();
      expect(smHost.mountedPresentationId).assertEqual(0);
      smHost.requestClose();
      expect(smHost.lifecycle).assertEqual('idle');
      expect(smHost.onDisappear(smId3)).assertFalse();

      // Lightbox split + old Lightbox close MUST NOT clear a newer Sheet!
      const lbId = shell.openModal({ kind: 'image_viewer', imageUri: 'file:///tmp/demo.png', imageLabel: 'Demo' });
      expect(shell.isLightboxPresented).assertTrue();
      expect(shell.activeLightboxPresentationId).assertEqual(lbId);
      expect(shell.isSheetModalPresented).assertFalse();
      const afterLbSheetId = shell.openModal({ kind: 'global_search', hostId: 'host-1' });
      expect(shell.isLightboxPresented).assertFalse();
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.onModalSheetAppear(afterLbSheetId)).assertTrue();
      // Stale Lightbox close callback MUST be rejected and leave the newer Sheet open!
      expect(shell.closeModalByPresentationId(lbId)).assertFalse();
      expect(shell.isSheetModalPresented).assertTrue();
      expect(shell.modalRoute?.kind).assertEqual('global_search');
      expect(shell.closeModalByPresentationId(afterLbSheetId)).assertTrue();
      expect(shell.onModalSheetDisappear(afterLbSheetId)).assertFalse();
      expect(shell.sheetLifecycleState).assertEqual('idle');
      // Clean Lightbox close by its own ID + consecutive image->diagram Lightbox swap
      const lbId2 = shell.openModal({ kind: 'image_viewer', imageUri: 'file:///tmp/img.png', imageLabel: 'Img' });
      expect(shell.isLightboxPresented).assertTrue();
      expect(shell.activeLightboxPresentationId).assertEqual(lbId2);
      const lbId3 = shell.openModal({ kind: 'diagram_viewer', title: 'Arch', diagramSource: 'graph TD; A-->B' });
      expect(lbId3 !== lbId2).assertTrue();
      expect(shell.isLightboxPresented).assertTrue();
      expect(shell.activeLightboxPresentationId).assertEqual(lbId3);
      expect(shell.closeModalByPresentationId(lbId2)).assertFalse();
      expect(shell.isLightboxPresented).assertTrue();
      expect(shell.closeModalByPresentationId(lbId3)).assertTrue();
      expect(shell.isLightboxPresented).assertFalse();

      // R5-N1: 1,000 consecutive open/appear/close/disappear cycles keep O(1) bounded completed records!
      const stressShell = new OrbitShellStore();
      const stressCompact = new OrbitSerializedSheetHost();
      let lastStressModalId = 0;
      let lastStressCompactId = 0;
      for (let i = 0; i < 1000; i++) {
        const mId = stressShell.openModal({ kind: 'global_search', hostId: 'host-1' });
        expect(stressShell.onModalSheetAppear(mId)).assertTrue();
        expect(stressShell.closeModalByPresentationId(mId)).assertTrue();
        expect(stressShell.onModalSheetDisappear(mId)).assertFalse();
        lastStressModalId = mId;

        const cId = stressCompact.requestOpen();
        expect(stressCompact.onAppear(cId)).assertTrue();
        stressCompact.requestClose();
        expect(stressCompact.onDisappear(cId)).assertFalse();
        lastStressCompactId = cId;
      }
      expect(stressShell.completedSheetRecordCount).assertEqual(0);
      expect(stressCompact.completedRecordCount).assertEqual(0);
      // Stale callbacks for ANY of the 1,000 completed IDs remain rejected via monotonic watermark!
      expect(stressShell.onModalSheetDisappear(1)).assertFalse();
      expect(stressShell.onModalSheetDisappear(lastStressModalId)).assertFalse();
      expect(stressCompact.onDisappear(1)).assertFalse();
      expect(stressCompact.onDisappear(lastStressCompactId)).assertFalse();

      // 5. P1-11: External keyboard shortcut arbitration
      const baseCtx: OrbitShortcutContext = {
        breakpoint: 'lg',
        isModalOpen: false,
        isTextInputFocused: false,
        isTerminalFocused: false,
        isSidePanelVisible: true
      };
      expect(resolveOrbitKeyboardShortcut(ORBIT_KEYCODE_K, false, true, false, false, baseCtx)).assertEqual('global_search');
      expect(resolveOrbitKeyboardShortcut(ORBIT_KEYCODE_T, false, true, true, false, baseCtx)).assertEqual('reopen_last_tab');
      expect(resolveOrbitKeyboardShortcut(ORBIT_KEYCODE_W, false, true, false, false, baseCtx)).assertEqual('close_panel_tab');
      expect(resolveOrbitKeyboardShortcut(ORBIT_KEYCODE_W, true, false, false, false, {
        breakpoint: 'lg',
        isModalOpen: false,
        isTextInputFocused: false,
        isTerminalFocused: false,
        isSidePanelVisible: false
      })).assertEqual('none');
      expect(resolveOrbitKeyboardShortcut(ORBIT_KEYCODE_K, false, true, false, false, {
        breakpoint: 'lg',
        isModalOpen: true,
        isTextInputFocused: false,
        isTerminalFocused: false,
        isSidePanelVisible: true
      })).assertEqual('none');
      expect(resolveOrbitKeyboardShortcut(ORBIT_KEYCODE_W, true, false, false, false, {
        breakpoint: 'lg',
        isModalOpen: false,
        isTextInputFocused: true,
        isTerminalFocused: true,
        isSidePanelVisible: true
      })).assertEqual('none');
      expect(resolveOrbitKeyboardShortcut(ORBIT_KEYCODE_W, false, true, false, false, {
        breakpoint: 'lg',
        isModalOpen: false,
        isTextInputFocused: true,
        isTerminalFocused: true,
        isSidePanelVisible: true
      })).assertEqual('close_panel_tab');

      // 6. 2-in-1 Title Button Avoidance
      shell.updateTitleButtonAvoidWidth(124, true);
      expect(shell.isFreeWindowDecorHidden).assertTrue();
      expect(shell.resolvedTopBarRightPaddingVp()).assertEqual(16 + 124);
    });

    it('fleetStoreDropsStaleOutOfOrderAndRemovedHostDetail', 0, async () => {
      const bridge = new FakeOutOfOrderFleetBridge();
      const fleet = new OrbitFleetStore();
      fleet.bindBridge(bridge);

      const req1 = fleet.reloadHostAsync('host-1');
      const req2 = fleet.reloadHostAsync('host-1');
      expect(bridge.pendingResolvers.length).assertEqual(2);

      // Newer req2 resolves FIRST with fresh data, then older req1 resolves with stale data.
      bridge.pendingResolvers[1](makeMinimalHostDetailJson('host-1', 'Fresh Host v2'));
      const res2 = await req2;
      expect(res2 !== null).assertTrue();
      expect(fleet.getCachedHostDetail('host-1')?.host.display_name).assertEqual('Fresh Host v2');

      bridge.pendingResolvers[0](makeMinimalHostDetailJson('host-1', 'Stale Host v1'));
      const res1 = await req1;
      expect(res1 === null).assertTrue();
      expect(fleet.getCachedHostDetail('host-1')?.host.display_name).assertEqual('Fresh Host v2');

      // P1-12: Removing a host while reloadHostAsync is in flight MUST drop late response & bump revision!
      const req3 = fleet.reloadHostAsync('host-1');
      expect(fleet.isHostDetailInFlight('host-1')).assertTrue();
      const revBeforeRemove = fleet.hostDetailRevision;
      fleet.removeCachedHost('host-1');
      expect(fleet.hostDetailRevision > revBeforeRemove).assertTrue();
      expect(fleet.getCachedHostDetail('host-1') === null).assertTrue();
      expect(fleet.isHostDetailInFlight('host-1')).assertFalse();

      bridge.pendingResolvers[2](makeMinimalHostDetailJson('host-1', 'Zombie Host'));
      const res3 = await req3;
      expect(res3 === null).assertTrue();
      expect(fleet.getCachedHostDetail('host-1') === null).assertTrue();

      // P1-12 + R3-P1-9: reloadHostSync supersedes in-flight async token AND bumps ONLY host-2's per-host revision.
      const h1RevBeforeH2 = fleet.getHostDetailRevision('host-1');
      const h2RevBeforeH2 = fleet.getHostDetailRevision('host-2');
      const req4 = fleet.reloadHostAsync('host-2');
      expect(fleet.isHostDetailInFlight('host-2')).assertTrue();
      bridge.syncDetailByHost.set('host-2', makeMinimalHostDetailJson('host-2', 'Sync Host v2'));
      fleet.reloadHostSync('host-2');
      expect(fleet.getHostDetailRevision('host-2') > h2RevBeforeH2).assertTrue();
      expect(fleet.getHostDetailRevision('host-1')).assertEqual(h1RevBeforeH2);
      expect(fleet.isHostDetailInFlight('host-2')).assertFalse();
      expect(fleet.getCachedHostDetail('host-2')?.host.display_name).assertEqual('Sync Host v2');
      bridge.pendingResolvers[3](makeMinimalHostDetailJson('host-2', 'Old Async v1'));
      const res4 = await req4;
      expect(res4 === null).assertTrue();
      expect(fleet.getCachedHostDetail('host-2')?.host.display_name).assertEqual('Sync Host v2');
    });

    it('personalAgentScopedBriefingStrictAgentFilterAndDoneCount', 0, () => {
      // P1-5 + R3-P1-5: Filtering `(host, agent)` MUST project mission root/workers strictly, exclude
      // sibling workers with opposite states and cross-host same-ID workers, and compute counts from that projection.
      const rawBriefing: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [
          {
            key: 'dec-a',
            kind: 'permission',
            host_id: 'host-1',
            host_name: 'Mac Studio',
            agent_id: 'agent-a',
            agent_title: 'Agent A',
            request_id: 'perm-1',
            summary: 'Run tests for A',
            risk_level: 'low',
            human_only: false,
            batch_eligible: true,
            router_recommendation: null,
            dispatched_by_personal_agent: false,
            updated_at_ms: 1700000001000
          },
          {
            key: 'dec-b',
            kind: 'permission',
            host_id: 'host-1',
            host_name: 'Mac Studio',
            agent_id: 'agent-b',
            agent_title: 'Agent B',
            request_id: 'perm-2',
            summary: 'Deploy B',
            risk_level: 'high',
            human_only: true,
            batch_eligible: false,
            router_recommendation: null,
            dispatched_by_personal_agent: false,
            updated_at_ms: 1700000002000
          }
        ],
        missions: [
          {
            key: 'mission-1',
            root: {
              host_id: 'host-1',
              host_name: 'Mac Studio',
              agent_id: 'root-1',
              title: 'Mission Root',
              provider: 'claude',
              state: 'running',
              step_preview: 'Coordinating workers',
              worktree_branch: null,
              updated_at_ms: 1700000005000
            },
            workers: [
              {
                host_id: 'host-1',
                host_name: 'Mac Studio',
                agent_id: 'agent-a',
                title: 'Agent A',
                provider: 'claude',
                state: 'done',
                step_preview: 'Finished A',
                worktree_branch: null,
                updated_at_ms: 1700000003000
              },
              {
                host_id: 'host-1',
                host_name: 'Mac Studio',
                agent_id: 'agent-b',
                title: 'Agent B',
                provider: 'claude',
                state: 'running',
                step_preview: 'Still running B',
                worktree_branch: null,
                updated_at_ms: 1700000004000
              },
              {
                host_id: 'host-2',
                host_name: 'Linux GPU',
                agent_id: 'agent-a',
                title: 'Remote Agent A',
                provider: 'codex',
                state: 'running',
                step_preview: 'Running on host-2',
                worktree_branch: null,
                updated_at_ms: 1700000006000
              }
            ],
            host_names: ['Mac Studio', 'Linux GPU'],
            done: 1,
            total: 3,
            state: 'running',
            updated_at_ms: 1700000006000
          }
        ],
        other_active: [
          {
            host_id: 'host-1',
            host_name: 'Mac Studio',
            agent_id: 'agent-a',
            title: 'Agent A',
            provider: 'claude',
            state: 'done',
            step_preview: 'Finished A',
            worktree_branch: null,
            updated_at_ms: 1700000003000
          }
        ],
        waiting: [],
        hosts: [
          {
            host_id: 'host-1',
            name: 'Mac Studio',
            online: true,
            is_personal_agent_host: false
          },
          {
            host_id: 'host-2',
            name: 'Linux GPU',
            online: true,
            is_personal_agent_host: false
          }
        ],
        counts: {
          needs_you: 2,
          running: 3,
          done_recent: 9,
          hosts_online: 2,
          hosts_total: 2
        },
        since_last_visit: {
          completed: 9,
          auto_handled: 0
        }
      };

      const bridge = new FakePersonalAgentBridge();
      bridge.briefingPayload = JSON.stringify(rawBriefing);
      const paStore = new OrbitPersonalAgentStore();
      paStore.bindBridge(bridge);

      // 1. Worker A only (done): MUST exclude sibling B (running) and host-2/agent-a (running); deduplicate other_active!
      const scopedA = paStore.fetchScopedBriefing('host-1', 'agent-a');
      expect(scopedA !== null).assertTrue();
      expect(scopedA?.needs_you.length).assertEqual(1);
      expect(scopedA?.needs_you[0].agent_id).assertEqual('agent-a');
      expect(scopedA?.missions.length).assertEqual(1);
      expect(scopedA?.missions[0].workers.length).assertEqual(1);
      expect(scopedA?.missions[0].workers[0].agent_id).assertEqual('agent-a');
      expect(scopedA?.missions[0].workers[0].host_id).assertEqual('host-1');
      expect(scopedA?.missions[0].state).assertEqual('done');
      expect(scopedA?.missions[0].done).assertEqual(1);
      expect(scopedA?.missions[0].total).assertEqual(1);
      expect(scopedA?.counts.running).assertEqual(0);
      expect(scopedA?.counts.done_recent).assertEqual(1);
      expect(scopedA?.since_last_visit.completed).assertEqual(1);

      // 2. Sibling worker B (running): opposite state from A -> 1 running, 0 done, no A!
      const scopedB = paStore.fetchScopedBriefing('host-1', 'agent-b');
      expect(scopedB !== null).assertTrue();
      expect(scopedB?.missions.length).assertEqual(1);
      expect(scopedB?.missions[0].workers.length).assertEqual(1);
      expect(scopedB?.missions[0].workers[0].agent_id).assertEqual('agent-b');
      expect(scopedB?.missions[0].state).assertEqual('running');
      expect(scopedB?.missions[0].done).assertEqual(0);
      expect(scopedB?.missions[0].total).assertEqual(1);
      expect(scopedB?.counts.running).assertEqual(1);
      expect(scopedB?.counts.done_recent).assertEqual(0);

      // 3. Root hit (`host-1`, `root-1`): projects strictly to root without sibling workers leaking!
      const scopedRoot = paStore.fetchScopedBriefing('host-1', 'root-1');
      expect(scopedRoot !== null).assertTrue();
      expect(scopedRoot?.missions.length).assertEqual(1);
      expect(scopedRoot?.missions[0].root.agent_id).assertEqual('root-1');
      expect(scopedRoot?.missions[0].workers.length).assertEqual(0);
      expect(scopedRoot?.missions[0].state).assertEqual('running');
      expect(scopedRoot?.missions[0].done).assertEqual(0);
      expect(scopedRoot?.missions[0].total).assertEqual(1);
      expect(scopedRoot?.counts.running).assertEqual(1);
      expect(scopedRoot?.counts.done_recent).assertEqual(0);

      // 4. Cross-host same agent ID (`host-2`, `agent-a`): isolated from `host-1/agent-a`!
      const scopedH2A = paStore.fetchScopedBriefing('host-2', 'agent-a');
      expect(scopedH2A !== null).assertTrue();
      expect(scopedH2A?.needs_you.length).assertEqual(0);
      expect(scopedH2A?.missions.length).assertEqual(1);
      expect(scopedH2A?.missions[0].workers.length).assertEqual(1);
      expect(scopedH2A?.missions[0].workers[0].host_id).assertEqual('host-2');
      expect(scopedH2A?.missions[0].state).assertEqual('running');
      expect(scopedH2A?.counts.running).assertEqual(1);
      expect(scopedH2A?.counts.done_recent).assertEqual(0);
    });

    it('cwdEquivalenceRejectsDifferentHomeUsersAndNormalizesRelPaths', 0, () => {
      // R3-P0-1 / R3-P1-6: Never strip `/home/<user>` or `/Users/<user>` when comparing cwds.
      expect(cwdPathsEquivalent('/home/alice/proj', '/home/bob/proj')).assertFalse();
      expect(cwdPathsEquivalent('/Users/alice/proj', '/Users/bob/proj')).assertFalse();
      expect(cwdPathsEquivalent('/root/proj', '/home/alice/proj')).assertFalse();
      expect(cwdPathsEquivalent('/home/alice/proj/', '/home/alice/./proj//')).assertTrue();
      expect(normalizeAbsoluteCwd('/home/bob/proj///')).assertEqual('/home/bob/proj');
      expect(cwdPathsEquivalent('~/proj', '/home/alice/proj')).assertFalse();
      expect(cwdPathsEquivalent('~/proj', '/home/alice/proj', '/home/alice')).assertTrue();
      expect(cwdPathsEquivalent('~/proj', '/home/bob/proj', '/home/alice')).assertFalse();
      expect(normalizeWorkspaceRelPath('/home/alice/proj/src/a.ts', '/home/alice/proj')).assertEqual('src/a.ts');
      expect(normalizeWorkspaceRelPath('./src//./a.ts', '/home/alice/proj')).assertEqual('src/a.ts');
      expect(normalizeWorkspaceRelPath('/home/bob/proj/src/a.ts', '/home/alice/proj'))
        .assertEqual('/home/bob/proj/src/a.ts');
    });

    it('sideQuestionCanonicalTabIdSurvivesPromotionRemapAndHydrationWithoutDuplicates', 0, () => {
      // R3-P1-3: Draft /btw -> promote -> optimistic remap -> Done MUST keep exactly ONE Q&A tab!
      const storage = new FakeSidePanelStorage();
      const store = new OrbitSidePanelStore(0);
      store.bindStorage(storage);
      store.switchScope('host-1', null);
      const draftScope = store.scopeKeyFor('host-1', null);
      const draftQaId = canonicalSideQuestionTabId(draftScope);
      store.openTab(
        makeTab(draftQaId, 'side_question', 'host-1', null, '/home/bob/proj'),
        { source: 'user', activate: true }
      );
      expect(store.tabs.length).assertEqual(1);
      expect(store.activeTabId).assertEqual(draftQaId);

      // Promote draft -> optimistic agent `opt_9`
      store.promoteDraftScope('host-1', 'opt_9', '/home/bob/proj');
      const optScope = store.scopeKeyFor('host-1', 'opt_9');
      const optQaId = canonicalSideQuestionTabId(optScope);
      expect(store.tabs.length).assertEqual(1);
      expect(store.tabs[0].tabId).assertEqual(optQaId);
      expect(store.activeTabId).assertEqual(optQaId);

      // Remap `opt_9` -> `real_9`
      store.remapOptimisticScopes('host-1', 'opt_9', 'real_9');
      const realScope = store.scopeKeyFor('host-1', 'real_9');
      const realQaId = canonicalSideQuestionTabId(realScope);
      expect(store.tabs.length).assertEqual(1);
      expect(store.tabs[0].tabId).assertEqual(realQaId);
      expect(store.activeTabId).assertEqual(realQaId);

      // Simulate `/btw Done` auto-open in `real_9` scope: MUST deduplicate to the SAME single Q&A tab!
      store.openTabInScope(
        realScope,
        makeTab(canonicalSideQuestionTabId(realScope), 'side_question', 'host-1', 'real_9', '/home/bob/proj'),
        { source: 'auto', activate: false }
      );
      const qaTabs = store.tabs.filter((t: OrbitSidePanelTab): boolean => t.kind === 'side_question');
      expect(qaTabs.length).assertEqual(1);
      expect(qaTabs[0].tabId).assertEqual(realQaId);

      // Legacy persisted `side_question:<scopeKey>` migrates cleanly to canonical ID on hydrate.
      const legacyTabJson = JSON.stringify(
        makeTab('side_question:host-1::real_9', 'side_question', 'host-1', 'real_9', '/home/bob/proj')
      );
      const legacyJson = `{"version":1,"byScope":{"host-1::real_9":{"open":true,"tabs":[${legacyTabJson}],"activeTabId":"side_question:host-1::real_9","unseenTabIds":["side_question:host-1::real_9"],"widthVp":360,"closedTabs":[],"userDismissed":false,"lastUsedAt":1700000000000,"lastUsedOrder":1}}}`;
      const legacyStorage = new FakeSidePanelStorage();
      legacyStorage.rawJson = legacyJson;
      const hydratedStore = new OrbitSidePanelStore(0);
      hydratedStore.bindStorage(legacyStorage);
      expect(hydratedStore.hydrate()).assertTrue();
      const restoredSnap = hydratedStore.switchScope('host-1', 'real_9');
      expect(restoredSnap.tabs.length).assertEqual(1);
      expect(restoredSnap.tabs[0].tabId).assertEqual(realQaId);
      expect(restoredSnap.activeTabId).assertEqual(realQaId);
      expect(restoredSnap.unseenTabIds.length).assertEqual(1);
      expect(restoredSnap.unseenTabIds[0]).assertEqual(realQaId);

      // R3-P1-3 (Round 4): Persisted v1 state where real scope contains BOTH stale draft Q&A
      // `["side_question","draft::H"]` AND real Q&A `["side_question","H::real"]` with activeTabId
      // still pointing at the stale draft ID MUST collapse to 1 canonical tab and migrate active/unseen/closed!
      const staleDraftQaId = canonicalSideQuestionTabId('draft::host-1');
      const staleDraftTabJson = JSON.stringify(
        makeTab(staleDraftQaId, 'side_question', 'host-1', null, '/home/bob/proj')
      );
      const realQaTabJson = JSON.stringify(
        makeTab(realQaId, 'side_question', 'host-1', 'real_9', '/home/bob/proj')
      );
      const closedDraftQaJson = JSON.stringify(
        makeTab('side_question:draft::host-1', 'side_question', 'host-1', null, '/home/bob/proj')
      );
      const dupJson =
        `{"version":1,"byScope":{"host-1::real_9":{"open":true,"tabs":[${staleDraftTabJson},${realQaTabJson}],` +
        `"activeTabId":${JSON.stringify(staleDraftQaId)},` +
        `"unseenTabIds":[${JSON.stringify(staleDraftQaId)},${JSON.stringify(realQaId)}],` +
        `"widthVp":360,"closedTabs":[${closedDraftQaJson}],"userDismissed":false,` +
        `"lastUsedAt":1700000000001,"lastUsedOrder":2}}}`;
      const dupStorage = new FakeSidePanelStorage();
      dupStorage.rawJson = dupJson;
      const dupStore = new OrbitSidePanelStore(0);
      dupStore.bindStorage(dupStorage);
      expect(dupStore.hydrate()).assertTrue();
      const dupSnap = dupStore.switchScope('host-1', 'real_9');
      expect(dupSnap.tabs.length).assertEqual(1);
      expect(dupSnap.tabs[0].tabId).assertEqual(realQaId);
      expect(dupSnap.tabs[0].agentId).assertEqual('real_9');
      expect(dupSnap.activeTabId).assertEqual(realQaId);
      expect(dupSnap.unseenTabIds.length).assertEqual(1);
      expect(dupSnap.unseenTabIds[0]).assertEqual(realQaId);
      expect(dupSnap.closedTabs.length).assertEqual(0);

      // R3-P1-3 (Round 5): Duplicate Q&A inside `closedTabs` ONLY (stale draft `agentId=null` BEFORE
      // canonical `agentId=real`), while open tabs only hold a terminal. Hydrate + reopenLast() MUST
      // upgrade closed metadata to canonical `agentId=real` while preserving LIFO position 0!
      const openTermJson = JSON.stringify(
        makeTab('terminal:host-1::real_9', 'terminal', 'host-1', 'real_9', '/home/bob/proj')
      );
      const closedDupJson =
        `{"version":1,"byScope":{"host-1::real_9":{"open":true,"tabs":[${openTermJson}],` +
        `"activeTabId":"terminal:host-1::real_9","unseenTabIds":[],"widthVp":360,` +
        `"closedTabs":[${staleDraftTabJson},${realQaTabJson}],"userDismissed":false,` +
        `"lastUsedAt":1700000000002,"lastUsedOrder":3}}}`;
      const closedDupStorage = new FakeSidePanelStorage();
      closedDupStorage.rawJson = closedDupJson;
      const closedDupStore = new OrbitSidePanelStore(0);
      closedDupStore.bindStorage(closedDupStorage);
      expect(closedDupStore.hydrate()).assertTrue();
      const closedDupSnap = closedDupStore.switchScope('host-1', 'real_9');
      expect(closedDupSnap.tabs.length).assertEqual(1);
      expect(closedDupSnap.closedTabs.length).assertEqual(1);
      expect(closedDupSnap.closedTabs[0].tabId).assertEqual(realQaId);
      expect(closedDupSnap.closedTabs[0].agentId).assertEqual('real_9');
      const reopenedQa = closedDupStore.reopenLast();
      expect(reopenedQa !== null).assertTrue();
      expect(reopenedQa?.tabId).assertEqual(realQaId);
      expect(reopenedQa?.agentId).assertEqual('real_9');
      expect(closedDupStore.activeTabId).assertEqual(realQaId);

      store.dispose();
      hydratedStore.dispose();
      dupStore.dispose();
      closedDupStore.dispose();
    });

    it('resolveRootScopeDistinguishesMissingAgentCardFromVerifiedRoot', 0, () => {
      // R3-P1-2: Cached host detail for B that omits `mid` MUST report `missingAgentCard: true`
      // and NEVER treat `B/mid` as a verified root!
      const lookup = new FakeParentLookup();
      lookup.setEnforceCachedHosts(true);
      lookup.setEnforceAgentCards(true);
      lookup.addEdge('host-C', 'child-1', 'host-B', 'mid-1');
      // Host B has valid cached HostDetail, but does NOT contain card `mid-1`!
      lookup.setHostCached('host-B', true);

      const store = new OrbitSidePanelStore(0);
      store.bindParentLookup(lookup);

      const missingRes = store.resolveRootScope('host-C', 'child-1');
      expect(missingRes.missingAgentCard).assertTrue();
      expect(missingRes.unresolvedHostId).assertEqual('host-B');

      // Direct Crew / Peer lookup starting at missing `host-B/mid-1` with `requireStartCardExists=true`
      // MUST also fail closed (`missingAgentCard: true`) instead of opening `host-B/mid-1`!
      const directMissing = store.resolveRootScope('host-B', 'mid-1', true);
      expect(directMissing.missingAgentCard).assertTrue();
      expect(directMissing.unresolvedHostId).assertEqual('host-B');

      // Now add `mid-1 -> host-A/root-1` on host-B and verified root card `root-1` (no parent) on host-A:
      lookup.addEdge('host-B', 'mid-1', 'host-A', 'root-1');
      lookup.addEdge('host-A', 'root-1', null, null);
      const resolved = store.resolveRootScope('host-C', 'child-1');
      expect(resolved.missingAgentCard).assertFalse();
      expect(resolved.unresolvedHostId === null).assertTrue();
      expect(resolved.hostId).assertEqual('host-A');
      expect(resolved.rootAgentId).assertEqual('root-1');

      store.dispose();
    });

    it('coldStartDeepLinkSurvivesConcurrentHostChangedInBothOrdersAndRespectsUserCancel', 0, async (): Promise<void> => {
      const fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('FIRST')},${makeTestHostCardJson('TARGET')}],"projects":[],"total_pending_approvals":0}`;
      const firstDetail = makeTestHostDetailJson('FIRST', [makeTestAgentCardJson('FIRST', 'first-a', '/repo')]);
      const targetDetail = makeTestHostDetailJson('TARGET', [
        makeTestAgentCardJson('TARGET', 'first-agent', '/repo'),
        makeTestAgentCardJson('TARGET', 'real', '/repo')
      ]);

      // 1. Old response (P1) returns FIRST, then superseding host_changed response (P2) returns.
      const bridgeA = new FakeOrbitCoreBridgeForStore();
      bridgeA.fleetJson = fleetJson;
      bridgeA.detailJsonByHost.set('FIRST', firstDetail);
      bridgeA.deferHostDetailForHosts.add('TARGET');
      const storeA = new OrbitHarmonyStore();
      storeA.handleDeepLinkUri('orbit://open?host=TARGET&agent=real');
      storeA.initializeWithTestBridge(bridgeA);
      await flushAsyncTurns(4);
      expect(bridgeA.deferredHostDetailQueue.length).assertEqual(1);

      // Real frame scheduler delivers host_changed(TARGET) triggering generation 2 (P2).
      bridgeA.emitCoreEvent('{"kind":"host_changed","host_id":"TARGET"}');
      storeA.eventScheduler.flushNow();
      await flushAsyncTurns(4);
      expect(bridgeA.deferredHostDetailQueue.length).assertEqual(2);

      // Resolve OLD (P1) first, then NEW (P2).
      bridgeA.deferredHostDetailQueue[0].resolve(targetDetail);
      await flushAsyncTurns(6);
      bridgeA.deferredHostDetailQueue[1].resolve(targetDetail);
      await flushAsyncTurns(8);

      expect(storeA.selectedHostId).assertEqual('TARGET');
      expect(storeA.selectedAgentId).assertEqual('real');
      expect(bridgeA.openedTimelines.includes('TARGET::real')).assertTrue();
      expect(bridgeA.openedTimelines.includes('TARGET::first-agent')).assertFalse();
      expect(storeA.toastIsError).assertFalse();
      storeA.sidePanelStore.dispose();

      // 2. Newer response (P2) returns FIRST, then stale P1 returns.
      const bridgeB = new FakeOrbitCoreBridgeForStore();
      bridgeB.fleetJson = fleetJson;
      bridgeB.detailJsonByHost.set('FIRST', firstDetail);
      bridgeB.deferHostDetailForHosts.add('TARGET');
      const storeB = new OrbitHarmonyStore();
      storeB.handleDeepLinkUri('orbit://open?host=TARGET&agent=real');
      storeB.initializeWithTestBridge(bridgeB);
      await flushAsyncTurns(4);
      bridgeB.emitCoreEvent('{"kind":"host_changed","host_id":"TARGET"}');
      storeB.eventScheduler.flushNow();
      await flushAsyncTurns(4);
      expect(bridgeB.deferredHostDetailQueue.length).assertEqual(2);

      // Resolve NEW (P2) first, then OLD (P1).
      bridgeB.deferredHostDetailQueue[1].resolve(targetDetail);
      await flushAsyncTurns(6);
      bridgeB.deferredHostDetailQueue[0].resolve(targetDetail);
      await flushAsyncTurns(8);

      expect(storeB.selectedHostId).assertEqual('TARGET');
      expect(storeB.selectedAgentId).assertEqual('real');
      expect(bridgeB.openedTimelines.includes('TARGET::real')).assertTrue();
      expect(bridgeB.openedTimelines.includes('TARGET::first-agent')).assertFalse();
      expect(storeB.toastIsError).assertFalse();
      storeB.sidePanelStore.dispose();

      // 3. Explicit user navigation to FIRST during concurrent refresh cancels the pending deep-link!
      const bridgeC = new FakeOrbitCoreBridgeForStore();
      bridgeC.fleetJson = fleetJson;
      bridgeC.detailJsonByHost.set('FIRST', firstDetail);
      bridgeC.deferHostDetailForHosts.add('TARGET');
      const storeC = new OrbitHarmonyStore();
      storeC.handleDeepLinkUri('orbit://open?host=TARGET&agent=real');
      storeC.initializeWithTestBridge(bridgeC);
      await flushAsyncTurns(4);
      bridgeC.emitCoreEvent('{"kind":"host_changed","host_id":"TARGET"}');
      storeC.eventScheduler.flushNow();
      await flushAsyncTurns(4);

      // User explicitly switches to FIRST while TARGET refreshes are in flight.
      storeC.selectHost('FIRST');
      await flushAsyncTurns(4);
      bridgeC.deferredHostDetailQueue[0].resolve(targetDetail);
      bridgeC.deferredHostDetailQueue[1].resolve(targetDetail);
      await flushAsyncTurns(8);

      expect(storeC.selectedHostId).assertEqual('FIRST');
      expect(bridgeC.openedTimelines.includes('TARGET::real')).assertFalse();
      storeC.sidePanelStore.dispose();
    });

    it('nativeGitReviewDiffRevisionIsolatesModesStatusOnlyUnchangedEmptyAndTimeout', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);
      expect(store.selectedHostId).assertEqual('H');
      expect(store.selectedAgentId).assertEqual('a1');

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      const amStatus = [
        '{"path":"f","status":"A","staged":true,"untracked":false}',
        '{"path":"f","status":"M","staged":false,"untracked":false}'
      ];
      const stagedOnlyStatus = [
        '{"path":"f","status":"A","staged":true,"untracked":false}'
      ];

      // 1. Staged mode on AM f: 4 coalesced status-only git_changed (diff_revision stays 0) MUST remain loading;
      //    real Staged GitDiffResponse (diff_revision 0 -> 1) immediately confirms `+index-only`.
      const tabF = makeTab('review:H:/repo:f', 'review', 'H', 'a1', '/repo', 'f');
      tabF.gitStaged = true;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(1, 'H', '/repo', 'f', '', [], amStatus, true, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, tabF, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', true, 'f', true, true);
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('loading');
      for (let i = 0; i < 4; i++) {
        bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      }
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('loading');
      expect(store.isScopedGitDiffConfirmed('H', '/repo', true, 'f')).assertFalse();

      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(2, 'H', '/repo', 'f', 'diff --git a/f b/f\n+index-only\n', ['f'], amStatus, true, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('confirmed');
      expect(store.isScopedGitDiffConfirmed('H', '/repo', true, 'f')).assertTrue();
      const sView = store.loadScopedGitReview('H', '/repo', true, 'f', false, false);
      expect(sView !== null && sView.staged === true && sView.diff_revision === 1).assertTrue();
      expect(sView !== null && sView.raw_diff.includes('+index-only')).assertTrue();

      // 2. Switch to Unstaged on the exact same file: physically isolated `(f, false)` slot confirms `+worktree-only`
      //    without disturbing `(f, true)`.
      store.sidePanelStore.updateTabGitStaged(scopeKey, tabF.tabId, false);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|U',
        makeTestGitReviewJson(2, 'H', '/repo', 'f', '', [], amStatus, false, 0)
      );
      store.loadScopedGitReview('H', '/repo', false, 'f', true, false);
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('loading');
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|U',
        makeTestGitReviewJson(3, 'H', '/repo', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], amStatus, false, 2)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('confirmed');
      const uView = store.loadScopedGitReview('H', '/repo', false, 'f', false, false);
      expect(uView !== null && uView.staged === false && uView.diff_revision === 2).assertTrue();
      expect(uView !== null && uView.raw_diff.includes('+worktree-only')).assertTrue();

      // 3. Unchanged-body refresh on U: status-only (diff_revision stays 2) keeps loading;
      //    real unchanged-body reply (diff_revision 2 -> 3) immediately confirms!
      store.loadScopedGitReview('H', '/repo', false, 'f', true, true);
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('loading');
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|U',
        makeTestGitReviewJson(4, 'H', '/repo', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], amStatus, false, 2)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('loading');

      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|U',
        makeTestGitReviewJson(5, 'H', '/repo', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], amStatus, false, 3)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', false, 'f', false, false)?.diff_revision).assertEqual(3);

      // 4. Non-empty to genuinely empty body (diff_revision 3 -> 4): confirms cleanly with 0 files.
      store.loadScopedGitReview('H', '/repo', false, 'f', true, true);
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('loading');
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|U',
        makeTestGitReviewJson(6, 'H', '/repo', 'f', '', [], stagedOnlyStatus, false, 4)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('confirmed');
      const emptyU = store.loadScopedGitReview('H', '/repo', false, 'f', false, false);
      expect(emptyU?.diff_files.length).assertEqual(0);
      expect(emptyU?.diff_revision).assertEqual(4);

      // 5. Eviction back to diff_revision === 0 on an already-confirmed slot safely demotes to unverified.
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|U',
        makeTestGitReviewJson(7, 'H', '/repo', 'f', '', [], [], false, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'f')).assertEqual('unverified');

      // 6. 1500ms timeout -> transitions to unverified; late valid diff_revision recovers cleanly to confirmed!
      const timeoutTab = makeTab('review:H:/repo:slow.ts', 'review', 'H', 'a1', '/repo', 'slow.ts');
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:slow.ts|U',
        makeTestGitReviewJson(1, 'H', '/repo', 'slow.ts', '', [], [], false, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, timeoutTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, 'slow.ts', true, true);
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'slow.ts')).assertEqual('loading');
      await new Promise<void>((resolve: () => void): void => {
        setTimeout(resolve, 1600);
      });
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'slow.ts')).assertEqual('unverified');
      expect(store.loadScopedGitReview('H', '/repo', false, 'slow.ts', false, false)?.diff_files.length).assertEqual(0);

      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:slow.ts|U',
        makeTestGitReviewJson(2, 'H', '/repo', 'slow.ts', 'diff --git a/slow.ts b/slow.ts\n+recovered\n', ['slow.ts'], [], false, 5)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'slow.ts')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', false, 'slow.ts', false, false)?.raw_diff).assertContain('+recovered');

      // 7. LRU 16 tracker eviction cap and removeHost cleanup
      store.sidePanelStore.dismissPanel();
      for (let i = 0; i < 20; i++) {
        store.loadScopedGitReview('H', '/repo', false, `other_${i}.ts`, true, false);
      }
      expect(store.gitDiffTrackerCount <= 16).assertTrue();
      store.removeHost('H');
      expect(store.gitDiffTrackerCount).assertEqual(0);
      store.sidePanelStore.dispose();
    });

    it('filesVisibleTriggersZeroGitReviewReadsAndReviewTakeoverReleasesOldPayload', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      const statusA = ['{"path":"a.ts","status":"M","staged":false,"untracked":false}'];
      const statusB = ['{"path":"b.ts","status":"M","staged":false,"untracked":false}'];

      // Sub-item A: Only Files tab is visible while Review is hidden -> 4 coalesced git_changed MUST do 0 gitReviewJson!
      store.sidePanelStore.openTabInScope(
        scopeKey,
        makeTab('files:H:/repo', 'files', 'H', 'a1', '/repo', null),
        { source: 'user', activate: true }
      );
      bridge.gitReviewJsonCalls = 0;
      bridge.workspaceFilesJsonCalls = 0;
      for (let i = 0; i < 4; i++) {
        bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      }
      store.eventScheduler.flushNow();
      expect(bridge.gitReviewJsonCalls).assertEqual(0);
      expect(bridge.workspaceFilesJsonCalls > 0).assertTrue();

      // Sub-item B: Dismiss panel (hidden) -> git_changed MUST also do 0 gitReviewJson!
      store.sidePanelStore.dismissPanel();
      bridge.gitReviewJsonCalls = 0;
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(bridge.gitReviewJsonCalls).assertEqual(0);

      // Sub-item C: Open Review B then Review A (active), confirm A so A holds a cached diff payload,
      // then close A so B takes over -> A's cached diff payload and tracker MUST be released immediately!
      const tabB = makeTab('review:H:/repo:b.ts', 'review', 'H', 'a1', '/repo', 'b.ts');
      const tabA = makeTab('review:H:/repo:a.ts', 'review', 'H', 'a1', '/repo', 'a.ts');
      store.sidePanelStore.openTabInScope(scopeKey, tabB, { source: 'user', activate: true });
      store.sidePanelStore.openTabInScope(scopeKey, tabA, { source: 'user', activate: true });

      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:a.ts|U',
        makeTestGitReviewJson(1, 'H', '/repo', 'a.ts', '', [], statusA, false, 0)
      );
      store.loadScopedGitReview('H', '/repo', false, 'a.ts', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:a.ts|U',
        makeTestGitReviewJson(2, 'H', '/repo', 'a.ts', 'diff --git a/a.ts b/a.ts\n+payload-a\n', ['a.ts'], statusA, false, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'a.ts')).assertEqual('confirmed');
      expect(store.hasCachedGitDiffPayload('H', '/repo', 'a.ts')).assertTrue();

      // Close active Review tab A -> Review tab B becomes active; A's payload is released immediately!
      store.sidePanelStore.close(tabA.tabId);
      expect(store.sidePanelStore.activeTab?.tabId).assertEqual(tabB.tabId);
      expect(store.hasCachedGitDiffPayload('H', '/repo', 'a.ts')).assertFalse();

      // Confirm B still loads cleanly after takeover.
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:b.ts|U',
        makeTestGitReviewJson(1, 'H', '/repo', 'b.ts', '', [], statusB, false, 0)
      );
      store.loadScopedGitReview('H', '/repo', false, 'b.ts', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:b.ts|U',
        makeTestGitReviewJson(2, 'H', '/repo', 'b.ts', 'diff --git a/b.ts b/b.ts\n+payload-b\n', ['b.ts'], statusB, false, 2)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'b.ts')).assertEqual('confirmed');
      expect(store.hasCachedGitDiffPayload('H', '/repo', 'b.ts')).assertTrue();
      expect(store.hasCachedGitDiffPayload('H', '/repo', 'a.ts')).assertFalse();

      store.sidePanelStore.dispose();
    });

    it('nativeGitReviewHandlesLargeDiffLiteralAllDotSlashAndTrailingSpacePaths', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');

      // 1. >32 KiB diff refresh where `raw_diff` prefix is truncated identically at 32 KiB:
      //    native `diff_revision` advances cleanly past 32 KiB without client-side hashing!
      const sharedPrefix32k = 'x'.repeat(32768);
      const makeRichReviewJson = (
        revision: number,
        selectedPath: string | null,
        rawPrefix: string,
        filesPayloadJson: string[],
        statusFilesJson: string[],
        staged: boolean,
        diffRevision: number
      ): string => {
        return `{"revision":${revision},"host_id":"H","cwd":"/repo","status":{"cwd":"/repo","is_git_repo":true,"branch":"main","upstream":null,"ahead":0,"behind":0,"files":[${statusFilesJson.join(',')}]},"selected_path":${JSON.stringify(selectedPath)},"staged":${staged ? 'true' : 'false'},"diff_revision":${diffRevision},"raw_diff":${JSON.stringify(rawPrefix)},"diff_files":[${filesPayloadJson.join(',')}]}`;
      };
      const bigStatus = ['{"path":"big.txt","status":"M","staged":false,"untracked":false}'];
      const fileBefore = '{"path":"big.txt","additions":2,"deletions":0,"lines":[{"kind":"add","old_line":null,"new_line":1,"content":"head"},{"kind":"add","old_line":null,"new_line":400,"content":"tail-before"}]}';
      const fileAfter = '{"path":"big.txt","additions":2,"deletions":0,"lines":[{"kind":"add","old_line":null,"new_line":1,"content":"head"},{"kind":"add","old_line":null,"new_line":400,"content":"tail-after"}]}';

      const bigTab = makeTab('review:H:/repo:big.txt', 'review', 'H', 'a1', '/repo', 'big.txt');
      bridge.gitReviewJsonBySlot.set('H|/repo|file:big.txt|U', makeRichReviewJson(1, 'big.txt', '', [], bigStatus, false, 0));
      store.sidePanelStore.openTabInScope(scopeKey, bigTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, 'big.txt', true, true);
      bridge.gitReviewJsonBySlot.set('H|/repo|file:big.txt|U', makeRichReviewJson(2, 'big.txt', sharedPrefix32k, [fileBefore], bigStatus, false, 1));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'big.txt')).assertEqual('confirmed');

      store.loadScopedGitReview('H', '/repo', false, 'big.txt', true, true);
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'big.txt')).assertEqual('loading');
      bridge.gitReviewJsonBySlot.set('H|/repo|file:big.txt|U', makeRichReviewJson(3, 'big.txt', sharedPrefix32k, [fileAfter], bigStatus, false, 2));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'big.txt')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', false, 'big.txt', false, false)?.diff_revision).assertEqual(2);

      // 2. Whole-repo (`null`) vs literal file `'__all__'` / `'./__all__'` in BOTH U and S modes:
      //    completely isolated with zero cross-scope or cross-mode collision!
      const twoFileStatus = [
        '{"path":"__all__","status":"A","staged":true,"untracked":false}',
        '{"path":"__all__","status":"M","staged":false,"untracked":false}',
        '{"path":"g","status":"M","staged":false,"untracked":false}'
      ];
      const wholeTab = makeTab('review:H:/repo:all:', 'review', 'H', 'a1', '/repo', null);
      wholeTab.gitStaged = true;
      bridge.gitReviewJsonBySlot.set('H|/repo|all:|S', makeTestGitReviewJson(1, 'H', '/repo', null, '', [], twoFileStatus, true, 0));
      store.sidePanelStore.openTabInScope(scopeKey, wholeTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', true, null, true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|all:|S',
        makeTestGitReviewJson(2, 'H', '/repo', null, 'diff --git a/__all__ b/__all__\n+whole-repo-staged\n', ['__all__'], twoFileStatus, true, 10)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, null)).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', true, null, false, false)?.raw_diff).assertContain('+whole-repo-staged');

      const literalAllTab = makeTab('review:H:/repo:file:__all__', 'review', 'H', 'a1', '/repo', './__all__');
      literalAllTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set('H|/repo|file:__all__|U', makeTestGitReviewJson(1, 'H', '/repo', '__all__', '', [], twoFileStatus, false, 0));
      store.sidePanelStore.openTabInScope(scopeKey, literalAllTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, './__all__', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:__all__|U',
        makeTestGitReviewJson(2, 'H', '/repo', '__all__', 'diff --git a/__all__ b/__all__\n+literal-all-unstaged\n', ['__all__'], twoFileStatus, false, 11)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, '__all__')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', false, '__all__', false, false)?.raw_diff).assertContain('+literal-all-unstaged');

      // 3. `./daily1.txt` normalizes to `daily1.txt` identically across fetch & lookup
      const untrackedStatus = ['{"path":"daily1.txt","status":"??","staged":false,"untracked":true}'];
      const synthPatch = '--- /dev/null\n+++ b/daily1.txt\n@@ -0,0 +1,1 @@\n+untracked-other\n';
      const dotSlashTab = makeTab('review:H:/repo:./daily1.txt', 'review', 'H', 'a1', '/repo', './daily1.txt');
      bridge.gitReviewJsonBySlot.set('H|/repo|file:daily1.txt|U', makeTestGitReviewJson(1, 'H', '/repo', 'daily1.txt', '', [], untrackedStatus, false, 0));
      store.sidePanelStore.openTabInScope(scopeKey, dotSlashTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, './daily1.txt', true, true);
      bridge.gitReviewJsonBySlot.set('H|/repo|file:daily1.txt|U', makeTestGitReviewJson(2, 'H', '/repo', 'daily1.txt', synthPatch, ['daily1.txt'], untrackedStatus, false, 12));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, './daily1.txt')).assertEqual('confirmed');
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'daily1.txt')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', false, './daily1.txt', false, false)?.raw_diff).assertContain('+untracked-other');

      // 4. R12-P1-4 / R13-P1-1 Trailing-space filename (`report.txt ` and `./report.txt `) MUST NEVER
      //    be trimmed and MUST remain strictly distinct from `report.txt` across diff headers,
      //    line quotes, and Stage/Unstage row actions!
      const spaceStatus = [
        '{"path":"report.txt","status":"M","staged":false,"untracked":false}',
        '{"path":"report.txt ","status":"??","staged":false,"untracked":true}'
      ];
      bridge.fetchGitDiffCalls = [];
      bridge.stagePathsCalls = [];
      bridge.unstagePathsCalls = [];
      const trimmedTab = makeTab('review:H:/repo:report.txt', 'review', 'H', 'a1', '/repo', 'report.txt');
      bridge.gitReviewJsonBySlot.set('H|/repo|file:report.txt|U', makeTestGitReviewJson(1, 'H', '/repo', 'report.txt', '', [], spaceStatus, false, 0));
      store.sidePanelStore.openTabInScope(scopeKey, trimmedTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, 'report.txt', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:report.txt|U',
        makeTestGitReviewJson(2, 'H', '/repo', 'report.txt', 'diff --git a/report.txt b/report.txt\n+trimmed-body\n', ['report.txt'], spaceStatus, false, 20)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'report.txt')).assertEqual('confirmed');
      const trimmedView = store.loadScopedGitReview('H', '/repo', false, 'report.txt', false, false);
      expect(trimmedView?.raw_diff).assertContain('+trimmed-body');
      expect(trimmedView?.diff_files[0]?.path).assertEqual('report.txt');
      expect(trimmedView !== null && !trimmedView.raw_diff.includes('+spaced-body')).assertTrue();

      const spacedTab = makeTab('review:H:/repo:report.txt ', 'review', 'H', 'a1', '/repo', './report.txt ');
      bridge.gitReviewJsonBySlot.set('H|/repo|file:report.txt |U', makeTestGitReviewJson(1, 'H', '/repo', 'report.txt ', '', [], spaceStatus, false, 0));
      store.sidePanelStore.openTabInScope(scopeKey, spacedTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, './report.txt ', true, true);
      expect(bridge.fetchGitDiffCalls.includes('H|/repo|report.txt |U')).assertTrue();
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:report.txt |U',
        makeTestGitReviewJson(3, 'H', '/repo', 'report.txt ', '--- /dev/null\n+++ b/report.txt \n+spaced-body\n', ['report.txt '], spaceStatus, false, 21)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'report.txt ')).assertEqual('confirmed');
      expect(store.getScopedGitDiffModeState('H', '/repo', false, './report.txt ')).assertEqual('confirmed');
      const spacedView = store.loadScopedGitReview('H', '/repo', false, 'report.txt ', false, false);
      expect(spacedView?.raw_diff).assertContain('+spaced-body');
      expect(spacedView?.selected_path).assertEqual('report.txt ');
      expect(spacedView?.diff_files[0]?.path).assertEqual('report.txt ');
      expect(spacedView !== null && !spacedView.raw_diff.includes('+trimmed-body')).assertTrue();

      // Verify line quotes AND scoped Stage/Unstage actions preserve exact trailing space!
      const spacedPath = spacedView !== null && spacedView.diff_files.length > 0 ? spacedView.diff_files[0].path : '';
      const trimmedPath = trimmedView !== null && trimmedView.diff_files.length > 0 ? trimmedView.diff_files[0].path : '';
      expect(spacedPath).assertEqual('report.txt ');
      expect(trimmedPath).assertEqual('report.txt');
      store.quotedSnippets = [];
      store.quoteDiffLineIntoComposer(spacedPath, {
        kind: 'added',
        old_line: null,
        new_line: 1,
        content: 'spaced-body'
      });
      store.quoteDiffLineIntoComposer(trimmedPath, {
        kind: 'added',
        old_line: null,
        new_line: 1,
        content: 'trimmed-body'
      });
      expect(store.quotedSnippets.length).assertEqual(2);
      expect(store.quotedSnippets[0].path).assertEqual('report.txt ');
      expect(store.quotedSnippets[0].lineLabel).assertEqual('report.txt :L1');
      expect(store.quotedSnippets[1].path).assertEqual('report.txt');
      expect(store.quotedSnippets[1].lineLabel).assertEqual('report.txt:L1');

      expect(await store.gitStageScopedPaths('H', '/repo', [spacedPath], false)).assertTrue();
      expect(await store.gitUnstageScopedPaths('H', '/repo', [spacedPath], false)).assertTrue();
      expect(await store.gitStageScopedPaths('H', '/repo', [trimmedPath], false)).assertTrue();
      expect(JSON.stringify(bridge.stagePathsCalls)).assertEqual(
        JSON.stringify(['H|/repo|["report.txt "]', 'H|/repo|["report.txt"]'])
      );
      expect(JSON.stringify(bridge.unstagePathsCalls)).assertEqual(
        JSON.stringify(['H|/repo|["report.txt "]'])
      );

      // Verify `openGitInspector('report.txt ')` preserves trailing space on the opened tab!
      store.openGitInspector('report.txt ', '/repo', 'H');
      expect(store.sidePanelStore.activeTab?.path).assertEqual('report.txt ');

      // 5. Global gitStagePaths captures origin `(targetHost, targetCwd)` before await: switching cwd
      //    mid-flight does NOT trigger a refresh on `/other`!
      store.setActiveCwd('/repo');
      bridge.fetchGitDiffCalls = [];
      bridge.deferNextGitMutation = true;
      const stagePromise = store.gitStagePaths(['f'], false);
      await flushAsyncTurns(2);
      store.setActiveCwd('/other');
      expect(bridge.deferredGitMutationResolvers.length).assertEqual(1);
      bridge.deferredGitMutationResolvers[0]('{"ok":true}');
      expect(await stagePromise).assertTrue();
      expect(bridge.fetchGitDiffCalls.some((c: string): boolean => c.indexOf('/other') >= 0)).assertFalse();

      store.sidePanelStore.dispose();
    });

    it('round14GitDiffRowActionsAndTrailingSpaceCwdIsolation', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/tmp/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      // 1. R14-P1-2: Opening directory pathspec `d` (with `d/one` and `d/two`) and filtering down
      //    to 1 visible row (`d/one`) MUST NEVER substitute `selected_path` (`"d"`) for row actions!
      expect(resolveGitDiffRowActionPath('d/one', 'd')).assertEqual('d/one');
      expect(resolveGitDiffRowActionPath('part b/tail', null)).assertEqual('part b/tail');
      expect(resolveGitDiffRowActionPath('', 'report.txt ')).assertEqual('report.txt ');
      bridge.stagePathsCalls = [];
      bridge.unstagePathsCalls = [];
      const dirStatus = [
        '{"path":"d/one","status":"M","staged":false,"untracked":false}',
        '{"path":"d/two","status":"M","staged":false,"untracked":false}'
      ];
      const dirReviewJson = makeTestGitReviewJson(
        1,
        'H',
        '/tmp/repo',
        'd',
        'diff --git a/d/one b/d/one\n+one-new\ndiff --git a/d/two b/d/two\n+two-new\n',
        ['d/one', 'd/two'],
        dirStatus,
        false,
        5
      );
      bridge.gitReviewJsonBySlot.set('H|/tmp/repo|file:d|U', dirReviewJson);
      store.openGitInspector('d', '/tmp/repo', 'H');
      store.loadScopedGitReview('H', '/tmp/repo', false, 'd', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo|file:d|U',
        makeTestGitReviewJson(
          2,
          'H',
          '/tmp/repo',
          'd',
          'diff --git a/d/one b/d/one\n+one-new\ndiff --git a/d/two b/d/two\n+two-new\n',
          ['d/one', 'd/two'],
          dirStatus,
          false,
          6
        )
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/repo"}');
      store.eventScheduler.flushNow();
      const loadedDirReview = store.loadScopedGitReview('H', '/tmp/repo', false, 'd', false, false);
      expect(loadedDirReview !== null && loadedDirReview.diff_files.length === 2).assertTrue();
      // Simulate filtering `diff_files` by query `one` -> single visible row `d/one`
      const dirFiles = loadedDirReview !== null ? loadedDirReview.diff_files : [];
      const dirSelectedPath = loadedDirReview !== null ? loadedDirReview.selected_path : null;
      const filteredOne = dirFiles.filter(
        (df): boolean => df.path.toLowerCase().indexOf('one') >= 0
      );
      expect(filteredOne.length).assertEqual(1);
      const actionPath = resolveGitDiffRowActionPath(filteredOne[0].path, dirSelectedPath);
      expect(actionPath).assertEqual('d/one');
      expect(await store.gitStageScopedPaths('H', '/tmp/repo', [actionPath], false)).assertTrue();
      expect(JSON.stringify(bridge.stagePathsCalls)).assertEqual(
        JSON.stringify(['H|/tmp/repo|["d/one"]'])
      );
      // Pure rename row action (`part b/tail`) stays distinct from independent `tail`
      expect(await store.gitUnstageScopedPaths('H', '/tmp/repo', ['part b/tail'], false)).assertTrue();
      expect(JSON.stringify(bridge.unstagePathsCalls)).assertEqual(
        JSON.stringify(['H|/tmp/repo|["part b/tail"]'])
      );

      // 2. R14-P1-3: Trailing-space cwd (`/tmp/repo `) MUST remain strictly distinct from `/tmp/repo`
      //    across `normalizeAbsoluteCwd`, `cwdPathsEquivalent`, `openGitInspector`, tab creation,
      //    `fetchGitDiff`, `gitReviewJson`, and `gitStageScopedPaths`!
      const plainCwd = '/tmp/repo';
      const spacedCwd = '/tmp/repo ';
      expect(normalizeAbsoluteCwd(spacedCwd)).assertEqual('/tmp/repo ');
      expect(normalizeAbsoluteCwd(`${spacedCwd}/.`)).assertEqual('/tmp/repo ');
      expect(cwdPathsEquivalent(plainCwd, spacedCwd)).assertFalse();
      expect(canonicalCwdToolTabId('review', 'H', plainCwd) !== canonicalCwdToolTabId('review', 'H', spacedCwd)).assertTrue();

      bridge.fetchGitDiffCalls = [];
      bridge.stagePathsCalls = [];
      const plainStatus = ['{"path":"f","status":"M","staged":false,"untracked":false}'];
      bridge.gitReviewJsonBySlot.set(
        `H|${plainCwd}|file:f|U`,
        makeTestGitReviewJson(10, 'H', plainCwd, 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], plainStatus, false, 10)
      );
      bridge.gitReviewJsonBySlot.set(
        `H|${spacedCwd}|file:f|U`,
        makeTestGitReviewJson(1, 'H', spacedCwd, 'f', '', [], plainStatus, false, 0)
      );

      store.openGitInspector('f', spacedCwd, 'H');
      const activeSpacedTab = store.sidePanelStore.activeTab;
      expect(activeSpacedTab !== null).assertTrue();
      expect(activeSpacedTab?.cwd).assertEqual(spacedCwd);
      expect(activeSpacedTab?.path).assertEqual('f');

      store.loadScopedGitReview('H', spacedCwd, false, 'f', true, true);
      expect(bridge.fetchGitDiffCalls.includes(`H|${spacedCwd}|f|U`)).assertTrue();
      expect(bridge.fetchGitDiffCalls.includes(`H|${plainCwd}|f|U`)).assertFalse();

      bridge.gitReviewJsonBySlot.set(
        `H|${spacedCwd}|file:f|U`,
        makeTestGitReviewJson(11, 'H', spacedCwd, 'f', 'diff --git a/f b/f\n+SPACED-PROJECT\n', ['f'], plainStatus, false, 11)
      );
      bridge.emitCoreEvent(`{"kind":"git_changed","host_id":"H","cwd":${JSON.stringify(spacedCwd)}}`);
      store.eventScheduler.flushNow();

      expect(store.getScopedGitDiffModeState('H', spacedCwd, false, 'f')).assertEqual('confirmed');
      const spacedRepoView = store.loadScopedGitReview('H', spacedCwd, false, 'f', false, false);
      expect(spacedRepoView?.raw_diff).assertContain('+SPACED-PROJECT');
      expect(spacedRepoView !== null && !spacedRepoView.raw_diff.includes('+worktree-only')).assertTrue();

      expect(await store.gitStageScopedPaths('H', spacedCwd, ['f'], false)).assertTrue();
      expect(JSON.stringify(bridge.stagePathsCalls)).assertEqual(
        JSON.stringify([`H|${spacedCwd}|["f"]`])
      );

      // 3. R14-P1-5 (ArkTS half): While staging `f` is deferred, switching active Review tab to `g`
      //    keeps `g` in `loading`; when `f` completes & native re-dispatches/delivers `g/U` (`+g-working`),
      //    `g` confirms cleanly without timing out to `unverified`, and hiding Review still incurs 0 sync reads!
      bridge.gitReviewJsonCalls = 0;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo|file:g|U',
        makeTestGitReviewJson(1, 'H', '/tmp/repo', 'g', '', [], plainStatus, false, 0)
      );
      bridge.deferNextGitMutation = true;
      const pendingStageF = store.gitStageScopedPaths('H', '/tmp/repo', ['f'], false);
      await flushAsyncTurns(1);
      store.openGitInspector('g', '/tmp/repo', 'H');
      store.loadScopedGitReview('H', '/tmp/repo', false, 'g', true, true);
      expect(store.getScopedGitDiffModeState('H', '/tmp/repo', false, 'g')).assertEqual('loading');
      // Resolve `f`'s stage mutation and then deliver post-mutation `g/U` revision 12
      bridge.deferredGitMutationResolvers[0]('{"ok":true}');
      expect(await pendingStageF).assertTrue();
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo|file:g|U',
        makeTestGitReviewJson(12, 'H', '/tmp/repo', 'g', 'diff --git a/g b/g\n+g-working\n', ['g'], plainStatus, false, 12)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/repo', false, 'g')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/repo', false, 'g', false, false)?.raw_diff).assertContain('+g-working');

      // Dismiss Review panel -> subsequent git_changed events trigger 0 synchronous gitReviewJson reads
      store.sidePanelStore.dismissPanel();
      const readsBeforeHidden = bridge.gitReviewJsonCalls;
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/repo"}');
      store.eventScheduler.flushNow();
      expect(bridge.gitReviewJsonCalls).assertEqual(readsBeforeHidden);

      // 4. R16-P1-1 & R16-P1-2 (ArkTS side): When a previously confirmed single-file Unstaged tab
      //    (`f/U` confirmed `+worktree-only` before Stage ack, or root `README.md/U` cleaned while
      //    only `other/README.md` remains dirty) is evicted by native `GitMutationResult`
      //    (`diff_revision` drops to 0), `onScopedGitDirtyArrived` immediately invalidates the
      //    tracker to `unverified` with 0 diff files (never keeping stale `+worktree-only`)!
      const amStatus = [
        '{"path":"f","status":"A","staged":true,"untracked":false}',
        '{"path":"f","status":"M","staged":false,"untracked":false}'
      ];
      const stagedOnlyStatus = [
        '{"path":"f","status":"A","staged":true,"untracked":false}'
      ];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo|file:f|U',
        makeTestGitReviewJson(1, 'H', '/tmp/repo', 'f', '', [], amStatus, false, 0)
      );
      store.openGitInspector('f', '/tmp/repo', 'H');
      store.loadScopedGitReview('H', '/tmp/repo', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo|file:f|U',
        makeTestGitReviewJson(20, 'H', '/tmp/repo', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], amStatus, false, 20)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/repo', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/repo', false, 'f', false, false)?.raw_diff).assertContain('+worktree-only');

      // Stage completes (`AM -> A `): native evicts `(cwd, Some("f"), false)` so `diff_revision` becomes 0.
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo|file:f|U',
        makeTestGitReviewJson(21, 'H', '/tmp/repo', 'f', '', [], stagedOnlyStatus, false, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/repo', false, 'f')).assertEqual('unverified');
      const afterAmToA = store.loadScopedGitReview('H', '/tmp/repo', false, 'f', false, false);
      expect(afterAmToA !== null && afterAmToA.diff_files.length === 0).assertTrue();
      expect(afterAmToA !== null && !afterAmToA.raw_diff.includes('+worktree-only')).assertTrue();

      store.sidePanelStore.dispose();
    });

    it('round17CrossCwdInvalidationVisibleSlotRefetchAndLegacyFallback', 0, async (): Promise<void> => {
      // 1. Pure helper unit checks: exact repo_prefix matching, staged-mode isolation, untracked dir
      //    summary (`bucket/`), trailing-space paths, and legacy daemon (`repo_prefix` omitted).
      expect(
        gitStatusMayHaveDiffForSlot(
          { cwd: '/repo', is_git_repo: true, branch: 'main', upstream: null, ahead: 0, behind: 0, files: [], repo_prefix: '' },
          'f',
          false
        )
      ).assertFalse();
      expect(
        gitStatusMayHaveDiffForSlot(
          {
            cwd: '/repo',
            is_git_repo: true,
            branch: 'main',
            upstream: null,
            ahead: 0,
            behind: 0,
            files: [
              { path: 'f', status: 'A', staged: true, untracked: false },
              { path: 'other/README.md', status: 'M', staged: false, untracked: false }
            ],
            repo_prefix: ''
          },
          'README.md',
          false
        )
      ).assertFalse();
      expect(
        gitStatusMayHaveDiffForSlot(
          {
            cwd: '/repo',
            is_git_repo: true,
            branch: 'main',
            upstream: null,
            ahead: 0,
            behind: 0,
            files: [{ path: 'f', status: 'A', staged: true, untracked: false }],
            repo_prefix: ''
          },
          'f',
          true
        )
      ).assertTrue();
      expect(
        gitStatusMayHaveDiffForSlot(
          {
            cwd: '/repo',
            is_git_repo: true,
            branch: 'main',
            upstream: null,
            ahead: 0,
            behind: 0,
            files: [{ path: 'bucket/', status: '??', staged: false, untracked: true }],
            repo_prefix: ''
          },
          'bucket/removed.txt',
          false
        )
      ).assertTrue();
      expect(
        gitStatusMayHaveDiffForSlot(
          {
            cwd: '/repo/pkg',
            is_git_repo: true,
            branch: 'main',
            upstream: null,
            ahead: 0,
            behind: 0,
            files: [{ path: 'pkg/f', status: 'M', staged: true, untracked: false }],
            repo_prefix: 'pkg'
          },
          'f',
          false
        )
      ).assertFalse();
      expect(
        gitStatusMayHaveDiffForSlot(
          {
            cwd: '/repo/pkg',
            is_git_repo: true,
            branch: 'main',
            upstream: null,
            ahead: 0,
            behind: 0,
            files: [{ path: 'pkg/g', status: 'M', staged: false, untracked: false }]
          },
          'g',
          false
        )
      ).assertTrue();
      expect(
        gitStatusMayHaveDiffForSlot(
          {
            cwd: '/repo/pkg',
            is_git_repo: true,
            branch: 'main',
            upstream: null,
            ahead: 0,
            behind: 0,
            files: [{ path: 'pkg/f', status: 'A', staged: true, untracked: false }]
          },
          'g',
          false
        )
      ).assertFalse();

      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      // 2. R17-P1-3: Visible confirmed single-file `f/S` (`+index-only`) is evicted by Stage ack
      //    (`diff_revision` -> 0 while `status.files` has `A f, staged=true`): `onScopedGitDirtyArrived`
      //    MUST automatically re-fetch exact `('H', '/repo', 'f', true)` once and confirm `+worktree-only`!
      const stagedFStatus = ['{"path":"f","status":"A","staged":true,"untracked":false}'];
      const fsTab = makeTab('review:H:/repo:f', 'review', 'H', 'a1', '/repo', 'f');
      fsTab.gitStaged = true;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(1, 'H', '/repo', 'f', '', [], stagedFStatus, true, 0, '')
      );
      store.sidePanelStore.openTabInScope(scopeKey, fsTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', true, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(2, 'H', '/repo', 'f', 'diff --git a/f b/f\n+index-only\n', ['f'], stagedFStatus, true, 2, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', true, 'f', false, false)?.raw_diff).assertContain('+index-only');

      // Stage completes: native evicts `f/S` (`diff_revision: 0`) and keeps `A f (staged=true)` in status.
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(3, 'H', '/repo', 'f', '', [], stagedFStatus, true, 0, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/repo|f|S']));
      // Second git_changed before reply arrives MUST NOT double-fetch `f/S`:
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(bridge.fetchGitDiffCalls.length).assertEqual(1);
      // Fresh post-mutation `f/S` reply confirms `+worktree-only`:
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(4, 'H', '/repo', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], stagedFStatus, true, 4, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('confirmed');
      const fsFresh = store.loadScopedGitReview('H', '/repo', true, 'f', false, false);
      expect(fsFresh?.raw_diff).assertContain('+worktree-only');
      expect(fsFresh !== null && !fsFresh.raw_diff.includes('+index-only')).assertTrue();

      // 3. R17-P1-4: Deleted `bucket/removed.txt` under untracked directory summary `bucket/` is
      //    evicted on mutation, triggers a single exact re-fetch, receives authentic 0-byte empty
      //    diff, and confirms 0 diff files (NEVER retaining `+obsolete-body`)!
      const bucketStatus = [
        '{"path":"f","status":"M","staged":true,"untracked":false}',
        '{"path":"bucket/","status":"??","staged":false,"untracked":true}'
      ];
      const removedTab = makeTab('review:H:/repo:bucket/removed.txt', 'review', 'H', 'a1', '/repo', 'bucket/removed.txt');
      removedTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:bucket/removed.txt|U',
        makeTestGitReviewJson(10, 'H', '/repo', 'bucket/removed.txt', '', [], bucketStatus, false, 0, '')
      );
      store.sidePanelStore.openTabInScope(scopeKey, removedTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, 'bucket/removed.txt', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:bucket/removed.txt|U',
        makeTestGitReviewJson(11, 'H', '/repo', 'bucket/removed.txt', '--- /dev/null\n+++ b/bucket/removed.txt\n+obsolete-body\n', ['bucket/removed.txt'], bucketStatus, false, 11, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'bucket/removed.txt')).assertEqual('confirmed');

      // File deleted on disk & unrelated `f` staged -> native evicts `bucket/removed.txt` to rev 0:
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:bucket/removed.txt|U',
        makeTestGitReviewJson(12, 'H', '/repo', 'bucket/removed.txt', '', [], bucketStatus, false, 0, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'bucket/removed.txt')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/repo|bucket/removed.txt|U']));
      const whileRefetchingRemoved = store.loadScopedGitReview('H', '/repo', false, 'bucket/removed.txt', false, false);
      expect(whileRefetchingRemoved !== null && whileRefetchingRemoved.diff_files.length === 0).assertTrue();
      expect(whileRefetchingRemoved !== null && !whileRefetchingRemoved.raw_diff.includes('+obsolete-body')).assertTrue();

      // Daemon returns authentic 0-byte empty diff for deleted file:
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:bucket/removed.txt|U',
        makeTestGitReviewJson(13, 'H', '/repo', 'bucket/removed.txt', '', [], bucketStatus, false, 13, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'bucket/removed.txt')).assertEqual('confirmed');
      const confirmedEmptyRemoved = store.loadScopedGitReview('H', '/repo', false, 'bucket/removed.txt', false, false);
      expect(confirmedEmptyRemoved !== null && confirmedEmptyRemoved.diff_files.length === 0).assertTrue();
      expect(confirmedEmptyRemoved !== null && confirmedEmptyRemoved.raw_diff === '').assertTrue();

      // 4. R17-P1-2: Subdir `/repo/pkg` `f/U` confirmed `+working-sub`, then root `/repo` stages
      //    `pkg/f` -> same-frame `git_changed` for `/repo` AND `/repo/pkg` invalidates `/repo/pkg`
      //    to `unverified` with 0 diff files and 0 redundant fetches!
      const pkgPreStatus = ['{"path":"pkg/f","status":"M","staged":false,"untracked":false}'];
      const pkgPostStatus = ['{"path":"pkg/f","status":"M","staged":true,"untracked":false}'];
      const pkgTab = makeTab('review:H:/repo/pkg:f', 'review', 'H', 'a1', '/repo/pkg', 'f');
      pkgTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo/pkg|file:f|U',
        makeTestGitReviewJson(20, 'H', '/repo/pkg', 'f', '', [], pkgPreStatus, false, 0, 'pkg')
      );
      store.sidePanelStore.openTabInScope(scopeKey, pkgTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo/pkg', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo/pkg|file:f|U',
        makeTestGitReviewJson(21, 'H', '/repo/pkg', 'f', 'diff --git a/pkg/f b/pkg/f\n+working-sub\n', ['pkg/f'], pkgPreStatus, false, 21, 'pkg')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo/pkg', false, 'f')).assertEqual('confirmed');

      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/repo/pkg|file:f|U',
        makeTestGitReviewJson(22, 'H', '/repo/pkg', 'f', '', [], pkgPostStatus, false, 0, 'pkg')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo/pkg', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      const pkgAfterRootStage = store.loadScopedGitReview('H', '/repo/pkg', false, 'f', false, false);
      expect(pkgAfterRootStage !== null && pkgAfterRootStage.diff_files.length === 0).assertTrue();
      expect(pkgAfterRootStage !== null && !pkgAfterRootStage.raw_diff.includes('+working-sub')).assertTrue();

      // 5. Negative control (R16-P1-1): Clean root `README.md/U` when only `other/README.md` is
      //    dirty MUST settle to `unverified` with 0 fetches!
      const otherReadmeStatus = ['{"path":"other/README.md","status":"M","staged":false,"untracked":false}'];
      const readmeTab = makeTab('review:H:/repo:README.md', 'review', 'H', 'a1', '/repo', 'README.md');
      readmeTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:README.md|U',
        makeTestGitReviewJson(30, 'H', '/repo', 'README.md', '', [], otherReadmeStatus, false, 0, '')
      );
      store.sidePanelStore.openTabInScope(scopeKey, readmeTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, 'README.md', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:README.md|U',
        makeTestGitReviewJson(31, 'H', '/repo', 'README.md', 'diff --git a/README.md b/README.md\n+old\n', ['README.md'], otherReadmeStatus, false, 31, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'README.md')).assertEqual('confirmed');
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:README.md|U',
        makeTestGitReviewJson(32, 'H', '/repo', 'README.md', '', [], otherReadmeStatus, false, 0, '')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, 'README.md')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // 6. R17-P1-5: Legacy daemon omitting `repo_prefix` on subdir `/legacy/pkg` `g/U` conservatively
      //    re-fetches when unstaged entries exist and confirms `+g-working`, while hidden Review
      //    incurs 0 sync `gitReviewJson` reads and 0 fetches!
      const legacyStatus = ['{"path":"pkg/g","status":"M","staged":false,"untracked":false}'];
      const legTab = makeTab('review:H:/legacy/pkg:g', 'review', 'H', 'a1', '/legacy/pkg', 'g');
      legTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/legacy/pkg|file:g|U',
        makeTestGitReviewJson(40, 'H', '/legacy/pkg', 'g', '', [], legacyStatus, false, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, legTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/legacy/pkg', false, 'g', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/legacy/pkg|file:g|U',
        makeTestGitReviewJson(41, 'H', '/legacy/pkg', 'g', 'diff --git a/pkg/g b/pkg/g\n+g-old\n', ['pkg/g'], legacyStatus, false, 41)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/legacy/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/legacy/pkg', false, 'g')).assertEqual('confirmed');

      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/legacy/pkg|file:g|U',
        makeTestGitReviewJson(42, 'H', '/legacy/pkg', 'g', '', [], legacyStatus, false, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/legacy/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/legacy/pkg', false, 'g')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/legacy/pkg|g|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/legacy/pkg|file:g|U',
        makeTestGitReviewJson(43, 'H', '/legacy/pkg', 'g', 'diff --git a/pkg/g b/pkg/g\n+g-working\n', ['pkg/g'], legacyStatus, false, 43)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/legacy/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/legacy/pkg', false, 'g')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/legacy/pkg', false, 'g', false, false)?.raw_diff).assertContain('+g-working');

      // Hidden Review panel -> 0 sync reads and 0 fetches even when native evicts cache!
      store.sidePanelStore.dismissPanel();
      const readsWhenHidden = bridge.gitReviewJsonCalls;
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/legacy/pkg|file:g|U',
        makeTestGitReviewJson(44, 'H', '/legacy/pkg', 'g', '', [], legacyStatus, false, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/legacy/pkg"}');
      store.eventScheduler.flushNow();
      expect(bridge.gitReviewJsonCalls).assertEqual(readsWhenHidden);
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      store.sidePanelStore.dispose();
    });

    it('round18InFlightAdoptionLoadingRecoveryAndAliasIsolation', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      // 1. R18-N1: Visible confirmed `f/S` is evicted while Rust core already re-dispatched a
      //    single live post-mutation request (`diff_in_flight: true`). ArkTS MUST adopt the live
      //    in-flight request (`modeState === 'loading'`, 0 duplicate `fetchGitDiff` calls!) and
      //    confirm immediately when that single reply arrives.
      const stagedFStatus = ['{"path":"f","status":"A","staged":true,"untracked":false}'];
      const fsTab = makeTab('review:H:/repo:f', 'review', 'H', 'a1', '/repo', 'f');
      fsTab.gitStaged = true;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(1, 'H', '/repo', 'f', '', [], stagedFStatus, true, 0, '', false, '/private/repo')
      );
      store.sidePanelStore.openTabInScope(scopeKey, fsTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', true, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(2, 'H', '/repo', 'f', 'diff --git a/f b/f\n+index-only\n', ['f'], stagedFStatus, true, 2, '', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('confirmed');

      // Mutation evicts `f/S` (`diff_revision: 0`) AND Rust already re-dispatched (`diff_in_flight: true`):
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(3, 'H', '/repo', 'f', '', [], stagedFStatus, true, 0, '', true, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('loading');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      // Single native reply arrives -> confirms immediately without waiting for a second request!
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|file:f|S',
        makeTestGitReviewJson(4, 'H', '/repo', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], stagedFStatus, true, 4, '', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', true, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', true, 'f', false, false)?.raw_diff).assertContain('+worktree-only');

      // 2. R18-P1-3: Visible `loading` whole-repo (`focusPath = null`) subdir `/repo/pkg` U tab
      //    receives `git_changed` with `diff_revision: 0, diff_in_flight: false` and remaining
      //    `pkg/g (U)` -> ArkTS triggers ONE exact recovery fetch for `('H', '/repo/pkg', null, U)`
      //    and confirms `+g-working`! When `diff_in_flight: true`, 0 duplicate fetches!
      const pkgDirtyStatus = [
        '{"path":"f","status":"M","staged":true,"untracked":false}',
        '{"path":"pkg/g","status":"M","staged":false,"untracked":false}'
      ];
      const pkgWholeTab = makeTab('review:H:/repo/pkg', 'review', 'H', 'a1', '/repo/pkg', null);
      pkgWholeTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo/pkg|all:|U',
        makeTestGitReviewJson(10, 'H', '/repo/pkg', null, '', [], pkgDirtyStatus, false, 0, 'pkg', true, '/private/repo')
      );
      store.sidePanelStore.openTabInScope(scopeKey, pkgWholeTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo/pkg', false, null, true, true);
      expect(store.getScopedGitDiffModeState('H', '/repo/pkg', false, null)).assertEqual('loading');

      // While `diff_in_flight: true`, `git_changed` does NOT double-fetch:
      bridge.fetchGitDiffCalls = [];
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo/pkg', false, null)).assertEqual('loading');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // If `diff_in_flight: false` while `pkg/g (U)` still exists, ArkTS recovers with ONE exact fetch:
      bridge.gitReviewJsonBySlot.set(
        'H|/repo/pkg|all:|U',
        makeTestGitReviewJson(11, 'H', '/repo/pkg', null, '', [], pkgDirtyStatus, false, 0, 'pkg', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo/pkg', false, null)).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/repo/pkg|__all__|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/repo/pkg|all:|U',
        makeTestGitReviewJson(12, 'H', '/repo/pkg', null, 'diff --git a/pkg/g b/pkg/g\n+g-working\n', ['pkg/g'], pkgDirtyStatus, false, 12, 'pkg', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo/pkg"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo/pkg', false, null)).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo/pkg', false, null, false, false)?.raw_diff).assertContain('+g-working');

      // 3. R18-P1-2: Symlink alias `/tmp/repo_link` confirmed `f/U` (`+worktree-only`), then real
      //    dir stages `f` -> alias receives `git_changed` with `diff_revision: 0`, `repo_prefix: ""`,
      //    `repo_root: "/private/repo"`, and staged-only `f` -> immediately settles to `unverified`,
      //    0 diff files, and 0 redundant fetches!
      const linkPreStatus = ['{"path":"f","status":"M","staged":false,"untracked":false}'];
      const linkTab = makeTab('review:H:/tmp/repo_link:f', 'review', 'H', 'a1', '/tmp/repo_link', 'f');
      linkTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo_link|file:f|U',
        makeTestGitReviewJson(20, 'H', '/tmp/repo_link', 'f', '', [], linkPreStatus, false, 0, '', false, '/private/repo')
      );
      store.sidePanelStore.openTabInScope(scopeKey, linkTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/repo_link', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo_link|file:f|U',
        makeTestGitReviewJson(21, 'H', '/tmp/repo_link', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], linkPreStatus, false, 21, '', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/repo_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/repo_link', false, 'f')).assertEqual('confirmed');

      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/repo_link|file:f|U',
        makeTestGitReviewJson(22, 'H', '/tmp/repo_link', 'f', '', [], stagedFStatus, false, 0, '', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/repo_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/repo_link', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      const linkAfter = store.loadScopedGitReview('H', '/tmp/repo_link', false, 'f', false, false);
      expect(linkAfter !== null && linkAfter.diff_files.length === 0).assertTrue();
      expect(linkAfter !== null && !linkAfter.raw_diff.includes('+worktree-only')).assertTrue();

      store.sidePanelStore.dispose();
    });

    it('round19RepoRebindRpcErrorRecoveryAndSinglePostMutationRefresh', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      // 1. R19-P1-1: Same `cwd` (`/tmp/rebind_link`) changes physical `repo_root` from A -> B.
      //    Fixed-target same-root refresh keeps confirmed `+worktree-only` with 0 fetches, while
      //    switching to B (native evicts diff to `diff_revision: 0`) automatically fetches B once
      //    and confirms `+second-working`!
      const dirtyFStatus = ['{"path":"f","status":"M","staged":false,"untracked":false}'];
      const rebindTab = makeTab('review:H:/tmp/rebind_link:f', 'review', 'H', 'a1', '/tmp/rebind_link', 'f');
      rebindTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/rebind_link|file:f|U',
        makeTestGitReviewJson(1, 'H', '/tmp/rebind_link', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/repo_a')
      );
      store.sidePanelStore.openTabInScope(scopeKey, rebindTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/rebind_link', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/rebind_link|file:f|U',
        makeTestGitReviewJson(2, 'H', '/tmp/rebind_link', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], dirtyFStatus, false, 2, '', false, '/private/repo_a')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/rebind_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/rebind_link', false, 'f')).assertEqual('confirmed');

      // Same physical root status refresh -> keeps confirmed cache & emits 0 fetches:
      bridge.fetchGitDiffCalls = [];
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/rebind_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/rebind_link', false, 'f')).assertEqual('confirmed');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.loadScopedGitReview('H', '/tmp/rebind_link', false, 'f', false, false)?.raw_diff).assertContain('+worktree-only');

      // Symlink repointed to Repo B -> native core evicts Repo A cache (`diff_revision: 0`):
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/rebind_link|file:f|U',
        makeTestGitReviewJson(3, 'H', '/tmp/rebind_link', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/repo_b')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/rebind_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/rebind_link', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/rebind_link|f|U']));
      const whileFetchingB = store.loadScopedGitReview('H', '/tmp/rebind_link', false, 'f', false, false);
      expect(whileFetchingB !== null && whileFetchingB.diff_files.length === 0).assertTrue();
      // Repo B diff arrives -> immediately confirms `+second-working`:
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/rebind_link|file:f|U',
        makeTestGitReviewJson(4, 'H', '/tmp/rebind_link', 'f', 'diff --git a/f b/f\n+second-working\n', ['f'], dirtyFStatus, false, 4, '', false, '/private/repo_b')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/rebind_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/rebind_link', false, 'f')).assertEqual('confirmed');
      const confirmedB = store.loadScopedGitReview('H', '/tmp/rebind_link', false, 'f', false, false);
      expect(confirmedB?.raw_diff).assertContain('+second-working');
      expect(confirmedB !== null && !confirmedB.raw_diff.includes('+worktree-only')).assertTrue();

      // 2. R19-P1-4 (Case 1: status === null during RpcError, then dir restored + GitStatusResponse):
      //    `RpcError` settles `diff_in_flight: false` and bumps `last_diff_error_revision` ->
      //    transitions `loading -> unverified` with 0 blind retries while missing. Once real dirty
      //    status arrives (`status_revision` advances 0 -> 1), performs strictly ONE bounded
      //    recovery fetch and confirms `+worktree-only`!
      const errTab = makeTab('review:H:/tmp/err_repo:f', 'review', 'H', 'a1', '/tmp/err_repo', 'f');
      errTab.gitStaged = false;
      const nullStatusNotInFlightErr1 = '{"revision":10,"host_id":"H","cwd":"/tmp/err_repo","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":false,"status_revision":0,"cwd_identity_epoch":0,"last_diff_error_revision":1,"raw_diff":"","diff_files":[]}';
      const nullStatusInFlight = '{"revision":10,"host_id":"H","cwd":"/tmp/err_repo","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":true,"status_revision":0,"cwd_identity_epoch":0,"last_diff_error_revision":0,"raw_diff":"","diff_files":[]}';
      bridge.gitReviewJsonBySlot.set('H|/tmp/err_repo|file:f|U', nullStatusInFlight);
      store.sidePanelStore.openTabInScope(scopeKey, errTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/err_repo', false, 'f', true, true);
      expect(store.getScopedGitDiffModeState('H', '/tmp/err_repo', false, 'f')).assertEqual('loading');

      // RpcError arrives while directory is missing (`status: null`, `diff_in_flight: false`, `last_diff_error_revision: 1`):
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set('H|/tmp/err_repo|file:f|U', nullStatusNotInFlightErr1);
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/err_repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/err_repo', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Directory restored & real dirty GitStatusResponse arrives (`status_revision: 1`) -> ONE recovery fetch!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/err_repo|file:f|U',
        makeTestGitReviewJson(11, 'H', '/tmp/err_repo', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/err_repo', 1, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/err_repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/err_repo', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/err_repo|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/err_repo|file:f|U',
        makeTestGitReviewJson(12, 'H', '/tmp/err_repo', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], dirtyFStatus, false, 12, '', false, '/private/err_repo', 1, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/err_repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/err_repo', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/err_repo', false, 'f', false, false)?.raw_diff).assertContain('+worktree-only');

      // Bounded failure guard: if the one-shot recovery ALSO fails with RpcError (`last_diff_error_revision: 2`)
      // while `status_revision` does NOT advance, MUST stop at `unverified` with 0 infinite retries!
      const errTab2 = makeTab('review:H:/tmp/err_repo2:f', 'review', 'H', 'a1', '/tmp/err_repo2', 'f');
      errTab2.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/err_repo2|file:f|U',
        '{"revision":20,"host_id":"H","cwd":"/tmp/err_repo2","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":true,"status_revision":0,"cwd_identity_epoch":0,"last_diff_error_revision":0,"raw_diff":"","diff_files":[]}'
      );
      store.sidePanelStore.openTabInScope(scopeKey, errTab2, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/err_repo2', false, 'f', true, true);
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/err_repo2|file:f|U',
        '{"revision":21,"host_id":"H","cwd":"/tmp/err_repo2","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":false,"status_revision":0,"cwd_identity_epoch":0,"last_diff_error_revision":1,"raw_diff":"","diff_files":[]}'
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/err_repo2"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/err_repo2', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      // Dirty status (`status_revision: 1`) triggers the single recovery fetch:
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/err_repo2|file:f|U',
        makeTestGitReviewJson(22, 'H', '/tmp/err_repo2', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/err_repo2', 1, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/err_repo2"}');
      store.eventScheduler.flushNow();
      expect(bridge.fetchGitDiffCalls.length).assertEqual(1);
      // That recovery fetch ALSO errors (`last_diff_error_revision: 2`, `status_revision` still 1) -> stops!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/err_repo2|file:f|U',
        makeTestGitReviewJson(23, 'H', '/tmp/err_repo2', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/err_repo2', 1, 0, 2)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/err_repo2"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/err_repo2', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(1);
      // Another event with the SAME `status_revision: 1` (e.g. sibling diff) -> STILL 0 retries!
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/err_repo2"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/err_repo2', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(1);

      // 3. R19-N2: Whole-repo U Stage reuses native core's single post-mutation `diff_in_flight`
      //    request (0 duplicate fetches from BOTH `gitStageScopedPaths` AND component post-write
      //    `loadScopedGitReview(..., true, false, true)`), confirming `+g-working` on the first
      //    reply, while explicit manual refresh (`reuseInFlight = false`) STILL fetches!
      const wholePostStatus = [
        '{"path":"f","status":"M","staged":true,"untracked":false}',
        '{"path":"g","status":"M","staged":false,"untracked":false}'
      ];
      const wholeTab = makeTab('review:H:/repo', 'review', 'H', 'a1', '/repo', null);
      wholeTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|all:|U',
        makeTestGitReviewJson(30, 'H', '/repo', null, 'diff --git a/f b/f\n+f\ndiff --git a/g b/g\n+g-working\n', ['f', 'g'], wholePostStatus, false, 30, '', false, '/private/repo')
      );
      store.sidePanelStore.openTabInScope(scopeKey, wholeTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/repo', false, null, true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|all:|U',
        makeTestGitReviewJson(31, 'H', '/repo', null, 'diff --git a/f b/f\n+f\ndiff --git a/g b/g\n+g-working\n', ['f', 'g'], wholePostStatus, false, 31, '', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, null)).assertEqual('confirmed');

      // Native core completes Stage `f` and already dispatched the single post-mutation whole-repo
      // U request (`diff_revision: 0, diff_in_flight: true`):
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|all:|U',
        makeTestGitReviewJson(32, 'H', '/repo', null, '', [], wholePostStatus, false, 0, '', true, '/private/repo')
      );
      const stagedOk = await store.gitStageScopedPaths('H', '/repo', ['f'], false);
      expect(stagedOk).assertTrue();
      // Store callback MUST adopt core's live in-flight request (0 duplicate fetches!):
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.getScopedGitDiffModeState('H', '/repo', false, null)).assertEqual('loading');
      // Component `runScopedWrite` callback calls `loadScopedGitReview(..., true, false, true)` ->
      // STILL 0 duplicate fetches!
      store.loadScopedGitReview('H', '/repo', false, null, true, false, true);
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.getScopedGitDiffModeState('H', '/repo', false, null)).assertEqual('loading');
      // Single core reply arrives -> confirms `+g-working` immediately on first reply!
      bridge.gitReviewJsonBySlot.set(
        'H|/repo|all:|U',
        makeTestGitReviewJson(33, 'H', '/repo', null, 'diff --git a/g b/g\n+g-working\n', ['g'], wholePostStatus, false, 33, '', false, '/private/repo')
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/repo"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/repo', false, null)).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/repo', false, null, false, false)?.raw_diff).assertContain('+g-working');

      // Explicit manual refresh (`reuseInFlight = false`) MUST still force a fresh fetch!
      bridge.fetchGitDiffCalls = [];
      store.loadScopedGitReview('H', '/repo', false, null, true, true, false);
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/repo|__all__|U']));

      store.sidePanelStore.dispose();
    });

    it('round20UnverifiedRebindSubdirPrefixSwitchTimeoutNonZeroErrorAndSiblingDiffIsolation', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      const stagedFStatus = ['{"path":"f","status":"M","staged":true,"untracked":false}'];
      const dirtyFStatus = ['{"path":"f","status":"M","staged":false,"untracked":false}'];

      // 1. R20-P1-1: Already-cleared (`unverified`, `diff_revision: 0`) link in clean Repo A
      //    repoints to dirty Repo B (`cwd_identity_epoch: 0 -> 1`): Rule A MUST automatically
      //    fetch `f/U` once and confirm `+second-working`! Switching to clean Repo C (`epoch: 1 -> 2`)
      //    MUST settle to `unverified` with 0 fetches!
      const linkTab = makeTab('review:H:/tmp/clean_to_dirty:f', 'review', 'H', 'a1', '/tmp/clean_to_dirty', 'f');
      linkTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/clean_to_dirty|file:f|U',
        makeTestGitReviewJson(1, 'H', '/tmp/clean_to_dirty', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/a', 1, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, linkTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/clean_to_dirty', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/clean_to_dirty|file:f|U',
        makeTestGitReviewJson(2, 'H', '/tmp/clean_to_dirty', 'f', 'diff --git a/f b/f\n+first-a\n', ['f'], dirtyFStatus, false, 2, '', false, '/private/a', 1, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/clean_to_dirty"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/clean_to_dirty', false, 'f')).assertEqual('confirmed');

      // Stage in A -> settles to `unverified`, 0 rows, 0 fetches:
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/clean_to_dirty|file:f|U',
        makeTestGitReviewJson(3, 'H', '/tmp/clean_to_dirty', 'f', '', [], stagedFStatus, false, 0, '', false, '/private/a', 2, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/clean_to_dirty"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/clean_to_dirty', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Now repoint link to dirty Repo B (`cwd_identity_epoch: 1`):
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/clean_to_dirty|file:f|U',
        makeTestGitReviewJson(4, 'H', '/tmp/clean_to_dirty', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/b', 3, 1, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/clean_to_dirty"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/clean_to_dirty', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/clean_to_dirty|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/clean_to_dirty|file:f|U',
        makeTestGitReviewJson(5, 'H', '/tmp/clean_to_dirty', 'f', 'diff --git a/f b/f\n+second-working\n', ['f'], dirtyFStatus, false, 5, '', false, '/private/b', 3, 1, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/clean_to_dirty"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/clean_to_dirty', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/clean_to_dirty', false, 'f', false, false)?.raw_diff).assertContain('+second-working');

      // Repoint link to CLEAN Repo C (`cwd_identity_epoch: 2`, empty files) -> 0 fetches & unverified!
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/clean_to_dirty|file:f|U',
        makeTestGitReviewJson(6, 'H', '/tmp/clean_to_dirty', 'f', '', [], [], false, 0, '', false, '/private/c', 4, 2, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/clean_to_dirty"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/clean_to_dirty', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // 2. R20-P1-2: Same worktree (`repo_root: "/private/mono"`), subdir link switches from
      //    `repo_prefix: "one"` to `repo_prefix: "two"` (`cwd_identity_epoch: 0 -> 1`) -> drops
      //    `+one-working`, fetches once, confirms `+two-working`!
      const oneTwoStatus = [
        '{"path":"one/f","status":"M","staged":false,"untracked":false}',
        '{"path":"two/f","status":"M","staged":false,"untracked":false}'
      ];
      const subTab = makeTab('review:H:/tmp/sub_link:f', 'review', 'H', 'a1', '/tmp/sub_link', 'f');
      subTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sub_link|file:f|U',
        makeTestGitReviewJson(10, 'H', '/tmp/sub_link', 'f', '', [], oneTwoStatus, false, 0, 'one', false, '/private/mono', 1, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, subTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/sub_link', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sub_link|file:f|U',
        makeTestGitReviewJson(11, 'H', '/tmp/sub_link', 'f', 'diff --git a/one/f b/one/f\n+one-working\n', ['one/f'], oneTwoStatus, false, 11, 'one', false, '/private/mono', 1, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sub_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sub_link', false, 'f')).assertEqual('confirmed');

      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sub_link|file:f|U',
        makeTestGitReviewJson(12, 'H', '/tmp/sub_link', 'f', '', [], oneTwoStatus, false, 0, 'two', false, '/private/mono', 2, 1, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sub_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sub_link', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/sub_link|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sub_link|file:f|U',
        makeTestGitReviewJson(13, 'H', '/tmp/sub_link', 'f', 'diff --git a/two/f b/two/f\n+two-working\n', ['two/f'], oneTwoStatus, false, 13, 'two', false, '/private/mono', 2, 1, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sub_link"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sub_link', false, 'f')).assertEqual('confirmed');
      const subConfirmed = store.loadScopedGitReview('H', '/tmp/sub_link', false, 'f', false, false);
      expect(subConfirmed?.raw_diff).assertContain('+two-working');
      expect(subConfirmed !== null && !subConfirmed.raw_diff.includes('+one-working')).assertTrue();

      // 3. R20-P1-5 ②: Previously confirmed old body (`diff_revision: 20`, `+old-working`), then
      //    user clicks manual Refresh which fails with `RpcError` (`last_diff_error_revision: 1`,
      //    while old `diff_revision` stays `20 > 0`!). Once dir restores & `status_revision`
      //    advances (`1 -> 2`), Rule C MUST recover once and confirm `+new-working` (`diff_revision: 21`)!
      const oldRevTab = makeTab('review:H:/tmp/old_rev_err:f', 'review', 'H', 'a1', '/tmp/old_rev_err', 'f');
      oldRevTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/old_rev_err|file:f|U',
        makeTestGitReviewJson(20, 'H', '/tmp/old_rev_err', 'f', 'diff --git a/f b/f\n+old-working\n', ['f'], dirtyFStatus, false, 20, '', false, '/private/old_rev_err', 1, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, oldRevTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/old_rev_err', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/old_rev_err|file:f|U',
        makeTestGitReviewJson(21, 'H', '/tmp/old_rev_err', 'f', 'diff --git a/f b/f\n+old-working\n', ['f'], dirtyFStatus, false, 21, '', false, '/private/old_rev_err', 1, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/old_rev_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/old_rev_err', false, 'f')).assertEqual('confirmed');

      // Manual Refresh while dir temporarily unavailable -> RpcError (`last_diff_error_revision: 1`)
      // while cached `diff_revision` is still `21` and `status_revision` is still `1`:
      store.loadScopedGitReview('H', '/tmp/old_rev_err', false, 'f', true, false, false);
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/old_rev_err|file:f|U',
        makeTestGitReviewJson(22, 'H', '/tmp/old_rev_err', 'f', 'diff --git a/f b/f\n+old-working\n', ['f'], dirtyFStatus, false, 21, '', false, '/private/old_rev_err', 1, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/old_rev_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/old_rev_err', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Directory restored & fresh `GitStatusResponse` advances `status_revision` (`1 -> 2`):
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/old_rev_err|file:f|U',
        makeTestGitReviewJson(23, 'H', '/tmp/old_rev_err', 'f', 'diff --git a/f b/f\n+old-working\n', ['f'], dirtyFStatus, false, 21, '', false, '/private/old_rev_err', 2, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/old_rev_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/old_rev_err', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/old_rev_err|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/old_rev_err|file:f|U',
        makeTestGitReviewJson(24, 'H', '/tmp/old_rev_err', 'f', 'diff --git a/f b/f\n+new-working\n', ['f'], dirtyFStatus, false, 24, '', false, '/private/old_rev_err', 2, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/old_rev_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/old_rev_err', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/old_rev_err', false, 'f', false, false)?.raw_diff).assertContain('+new-working');

      // 4. R20-P1-5 ① + R20-P1-6: `RpcError` arrives AFTER UI already became `unverified` (e.g.
      //    post-1.5s timeout), THEN sibling `g/U` diff fires `git_changed` with unchanged
      //    `status_revision: 1` (MUST NOT consume recovery budget — 0 fetches!), THEN real
      //    `GitStatusResponse` advances `status_revision: 1 -> 2` -> fires strictly ONE recovery
      //    fetch and confirms `+worktree-only`!
      const sibTab = makeTab('review:H:/tmp/sib_err:f', 'review', 'H', 'a1', '/tmp/sib_err', 'f');
      sibTab.gitStaged = false;
      const fgStatus = [
        '{"path":"f","status":"M","staged":false,"untracked":false}',
        '{"path":"g","status":"M","staged":false,"untracked":false}'
      ];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sib_err|file:f|U',
        makeTestGitReviewJson(30, 'H', '/tmp/sib_err', 'f', '', [], fgStatus, false, 0, '', true, '/private/sib_err', 1, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, sibTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/sib_err', false, 'f', true, false);
      // Simulate UI already timed out (`modeState === 'unverified'`) BEFORE `RpcError` arrives
      // by settling via a clean non-error snapshot first (or waiting >1.5s), then delivering late `RpcError`:
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sib_err|file:f|U',
        '{"revision":31,"host_id":"H","cwd":"/tmp/sib_err","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":false,"status_revision":1,"cwd_identity_epoch":0,"last_diff_error_revision":0,"raw_diff":"","diff_files":[]}'
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sib_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sib_err', false, 'f')).assertEqual('unverified');

      // Late `RpcError` arrives AFTER already `unverified` (`last_diff_error_revision: 1`, `status_revision: 1`):
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sib_err|file:f|U',
        makeTestGitReviewJson(32, 'H', '/tmp/sib_err', 'f', '', [], fgStatus, false, 0, '', false, '/private/sib_err', 1, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sib_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sib_err', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Sibling `g/U` diff arrives on `/tmp/sib_err` -> `git_changed` fires, `status_revision`
      // STAYS `1` -> MUST emit 0 premature fetches and preserve `pendingErrorRecovery`!
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sib_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sib_err', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Directory restores & real `GitStatusResponse` advances `status_revision` (`1 -> 2`):
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sib_err|file:f|U',
        makeTestGitReviewJson(33, 'H', '/tmp/sib_err', 'f', '', [], fgStatus, false, 0, '', false, '/private/sib_err', 2, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sib_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sib_err', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/sib_err|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/sib_err|file:f|U',
        makeTestGitReviewJson(34, 'H', '/tmp/sib_err', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], fgStatus, false, 34, '', false, '/private/sib_err', 2, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/sib_err"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/sib_err', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/sib_err', false, 'f', false, false)?.raw_diff).assertContain('+worktree-only');

      store.sidePanelStore.dispose();
    });

    it('round21DiffFirstRebindPendingStatusAndSameFrameCoalescedErrorRecovery', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      const stagedFStatus = ['{"path":"f","status":"M","staged":true,"untracked":false}'];
      const dirtyFStatus = ['{"path":"f","status":"M","staged":false,"untracked":false}'];

      // 1. R21-P1-1: Routine whole-repo diff reveals A -> B switch (`cwd_identity_epoch: 1`,
      //    `status: null`, `status_identity_epoch: 0`) BEFORE Repo B's `GitStatusResponse` arrives!
      //    Visible `f/U` (already cleared in Repo A) MUST stay pending (`unverified`, 0 premature
      //    fetches) and then fire strictly ONE fetch once Repo B's matching status (`status_identity_epoch: 1`)
      //    arrives—confirming `+second-working`! And for a CLEAN Repo B (`files: []`), 0 fetches!
      const diffFirstTab = makeTab('review:H:/tmp/r21_diff_first:f', 'review', 'H', 'a1', '/tmp/r21_diff_first', 'f');
      diffFirstTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_diff_first|file:f|U',
        makeTestGitReviewJson(1, 'H', '/tmp/r21_diff_first', 'f', '', [], stagedFStatus, false, 0, '', false, '/private/a', 1, 0, 0, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, diffFirstTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r21_diff_first', false, 'f', true, true);
      bridge.fetchGitDiffCalls = [];
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_diff_first"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_diff_first', false, 'f')).assertEqual('unverified');

      // Whole-repo diff arrives first: `cwd_identity_epoch: 1`, `status: null`, `status_identity_epoch: 0`!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_diff_first|file:f|U',
        '{"revision":2,"host_id":"H","cwd":"/tmp/r21_diff_first","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":false,"status_revision":1,"status_identity_epoch":0,"cwd_identity_epoch":1,"last_diff_error_revision":0,"last_diff_error_status_revision":0,"raw_diff":"","diff_files":[]}'
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_diff_first"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_diff_first', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Now Repo B's real dirty status arrives (`status_identity_epoch: 1 === cwd_identity_epoch: 1`)!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_diff_first|file:f|U',
        makeTestGitReviewJson(3, 'H', '/tmp/r21_diff_first', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/b', 2, 1, 0, 1, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_diff_first"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_diff_first', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/r21_diff_first|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_diff_first|file:f|U',
        makeTestGitReviewJson(4, 'H', '/tmp/r21_diff_first', 'f', 'diff --git a/f b/f\n+second-working\n', ['f'], dirtyFStatus, false, 4, '', false, '/private/b', 2, 1, 0, 1, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_diff_first"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_diff_first', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/r21_diff_first', false, 'f', false, false)?.raw_diff).assertContain('+second-working');

      // Clean Repo C control after diff-first switch (`epoch: 2`, `status: null` -> `status: []`): 0 fetches!
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_diff_first|file:f|U',
        '{"revision":5,"host_id":"H","cwd":"/tmp/r21_diff_first","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":false,"status_revision":2,"status_identity_epoch":0,"cwd_identity_epoch":2,"last_diff_error_revision":0,"last_diff_error_status_revision":0,"raw_diff":"","diff_files":[]}'
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_diff_first"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_diff_first', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_diff_first|file:f|U',
        makeTestGitReviewJson(6, 'H', '/tmp/r21_diff_first', 'f', '', [], [], false, 0, '', false, '/private/c', 3, 2, 0, 2, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_diff_first"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_diff_first', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // 2. R21-P1-3: Error (`last_diff_error_revision: 1`, `last_diff_error_status_revision: 1`)
      //    AND post-error dirty status (`status_revision: 2`) coalesced in the SAME frame flush ->
      //    MUST immediately trigger strictly ONE recovery fetch in that same flush and confirm
      //    `+worktree-only`!
      const coalTab = makeTab('review:H:/tmp/r21_coal:f', 'review', 'H', 'a1', '/tmp/r21_coal', 'f');
      coalTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_coal|file:f|U',
        makeTestGitReviewJson(10, 'H', '/tmp/r21_coal', 'f', '', [], dirtyFStatus, false, 0, '', true, '/private/r21_coal', 1, 0, 0, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, coalTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r21_coal', false, 'f', true, false);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_coal', false, 'f')).assertEqual('loading');

      // Both RpcError (at status_rev=1) AND restored dirty GitStatusResponse (status_rev=2) land
      // BEFORE the single frame flush:
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_coal|file:f|U',
        makeTestGitReviewJson(11, 'H', '/tmp/r21_coal', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/r21_coal', 2, 0, 1, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_coal"}');
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_coal"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_coal', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/r21_coal|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_coal|file:f|U',
        makeTestGitReviewJson(12, 'H', '/tmp/r21_coal', 'f', 'diff --git a/f b/f\n+worktree-only\n', ['f'], dirtyFStatus, false, 12, '', false, '/private/r21_coal', 2, 0, 1, 0, 1)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_coal"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_coal', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/r21_coal', false, 'f', false, false)?.raw_diff).assertContain('+worktree-only');

      // 3. R21-P1-3 Reverse Control: Status (`status_revision: 2`) arrives FIRST, THEN Error
      //    (`last_diff_error_revision: 1`, `last_diff_error_status_revision: 2`) settles in the
      //    SAME frame -> `2 > 2` is FALSE -> MUST settle to `unverified` with 0 premature fetches,
      //    and recover strictly once `status_revision` advances to `3`!
      const revTab = makeTab('review:H:/tmp/r21_coal_rev:f', 'review', 'H', 'a1', '/tmp/r21_coal_rev', 'f');
      revTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_coal_rev|file:f|U',
        makeTestGitReviewJson(20, 'H', '/tmp/r21_coal_rev', 'f', '', [], dirtyFStatus, false, 0, '', true, '/private/r21_coal_rev', 1, 0, 0, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, revTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r21_coal_rev', false, 'f', true, false);
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_coal_rev|file:f|U',
        makeTestGitReviewJson(21, 'H', '/tmp/r21_coal_rev', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/r21_coal_rev', 2, 0, 1, 0, 2)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_coal_rev"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_coal_rev', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      // Now next status (`status_revision: 3 > 2`) triggers the single recovery fetch!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r21_coal_rev|file:f|U',
        makeTestGitReviewJson(22, 'H', '/tmp/r21_coal_rev', 'f', '', [], dirtyFStatus, false, 0, '', false, '/private/r21_coal_rev', 3, 0, 1, 0, 2)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r21_coal_rev"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r21_coal_rev', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/r21_coal_rev|f|U']));

      store.sidePanelStore.dispose();
    });

    it('round22DelayedMutationRecheckRecoveryAndDiffFirstSplitOrders', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      const stagedOnlyStatus = ['{"path":"f","status":"M","staged":true,"untracked":false}'];
      const bothModesStatus = [
        '{"path":"f","status":"M","staged":true,"untracked":false}',
        '{"path":"f","status":"M","staged":false,"untracked":false}'
      ];
      const dirtyFStatus = ['{"path":"f","status":"M","staged":false,"untracked":false}'];

      // 1. R22-P1-2 (Rule D): Visible `f/U` already confirmed `+edited-after-stage` (`[true, false]`)
      //    when a delayed Stage ack temporarily overwrites status with `[true]` (`diff_revision: 0`),
      //    demoting to `unverified`. When the coalesced post-completion `GetGitStatus` returns
      //    `[true, false]` with a strictly newer `status_revision`, Rule D MUST dispatch strictly
      //    ONE targeted `f/U` fetch and re-confirm `+edited-after-stage` (while same-revision and
      //    clean-status events emit 0 fetches).
      const delayedTab = makeTab('review:H:/tmp/r22_stage:f', 'review', 'H', 'a1', '/tmp/r22_stage', 'f');
      delayedTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_stage|file:f|U',
        makeTestGitReviewJson(1, 'H', '/tmp/r22_stage', 'f', '', [], bothModesStatus, false, 0, '', false, '/private/r22_stage', 2, 0, 0, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, delayedTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r22_stage', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_stage|file:f|U',
        makeTestGitReviewJson(2, 'H', '/tmp/r22_stage', 'f', 'diff --git a/f b/f\n+edited-after-stage\n', ['f'], bothModesStatus, false, 2, '', false, '/private/r22_stage', 2, 0, 0, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_stage"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_stage', false, 'f')).assertEqual('confirmed');

      // Delayed Stage ack arrives (`[true]`, `diff_revision: 0`, `status_revision: 3`) -> demotes to `unverified`, 0 fetches:
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_stage|file:f|U',
        makeTestGitReviewJson(3, 'H', '/tmp/r22_stage', 'f', '', [], stagedOnlyStatus, false, 0, '', false, '/private/r22_stage', 3, 0, 0, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_stage"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_stage', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Negative controls: same status_revision (e.g. sibling diff) AND clean status_revision=4 -> 0 fetches:
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_stage"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_stage', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_stage|file:f|U',
        makeTestGitReviewJson(4, 'H', '/tmp/r22_stage', 'f', '', [], stagedOnlyStatus, false, 0, '', false, '/private/r22_stage', 4, 0, 0, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_stage"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_stage', false, 'f')).assertEqual('unverified');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      // Coalesced post-completion `GetGitStatus` returns `[true, false]` (`status_revision: 5 > 4`) -> ONE exact fetch!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_stage|file:f|U',
        makeTestGitReviewJson(5, 'H', '/tmp/r22_stage', 'f', '', [], bothModesStatus, false, 0, '', false, '/private/r22_stage', 5, 0, 0, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_stage"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_stage', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/r22_stage|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_stage|file:f|U',
        makeTestGitReviewJson(6, 'H', '/tmp/r22_stage', 'f', 'diff --git a/f b/f\n+edited-after-stage\n', ['f'], bothModesStatus, false, 3, '', false, '/private/r22_stage', 5, 0, 0, 0, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_stage"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_stage', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/r22_stage', false, 'f', false, false)?.raw_diff).assertContain('+edited-after-stage');

      // 2. R22-P1-1: Single-refresh split where B's exact `f/U` diff arrives (`cwd_identity_epoch: 1`,
      //    `status: null`, `diff_revision: 10`) BEFORE B's automatic verification `GetGitStatus` returns ->
      //    confirms `+second-working` immediately AND stays confirmed with 0 extra diff fetches once B's
      //    matching status (`status_identity_epoch: 1`) lands!
      const splitTab = makeTab('review:H:/tmp/r22_split:f', 'review', 'H', 'a1', '/tmp/r22_split', 'f');
      splitTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_split|file:f|U',
        makeTestGitReviewJson(10, 'H', '/tmp/r22_split', 'f', '', [], stagedOnlyStatus, false, 0, '', false, '/private/r22_a', 1, 0, 0, 0, 0)
      );
      store.sidePanelStore.openTabInScope(scopeKey, splitTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r22_split', false, 'f', true, true);
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_split|file:f|U',
        '{"revision":11,"host_id":"H","cwd":"/tmp/r22_split","status":null,"selected_path":"f","staged":false,"diff_revision":10,"diff_in_flight":false,"status_revision":1,"status_identity_epoch":0,"cwd_identity_epoch":1,"last_diff_error_revision":0,"last_diff_error_status_revision":0,"raw_diff":"diff --git a/f b/f\\n+second-working\\n","diff_files":[{"path":"f","status":"M","additions":1,"deletions":0,"truncated":false,"lines":[{"kind":"added","old_line":null,"new_line":2,"text":"+second-working"}]}]}'
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_split"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_split', false, 'f')).assertEqual('confirmed');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.loadScopedGitReview('H', '/tmp/r22_split', false, 'f', false, false)?.raw_diff).assertContain('+second-working');

      // Now B's automatic verification `GetGitStatus` lands (`status_identity_epoch: 1`) -> stays confirmed, 0 extra fetches!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r22_split|file:f|U',
        makeTestGitReviewJson(12, 'H', '/tmp/r22_split', 'f', 'diff --git a/f b/f\n+second-working\n', ['f'], dirtyFStatus, false, 10, '', false, '/private/r22_b', 2, 1, 0, 1, 0)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r22_split"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r22_split', false, 'f')).assertEqual('confirmed');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.loadScopedGitReview('H', '/tmp/r22_split', false, 'f', false, false)?.status?.repo_root).assertEqual('/private/r22_b');

      store.sidePanelStore.dispose();
    });

    it('round23ReverifyWaitPreservesConfirmedSixLinesAndColdCleanBZeroFetch', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/repo')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);

      const bothModesStatus = [
        '{"path":"f","status":"M","staged":true,"untracked":false}',
        '{"path":"f","status":"M","staged":false,"untracked":false}'
      ];

      // 1. R22-P1-2 Three-timepoint preservation (T1 / T2 >1.6s wait / T3):
      //    T1: Active `f/U` confirmed with `+edited-after-stage`, `diff_revision: 2`, `[true, false]`.
      const rvTab = makeTab('review:H:/tmp/r23_rv:f', 'review', 'H', 'a1', '/tmp/r23_rv', 'f');
      rvTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_rv|file:f|U',
        makeTestGitReviewJson(1, 'H', '/tmp/r23_rv', 'f', '', [], bothModesStatus, false, 0, '', false, '/private/r23_rv', 2, 0, 0, 0, 0, false)
      );
      store.sidePanelStore.openTabInScope(scopeKey, rvTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r23_rv', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_rv|file:f|U',
        makeTestGitReviewJson(2, 'H', '/tmp/r23_rv', 'f', 'diff --git a/f b/f\n+edited-after-stage\n', ['f'], bothModesStatus, false, 2, '', false, '/private/r23_rv', 2, 0, 0, 0, 0, false)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r23_rv"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r23_rv', false, 'f')).assertEqual('confirmed');
      const t1View = store.loadScopedGitReview('H', '/tmp/r23_rv', false, 'f', false, false);
      expect(t1View?.diff_revision).assertEqual(2);
      expect(t1View?.raw_diff).assertContain('+edited-after-stage');

      //    T2: Delayed Stage ack settles as `status_reverify_pending: true`, keeping `[true, false]`
      //    and `diff_revision: 2` intact! Both `git_changed` AND post-write `loadScopedGitReview(..., true, false, true)`
      //    MUST preserve `confirmed`, `diff_revision: 2`, and `+edited-after-stage` across >1.6s wait with 0 fetches!
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_rv|file:f|U',
        makeTestGitReviewJson(3, 'H', '/tmp/r23_rv', 'f', 'diff --git a/f b/f\n+edited-after-stage\n', ['f'], bothModesStatus, false, 2, '', false, '/private/r23_rv', 2, 0, 0, 0, 0, true)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r23_rv"}');
      store.eventScheduler.flushNow();
      const t2PostWrite = store.loadScopedGitReview('H', '/tmp/r23_rv', false, 'f', true, false, true);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r23_rv', false, 'f')).assertEqual('confirmed');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(t2PostWrite?.diff_revision).assertEqual(2);
      expect(t2PostWrite?.status_reverify_pending).assertEqual(true);
      expect(t2PostWrite?.raw_diff).assertContain('+edited-after-stage');

      // Hold re-verify response for >1.5s -> MUST STILL remain `confirmed`, rev 2, `+edited-after-stage`!
      await new Promise<void>((resolve: () => void): void => {
        setTimeout(resolve, 1650);
      });
      expect(store.getScopedGitDiffModeState('H', '/tmp/r23_rv', false, 'f')).assertEqual('confirmed');
      const t2AfterWait = store.loadScopedGitReview('H', '/tmp/r23_rv', false, 'f', false, false);
      expect(t2AfterWait?.diff_revision).assertEqual(2);
      expect(t2AfterWait?.raw_diff).assertContain('+edited-after-stage');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      //    T3: Authoritative re-verify lands (`status_reverify_pending: false`, `status_revision: 3`,
      //    `diff_revision: 0`, `[true, false]`) -> dispatches strictly ONE fresh `f/U` fetch & confirms rev 3!
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_rv|file:f|U',
        makeTestGitReviewJson(4, 'H', '/tmp/r23_rv', 'f', '', [], bothModesStatus, false, 0, '', false, '/private/r23_rv', 3, 0, 0, 0, 0, false)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r23_rv"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r23_rv', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/r23_rv|f|U']));
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_rv|file:f|U',
        makeTestGitReviewJson(5, 'H', '/tmp/r23_rv', 'f', 'diff --git a/f b/f\n+edited-after-stage\n', ['f'], bothModesStatus, false, 3, '', false, '/private/r23_rv', 3, 0, 0, 0, 0, false)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r23_rv"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r23_rv', false, 'f')).assertEqual('confirmed');
      const t3View = store.loadScopedGitReview('H', '/tmp/r23_rv', false, 'f', false, false);
      expect(t3View?.diff_revision).assertEqual(3);
      expect(t3View?.status_reverify_pending).assertEqual(false);
      expect(t3View?.raw_diff).assertContain('+edited-after-stage');

      // 2. R22-P1-1 Cold-start "dirty A / clean B" control: B's clean diff arrives first
      //    (`cwd_identity_epoch: 1`, `status: null`, `diff_revision: 2`, empty diff), followed by
      //    B's clean verification status (`status_identity_epoch: 1`, `files: []`) -> 0 extra f/U fetches!
      const coldCleanTab = makeTab('review:H:/tmp/r23_cold_clean:f', 'review', 'H', 'a1', '/tmp/r23_cold_clean', 'f');
      coldCleanTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_cold_clean|file:f|U',
        '{"revision":10,"host_id":"H","cwd":"/tmp/r23_cold_clean","status":null,"selected_path":"f","staged":false,"diff_revision":0,"diff_in_flight":true,"status_revision":0,"status_identity_epoch":0,"status_reverify_pending":false,"cwd_identity_epoch":0,"last_diff_error_revision":0,"last_diff_error_status_revision":0,"raw_diff":"","diff_files":[]}'
      );
      store.sidePanelStore.openTabInScope(scopeKey, coldCleanTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r23_cold_clean', false, 'f', true, true);
      bridge.fetchGitDiffCalls = [];
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_cold_clean|file:f|U',
        '{"revision":11,"host_id":"H","cwd":"/tmp/r23_cold_clean","status":null,"selected_path":"f","staged":false,"diff_revision":2,"diff_in_flight":false,"status_revision":0,"status_identity_epoch":0,"status_reverify_pending":false,"cwd_identity_epoch":1,"last_diff_error_revision":0,"last_diff_error_status_revision":0,"raw_diff":"","diff_files":[]}'
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r23_cold_clean"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r23_cold_clean', false, 'f')).assertEqual('confirmed');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      bridge.gitReviewJsonBySlot.set(
        'H|/tmp/r23_cold_clean|file:f|U',
        makeTestGitReviewJson(12, 'H', '/tmp/r23_cold_clean', 'f', '', [], [], false, 2, '', false, '/private/r23_b_clean', 1, 1, 0, 1, 0, false)
      );
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r23_cold_clean"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r23_cold_clean', false, 'f')).assertEqual('confirmed');
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.loadScopedGitReview('H', '/tmp/r23_cold_clean', false, 'f', false, false)?.diff_files.length).assertEqual(0);

      store.sidePanelStore.dispose();
    });

    it('round24ReverifyWaitWithConcurrentSameSlotDiffInFlightPreservesConfirmedSixLines', 0, async (): Promise<void> => {
      const bridge = new FakeOrbitCoreBridgeForStore();
      bridge.fleetJson = `{"revision":1,"hosts":[${makeTestHostCardJson('H')}],"projects":[],"total_pending_approvals":0}`;
      bridge.detailJsonByHost.set('H', makeTestHostDetailJson('H', [makeTestAgentCardJson('H', 'a1', '/tmp/r24_rv')]));

      const store = new OrbitHarmonyStore();
      store.windowWidthVp = 1280;
      store.initializeWithTestBridge(bridge);
      await flushAsyncTurns(8);

      const scopeKey = store.sidePanelStore.scopeKeyFor('H', 'a1');
      store.sidePanelStore.switchScopeByKey(scopeKey);
      expect(store.selectedHostId).assertEqual('H');
      expect(store.activeCwd).assertEqual('/tmp/r24_rv');

      const bothModesStatus = [
        '{"path":"f","status":"M","staged":true,"untracked":false}',
        '{"path":"f","status":"M","staged":false,"untracked":false}'
      ];
      const sixLineDiff =
        'diff --git a/f b/f\n--- a/f\n+++ b/f\n@@ -1,2 +1,3 @@\n line1\n+edited-after-stage\n';
      const makeSixLineReviewJson = (
        revision: number,
        diffRevision: number,
        diffInFlight: boolean,
        statusRevision: number,
        statusReverifyPending: boolean,
        hasBody: boolean
      ): string => {
        const linesJson = hasBody
          ? [
              '{"kind":"meta","old_line":null,"new_line":null,"content":"diff --git a/f b/f"}',
              '{"kind":"meta","old_line":null,"new_line":null,"content":"--- a/f"}',
              '{"kind":"meta","old_line":null,"new_line":null,"content":"+++ b/f"}',
              '{"kind":"hunk_header","old_line":null,"new_line":null,"content":"@@ -1,2 +1,3 @@"}',
              '{"kind":"context","old_line":1,"new_line":1,"content":"line1"}',
              '{"kind":"added","old_line":null,"new_line":2,"content":"+edited-after-stage"}'
            ].join(',')
          : '';
        const diffFilesJson = hasBody
          ? `[{"path":"f","additions":1,"deletions":0,"lines":[${linesJson}]}]`
          : '[]';
        return `{"revision":${revision},"host_id":"H","cwd":"/tmp/r24_rv","status":{"cwd":"/tmp/r24_rv","is_git_repo":true,"branch":"main","upstream":null,"ahead":0,"behind":0,"files":[${bothModesStatus.join(',')}],"repo_prefix":"","repo_root":"/private/r24_rv"},"selected_path":"f","staged":false,"diff_revision":${diffRevision},"diff_in_flight":${diffInFlight ? 'true' : 'false'},"status_revision":${statusRevision},"status_identity_epoch":0,"status_reverify_pending":${statusReverifyPending ? 'true' : 'false'},"cwd_identity_epoch":0,"last_diff_error_revision":0,"last_diff_error_status_revision":0,"raw_diff":${JSON.stringify(hasBody ? sixLineDiff : '')},"diff_files":${diffFilesJson}}`;
      };

      // T0 -> T1: Active `f/U` review confirms `+edited-after-stage` (6 lines, diff_revision=2, [true, false]).
      const rvTab = makeTab('review:H:/tmp/r24_rv:f', 'review', 'H', 'a1', '/tmp/r24_rv', 'f');
      rvTab.gitStaged = false;
      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(1, 0, false, 2, false, false));
      store.sidePanelStore.openTabInScope(scopeKey, rvTab, { source: 'user', activate: true });
      store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', true, true);
      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(2, 2, false, 3, false, true));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r24_rv"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      const t1View = store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', false, false);
      expect(t1View?.diff_revision).assertEqual(2);
      expect(t1View?.diff_files.length).assertEqual(1);
      expect(t1View?.diff_files[0].lines.length).assertEqual(6);
      expect(t1View?.raw_diff).assertContain('+edited-after-stage');

      // Concurrent same-slot production read via `selectGitFileFilter('f')` puts a `GetGitDiff` in flight
      // on `(H, /tmp/r24_rv, 'f', U)` while the delayed Stage ack is still withheld:
      bridge.fetchGitDiffCalls = [];
      store.selectGitFileFilter('f');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/r24_rv|f|U']));
      bridge.fetchGitDiffCalls = [];

      // Delayed Stage ack settles with `status_reverify_pending=true` while the concurrent same-slot
      // `selectGitFileFilter('f')` diff is STILL in flight (`diff_in_flight=true`, `diff_revision=2`, 6 lines):
      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(3, 2, true, 3, true, true));
      const stagedOk = await store.gitStageScopedPaths('H', '/tmp/r24_rv', ['f'], false);
      expect(stagedOk).assertTrue();
      // Mirror `OrbitScopedGitTabView.runScopedWrite` -> `reloadScopedGit(true, false, true)`:
      const t2PostWrite = store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', true, false, true);
      // Timepoint 1 (next frame): MUST stay `confirmed` with rev 2, 6 lines visible, 0 duplicate fetches!
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      expect(t2PostWrite?.diff_revision).assertEqual(2);
      expect(t2PostWrite?.diff_in_flight).assertEqual(true);
      expect(t2PostWrite?.status_reverify_pending).assertEqual(true);
      expect(t2PostWrite?.diff_files.length).assertEqual(1);
      expect(t2PostWrite?.diff_files[0].lines.length).assertEqual(6);
      expect(t2PostWrite?.raw_diff).assertContain('+edited-after-stage');

      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r24_rv"}');
      store.eventScheduler.flushNow();
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      const t2NextFrame = store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', false, false);
      expect(t2NextFrame?.diff_revision).assertEqual(2);
      expect(t2NextFrame?.diff_files.length).assertEqual(1);
      expect(t2NextFrame?.diff_files[0].lines.length).assertEqual(6);
      expect(t2NextFrame?.raw_diff).assertContain('+edited-after-stage');

      // Timepoint 2 (+1,600 ms): MUST STILL remain `confirmed` with rev 2 & 6 lines (NO timeout wipe to `unverified`)!
      await new Promise<void>((resolve: () => void): void => {
        setTimeout(resolve, 1650);
      });
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      const t2After1600Ms = store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', false, false);
      expect(t2After1600Ms?.diff_revision).assertEqual(2);
      expect(t2After1600Ms?.status_reverify_pending).assertEqual(true);
      expect(t2After1600Ms?.diff_files.length).assertEqual(1);
      expect(t2After1600Ms?.diff_files[0].lines.length).assertEqual(6);
      expect(t2After1600Ms?.raw_diff).assertContain('+edited-after-stage');

      // Timepoint 3a (reverify arrives first while core drains & re-dispatches the in-flight diff once):
      // `status_reverify_pending=false`, `status_revision=4`, `diff_revision=0`, `diff_in_flight=true`
      // -> ArkTS adopts core's re-dispatched in-flight request (0 duplicate fetches) and confirms rev 3 (6 lines)!
      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(4, 0, true, 4, false, false));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r24_rv"}');
      store.eventScheduler.flushNow();
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('loading');

      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(5, 3, false, 4, false, true));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r24_rv"}');
      store.eventScheduler.flushNow();
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      const t3View = store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', false, false);
      expect(t3View?.diff_revision).assertEqual(3);
      expect(t3View?.status_reverify_pending).assertEqual(false);
      expect(t3View?.diff_files.length).assertEqual(1);
      expect(t3View?.diff_files[0].lines.length).assertEqual(6);
      expect(t3View?.raw_diff).assertContain('+edited-after-stage');

      // Timepoint 3b (alternate order: concurrent in-flight diff arrives BEFORE reverify completes ->
      // smoothly advances confirmed rev 3 -> 4 during reverify wait, then reverify evicts & dispatches 1 exact fetch -> rev 5):
      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(6, 3, true, 4, true, true));
      const t3bPostWrite = store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', true, false, true);
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      expect(t3bPostWrite?.diff_revision).assertEqual(3);
      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(7, 4, false, 4, true, true));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r24_rv"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      expect(store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', false, false)?.diff_revision).assertEqual(4);
      expect(bridge.fetchGitDiffCalls.length).assertEqual(0);

      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(8, 0, false, 5, false, false));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r24_rv"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('loading');
      expect(JSON.stringify(bridge.fetchGitDiffCalls)).assertEqual(JSON.stringify(['H|/tmp/r24_rv|f|U']));
      bridge.gitReviewJsonBySlot.set('H|/tmp/r24_rv|file:f|U', makeSixLineReviewJson(9, 5, false, 5, false, true));
      bridge.emitCoreEvent('{"kind":"git_changed","host_id":"H","cwd":"/tmp/r24_rv"}');
      store.eventScheduler.flushNow();
      expect(store.getScopedGitDiffModeState('H', '/tmp/r24_rv', false, 'f')).assertEqual('confirmed');
      const t3bFinal = store.loadScopedGitReview('H', '/tmp/r24_rv', false, 'f', false, false);
      expect(t3bFinal?.diff_revision).assertEqual(5);
      expect(t3bFinal?.status_reverify_pending).assertEqual(false);
      expect(t3bFinal?.diff_files[0].lines.length).assertEqual(6);

      store.sidePanelStore.dispose();
    });
  });
}
