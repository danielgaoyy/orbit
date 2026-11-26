import { describe, expect, it } from '@ohos/hypium';
import {
  AgentCardDto,
  BriefingDecisionDto,
  BriefingGoalSnapshotDto,
  BriefingMissionDto,
  BriefingWaitItemDto,
  BriefingWorkerDto,
  DiffFileDto,
  FleetSidebarDto,
  GoalRunRecordDto,
  HostCardDto,
  HostDetailDto,
  PersonalAgentActivityEntryDto,
  PersonalAgentBriefingDto,
  PersonalRouterConfigDto,
  ProviderModelCatalogDto,
  PushPreferencesDto,
  SessionSubagentEntryDto,
  SidebarConversationDto,
  SidebarProjectGroupDto,
  TimelineItemDto,
  cwdPathsEquivalent
} from '../../../main/ets/model/OrbitTypes';
import {
  parseOrbitOpenDeepLink,
  resolveDeepLinkPostNavigationAction,
  validateDeepLinkAgentInDetail
} from '../../../main/ets/model/OrbitDeepLink';
import {
  OrbitFleetBridge,
  OrbitPersonalAgentBridge
} from '../../../main/ets/model/OrbitCoreBridge';
import {
  OrbitEventFlushHandler,
  OrbitEventFrameScheduler,
  isStageScopeVisibleForEvent
} from '../../../main/ets/model/OrbitEventFrameScheduler';
import {
  NotificationVisibilityContext,
  OrbitNotificationEngine,
  buildLocalNotificationCopy,
  defaultPushPreferences
} from '../../../main/ets/model/OrbitNotificationEngine';
import {
  ORBIT_MAX_VISIBLE_THREADS_PER_PROJECT,
  OrbitFleetStore,
  OrbitSidebarRow,
  buildOrbitSidebarRows
} from '../../../main/ets/model/stores/OrbitFleetStore';
import {
  OrbitPersonalAgentStore,
  OrbitPersonalAgentVisitStorage
} from '../../../main/ets/model/stores/OrbitPersonalAgentStore';
import { OrbitTimelineItemModel } from '../../../main/ets/model/viewmodels/OrbitStageModels';
import {
  mapCrewSubagentStatus,
  resolveAgentFallbackLabel,
  resolveCompactAgentModelLabel,
  resolveCrewSubagentDepth,
  resolveCrewSubagentHostId
} from '../../../main/ets/model/OrbitAgentDisplayModel';
import {
  ORBIT_SUBAGENT_AVATAR_IDS,
  ORBIT_SUBAGENT_AVATAR_ROSTER,
  OrbitAvatarSeedKeyed,
  OrbitSubagentAvatarAsset,
  assignSubagentCharacters,
  getSubagentCharacterById,
  hashSubagentAvatarSeed,
  resolveOrbitSubagentAvatar,
  resolveSubagentCharacter
} from '../../../main/ets/model/OrbitSubagentAvatarModel';
import {
  QueuedDecisionCard,
  buildDecisionFocusTarget,
  findFocusedDecisionQueueIndex
} from '../../../main/ets/model/OrbitDecisionFocusModel';
import {
  gitTargetPathsEqual,
  isGitCollabResponseCurrent,
  normalizeGitTargetPath,
  resolveDirectoryBrowserCurrentPath,
  resolveDirectoryBrowserParentPath,
  selectDisplayedDiffFiles,
  shouldApplyGitDiffForTarget
} from '../../../main/ets/model/OrbitDiffInspectorModel';
import {
  BatchApprovalHostOutcome,
  CompactTimeTemplates,
  EN_COMPACT_TIME_TEMPLATES,
  FrozenBatchApprovalTarget,
  aggregateBatchApprovalOutcomes,
  classifyBatchApprovalFeedback,
  collectAncestorsWithActiveDescendants,
  computeSinceLastVisitSummary,
  formatCompactTimeWithTemplates,
  intersectFrozenBatchTargets,
  mergePersonalAgentSetupConfig,
  normalizePersonalRouterConfig,
  paginateInProgressWorkers,
  parseBatchApprovalHostResponse,
  partitionBriefingWorkers,
  reconcileBriefingPresentationOnResize,
  resolveCountdownTimeSpec,
  resolveEffectiveBriefingWorkerStatus,
  resolvePaIdentityArrivalPresentation,
  resolvePaSessionSelectPresentation,
  resolveRelativeTimeSpec,
  selectConfirmedArchivableWorkers,
  snapshotBatchApprovalTargets
} from '../../../main/ets/components/personal-agent/PersonalAgentBriefingModel';
import { resolveTopBarDisplayModel } from '../../../main/ets/components/shell/TopBarDisplayModel';

const EXPECTED_DESKTOP_AVATAR_IDS: string[] = [
  'red-fox',
  'shiba-inu',
  'orange-tabby',
  'giant-panda',
  'sea-otter',
  'barn-owl',
  'emperor-penguin',
  'capybara',
  'red-panda',
  'holland-lop',
  'corgi',
  'sika-deer',
  'koala',
  'raccoon',
  'arctic-fox',
  'russian-blue',
  'husky',
  'cream-alpaca',
  'european-hedgehog',
  'red-squirrel',
  'harbor-seal',
  'snowy-owl',
  'mallard-duck',
  'gray-chinchilla'
];

// Independent ground-truth key -> character ID vectors generated directly from
// `apps/desktop/src/lib/subagent-avatar.ts` (Nit-3).
const DESKTOP_COLLIDING_TRUTH: string[][] = [
  ['00', 'giant-panda'],
  ['01', 'sea-otter'],
  ['02', 'barn-owl'],
  ['03', 'emperor-penguin'],
  ['04', 'capybara'],
  ['05', 'red-panda'],
  ['06', 'holland-lop'],
  ['07', 'corgi'],
  ['08', 'sika-deer'],
  ['09', 'koala'],
  ['10', 'raccoon'],
  ['11', 'arctic-fox'],
  ['12', 'russian-blue'],
  ['13', 'husky'],
  ['14', 'cream-alpaca'],
  ['15', 'european-hedgehog'],
  ['16', 'red-squirrel'],
  ['17', 'harbor-seal'],
  ['18', 'snowy-owl'],
  ['19', 'mallard-duck'],
  ['20', 'gray-chinchilla'],
  ['21', 'red-fox'],
  ['22', 'shiba-inu'],
  ['23', 'orange-tabby'],
  ['24', 'giant-panda']
];

const DESKTOP_SPECIALISTS_TRUTH: string[][] = [
  ['managed:local:subagent-0', 'raccoon'],
  ['managed:local:subagent-1', 'orange-tabby'],
  ['managed:local:subagent-2', 'russian-blue'],
  ['managed:local:subagent-3', 'sea-otter'],
  ['managed:local:subagent-4', 'cream-alpaca'],
  ['managed:local:subagent-5', 'corgi'],
  ['managed:local:subagent-6', 'snowy-owl'],
  ['managed:local:subagent-7', 'husky'],
  ['managed:local:subagent-8', 'gray-chinchilla'],
  ['managed:local:subagent-9', 'giant-panda'],
  ['managed:local:subagent-10', 'mallard-duck'],
  ['managed:local:subagent-11', 'holland-lop'],
  ['managed:local:subagent-12', 'red-fox'],
  ['managed:local:subagent-13', 'sika-deer'],
  ['managed:local:subagent-14', 'european-hedgehog'],
  ['managed:local:subagent-15', 'barn-owl'],
  ['managed:local:subagent-16', 'harbor-seal'],
  ['managed:local:subagent-17', 'capybara'],
  ['managed:local:subagent-18', 'arctic-fox'],
  ['managed:local:subagent-19', 'shiba-inu'],
  ['managed:local:subagent-20', 'emperor-penguin'],
  ['managed:local:subagent-21', 'red-squirrel'],
  ['managed:local:subagent-22', 'red-panda'],
  ['managed:local:subagent-23', 'koala']
];

interface KeyedSeedFixture extends OrbitAvatarSeedKeyed {
  readonly key: string;
}

class MemoryVisitStorage implements OrbitPersonalAgentVisitStorage {
  public readonly data: Map<string, number> = new Map<string, number>();

  getLastVisitMs(hostId: string, agentId: string): number {
    return this.data.get(`${hostId}::${agentId}`) ?? 0;
  }

  setLastVisitMs(hostId: string, agentId: string, tsMs: number): void {
    this.data.set(`${hostId}::${agentId}`, tsMs);
  }
}

class MockFlushHandler implements OrbitEventFlushHandler {
  public bgHostHits: string[] = [];
  public bgTimelineHits: string[] = [];

  getSelectedHostId(): string {
    return 'host-1';
  }

  isStageScopeVisible(_hostId: string, _agentId: string): boolean {
    return false;
  }

  isGitScopeVisible(_hostId: string, _cwd: string): boolean {
    return false;
  }

  isTerminalScopeVisible(_hostId: string, _slot: number): boolean {
    return false;
  }

  onFleetDirty(): void {}
  onBriefingDirty(): void {}
  onSelectedHostDirty(_hostId: string): void {}

  onBackgroundHostDirty(hostId: string): void {
    this.bgHostHits.push(hostId);
  }

  onVisibleTimelineDirty(_hostId: string, _agentId: string): void {}

  onBackgroundTimelineDirty(hostId: string, agentId: string): void {
    this.bgTimelineHits.push(`${hostId}::${agentId}`);
  }

  onVisibleGitDirty(_hostId: string, _cwd: string): void {}
  onBackgroundGitDirty(_hostId: string, _cwd: string): void {}
  onVisibleTerminalDirty(_hostId: string, _slot: number): void {}
  onResync(): void {}
}

class MockStaleRevisionFleetBridge implements OrbitFleetBridge {
  private readonly responses: Map<string, string[]> = new Map<string, string[]>();
  private readonly resolvers: Map<string, Array<(val: string) => void>> = new Map<string, Array<(val: string) => void>>();
  public hostDetailAsyncCalls: number = 0;

  enqueueResponse(hostId: string, json: string): void {
    const list = this.responses.get(hostId) ?? [];
    list.push(json);
    this.responses.set(hostId, list);
  }

  fleetOverviewJson(): string {
    return '{"revision":1,"hosts":[],"projects":[],"total_pending_approvals":0}';
  }

  fleetOverviewJsonAsync(): Promise<string> {
    return Promise.resolve(this.fleetOverviewJson());
  }

  hostDetailJson(_hostId: string): string {
    return 'null';
  }

  hostDetailJsonAsync(hostId: string): Promise<string> {
    this.hostDetailAsyncCalls += 1;
    const queued = this.responses.get(hostId);
    if (queued !== undefined && queued.length > 0) {
      const next = queued.shift()!;
      return Promise.resolve(next);
    }
    return new Promise<string>((resolve: (val: string) => void): void => {
      const waiters = this.resolvers.get(hostId) ?? [];
      waiters.push(resolve);
      this.resolvers.set(hostId, waiters);
    });
  }

  resolvePendingAt(hostId: string, idx: number, json: string): void {
    const waiters = this.resolvers.get(hostId);
    if (waiters !== undefined && idx >= 0 && idx < waiters.length) {
      waiters[idx](json);
    }
  }

  fleetSidebarByRequestJson(_reqJson: string): string {
    return '{"revision":1,"projects":[],"launchpad_projects":[],"needs_attention":[],"pinned_conversations":[]}';
  }

  fleetSidebarByRequestJsonAsync(_reqJson: string): Promise<string> {
    return Promise.resolve(this.fleetSidebarByRequestJson(''));
  }
}

class MockPersonalAgentBridge implements OrbitPersonalAgentBridge {
  public briefingCalls: number = 0;
  public activityCalls: number = 0;
  public failNextBriefing: boolean = false;
  public briefingPayload: string =
    '{"personal_agent":{"host_id":"host-1","agent_id":"pa-1","title":"Personal Agent","host_name":"Mac"},' +
    '"needs_you":[],"missions":[],"other_active":[],"waiting":[],"hosts":[],' +
    '"counts":{"needs_you":0,"running":0,"done_recent":0,"hosts_online":1,"hosts_total":1},' +
    '"since_last_visit":{"completed":0,"auto_handled":0}}';

  personalAgentBriefingJson(_primaryHostId: string | null): string {
    this.briefingCalls += 1;
    if (this.failNextBriefing) {
      this.failNextBriefing = false;
      throw new Error('bridge unavailable');
    }
    return this.briefingPayload;
  }

  listPersonalAgentActivityJson(_hostId: string, _limit: number, _sinceMs: number | null): Promise<string> {
    this.activityCalls += 1;
    return Promise.resolve('[]');
  }
}

function makeHostCard(hostId: string, name: string, connState: string = 'online_direct'): HostCardDto {
  return {
    host_id: hostId,
    display_name: name,
    hostname: name,
    custom_name: null,
    ssh_alias: null,
    route_mode: 'auto',
    remote_daemon_port: 6868,
    custom_direct_endpoint: null,
    via_ssh_tunnel: false,
    last_ping_ms: 8,
    daemon_version: null,
    os_arch: null,
    daemon_pubkey_b64: 'pk',
    direct_endpoints: [],
    relay_url: null,
    connection_state: connState,
    last_error: null,
    available_providers: ['claude_code', 'codex'],
    workspace_count: 1,
    agent_count: 1,
    pending_approval_count: 0,
    is_local: false
  };
}

function makeAgent(
  agentId: string,
  title: string,
  cwd: string,
  state: string = 'idle',
  isRunning: boolean = false,
  hasRunningSubagents: boolean = false
): AgentCardDto {
  return {
    agent_id: agentId,
    workspace_id: 'ws-1',
    cwd: cwd,
    title: title,
    provider: 'claude_code',
    provider_label: 'Claude',
    model: 'claude-sonnet-4-5',
    effort: 'medium',
    mode: 'accept_edits',
    status: { state: state },
    status_label: isRunning ? 'Running' : 'Idle',
    is_running: isRunning,
    has_running_subagents: hasRunningSubagents,
    is_awaiting_permission: state === 'awaiting_permission',
    parent_agent_id: null,
    latest_seq: 5,
    created_at_rfc3339: '2026-11-26T10:00:00Z',
    updated_at_rfc3339: '2026-11-26T10:05:00Z'
  };
}

function makeHostDetail(
  hostId: string,
  name: string,
  revision: number,
  agents: AgentCardDto[],
  goals: GoalRunRecordDto[] = [],
  connState: string = 'online_direct'
): HostDetailDto {
  return {
    revision: revision,
    host: makeHostCard(hostId, name, connState),
    discovered_projects: [],
    directory_listing: null,
    models_by_provider: {},
    workspaces: [],
    agents: agents,
    last_created_agent_id: null,
    quotas: [],
    provider_catalogs: [],
    terminals: [],
    active_terminal_id: null,
    active_terminal_slot: 1,
    goal_runs: goals
  };
}

function makeSidebarConv(hostId: string, agentId: string, title: string, cwd: string): SidebarConversationDto {
  return {
    host_id: hostId,
    agent: makeAgent(agentId, title, cwd),
    is_worktree: false,
    is_pinned: false,
    is_unread: false,
    seen_seq: 1
  };
}

function makeProjectGroup(
  id: string,
  hostId: string,
  name: string,
  cwd: string,
  isHome: boolean,
  conversations: SidebarConversationDto[]
): SidebarProjectGroupDto {
  return {
    id: id,
    host_id: hostId,
    name: name,
    primary_cwd: cwd,
    directories: [cwd],
    git_branch: isHome ? null : 'main',
    pinned_project_id: isHome ? null : `pin-${id}`,
    is_home_workspace: isHome,
    sort_weight_ms: 1700000000000,
    conversations: conversations
  };
}

function makeBriefingWorker(hostId: string, agentId: string, state: string): BriefingWorkerDto {
  return {
    host_id: hostId,
    host_name: 'Mac Studio',
    agent_id: agentId,
    title: `Worker ${agentId}`,
    provider: 'codex',
    state: state,
    step_preview: state === 'running' ? 'Running tests' : 'Completed',
    worktree_branch: null,
    updated_at_ms: 1700000005000
  };
}

function makeDecision(
  hostId: string,
  agentId: string,
  requestId: string,
  batchEligible: boolean = true,
  humanOnly: boolean = false
): BriefingDecisionDto {
  return {
    key: `dec_${hostId}_${agentId}_${requestId}`,
    kind: 'permission',
    host_id: hostId,
    host_name: 'Mac Studio',
    agent_id: agentId,
    agent_title: `Secret Session ${agentId}`,
    request_id: requestId,
    summary: `Run sensitive command for ${requestId}`,
    risk_level: 'low',
    human_only: humanOnly,
    batch_eligible: batchEligible,
    router_recommendation: null,
    dispatched_by_personal_agent: true,
    updated_at_ms: 1700000009000
  };
}

export default function orbitPr2Test(): void {
  describe('OrbitPr2SubagentAvatarParity', () => {
    it('preserves exact 24-character desktop roster order, ID lookup, and localized name resources', 0, () => {
      expect(ORBIT_SUBAGENT_AVATAR_ROSTER.length).assertEqual(24);
      expect(ORBIT_SUBAGENT_AVATAR_IDS.length).assertEqual(24);
      for (let i = 0; i < EXPECTED_DESKTOP_AVATAR_IDS.length; i += 1) {
        expect(ORBIT_SUBAGENT_AVATAR_IDS[i]).assertEqual(EXPECTED_DESKTOP_AVATAR_IDS[i]);
        expect(ORBIT_SUBAGENT_AVATAR_ROSTER[i].id).assertEqual(EXPECTED_DESKTOP_AVATAR_IDS[i]);
        expect(ORBIT_SUBAGENT_AVATAR_ROSTER[i].nameResource !== undefined).assertTrue();
        const found = getSubagentCharacterById(EXPECTED_DESKTOP_AVATAR_IDS[i]);
        expect(found !== null).assertTrue();
        expect(found?.id).assertEqual(EXPECTED_DESKTOP_AVATAR_IDS[i]);
      }
      expect(getSubagentCharacterById('') === null).assertTrue();
      expect(getSubagentCharacterById('fox') === null).assertTrue();
      expect(getSubagentCharacterById(null) === null).assertTrue();
      expect(getSubagentCharacterById(undefined) === null).assertTrue();
    });

    it('matches desktop FNV-1a hash and standalone seed vectors verbatim', 0, () => {
      expect(hashSubagentAvatarSeed('hello')).assertEqual(0x4f9f2cab);
      expect(resolveSubagentCharacter('hello').id).assertEqual('giant-panda');
      expect(resolveSubagentCharacter('  codebase-researcher  ').id).assertEqual('orange-tabby');
      expect(resolveSubagentCharacter('  ').id).assertEqual('emperor-penguin');
      expect(resolveOrbitSubagentAvatar('unknown-animal', 'hello').id).assertEqual('giant-panda');
      expect(resolveOrbitSubagentAvatar('red-fox', 'hello').id).assertEqual('red-fox');
    });

    it('matches independent desktop key->id ground truth for colliding and specialist rosters (Nit-3)', 0, () => {
      const collidingItems: KeyedSeedFixture[] = [];
      for (let i = 0; i < 25; i += 1) {
        collidingItems.push({ key: i < 10 ? `0${i}` : `${i}` });
      }
      const sameSeed = (_item: KeyedSeedFixture): string => ' hello ';
      const assignedC: Map<string, OrbitSubagentAvatarAsset> = assignSubagentCharacters(collidingItems, sameSeed);
      const assignedCRev = assignSubagentCharacters(collidingItems.slice().reverse(), sameSeed);
      expect(assignedC.size).assertEqual(25);
      for (let i = 0; i < DESKTOP_COLLIDING_TRUTH.length; i += 1) {
        const key = DESKTOP_COLLIDING_TRUTH[i][0];
        const expectedId = DESKTOP_COLLIDING_TRUTH[i][1];
        expect(assignedC.get(key)?.id).assertEqual(expectedId);
        expect(assignedCRev.get(key)?.id).assertEqual(expectedId);
      }

      const specItems: KeyedSeedFixture[] = [];
      for (let i = 0; i < 24; i += 1) {
        specItems.push({ key: `managed:local:subagent-${i}` });
      }
      const assignedS = assignSubagentCharacters(specItems);
      const assignedSRev = assignSubagentCharacters(specItems.slice().reverse());
      const assignedSBlank = assignSubagentCharacters(specItems, (): string => ' ');
      for (let i = 0; i < DESKTOP_SPECIALISTS_TRUTH.length; i += 1) {
        const key = DESKTOP_SPECIALISTS_TRUTH[i][0];
        const expectedId = DESKTOP_SPECIALISTS_TRUTH[i][1];
        expect(assignedS.get(key)?.id).assertEqual(expectedId);
        expect(assignedSRev.get(key)?.id).assertEqual(expectedId);
        expect(assignedSBlank.get(key)?.id).assertEqual(expectedId);
      }
      expect(assignSubagentCharacters([]).size).assertEqual(0);
    });
  });

  describe('OrbitPr2BatchApprovalAndArchiveSafety', () => {
    it('freezes confirmed batch approval targets across added, replaced, and canceled requests (P0-1)', 0, () => {
      const decA = makeDecision('h1', 'ag-a', 'req-a');
      const decB = makeDecision('h1', 'ag-b', 'req-b');
      const decA2 = makeDecision('h1', 'ag-a', 'req-a-v2');
      const decHuman = makeDecision('h1', 'ag-h', 'req-h', true, true);

      // Case 1: Added — user opened dialog seeing only [A]; B arrived before confirm.
      const frozen1: FrozenBatchApprovalTarget[] = snapshotBatchApprovalTargets([decA, decHuman]);
      expect(frozen1.length).assertEqual(1);
      const submittedAdded = intersectFrozenBatchTargets(frozen1, [decA, decB, decHuman]);
      const batch1 = submittedAdded.get('h1') ?? [];
      expect(batch1.length).assertEqual(1);
      expect(batch1[0].permission_request_id).assertEqual('req-a');

      // Case 2: Replaced — A(req-a) was replaced by A(req-a-v2) before confirm -> nothing submitted.
      const submittedReplaced = intersectFrozenBatchTargets(frozen1, [decA2]);
      expect(submittedReplaced.size).assertEqual(0);

      // Case 3: Canceled — user froze [A, B]; B was canceled before confirm -> only A submitted.
      const frozen3 = snapshotBatchApprovalTargets([decA, decB]);
      expect(frozen3.length).assertEqual(2);
      const submittedCanceled = intersectFrozenBatchTargets(frozen3, [decA]);
      const batch3 = submittedCanceled.get('h1') ?? [];
      expect(batch3.length).assertEqual(1);
      expect(batch3[0].permission_request_id).assertEqual('req-a');
    });

    it('parses server batch approval receipts accurately including all-skipped responses (P1-2)', 0, () => {
      const allSkipped = parseBatchApprovalHostResponse(
        'h1',
        '{"resolved_count":0,"skipped_count":2,"skipped_reasons":[{"agent_id":"ag-1","permission_request_id":"req-1","reason":"risk escalated"}]}'
      );
      expect(allSkipped.failed).assertFalse();
      expect(allSkipped.resolvedCount).assertEqual(0);
      expect(allSkipped.skippedCount).assertEqual(2);
      expect(allSkipped.firstSkipReason).assertEqual('risk escalated');

      const partial = parseBatchApprovalHostResponse(
        'h2',
        '{"resolved_count":1,"skipped_count":1,"skipped_reasons":[{"agent_id":"ag-2","permission_request_id":"req-2","reason":"expired"}]}'
      );
      const failedHost: BatchApprovalHostOutcome = {
        hostId: 'h3',
        resolvedCount: 0,
        skippedCount: 0,
        firstSkipReason: '',
        failed: true
      };
      const agg = aggregateBatchApprovalOutcomes([allSkipped, partial, failedHost]);
      expect(agg.resolvedCount).assertEqual(1);
      expect(agg.skippedCount).assertEqual(3);
      expect(agg.failedHostCount).assertEqual(1);
      expect(agg.firstSkipReason).assertEqual('risk escalated');
    });

    it('keeps provider-native running subagents unarchivable and matches desktop vectors (P1-1)', 0, () => {
      const workerRunning = makeBriefingWorker('host-1', 'w-native', 'running');
      const liveNativeSubagent = makeAgent('w-native', 'Native Parent', '/repo', 'idle', false, true);

      // Desktop vector 1: provider-native running subagent (no active managed child) -> running
      expect(resolveEffectiveBriefingWorkerStatus(workerRunning, liveNativeSubagent, false))
        .assertEqual('running');
      // Desktop vector 2: finished parent with active managed child -> completed
      expect(resolveEffectiveBriefingWorkerStatus(workerRunning, liveNativeSubagent, true))
        .assertEqual('completed');

      const doneWorker = makeBriefingWorker('host-1', 'w-done', 'done');
      const liveDone = makeAgent('w-done', 'Done Child', '/repo', 'idle', false, false);
      const mission: BriefingMissionDto = {
        key: 'm-native',
        root: workerRunning,
        workers: [doneWorker],
        host_names: ['Mac Studio'],
        done: 1,
        total: 2,
        state: 'running',
        updated_at_ms: 1700000005000
      };
      const details = new Map<string, HostDetailDto>();
      details.set('host-1', makeHostDetail('host-1', 'Mac Studio', 1, [liveNativeSubagent, liveDone]));

      const partitioned = partitionBriefingWorkers([mission], [], [], details);
      expect(partitioned.runningMissionWorkers.length).assertEqual(1);
      expect(partitioned.runningMissionWorkers[0].worker.agent_id).assertEqual('w-native');
      expect(partitioned.archivableCompletedWorkers.length).assertEqual(1);
      expect(partitioned.archivableCompletedWorkers[0].worker.agent_id).assertEqual('w-done');

      // Re-validation right before Archive RPC strips a worker if it gained a running subagent
      const briefing: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [],
        missions: [mission],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 0, running: 1, done_recent: 1, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const updatedDoneNowRunning = makeAgent('w-done', 'Done Child', '/repo', 'running', true, false);
      const latestDetails = new Map<string, HostDetailDto>();
      latestDetails.set('host-1', makeHostDetail('host-1', 'Mac Studio', 2, [liveNativeSubagent, updatedDoneNowRunning]));
      const verified = selectConfirmedArchivableWorkers(
        partitioned.archivableCompletedWorkers,
        briefing,
        latestDetails
      );
      expect(verified.length).assertEqual(0);

      // P1-1 cascade protection: idle parent with a running same-workspace managed child (no open_tab)
      // MUST remain excluded from archivableCompletedWorkers AND selectConfirmedArchivableWorkers so
      // daemon cascade ArchiveAgent never kills running managed descendants.
      const parentIdle = makeAgent('p-root', 'Parent', '/repo', 'idle', false, true);
      const managedChildRunning = makeAgent('c-worker', 'Worker', '/repo', 'running', true, false);
      managedChildRunning.parent_agent_id = 'p-root';
      const managedMission: BriefingMissionDto = {
        key: 'mission-cascade',
        root: makeBriefingWorker('host-1', 'p-root', 'running'),
        workers: [makeBriefingWorker('host-1', 'c-worker', 'running')],
        host_names: ['Mac Studio'],
        done: 0,
        total: 2,
        state: 'running',
        updated_at_ms: 1700000010000
      };
      const cascadeDetails = new Map<string, HostDetailDto>();
      cascadeDetails.set('host-1', makeHostDetail('host-1', 'Mac Studio', 3, [parentIdle, managedChildRunning]));
      const cascadePartition = partitionBriefingWorkers([managedMission], [], [], cascadeDetails);
      expect(cascadePartition.runningMissionWorkers.length).assertEqual(1);
      expect(cascadePartition.runningMissionWorkers[0].worker.agent_id).assertEqual('c-worker');
      expect(cascadePartition.completedWorkers.length).assertEqual(1);
      expect(cascadePartition.completedWorkers[0].worker.agent_id).assertEqual('p-root');
      expect(cascadePartition.completedWorkers[0].archivable).assertFalse();
      expect(cascadePartition.archivableCompletedWorkers.length).assertEqual(0);

      // Multi-hop grandchild running under idle child -> BOTH child and root are blocked from archiving.
      const grandParent = makeAgent('gp-1', 'Grandparent', '/repo', 'idle', false, false);
      const midChild = makeAgent('mc-1', 'Mid Child', '/repo', 'idle', false, false);
      midChild.parent_agent_id = 'gp-1';
      const grandChild = makeAgent('gc-1', 'Grandchild', '/repo', 'running', true, false);
      grandChild.parent_agent_id = 'mc-1';
      const deepDetails = new Map<string, HostDetailDto>();
      deepDetails.set('host-1', makeHostDetail('host-1', 'Mac Studio', 4, [grandParent, midChild, grandChild]));
      const blockedAncestors = collectAncestorsWithActiveDescendants(deepDetails, [], new Set<string>());
      expect(blockedAncestors.has('host-1::mc-1')).assertTrue();
      expect(blockedAncestors.has('host-1::gp-1')).assertTrue();

      // P1-1 (a): Newer Briefing marks leaf=running while cached HostDetail is stale (5s older, idle)
      // -> neither parent nor leaf may be archivable!
      const staleParent = makeAgent('r-parent', 'Remote Parent', '/repo', 'idle', false, false);
      staleParent.updated_at_rfc3339 = '2026-11-26T10:00:00Z';
      const staleLeaf = makeAgent('r-leaf', 'Remote Leaf', '/repo', 'idle', false, false);
      staleLeaf.parent_agent_id = 'r-parent';
      staleLeaf.updated_at_rfc3339 = '2026-11-26T10:00:00Z';
      const newerTsMs = Date.parse('2026-11-26T10:00:05Z');
      const staleMission: BriefingMissionDto = {
        key: 'mission-stale-host-detail',
        root: makeBriefingWorker('host-R', 'r-parent', 'done'),
        workers: [makeBriefingWorker('host-R', 'r-leaf', 'running')],
        host_names: ['Remote R'],
        done: 1,
        total: 2,
        state: 'running',
        updated_at_ms: newerTsMs
      };
      staleMission.root.updated_at_ms = newerTsMs;
      staleMission.workers[0].updated_at_ms = newerTsMs;
      const staleDetails = new Map<string, HostDetailDto>();
      staleDetails.set('host-R', makeHostDetail('host-R', 'Remote R', 1, [staleParent, staleLeaf]));
      const stalePartition = partitionBriefingWorkers([staleMission], [], [], staleDetails);
      expect(stalePartition.runningMissionWorkers.length).assertEqual(1);
      expect(stalePartition.runningMissionWorkers[0].worker.agent_id).assertEqual('r-leaf');
      expect(stalePartition.archivableCompletedWorkers.length).assertEqual(0);

      // P1-1 (b): Remote host R HostDetail NOT loaded yet, mission chain L/root -> R/middle(done) -> R/leaf(running)
      // -> conservatively exclude R/middle AND L/root from Archive done!
      const localRoot = makeAgent('l-root', 'Local Root', '/repo', 'idle', false, false);
      const unloadedRemoteMission: BriefingMissionDto = {
        key: 'mission-unloaded-remote',
        root: makeBriefingWorker('host-L', 'l-root', 'done'),
        workers: [
          makeBriefingWorker('host-R', 'r-middle', 'done'),
          makeBriefingWorker('host-R', 'r-leaf', 'running')
        ],
        host_names: ['Local L', 'Remote R'],
        done: 2,
        total: 3,
        state: 'running',
        updated_at_ms: newerTsMs
      };
      const onlyLocalDetails = new Map<string, HostDetailDto>();
      onlyLocalDetails.set('host-L', makeHostDetail('host-L', 'Local L', 1, [localRoot]));
      const unloadedPartition = partitionBriefingWorkers([unloadedRemoteMission], [], [], onlyLocalDetails);
      expect(unloadedPartition.runningMissionWorkers.length).assertEqual(1);
      expect(unloadedPartition.runningMissionWorkers[0].worker.agent_id).assertEqual('r-leaf');
      expect(unloadedPartition.archivableCompletedWorkers.length).assertEqual(0);

      // P1-1 (c): Remote host R HostDetail ALREADY caches `r-middle`, while newly started `r-leaf(running)`
      // is NOT yet in R's HostDetail -> `r-leaf.parent_agent_id` is unknown, so `r-middle` AND `l-root`
      // MUST remain excluded from Archive done (0 archive RPCs)!
      const cachedMiddle = makeAgent('r-middle', 'Remote Middle', '/repo', 'idle', false, false);
      cachedMiddle.parent_agent_id = 'l-root';
      cachedMiddle.parent_host_id = 'host-L';
      const partialRemoteDetails = new Map<string, HostDetailDto>();
      partialRemoteDetails.set('host-L', makeHostDetail('host-L', 'Local L', 1, [localRoot]));
      partialRemoteDetails.set('host-R', makeHostDetail('host-R', 'Remote R', 1, [cachedMiddle]));
      const partialRemotePartition = partitionBriefingWorkers(
        [unloadedRemoteMission],
        [],
        [],
        partialRemoteDetails
      );
      expect(partialRemotePartition.runningMissionWorkers.length).assertEqual(1);
      expect(partialRemotePartition.runningMissionWorkers[0].worker.agent_id).assertEqual('r-leaf');
      expect(partialRemotePartition.archivableCompletedWorkers.length).assertEqual(0);
      const partialRemoteBriefing: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [],
        missions: [unloadedRemoteMission],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 0, running: 1, done_recent: 2, hosts_online: 2, hosts_total: 2 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const partialConfirmed = selectConfirmedArchivableWorkers(
        partialRemotePartition.completedWorkers,
        partialRemoteBriefing,
        partialRemoteDetails
      );
      expect(partialConfirmed.length).assertEqual(0);

      // P1-1 (d): Real Rust core projection with armed timer on remote leaf (`middle=done`, `leaf=idle`,
      // `waiting=[timer:R:leaf]`) while R's HostDetail is NOT cached -> Archive done MUST exclude
      // `leaf`, `middle`, AND `root` (0 archive RPCs)!
      const timerMission: BriefingMissionDto = {
        key: 'mission-waiting-timer',
        root: makeBriefingWorker('host-L', 'l-root', 'done'),
        workers: [
          makeBriefingWorker('host-R', 'r-middle', 'done'),
          makeBriefingWorker('host-R', 'r-leaf', 'idle')
        ],
        host_names: ['Local L', 'Remote R'],
        done: 2,
        total: 3,
        state: 'done',
        updated_at_ms: newerTsMs
      };
      const timerWaiting: BriefingWaitItemDto[] = [
        {
          key: 'timer:host-R:r-leaf',
          kind: 'timer',
          host_id: 'host-R',
          agent_id: 'r-leaf',
          title: 'Wait for deploy',
          detail: 'Timer armed',
          due_at_ms: newerTsMs + 60000
        }
      ];
      const timerPartition = partitionBriefingWorkers(
        [timerMission],
        [],
        [],
        onlyLocalDetails,
        timerWaiting
      );
      expect(timerPartition.archivableCompletedWorkers.length).assertEqual(0);
      const timerBriefing: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [],
        missions: [timerMission],
        other_active: [],
        waiting: timerWaiting,
        hosts: [],
        counts: { needs_you: 0, running: 0, done_recent: 2, hosts_online: 2, hosts_total: 2 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const timerConfirmed = selectConfirmedArchivableWorkers(
        timerPartition.completedWorkers,
        timerBriefing,
        onlyLocalDetails
      );
      expect(timerConfirmed.length).assertEqual(0);
    });

    it('reports partial host failures with error severity in batch approval feedback (P1-2)', 0, () => {
      const partialSummary = aggregateBatchApprovalOutcomes([
        parseBatchApprovalHostResponse('h1', '{"resolved_count":1,"skipped_count":0,"skipped_reasons":[]}'),
        { hostId: 'h2', resolvedCount: 0, skippedCount: 0, firstSkipReason: '', failed: true }
      ]);
      const partialPlan = classifyBatchApprovalFeedback(partialSummary);
      expect(partialPlan.kind).assertEqual('partial_failure');
      expect(partialPlan.isError).assertTrue();
      expect(partialPlan.resolvedCount).assertEqual(1);
      expect(partialPlan.skippedCount).assertEqual(0);
      expect(partialPlan.failedHostCount).assertEqual(1);

      const skippedFailedSummary = aggregateBatchApprovalOutcomes([
        parseBatchApprovalHostResponse('h1', '{"resolved_count":0,"skipped_count":2,"skipped_reasons":[{"reason":"expired"}]}'),
        { hostId: 'h2', resolvedCount: 0, skippedCount: 0, firstSkipReason: '', failed: true }
      ]);
      const skippedFailedPlan = classifyBatchApprovalFeedback(skippedFailedSummary);
      expect(skippedFailedPlan.kind).assertEqual('skipped_and_failed');
      expect(skippedFailedPlan.isError).assertTrue();
      expect(skippedFailedPlan.skippedCount).assertEqual(2);
      expect(skippedFailedPlan.failedHostCount).assertEqual(1);
    });

    it('preserves another host existing scope, pre_review_mode, memory, and webhook on setup (P1-3)', 0, () => {
      const hostAConfig: PersonalRouterConfigDto = {
        enabled: true,
        router_agent_id: 'pa-a',
        provider: 'claude_code',
        acp_agent_id: null,
        model: 'claude-sonnet-4-5',
        effort: 'medium',
        cwd: '/Users/a',
        scope: 'global_fleet',
        pre_review_mode: 'medium_risk_only',
        share_host_hermes_memory: true,
        soul_prompt_override: 'Prompt A',
        external_webhook_url: null,
        has_external_webhook_secret: false
      };
      // Rust omits Option::None fields (`model`, `cwd`, `acp_agent_id`, `soul_prompt_override` undefined).
      const hostBConfigOmittedFields: PersonalRouterConfigDto = {
        enabled: false,
        provider: 'antigravity',
        scope: 'local_host_only',
        pre_review_mode: 'human_assisted',
        share_host_hermes_memory: false,
        external_webhook_url: 'https://hooks.internal.example/b',
        has_external_webhook_secret: true
      };
      const normalizedB = normalizePersonalRouterConfig(hostBConfigOmittedFields);
      expect(normalizedB.model).assertEqual(null);
      expect(normalizedB.cwd).assertEqual(null);
      expect(normalizedB.scope).assertEqual('local_host_only');
      expect(normalizedB.pre_review_mode).assertEqual('human_assisted');

      const mergedB = mergePersonalAgentSetupConfig('codex', 'gpt-5.4', hostBConfigOmittedFields);
      expect(mergedB.enabled).assertTrue();
      expect(mergedB.provider).assertEqual('codex');
      expect(mergedB.model).assertEqual('gpt-5.4');
      expect(mergedB.router_agent_id).assertEqual(null);
      expect(mergedB.scope).assertEqual('local_host_only');
      expect(mergedB.pre_review_mode).assertEqual('human_assisted');
      expect(mergedB.share_host_hermes_memory).assertFalse();
      expect(mergedB.external_webhook_url).assertEqual('https://hooks.internal.example/b');
      expect(mergedB.has_external_webhook_secret).assertTrue();
      expect(hostAConfig.scope).assertEqual('global_fleet');
    });

    it('paginates combined In progress workers with exact hidden count (Nit-1)', 0, () => {
      const missions: BriefingMissionDto[] = [];
      const otherWorkers: BriefingWorkerDto[] = [];
      for (let i = 0; i < 3; i += 1) {
        const w = makeBriefingWorker('host-1', `m-${i}`, 'running');
        missions.push({
          key: `mk-${i}`,
          root: w,
          workers: [],
          host_names: ['Mac Studio'],
          done: 0,
          total: 1,
          state: 'running',
          updated_at_ms: 1700000005000 + i
        });
        otherWorkers.push(makeBriefingWorker('host-1', `o-${i}`, 'running'));
      }
      const partitioned = partitionBriefingWorkers(missions, otherWorkers, [], new Map<string, HostDetailDto>());
      const collapsed = paginateInProgressWorkers(
        partitioned.runningMissionWorkers,
        partitioned.runningOtherWorkers,
        false,
        5
      );
      expect(collapsed.visibleMissionWorkers.length).assertEqual(3);
      expect(collapsed.visibleOtherWorkers.length).assertEqual(2);
      expect(collapsed.hiddenCount).assertEqual(1);
      expect(collapsed.showToggle).assertTrue();

      const expanded = paginateInProgressWorkers(
        partitioned.runningMissionWorkers,
        partitioned.runningOtherWorkers,
        true,
        5
      );
      expect(expanded.visibleMissionWorkers.length).assertEqual(3);
      expect(expanded.visibleOtherWorkers.length).assertEqual(3);
      expect(expanded.hiddenCount).assertEqual(0);
    });
  });

  describe('OrbitPr2NotificationsSchedulerDeepLinkAndVisit', () => {
    it('enforces daemon turn/goal state machine, visibility, and zero-knowledge preview (P1-5, P1-6)', 0, () => {
      const engine = new OrbitNotificationEngine();
      const prefsNoPreview: PushPreferencesDto = defaultPushPreferences();
      const fgOtherSession: NotificationVisibilityContext = {
        isAppInBackground: false,
        isNewSessionDraft: false,
        showPersonalAgentSetup: false,
        selectedHostId: 'host-1',
        selectedAgentId: 'ag-viewing'
      };
      const fgSameSession: NotificationVisibilityContext = {
        isAppInBackground: false,
        isNewSessionDraft: false,
        showPersonalAgentSetup: false,
        selectedHostId: 'host-1',
        selectedAgentId: 'ag-goal'
      };

      const agClosed = makeAgent('ag-close', 'Secret Close Title', '/repo', 'running', true);
      const agAwait = makeAgent('ag-await', 'Secret Await Title', '/repo', 'awaiting_permission', false);
      const agGoal = makeAgent('ag-goal', 'Secret Goal Agent', '/repo', 'idle', false);
      const runningGoal: GoalRunRecordDto = {
        goal_id: 'g-1',
        agent_id: 'ag-goal',
        workspace_id: 'ws-1',
        objective: 'Deploy production secret keys',
        criteria: [],
        max_iterations: 3,
        current_iteration: 1,
        max_duration_secs: 300,
        status: 'running',
        started_at_ms: 1000,
        updated_at_ms: 2000
      };
      // Baseline seed — emits nothing
      const baseNotifs = engine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 1, [agClosed, agAwait, agGoal], [runningGoal]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(baseNotifs.length).assertEqual(0);

      // running -> closed must NOT emit turn_completed; awaiting_permission -> idle MUST emit turn_completed
      const agNowClosed = makeAgent('ag-close', 'Secret Close Title', '/repo', 'closed', false);
      const agNowIdle = makeAgent('ag-await', 'Secret Await Title', '/repo', 'idle', false);
      const finishedGoal: GoalRunRecordDto = {
        goal_id: 'g-1',
        agent_id: 'ag-goal',
        workspace_id: 'ws-1',
        objective: 'Deploy production secret keys',
        criteria: [],
        max_iterations: 3,
        current_iteration: 2,
        max_duration_secs: 300,
        status: 'succeeded',
        started_at_ms: 1000,
        updated_at_ms: 3000
      };

      // Viewing the goal's own session in foreground suppresses goal_finished
      const sameSessionNotifs = engine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 2, [agNowClosed, agNowIdle, agGoal], [finishedGoal]),
        fgSameSession,
        prefsNoPreview
      );
      expect(sameSessionNotifs.length).assertEqual(1);
      expect(sameSessionNotifs[0].eventKind).assertEqual('turn_completed');
      expect(sameSessionNotifs[0].agentId).assertEqual('ag-await');
      // Zero-knowledge privacy: include_message_preview=false leaks NO session title in title or body
      expect(sameSessionNotifs[0].title.indexOf('Secret Await Title')).assertEqual(-1);
      expect(sameSessionNotifs[0].body.indexOf('Secret Await Title')).assertEqual(-1);

      // Verify goal_finished fires for non-viewed session and hides objective when preview=false
      const goalEngine = new OrbitNotificationEngine();
      goalEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 1, [agGoal], [runningGoal]),
        fgOtherSession,
        prefsNoPreview
      );
      const goalNotifs = goalEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 2, [agGoal], [finishedGoal]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(goalNotifs.length).assertEqual(1);
      expect(goalNotifs[0].eventKind).assertEqual('goal_finished');
      expect(goalNotifs[0].title.indexOf('Deploy production secret keys')).assertEqual(-1);
      expect(goalNotifs[0].body.indexOf('Deploy production secret keys')).assertEqual(-1);

      // When include_message_preview=true, preview appears ONLY in body
      const previewBuilt = buildLocalNotificationCopy(
        'goal_finished',
        'Mac Studio',
        true,
        'Goal succeeded: Deploy production secret keys'
      );
      expect(previewBuilt.title).assertEqual('Goal finished · Mac Studio');
      expect(previewBuilt.body.indexOf('Deploy production secret keys') >= 0).assertTrue();

      // P1-5: initializing -> idle MUST NOT emit turn_completed (matches daemon push.rs:580)
      const initEngine = new OrbitNotificationEngine();
      const agInit = makeAgent('ag-init', 'Init Thread', '/repo', 'initializing', false);
      agInit.latest_seq = 1;
      initEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 1, [agInit]),
        fgOtherSession,
        prefsNoPreview
      );
      const agAfterInit = makeAgent('ag-init', 'Init Thread', '/repo', 'idle', false);
      agAfterInit.latest_seq = 2;
      const initNotifs = initEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 2, [agAfterInit]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(initNotifs.length).assertEqual(0);

      // P1-5: late stale StageDelta `running` after HostDetail `idle` MUST NOT trigger duplicate completion
      const mergeEngine = new OrbitNotificationEngine();
      const agRunSeq5 = makeAgent('ag-m', 'Merge', '/repo', 'running', true);
      agRunSeq5.latest_seq = 5;
      agRunSeq5.updated_at_rfc3339 = '2026-11-26T10:00:05Z';
      mergeEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 1, [agRunSeq5]),
        fgOtherSession,
        prefsNoPreview
      );
      const agIdleSeq6 = makeAgent('ag-m', 'Merge', '/repo', 'idle', false);
      agIdleSeq6.latest_seq = 6;
      agIdleSeq6.updated_at_rfc3339 = '2026-11-26T10:00:06Z';
      const firstDone = mergeEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 2, [agIdleSeq6]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(firstDone.length).assertEqual(1);
      // Stale StageDelta arrives out of order with old seq=5 running
      const staleStage = mergeEngine.observeStageDelta(
        'host-1',
        'Mac Studio',
        agRunSeq5,
        [],
        fgOtherSession,
        prefsNoPreview
      );
      expect(staleStage.length).assertEqual(0);
      // Next HostDetail repeats idle seq=6 -> MUST emit 0 duplicates
      const duplicateCheck = mergeEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 3, [agIdleSeq6]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(duplicateCheck.length).assertEqual(0);

      // P1-5: Unversioned (seq=0) same-ms stale Stage `running` after versioned idle (seq=6)
      // MUST NOT regress state or cause a duplicate turn_completed on next seq=7 idle!
      const agZeroSeqRunning = makeAgent('ag-m', 'Merge', '/repo', 'running', true);
      agZeroSeqRunning.latest_seq = 0;
      agZeroSeqRunning.updated_at_rfc3339 = '2026-11-26T10:00:06Z';
      const zeroSeqStage = mergeEngine.observeStageDelta(
        'host-1',
        'Mac Studio',
        agZeroSeqRunning,
        [],
        fgOtherSession,
        prefsNoPreview
      );
      expect(zeroSeqStage.length).assertEqual(0);
      const agIdleSeq7 = makeAgent('ag-m', 'Merge', '/repo', 'idle', false);
      agIdleSeq7.latest_seq = 7;
      agIdleSeq7.updated_at_rfc3339 = '2026-11-26T10:00:07Z';
      const afterZeroSeqIdle = mergeEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac Studio', 4, [agIdleSeq7]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(afterZeroSeqIdle.length).assertEqual(0);

      // P1-5: Coarse Briefing display states (initializing->running, closed->done) MUST NEVER
      // emit turn_completed notifications!
      const briefingOnlyEngine = new OrbitNotificationEngine();
      const mkBriefingWithWorker = (state: string, ts: number, goalSnaps: BriefingGoalSnapshotDto[] = []): PersonalAgentBriefingDto => ({
        personal_agent: null,
        needs_you: [],
        missions: [{
          key: 'm-coarse',
          root: makeBriefingWorker('host-R', 'w-coarse', state),
          workers: [],
          host_names: ['Remote R'],
          done: state === 'done' ? 1 : 0,
          total: 1,
          state: state === 'done' ? 'done' : 'running',
          updated_at_ms: ts
        }],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 0, running: state === 'running' ? 1 : 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 },
        goal_snapshots: goalSnaps
      });
      const b1 = briefingOnlyEngine.observeBriefing(
        mkBriefingWithWorker('running', 1000),
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(b1.length).assertEqual(0);
      const b2 = briefingOnlyEngine.observeBriefing(
        mkBriefingWithWorker('done', 2000),
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(b2.length).assertEqual(0);

      // P1-7: Unselected host regular-session goal running -> succeeded fires 1 goal_finished via zero-IO Briefing
      const goalEngineBriefing = new OrbitNotificationEngine();
      const runningSnap: BriefingGoalSnapshotDto = {
        host_id: 'host-unselected',
        host_name: 'Linux Build Box',
        agent_id: 'ag-regular',
        goal_id: 'g-remote-1',
        status: 'running',
        objective: 'Deploy secret build',
        updated_at_ms: 1000
      };
      const doneSnap: BriefingGoalSnapshotDto = {
        host_id: 'host-unselected',
        host_name: 'Linux Build Box',
        agent_id: 'ag-regular',
        goal_id: 'g-remote-1',
        status: 'succeeded',
        objective: 'Deploy secret build',
        updated_at_ms: 2000
      };
      expect(
        goalEngineBriefing.observeBriefing(
          mkBriefingWithWorker('done', 1000, [runningSnap]),
          fgOtherSession,
          (_hid: string): PushPreferencesDto => prefsNoPreview
        ).length
      ).assertEqual(0);
      const remoteGoalNotifs = goalEngineBriefing.observeBriefing(
        mkBriefingWithWorker('done', 2000, [doneSnap]),
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(remoteGoalNotifs.length).assertEqual(1);
      expect(remoteGoalNotifs[0].eventKind).assertEqual('goal_finished');
      expect(remoteGoalNotifs[0].hostId).assertEqual('host-unselected');
      expect(remoteGoalNotifs[0].agentId).assertEqual('ag-regular');
      // Zero-knowledge preview check
      expect(remoteGoalNotifs[0].title.indexOf('Deploy secret build')).assertEqual(-1);
      expect(remoteGoalNotifs[0].body.indexOf('Deploy secret build')).assertEqual(-1);

      // P1-7 cross-source shared goal baseline (order 1): Briefing #1 sees running -> HostDetail #1 sees succeeded -> Briefing #2 sees succeeded => strictly 1 notification
      const crossGoalEngine1 = new OrbitNotificationEngine();
      const goalRecordDone: GoalRunRecordDto = {
        goal_id: 'g-cross-1',
        agent_id: 'ag-regular',
        workspace_id: 'ws-1',
        objective: 'Cross-source goal',
        criteria: [],
        max_iterations: 5,
        current_iteration: 2,
        max_duration_secs: 600,
        status: 'succeeded',
        started_at_ms: 1000,
        updated_at_ms: 2000
      };
      const crossRunningSnap: BriefingGoalSnapshotDto = {
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-regular',
        goal_id: 'g-cross-1',
        status: 'running',
        objective: 'Cross-source goal',
        updated_at_ms: 1000
      };
      const crossDoneSnap: BriefingGoalSnapshotDto = {
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-regular',
        goal_id: 'g-cross-1',
        status: 'succeeded',
        objective: 'Cross-source goal',
        updated_at_ms: 2000
      };
      const c1Step1 = crossGoalEngine1.observeBriefing(
        mkBriefingWithWorker('done', 1000, [crossRunningSnap]),
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const c1Step2 = crossGoalEngine1.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [makeAgent('ag-regular', 'Regular', '/repo')], [goalRecordDone]),
        fgOtherSession,
        prefsNoPreview
      );
      const c1Step3 = crossGoalEngine1.observeBriefing(
        mkBriefingWithWorker('done', 2000, [crossDoneSnap]),
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(c1Step1.length).assertEqual(0);
      expect(c1Step2.length).assertEqual(1);
      expect(c1Step2[0].eventKind).assertEqual('goal_finished');
      expect(c1Step3.length).assertEqual(0);

      // P1-7 cross-source shared goal baseline (order 2): HostDetail #1 sees running -> Briefing #1 sees succeeded -> HostDetail #2 sees succeeded => strictly 1 notification
      const crossGoalEngine2 = new OrbitNotificationEngine();
      const goalRecordRunning: GoalRunRecordDto = {
        goal_id: 'g-cross-2',
        agent_id: 'ag-regular',
        workspace_id: 'ws-1',
        objective: 'Cross-source reverse',
        criteria: [],
        max_iterations: 5,
        current_iteration: 1,
        max_duration_secs: 600,
        status: 'running',
        started_at_ms: 1000,
        updated_at_ms: 1500
      };
      const goalRecordDone2: GoalRunRecordDto = {
        goal_id: 'g-cross-2',
        agent_id: 'ag-regular',
        workspace_id: 'ws-1',
        objective: 'Cross-source reverse',
        criteria: [],
        max_iterations: 5,
        current_iteration: 2,
        max_duration_secs: 600,
        status: 'succeeded',
        started_at_ms: 1000,
        updated_at_ms: 2000
      };
      const crossDoneSnap2: BriefingGoalSnapshotDto = {
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-regular',
        goal_id: 'g-cross-2',
        status: 'succeeded',
        objective: 'Cross-source reverse',
        updated_at_ms: 2000
      };
      const c2Step1 = crossGoalEngine2.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [makeAgent('ag-regular', 'Regular', '/repo')], [goalRecordRunning]),
        fgOtherSession,
        prefsNoPreview
      );
      const c2Step2 = crossGoalEngine2.observeBriefing(
        mkBriefingWithWorker('done', 2000, [crossDoneSnap2]),
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const c2Step3 = crossGoalEngine2.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 2, [makeAgent('ag-regular', 'Regular', '/repo')], [goalRecordDone2]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(c2Step1.length).assertEqual(0);
      expect(c2Step2.length).assertEqual(1);
      expect(c2Step2[0].eventKind).assertEqual('goal_finished');
      expect(c2Step3.length).assertEqual(0);

      // Cross-source agent_error dedup: multi-line error reported via Briefing (collapsed/truncated)
      // AND HostDetail (raw multi-line) fires strictly 1 agent_error per error episode!
      const errDedupEngine = new OrbitNotificationEngine();
      const baseIdleAg = makeAgent('ag-err', 'Worker Err', '/repo', 'idle', false, false);
      const emptyBriefing: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [],
        missions: [],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 0, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      // Baseline both sources cleanly
      expect(
        errDedupEngine.observeHostDetail('host-1', makeHostDetail('host-1', 'Mac', 1, [baseIdleAg]), fgOtherSession, prefsNoPreview).length
      ).assertEqual(0);
      expect(
        errDedupEngine.observeBriefing(emptyBriefing, fgOtherSession, (_hid: string): PushPreferencesDto => prefsNoPreview).length
      ).assertEqual(0);

      // Error arrives via Briefing (collapsed whitespace summary), then HostDetail (raw multi-line message)
      const errDecision: BriefingDecisionDto = {
        key: 'err:host-1:ag-err',
        kind: 'error',
        host_id: 'host-1',
        host_name: 'Mac',
        agent_id: 'ag-err',
        agent_title: 'Worker Err',
        request_id: null,
        summary: 'provider failed while writing...',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: Date.parse('2026-11-26T10:06:00Z')
      };
      const errBriefing: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [errDecision],
        missions: [],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const errAgent = makeAgent('ag-err', 'Worker Err', '/repo', 'error', false, false);
      errAgent.latest_seq = 12;
      errAgent.updated_at_rfc3339 = '2026-11-26T10:06:00Z';
      errAgent.status = { state: 'error', message: 'provider failed\nwhile writing' };
      const errNotif1 = errDedupEngine.observeBriefing(
        errBriefing,
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const errNotif2 = errDedupEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac', 2, [errAgent]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(errNotif1.length).assertEqual(1);
      expect(errNotif1[0].eventKind).assertEqual('agent_error');
      expect(errNotif2.length).assertEqual(0);

      // Recovery -> new error episode emits 1 fresh notification
      const recoveredAgent = makeAgent('ag-err', 'Worker Err', '/repo', 'running', true, false);
      recoveredAgent.latest_seq = 13;
      recoveredAgent.updated_at_rfc3339 = '2026-11-26T10:07:00Z';
      expect(
        errDedupEngine.observeHostDetail('host-1', makeHostDetail('host-1', 'Mac', 3, [recoveredAgent]), fgOtherSession, prefsNoPreview).length
      ).assertEqual(0);
      const secondErrAgent = makeAgent('ag-err', 'Worker Err', '/repo', 'error', false, false);
      secondErrAgent.latest_seq = 14;
      secondErrAgent.updated_at_rfc3339 = '2026-11-26T10:08:00Z';
      secondErrAgent.status = { state: 'error', message: 'provider failed\nwhile writing' };
      const errNotif3 = errDedupEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac', 4, [secondErrAgent]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(errNotif3.length).assertEqual(1);
      expect(errNotif3[0].eventKind).assertEqual('agent_error');

      // Versioned cross-source agent_error dedup (Scenario A):
      // Baseline running(seq=5, t1) -> Briefing error(t2) -> stale baseline running(seq=5, t1)
      // -> HostDetail error(seq=6, t2) => stale running MUST NOT clear active error (strictly 1 notif)!
      const verErrEngine = new OrbitNotificationEngine();
      const t1Iso = '2026-11-26T10:00:00Z';
      const t2Iso = '2026-11-26T10:00:10Z';
      const t2Ms = Date.parse(t2Iso);
      const t3Ms = Date.parse('2026-11-26T10:00:20Z');
      const t4Ms = Date.parse('2026-11-26T10:00:30Z');
      const baseRunAg = makeAgent('ag-v', 'Ver Worker', '/repo', 'running', true, false);
      baseRunAg.latest_seq = 5;
      baseRunAg.updated_at_rfc3339 = t1Iso;
      expect(
        verErrEngine.observeHostDetail('host-1', makeHostDetail('host-1', 'Mac', 1, [baseRunAg]), fgOtherSession, prefsNoPreview).length
      ).assertEqual(0);
      expect(
        verErrEngine.observeBriefing(emptyBriefing, fgOtherSession, (_hid: string): PushPreferencesDto => prefsNoPreview).length
      ).assertEqual(0);

      const verErrDecision1: BriefingDecisionDto = {
        key: 'err:host-1:ag-v',
        kind: 'error',
        host_id: 'host-1',
        host_name: 'Mac',
        agent_id: 'ag-v',
        agent_title: 'Ver Worker',
        request_id: null,
        summary: 'provider failed while writing',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: t2Ms
      };
      const verBriefingErr1: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [verErrDecision1],
        missions: [],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      expect(
        verErrEngine.observeBriefing(verBriefingErr1, fgOtherSession, (_hid: string): PushPreferencesDto => prefsNoPreview).length
      ).assertEqual(1);
      // Stale baseline-identical running snapshot MUST NOT clear active error episode!
      expect(
        verErrEngine.observeHostDetail('host-1', makeHostDetail('host-1', 'Mac', 1, [baseRunAg]), fgOtherSession, prefsNoPreview).length
      ).assertEqual(0);
      const verHostErr1 = makeAgent('ag-v', 'Ver Worker', '/repo', 'error', false, false);
      verHostErr1.latest_seq = 6;
      verHostErr1.updated_at_rfc3339 = t2Iso;
      verHostErr1.status = { state: 'error', message: 'provider failed\nwhile writing' };
      expect(
        verErrEngine.observeHostDetail('host-1', makeHostDetail('host-1', 'Mac', 2, [verHostErr1]), fgOtherSession, prefsNoPreview).length
      ).assertEqual(0);

      // Scenario B: Both sources saw Error #1; ONLY Briefing observes newer recovery (t3) and
      // independent Error #2 (t4) without HostDetail updating -> Error #2 MUST emit 1 notification!
      const recoveredWorker: BriefingWorkerDto = makeBriefingWorker('host-1', 'ag-v', 'running');
      recoveredWorker.updated_at_ms = t3Ms;
      const verBriefingRecovered: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [],
        missions: [],
        other_active: [recoveredWorker],
        waiting: [],
        hosts: [],
        counts: { needs_you: 0, running: 1, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      expect(
        verErrEngine.observeBriefing(verBriefingRecovered, fgOtherSession, (_hid: string): PushPreferencesDto => prefsNoPreview).length
      ).assertEqual(0);
      // Stale old HostDetail error (t2 <= t3) MUST NOT re-trigger Error #1 after recovery
      expect(
        verErrEngine.observeHostDetail('host-1', makeHostDetail('host-1', 'Mac', 2, [verHostErr1]), fgOtherSession, prefsNoPreview).length
      ).assertEqual(0);
      const verErrDecision2: BriefingDecisionDto = {
        key: 'err:host-1:ag-v',
        kind: 'error',
        host_id: 'host-1',
        host_name: 'Mac',
        agent_id: 'ag-v',
        agent_title: 'Ver Worker',
        request_id: null,
        summary: 'second distinct failure',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: t4Ms
      };
      const verBriefingErr2: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [verErrDecision2],
        missions: [],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const secondBriefingNotif = verErrEngine.observeBriefing(
        verBriefingErr2,
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(secondBriefingNotif.length).assertEqual(1);
      expect(secondBriefingNotif[0].eventKind).assertEqual('agent_error');

      // Scenario C (shared per-(host, agent) baseline): Briefing observes R/w running first, then
      // the VERY FIRST HostDetail for host R reports error -> MUST immediately emit 1 agent_error
      // (NOT swallowed by host-level baseline), and subsequent Briefing error emits 0!
      const crossBaseErrEngine = new OrbitNotificationEngine();
      const rwRunWorker: BriefingWorkerDto = makeBriefingWorker('host-R', 'w', 'running');
      rwRunWorker.updated_at_ms = t2Ms;
      const rwBriefingRun: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [],
        missions: [],
        other_active: [rwRunWorker],
        waiting: [],
        hosts: [],
        counts: { needs_you: 0, running: 1, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const rwHostErr = makeAgent('w', 'Remote Worker', '/repo', 'error', false, false);
      rwHostErr.latest_seq = 8;
      rwHostErr.updated_at_rfc3339 = '2026-11-26T10:00:20Z';
      rwHostErr.status = { state: 'error', message: 'remote crash' };
      const rwErrDecision: BriefingDecisionDto = {
        key: 'err:host-R:w',
        kind: 'error',
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'w',
        agent_title: 'Remote Worker',
        request_id: null,
        summary: 'remote crash',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: t3Ms
      };
      const rwBriefingErr: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [rwErrDecision],
        missions: [],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const cbStep1 = crossBaseErrEngine.observeBriefing(
        rwBriefingRun,
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const cbStep2 = crossBaseErrEngine.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [rwHostErr]),
        fgOtherSession,
        prefsNoPreview
      );
      const cbStep3 = crossBaseErrEngine.observeBriefing(
        rwBriefingErr,
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(cbStep1.length).assertEqual(0);
      expect(cbStep2.length).assertEqual(1);
      expect(cbStep2[0].eventKind).assertEqual('agent_error');
      expect(cbStep3.length).assertEqual(0);

      // Scenario D (Briefing recovery boundary t=3000 rejects late Round-1 HostDetail error
      // with higher seq=11 > baseline seq=10 but older t=2500 < 3000, so Round-2 t=4000 fires!):
      const lateErrBoundaryEngine = new OrbitNotificationEngine();
      const b1000Iso = '2026-11-26T11:00:01.000Z';
      const b2000Ms = Date.parse('2026-11-26T11:00:02.000Z');
      const b2500Iso = '2026-11-26T11:00:02.500Z';
      const b3000Ms = Date.parse('2026-11-26T11:00:03.000Z');
      const b4000Ms = Date.parse('2026-11-26T11:00:04.000Z');

      const seq10Run = makeAgent('ag-seq', 'Seq Worker', '/repo', 'running', true, false);
      seq10Run.latest_seq = 10;
      seq10Run.updated_at_rfc3339 = b1000Iso;
      const dStep0Host = lateErrBoundaryEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac', 1, [seq10Run]),
        fgOtherSession,
        prefsNoPreview
      );
      const dStep0Briefing = lateErrBoundaryEngine.observeBriefing(
        emptyBriefing,
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(dStep0Host.length).assertEqual(0);
      expect(dStep0Briefing.length).assertEqual(0);

      const dErr1Decision: BriefingDecisionDto = {
        key: 'err:host-1:ag-seq',
        kind: 'error',
        host_id: 'host-1',
        host_name: 'Mac',
        agent_id: 'ag-seq',
        agent_title: 'Seq Worker',
        request_id: null,
        summary: 'first error',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: b2000Ms
      };
      const dStep1 = lateErrBoundaryEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [dErr1Decision],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(dStep1.length).assertEqual(1);
      expect(dStep1[0].eventKind).assertEqual('agent_error');

      const dRecWorker: BriefingWorkerDto = makeBriefingWorker('host-1', 'ag-seq', 'running');
      dRecWorker.updated_at_ms = b3000Ms;
      const dStep2 = lateErrBoundaryEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [],
          missions: [],
          other_active: [dRecWorker],
          waiting: [],
          hosts: [],
          counts: { needs_you: 0, running: 1, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(dStep2.length).assertEqual(0);

      // Late Round-1 HostDetail error (seq=11 > baseline 10, but t=2500 < recoveredAtMs=3000) -> 0!
      const lateRound1Err = makeAgent('ag-seq', 'Seq Worker', '/repo', 'error', false, false);
      lateRound1Err.latest_seq = 11;
      lateRound1Err.updated_at_rfc3339 = b2500Iso;
      lateRound1Err.status = { state: 'error', message: 'first error late host detail' };
      const dStep3 = lateErrBoundaryEngine.observeHostDetail(
        'host-1',
        makeHostDetail('host-1', 'Mac', 2, [lateRound1Err]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(dStep3.length).assertEqual(0);

      // Real Round-2 Briefing error (t=4000 > 3000) -> MUST fire 1!
      const dErr2Decision: BriefingDecisionDto = {
        key: 'err:host-1:ag-seq',
        kind: 'error',
        host_id: 'host-1',
        host_name: 'Mac',
        agent_id: 'ag-seq',
        agent_title: 'Seq Worker',
        request_id: null,
        summary: 'second distinct error',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: b4000Ms
      };
      const dStep4 = lateErrBoundaryEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [dErr2Decision],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(dStep4.length).assertEqual(1);
      expect(dStep4[0].eventKind).assertEqual('agent_error');
      expect(dStep1.length + dStep2.length + dStep3.length + dStep4.length).assertEqual(2);

      // Scenario E (Versioned cross-source non-error baseline before first error episode):
      // Briefing(running@3000) -> first HostDetail(error/11@2500) -> Briefing(error@4000) => 0, 0, 1!
      const crossVerEngine1 = new OrbitNotificationEngine();
      const eRunWorker: BriefingWorkerDto = makeBriefingWorker('host-R', 'ag-v', 'running');
      eRunWorker.updated_at_ms = b3000Ms;
      const eStep1 = crossVerEngine1.observeBriefing(
        {
          personal_agent: null,
          needs_you: [],
          missions: [],
          other_active: [eRunWorker],
          waiting: [],
          hosts: [],
          counts: { needs_you: 0, running: 1, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const eStaleHostErr = makeAgent('ag-v', 'Ver Worker', '/repo', 'error', false, false);
      eStaleHostErr.latest_seq = 11;
      eStaleHostErr.updated_at_rfc3339 = b2500Iso;
      eStaleHostErr.status = { state: 'error', message: 'old pre-baseline error' };
      const eStep2 = crossVerEngine1.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [eStaleHostErr]),
        fgOtherSession,
        prefsNoPreview
      );
      const eRealBriefingErr: BriefingDecisionDto = {
        key: 'err:host-R:ag-v',
        kind: 'error',
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-v',
        agent_title: 'Ver Worker',
        request_id: null,
        summary: 'real error at 4000',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: b4000Ms
      };
      const eStep3 = crossVerEngine1.observeBriefing(
        {
          personal_agent: null,
          needs_you: [eRealBriefingErr],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(eStep1.length).assertEqual(0);
      expect(eStep2.length).assertEqual(0);
      expect(eStep3.length).assertEqual(1);
      expect(eStep3[0].eventKind).assertEqual('agent_error');

      // Scenario F (Same-ms variant):
      // Briefing(running@3000) -> first HostDetail(error/11@3000) -> Briefing(error@4000) => 0, 0, 1!
      const crossVerEngine2 = new OrbitNotificationEngine();
      const b3000Iso = '2026-11-26T11:00:03.000Z';
      const fStep1 = crossVerEngine2.observeBriefing(
        {
          personal_agent: null,
          needs_you: [],
          missions: [],
          other_active: [eRunWorker],
          waiting: [],
          hosts: [],
          counts: { needs_you: 0, running: 1, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const fSameMsHostErr = makeAgent('ag-v', 'Ver Worker', '/repo', 'error', false, false);
      fSameMsHostErr.latest_seq = 11;
      fSameMsHostErr.updated_at_rfc3339 = b3000Iso;
      fSameMsHostErr.status = { state: 'error', message: 'same-ms unsequenced briefing tie' };
      const fStep2 = crossVerEngine2.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [fSameMsHostErr]),
        fgOtherSession,
        prefsNoPreview
      );
      const fStep3 = crossVerEngine2.observeBriefing(
        {
          personal_agent: null,
          needs_you: [eRealBriefingErr],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(fStep1.length).assertEqual(0);
      expect(fStep2.length).assertEqual(0);
      expect(fStep3.length).assertEqual(1);

      // Scenario G (Same-ms HostDetail strictly advancing seq):
      // HostDetail(running/12@3000) -> HostDetail(error/13@3000) => 0, 1!
      const sameMsHostEngine = new OrbitNotificationEngine();
      const gRun12 = makeAgent('ag-sm', 'SameMs Worker', '/repo', 'running', true, false);
      gRun12.latest_seq = 12;
      gRun12.updated_at_rfc3339 = b3000Iso;
      const gErr13 = makeAgent('ag-sm', 'SameMs Worker', '/repo', 'error', false, false);
      gErr13.latest_seq = 13;
      gErr13.updated_at_rfc3339 = b3000Iso;
      gErr13.status = { state: 'error', message: 'real same-ms error' };
      expect(
        sameMsHostEngine.observeHostDetail('host-R', makeHostDetail('host-R', 'Remote R', 1, [gRun12]), fgOtherSession, prefsNoPreview).length
      ).assertEqual(0);
      const gStep2 = sameMsHostEngine.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 2, [gErr13]),
        fgOtherSession,
        prefsNoPreview
      );
      expect(gStep2.length).assertEqual(1);
      expect(gStep2[0].eventKind).assertEqual('agent_error');

      // Scenario H (Symmetric reverse protection):
      // HostDetail(running/12@3000) -> Briefing(error@2500) -> Briefing(error@4000) => 0, 0, 1!
      const revVerEngine = new OrbitNotificationEngine();
      const b2500Ms = Date.parse(b2500Iso);
      const hRun12 = makeAgent('ag-v', 'Ver Worker', '/repo', 'running', true, false);
      hRun12.latest_seq = 12;
      hRun12.updated_at_rfc3339 = b3000Iso;
      const hStep1 = revVerEngine.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [hRun12]),
        fgOtherSession,
        prefsNoPreview
      );
      const hStaleBriefingErr: BriefingDecisionDto = {
        key: 'err:host-R:ag-v',
        kind: 'error',
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-v',
        agent_title: 'Ver Worker',
        request_id: null,
        summary: 'stale briefing error at 2500',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: b2500Ms
      };
      const hStep2 = revVerEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [hStaleBriefingErr],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const hStep3 = revVerEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [eRealBriefingErr],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(hStep1.length).assertEqual(0);
      expect(hStep2.length).assertEqual(0);
      expect(hStep3.length).assertEqual(1);
      expect(hStep3[0].eventKind).assertEqual('agent_error');

      // Scenario I (Symmetric same-ms unsequenced Briefing error after HostDetail running):
      // HostDetail(running/12@3000) -> late Briefing(error@3000) -> Briefing(error@4000) => 0, 0, 1!
      const revSameMsEngine = new OrbitNotificationEngine();
      const iStep1 = revSameMsEngine.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [hRun12]),
        fgOtherSession,
        prefsNoPreview
      );
      const iSameMsBriefingErr: BriefingDecisionDto = {
        key: 'err:host-R:ag-v',
        kind: 'error',
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-v',
        agent_title: 'Ver Worker',
        request_id: null,
        summary: 'late same-ms briefing error from error/11@3000',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: b3000Ms
      };
      const iStep2 = revSameMsEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [iSameMsBriefingErr],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const iStep3 = revSameMsEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [eRealBriefingErr],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(iStep1.length).assertEqual(0);
      expect(iStep2.length).assertEqual(0);
      expect(iStep3.length).assertEqual(1);
      expect(iStep3[0].eventKind).assertEqual('agent_error');

      // Scenario J (Late same-ms Briefing running MUST NOT clear active HostDetail error episode):
      // HostDetail(running/10@1000) -> HostDetail(error/11@3000) -> late Briefing(running@3000)
      // -> duplicate Briefing(error@3000) MUST stay 0; later Briefing(running@3500) -> Briefing(error@4000) => 1!
      const sameMsRecoveryEngine = new OrbitNotificationEngine();
      const jRun10 = makeAgent('ag-j', 'Rec Worker', '/repo', 'running', true, false);
      jRun10.latest_seq = 10;
      jRun10.updated_at_rfc3339 = b1000Iso;
      const jErr11 = makeAgent('ag-j', 'Rec Worker', '/repo', 'error', false, false);
      jErr11.latest_seq = 11;
      jErr11.updated_at_rfc3339 = b3000Iso;
      jErr11.status = { state: 'error', message: 'active error at 3000' };
      const jStep1 = sameMsRecoveryEngine.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 1, [jRun10]),
        fgOtherSession,
        prefsNoPreview
      );
      const jStep2 = sameMsRecoveryEngine.observeHostDetail(
        'host-R',
        makeHostDetail('host-R', 'Remote R', 2, [jErr11]),
        fgOtherSession,
        prefsNoPreview
      );
      const jLateRunWorker: BriefingWorkerDto = makeBriefingWorker('host-R', 'ag-j', 'running');
      jLateRunWorker.updated_at_ms = b3000Ms;
      const jStep3 = sameMsRecoveryEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [],
          missions: [],
          other_active: [jLateRunWorker],
          waiting: [],
          hosts: [],
          counts: { needs_you: 0, running: 1, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const jDupBriefingErr: BriefingDecisionDto = {
        key: 'err:host-R:ag-j',
        kind: 'error',
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-j',
        agent_title: 'Rec Worker',
        request_id: null,
        summary: 'same error echoed by briefing at 3000',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: b3000Ms
      };
      const jStep4 = sameMsRecoveryEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [jDupBriefingErr],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const b3500Ms = Date.parse('2026-11-26T11:00:03.500Z');
      const jRealRecWorker: BriefingWorkerDto = makeBriefingWorker('host-R', 'ag-j', 'running');
      jRealRecWorker.updated_at_ms = b3500Ms;
      const jStep5 = sameMsRecoveryEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [],
          missions: [],
          other_active: [jRealRecWorker],
          waiting: [],
          hosts: [],
          counts: { needs_you: 0, running: 1, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      const jRound2Err: BriefingDecisionDto = {
        key: 'err:host-R:ag-j',
        kind: 'error',
        host_id: 'host-R',
        host_name: 'Remote R',
        agent_id: 'ag-j',
        agent_title: 'Rec Worker',
        request_id: null,
        summary: 'fresh round-2 error at 4000',
        risk_level: 'high',
        human_only: true,
        batch_eligible: false,
        router_recommendation: null,
        dispatched_by_personal_agent: false,
        updated_at_ms: b4000Ms
      };
      const jStep6 = sameMsRecoveryEngine.observeBriefing(
        {
          personal_agent: null,
          needs_you: [jRound2Err],
          missions: [],
          other_active: [],
          waiting: [],
          hosts: [],
          counts: { needs_you: 1, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
          since_last_visit: { completed: 0, auto_handled: 0 }
        },
        fgOtherSession,
        (_hid: string): PushPreferencesDto => prefsNoPreview
      );
      expect(jStep1.length).assertEqual(0);
      expect(jStep2.length).assertEqual(1);
      expect(jStep3.length).assertEqual(0);
      expect(jStep4.length).assertEqual(0);
      expect(jStep5.length).assertEqual(0);
      expect(jStep6.length).assertEqual(1);
      expect(jStep6[0].eventKind).assertEqual('agent_error');
    });

    it('treats backgrounded Stage as invisible, triggers 0 HostDetail on invisible timeline, and drops stale revisions (P1-7)', 0, async () => {
      // App in background -> even previously selected (host-1, ag-selected) is NOT visible!
      expect(isStageScopeVisibleForEvent(true, false, 'host-1', 'ag-selected', 'host-1', 'ag-selected')).assertFalse();
      expect(isStageScopeVisibleForEvent(false, false, 'host-1', 'ag-selected', 'host-1', 'ag-selected')).assertTrue();
      expect(isStageScopeVisibleForEvent(false, true, 'host-1', 'ag-selected', 'host-1', 'ag-selected')).assertFalse();

      const handler = new MockFlushHandler();
      const scheduler = new OrbitEventFrameScheduler(handler);
      scheduler.setBackgroundMode(true);
      expect(scheduler.isBackgroundMode()).assertTrue();
      scheduler.enqueue('{"kind":"host_changed","host_id":"host-bg"}');
      scheduler.enqueue('{"kind":"timeline_changed","host_id":"host-bg","agent_id":"a1"}');
      scheduler.enqueue('{"kind":"timeline_changed","host_id":"host-bg","agent_id":"a2"}');
      scheduler.flushNow();
      expect(handler.bgHostHits.length).assertEqual(1);
      expect(handler.bgHostHits[0]).assertEqual('host-bg');
      expect(handler.bgTimelineHits.length).assertEqual(2);
      scheduler.dispose();

      // Invisible timeline_changed events mark rows dirty in OrbitFleetStore with 0 host_detail_json_async calls
      const probeBridge = new MockStaleRevisionFleetBridge();
      const bgFleet = new OrbitFleetStore();
      bgFleet.bindBridge(probeBridge);
      for (let i = 0; i < 5; i += 1) {
        bgFleet.markConversationDirty('host-bg', `ag-hidden-${i}`);
      }
      expect(probeBridge.hostDetailAsyncCalls).assertEqual(0);
      expect(bgFleet.isConversationDirty('host-bg', 'ag-hidden-0')).assertTrue();
      expect(bgFleet.isConversationDirty('host-bg', 'ag-hidden-4')).assertTrue();

      // Out-of-order / lower revision response must be dropped by OrbitFleetStore
      const mockBridge = new MockStaleRevisionFleetBridge();
      const fleet = new OrbitFleetStore();
      fleet.bindBridge(mockBridge);
      fleet.setSelectedHostId('host-1');
      const pOld = fleet.reloadHostAsync('host-1');
      const pNew = fleet.reloadHostAsync('host-1');
      mockBridge.resolvePendingAt(
        'host-1',
        1,
        JSON.stringify(makeHostDetail('host-1', 'Mac Studio', 10, [makeAgent('ag-new', 'New', '/repo')]))
      );
      mockBridge.resolvePendingAt(
        'host-1',
        0,
        JSON.stringify(makeHostDetail('host-1', 'Mac Studio', 9, [makeAgent('ag-old', 'Old', '/repo')]))
      );
      const resNew = await pNew;
      const resOld = await pOld;
      expect(resNew !== null).assertTrue();
      expect(resOld === null).assertTrue();
      expect(fleet.getCachedHostDetail('host-1')?.revision).assertEqual(10);
    });

    it('requires complete (host, agent) for approval focus without seq hijacking and validates deep-link agent (P1-8)', 0, () => {
      const parsed = parseOrbitOpenDeepLink(
        'orbit://open?host=host%2Fmac&agent=ag%3A1&event_kind=permission_required&request_id=req_2&seq=42#top'
      );
      expect(parsed !== null).assertTrue();
      expect(parsed?.hostId).assertEqual('host/mac');
      expect(parsed?.agentId).assertEqual('ag:1');
      expect(parsed?.eventKind).assertEqual('permission_required');
      expect(parsed?.requestId).assertEqual('req_2');
      expect(parsed?.targetSeq).assertEqual(42);

      // Host-only permission deep-link MUST NOT create an empty-agent focus target!
      const hostOnlyParsed = parseOrbitOpenDeepLink('orbit://open?host=L&event_kind=permission_required&seq=17');
      expect(hostOnlyParsed !== null).assertTrue();
      expect(hostOnlyParsed?.agentId).assertEqual('');
      expect(
        buildDecisionFocusTarget(
          hostOnlyParsed?.eventKind ?? '',
          hostOnlyParsed?.hostId ?? '',
          hostOnlyParsed?.agentId ?? '',
          hostOnlyParsed?.requestId ?? '',
          hostOnlyParsed?.targetSeq ?? 0
        ) === null
      ).assertTrue();
      // Host-only permission/error deep link MUST NOT trigger approval/error reloadStage or toast
      expect(resolveDeepLinkPostNavigationAction('permission_required', '')).assertEqual('none');
      expect(resolveDeepLinkPostNavigationAction('agent_error', '')).assertEqual('none');
      expect(resolveDeepLinkPostNavigationAction('permission_required', 'ag-1')).assertEqual('permission_required');
      expect(resolveDeepLinkPostNavigationAction('agent_error', 'ag-1')).assertEqual('agent_error');
      expect(resolveDeepLinkPostNavigationAction('goal_finished', '')).assertEqual('automations');
      // Full (host, agent) permission target is accepted; non-permission event is rejected
      const validFocus = buildDecisionFocusTarget('permission_required', 'L', 'ag-1', '', 17);
      expect(validFocus !== null).assertTrue();
      expect(validFocus?.seq).assertEqual(17);
      expect(buildDecisionFocusTarget('turn_completed', 'L', 'ag-1', '', 17) === null).assertTrue();

      const perm1: TimelineItemDto = { item_type: 'permission_request', seq: 17, request_id: 'req_B', title: 'Other' };
      const perm2: TimelineItemDto = { item_type: 'permission_request', seq: 42, request_id: 'req_2', title: 'Second' };
      const queue: QueuedDecisionCard[] = [
        { key: 'perm_req_B', kind: 'permission', seq: 17, item: new OrbitTimelineItemModel(perm1) },
        { key: 'perm_req_2', kind: 'permission', seq: 42, item: new OrbitTimelineItemModel(perm2) }
      ];
      expect(findFocusedDecisionQueueIndex(queue, 'req_2', 0, 'h1', 'ag1', 'h1', 'ag1')).assertEqual(1);
      // Expired req_A sharing seq=17 with req_B MUST return -1 (never hijack req_B!)
      expect(findFocusedDecisionQueueIndex(queue, 'req_A', 17, 'h1', 'ag1', 'h1', 'ag1')).assertEqual(-1);
      // Empty targetAgent MUST return -1 even when host & seq match current session B!
      expect(findFocusedDecisionQueueIndex(queue, '', 17, 'L', '', 'L', 'B')).assertEqual(-1);
      // Wrong session scope returns -1
      expect(findFocusedDecisionQueueIndex(queue, 'req_2', 0, 'h1', 'ag1', 'h1', 'ag_other')).assertEqual(-1);
      // Only when requestId is empty AND (host, agent) match may permission seq match
      expect(findFocusedDecisionQueueIndex(queue, '', 42, 'h1', 'ag1', 'h1', 'ag1')).assertEqual(1);

      const onlineDetail = makeHostDetail('h1', 'Mac', 1, [makeAgent('ag-live', 'Live', '/repo')], [], 'online_direct');
      const offlineDetail = makeHostDetail('h1', 'Mac', 1, [], [], 'disconnected');
      expect(validateDeepLinkAgentInDetail(onlineDetail, 'h1', 'ag-live')).assertEqual('selected');
      expect(validateDeepLinkAgentInDetail(onlineDetail, 'h1', 'ag-gone')).assertEqual('not_found');
      expect(validateDeepLinkAgentInDetail(offlineDetail, 'h1', 'ag-gone')).assertEqual('pending');
      expect(validateDeepLinkAgentInDetail(null, 'h1', 'ag-live')).assertEqual('pending');
    });

    it('persists last-visit timestamp per (hostId, agentId) across restart without cross-PA bleed (P1-9)', 0, () => {
      const storage = new MemoryVisitStorage();
      const paStore1 = new OrbitPersonalAgentStore();
      paStore1.bindVisitStorage(storage);

      // Host A PA leaves at t=1000; Host B PA leaves at t=5000.
      paStore1.recordPersonalAgentVisitNow('host-a', 'pa-a', 1000);
      paStore1.recordPersonalAgentVisitNow('host-b', 'pa-b', 5000);

      // Simulate cold restart with a brand-new store instance bound to the same persisted storage.
      const paStore2 = new OrbitPersonalAgentStore();
      paStore2.bindVisitStorage(storage);

      const snapA = paStore2.enterPersonalAgentVisit('host-a', 'pa-a');
      expect(snapA).assertEqual(1000);
      expect(paStore2.sinceLastVisitMs).assertEqual(1000);

      // Activity at t=3000 counts for Host A PA (>1000) but NOT for Host B PA (<=5000)
      const activity: PersonalAgentActivityEntryDto[] = [
        {
          seq: 1,
          timestamp_ms: 3000,
          actor: 'personal_agent',
          decision: 'allow_once',
          host_id: 'host-a',
          agent_id: 'w-1',
          agent_title: 'Worker',
          summary: 'Auto approved',
          risk_level: 'low',
          reason: 'Safe',
          request_id: 'r1'
        }
      ];
      const briefingZeroCore: PersonalAgentBriefingDto = {
        personal_agent: null,
        needs_you: [],
        missions: [],
        other_active: [],
        waiting: [],
        hosts: [],
        counts: { needs_you: 0, running: 0, done_recent: 0, hosts_online: 1, hosts_total: 1 },
        since_last_visit: { completed: 0, auto_handled: 0 }
      };
      const awayA = computeSinceLastVisitSummary(briefingZeroCore, activity, paStore2.sinceLastVisitMs);
      expect(awayA.hasSummary).assertTrue();
      expect(awayA.autoHandled).assertEqual(1);

      const snapB = paStore2.enterPersonalAgentVisit('host-b', 'pa-b');
      expect(snapB).assertEqual(5000);
      const awayB = computeSinceLastVisitSummary(briefingZeroCore, activity, paStore2.sinceLastVisitMs);
      expect(awayB.hasSummary).assertFalse();
      expect(awayB.autoHandled).assertEqual(0);
    });
  });

  describe('OrbitPr2SidebarTopBarResizeAndModelFallback', () => {
    it('filters empty home workspace, excludes PA from Pinned/Projects, and paginates threads', 0, () => {
      const paConv = makeSidebarConv('host-1', 'pa-1', 'Personal Agent', '/Users/jess');
      const regularConv = makeSidebarConv('host-1', 'ag-1', 'Fix sidebar', '/Users/jess/Code/orbit');
      const homeOnlyWithPa = makeProjectGroup('home-grp', 'host-1', 'jess', '/Users/jess', true, [paConv]);
      const emptyPinnedRepo = makeProjectGroup('empty-repo', 'host-1', 'docs', '/Users/jess/Code/docs', false, []);
      const activeRepo = makeProjectGroup('orbit-repo', 'host-1', 'orbit', '/Users/jess/Code/orbit', false, [regularConv]);

      const sidebar: FleetSidebarDto = {
        revision: 1,
        needs_attention: [paConv],
        pinned_conversations: [paConv, regularConv],
        projects: [homeOnlyWithPa, emptyPinnedRepo, activeRepo],
        launchpad_projects: []
      };

      const store = new OrbitFleetStore();
      store.setSelectedHostId('host-1');
      store.setPersonalAgentKey('host-1', 'pa-1');
      store.applySidebarView(sidebar);

      const keys = store.sidebarRows.map((r: OrbitSidebarRow): string => r.key);
      expect(keys.indexOf('conv:attn:host-1:pa-1') >= 0).assertTrue();
      expect(keys.indexOf('conv:pin:host-1:pa-1')).assertEqual(-1);
      expect(keys.indexOf('proj:home-grp')).assertEqual(-1);
      expect(keys.indexOf('proj:empty-repo') >= 0).assertTrue();
      expect(keys.indexOf('proj:orbit-repo') >= 0).assertTrue();

      const convs: SidebarConversationDto[] = [];
      for (let i = 0; i < ORBIT_MAX_VISIBLE_THREADS_PER_PROJECT + 2; i += 1) {
        convs.push(makeSidebarConv('host-1', `ag-${i}`, `Thread ${i}`, '/Users/jess/Code/orbit'));
      }
      const group = makeProjectGroup('orbit-repo', 'host-1', 'orbit', '/Users/jess/Code/orbit', false, convs);
      const pageRows = buildOrbitSidebarRows(
        { revision: 2, needs_attention: [], pinned_conversations: [], projects: [group], launchpad_projects: [] },
        'host-1',
        '',
        new Set<string>(),
        new Set<string>(),
        ''
      );
      const moreRow = pageRows.find((r: OrbitSidebarRow): boolean => r.key === 'proj:more:orbit-repo');
      expect(moreRow?.hiddenCount).assertEqual(2);
    });

    it('reconciles Briefing Sheet vs 304 vp rail bidirectionally across 1023->1024->1023 vp (P1-12)', 0, () => {
      // Open Sheet at 1023 -> widens to 1024 -> shrinks back to 1023: open state preserved bidirectionally.
      const toWide = reconcileBriefingPresentationOnResize(1023, 1024, true, true, false, 1024, true);
      expect(toWide.briefingViewingOpen).assertTrue();
      expect(toWide.showBriefingSheet).assertFalse();
      expect(toWide.showBriefingPanel).assertTrue();

      const backToCompact = reconcileBriefingPresentationOnResize(
        1024,
        1023,
        true,
        toWide.showBriefingSheet,
        toWide.showBriefingPanel,
        1024,
        toWide.briefingViewingOpen
      );
      expect(backToCompact.briefingViewingOpen).assertTrue();
      expect(backToCompact.showBriefingSheet).assertTrue();
      expect(backToCompact.showBriefingPanel).assertFalse();

      // Explicitly closed Briefing stays closed across 1023 -> 1024 -> 1023
      const closedWide = reconcileBriefingPresentationOnResize(1023, 1024, true, false, false, 1024, false);
      expect(closedWide.showBriefingSheet).assertFalse();
      expect(closedWide.showBriefingPanel).assertFalse();
      const closedBack = reconcileBriefingPresentationOnResize(1024, 1023, true, false, false, 1024, false);
      expect(closedBack.showBriefingSheet).assertFalse();
      expect(closedBack.showBriefingPanel).assertFalse();

      // Non-PA session always forces Sheet & rail closed
      const nonPa = reconcileBriefingPresentationOnResize(800, 900, false, true, true, 1024, true);
      expect(nonPa.showBriefingSheet).assertFalse();
      expect(nonPa.showBriefingPanel).assertFalse();

      // Cold-start wide (1200 vp) deep link where PA identity arrives late auto-opens 304 vp rail
      // and yields any layout-default Diff rail (`userOpenedDiffInspector = false`), while preserving
      // an explicitly user-opened Diff rail (`userOpenedDiffInspector = true`).
      const latePaOpen = resolvePaIdentityArrivalPresentation(1200, true, false, false, false, false, false, 1024);
      expect(latePaOpen.briefingViewingOpen).assertTrue();
      expect(latePaOpen.showBriefingPanel).assertTrue();
      expect(latePaOpen.showBriefingSheet).assertFalse();
      expect(latePaOpen.shouldCloseDefaultDiffInspector).assertTrue();
      const latePaUserDiff = resolvePaIdentityArrivalPresentation(1200, true, false, false, false, true, false, 1024);
      expect(latePaUserDiff.showBriefingPanel).assertFalse();
      expect(latePaUserDiff.shouldCloseDefaultDiffInspector).assertFalse();
      // Respects explicit user dismissal when PA identity refreshes later
      const latePaDismissed = resolvePaIdentityArrivalPresentation(1200, true, false, false, false, false, true, 1024);
      expect(latePaDismissed.briefingViewingOpen).assertFalse();
      expect(latePaDismissed.showBriefingPanel).assertFalse();

      // Same-PA reselect (e.g. second deep link to already-active PA) preserves user dismissal!
      const reselectDismissed = resolvePaSessionSelectPresentation(1200, true, false, false, false, true, false, 1024);
      expect(reselectDismissed.showBriefingPanel).assertFalse();
      expect(reselectDismissed.briefingViewingOpen).assertFalse();
      // Fresh navigation into PA on wide screen opens Briefing and closes layout-default Diff
      const freshPaSelect = resolvePaSessionSelectPresentation(1200, false, false, false, false, false, false, 1024);
      expect(freshPaSelect.showBriefingPanel).assertTrue();
      expect(freshPaSelect.shouldCloseDefaultDiffInspector).assertTrue();
    });

    it('keeps host name visible and never shows tilde for draft, titled session, and PA (P1-15)', 0, () => {
      for (const emptyCwd of ['', '~']) {
        // 1. Draft session
        const draft = resolveTopBarDisplayModel({
          hasPairedHosts: true,
          isShowingPaSetup: false,
          isPersonalAgentSession: false,
          isDraft: true,
          currentAgentTitle: '',
          activeCwd: emptyCwd,
          hostName: 'Mac Studio',
          hostCount: 2,
          isConnectionHealthy: true,
          connectionState: 'online_direct',
          viaSshTunnel: false,
          gitBranch: ''
        });
        expect(draft.titleKind).assertEqual('text');
        expect(draft.titleText).assertEqual('Mac Studio');
        expect(draft.subtitleKind).assertEqual('select_project');
        expect(draft.titleText.indexOf('~')).assertEqual(-1);

        // 2. Titled regular session: keeps title AND surfaces host name in subtitle
        const titled = resolveTopBarDisplayModel({
          hasPairedHosts: true,
          isShowingPaSetup: false,
          isPersonalAgentSession: false,
          isDraft: false,
          currentAgentTitle: 'Refactor Auth',
          activeCwd: emptyCwd,
          hostName: 'Linux Devbox',
          hostCount: 2,
          isConnectionHealthy: true,
          connectionState: 'online_relay',
          viaSshTunnel: false,
          gitBranch: ''
        });
        expect(titled.titleKind).assertEqual('text');
        expect(titled.titleText).assertEqual('Refactor Auth');
        expect(titled.subtitleKind).assertEqual('host_select_project');
        expect(titled.subtitleHostName).assertEqual('Linux Devbox');

        // 3. Personal Agent session: keeps PA title AND surfaces host name in subtitle
        const pa = resolveTopBarDisplayModel({
          hasPairedHosts: true,
          isShowingPaSetup: false,
          isPersonalAgentSession: true,
          isDraft: false,
          currentAgentTitle: 'Personal Agent',
          activeCwd: emptyCwd,
          hostName: 'Mac mini',
          hostCount: 1,
          isConnectionHealthy: true,
          connectionState: 'online_direct',
          viaSshTunnel: false,
          gitBranch: ''
        });
        expect(pa.titleKind).assertEqual('pa');
        expect(pa.subtitleKind).assertEqual('host_select_project');
        expect(pa.subtitleHostName).assertEqual('Mac mini');
      }
    });

    it('filters default model slugs and resolves ACP agent labels (P1-10)', 0, () => {
      const catalogs: ProviderModelCatalogDto[] = [
        {
          provider: 'claude_code',
          models: [
            { id: 'claude-sonnet-4-5', label: 'Claude Sonnet 4.5', description: '', is_default: true, default_effort: null, thinking_options: [] }
          ],
          configured_default_model: null,
          configured_default_effort: null
        }
      ];
      const agDefault = makeAgent('ag-d', 'Test', '/repo');
      agDefault.model = 'Default';
      expect(resolveCompactAgentModelLabel(agDefault, catalogs)).assertEqual('');

      const agClaude = makeAgent('ag-c', 'Test', '/repo');
      agClaude.model = 'claude-sonnet-4-5';
      expect(resolveCompactAgentModelLabel(agClaude, catalogs)).assertEqual('Sonnet 4.5');

      const agAcp = makeAgent('ag-acp', 'ACP', '/repo');
      agAcp.provider = 'acp';
      agAcp.model = 'custom-acp-model';
      agAcp.labels = { 'orbit.acp_agent_id': 'opencode-cli' };
      expect(resolveCompactAgentModelLabel(agAcp, catalogs)).assertEqual('custom-acp-model');
      expect(resolveAgentFallbackLabel(agAcp)).assertEqual('opencode-cli');

      // P1-10: Crew consumes core-projected host_id & depth (including remote depth=4) and maps denied -> failed
      const remoteSub: SessionSubagentEntryDto = {
        id: 'sub-remote-4',
        role: 'Remote Verifier',
        prompt_preview: 'Run suite',
        status: 'denied',
        child_agent_id: 'ag-remote-child',
        last_seq: 9,
        host_id: 'host-remote',
        host_name: 'Linux Box',
        depth: 4
      };
      expect(resolveCrewSubagentHostId(remoteSub, 'host-local')).assertEqual('host-remote');
      expect(
        resolveCrewSubagentDepth(
          remoteSub,
          'ag-parent',
          'host-local',
          (_hid: string, _aid: string): AgentCardDto | null => null
        )
      ).assertEqual(4);
      expect(mapCrewSubagentStatus(remoteSub, null)).assertEqual('failed');
      // P1-10: Explicit wire `denied` ALWAYS maps to `failed` and NEVER compares parent `last_seq`
      // against child's independent `latest_seq` (even stale cached `running` with latest_seq=90 > last_seq=17)!
      const deniedParentSub: SessionSubagentEntryDto = {
        id: 'sub-denied-17',
        role: 'Remote Verifier',
        prompt_preview: 'Run suite',
        status: 'denied',
        child_agent_id: 'ag-remote-child',
        last_seq: 17,
        host_id: 'host-remote',
        host_name: 'Linux Box',
        depth: 4
      };
      const staleCachedIdle = makeAgent('ag-remote-child', 'Remote Verifier', '/repo', 'idle', false, false);
      staleCachedIdle.latest_seq = 8;
      expect(mapCrewSubagentStatus(deniedParentSub, staleCachedIdle)).assertEqual('failed');
      const staleChildRunningHigherSeq = makeAgent('ag-remote-child', 'Remote Verifier', '/repo', 'running', true, false);
      staleChildRunningHigherSeq.latest_seq = 90;
      expect(mapCrewSubagentStatus(deniedParentSub, staleChildRunningHigherSeq)).assertEqual('failed');
    });

    it('isolates pinned Diff empty state & collab seq, formats trilingual time, and reuses same-visit briefing without timeout refetch (P1-4, Nit-2, Nit-4)', 0, () => {
      // P1-4: Pinned decision Diff inspector with empty target diff MUST NOT fall back to PA sessionDiffs!
      const paOnlyDiff: DiffFileDto = {
        path: 'pa-only.txt',
        additions: 2,
        deletions: 0,
        lines: []
      };
      const pinnedEmpty = selectDisplayedDiffFiles(true, [], [paOnlyDiff]);
      expect(pinnedEmpty.length).assertEqual(0);
      const unpinnedFallback = selectDisplayedDiffFiles(false, [], [paOnlyDiff]);
      expect(unpinnedFallback.length).assertEqual(1);
      expect(unpinnedFallback[0].path).assertEqual('pa-only.txt');

      // P1-4: Late PR/stash response from repo A (seq 1) is rejected once inspector is pinned to repo B (seq 2)
      expect(isGitCollabResponseCurrent(1, 2, 'host-1', 'host-1', '/repo/a', '/repo/b')).assertFalse();
      expect(isGitCollabResponseCurrent(2, 2, 'host-1', 'host-1', '/repo/b', '/repo/b')).assertTrue();
      // P1-4: When pinned to (L, /repo/worker), switching activeCwd to /repo/pa (e.g. sidebar File Explorer)
      // MUST NOT overwrite the pinned inspector's diff list — including distinct /home/<user>/project paths!
      expect(shouldApplyGitDiffForTarget(true, 'host-L', '/repo/worker', 'host-L', '/repo/pa')).assertFalse();
      expect(shouldApplyGitDiffForTarget(true, 'host-L', '/repo/worker', 'host-R', '/repo/worker')).assertFalse();
      expect(shouldApplyGitDiffForTarget(true, 'host-L', '/repo/worker', 'host-L', '/repo/worker')).assertTrue();
      expect(shouldApplyGitDiffForTarget(false, '', '', 'host-L', '/repo/pa')).assertTrue();
      // Distinct absolute user home dirs on the same host MUST NEVER collide!
      expect(gitTargetPathsEqual('/home/worker/project', '/home/pa/project')).assertFalse();
      expect(gitTargetPathsEqual('/Users/alice/project', '/Users/bob/project')).assertFalse();
      expect(gitTargetPathsEqual('/home/worker/project/', '/home/worker/project')).assertTrue();
      expect(gitTargetPathsEqual('~/project', '/home/worker/project', '/home/worker')).assertTrue();
      expect(gitTargetPathsEqual('~/project', '/home/pa/project', '/home/worker')).assertFalse();
      expect(gitTargetPathsEqual('~/project', '/home/pa/project')).assertFalse();
      expect(shouldApplyGitDiffForTarget(true, 'host-L', '/home/worker/project', 'host-L', '/home/pa/project')).assertFalse();
      expect(cwdPathsEquivalent('/home/worker/project', '/home/pa/project')).assertFalse();
      expect(cwdPathsEquivalent('~/project', '/home/worker/project')).assertTrue();
      // POSIX literal backslash directory names (`team\project` vs `team/project`) MUST stay distinct;
      // only explicit Windows drive/UNC paths normalize `\` -> `/`.
      expect(gitTargetPathsEqual('/tmp/team\\project', '/tmp/team/project')).assertFalse();
      expect(shouldApplyGitDiffForTarget(true, 'host-L', '/tmp/team\\project', 'host-L', '/tmp/team/project')).assertFalse();
      expect(gitTargetPathsEqual('/tmp/team\\project/', '/tmp/team\\project')).assertTrue();
      expect(gitTargetPathsEqual('C:\\Users\\alice\\repo\\', 'C:/Users/alice/repo')).assertTrue();
      // Legal POSIX trailing space directory names MUST stay strictly distinct (never `.trim()` paths!)
      expect(gitTargetPathsEqual('/tmp/project', '/tmp/project ')).assertFalse();
      expect(cwdPathsEquivalent('/tmp/project', '/tmp/project ')).assertFalse();
      expect(cwdPathsEquivalent('/tmp/project /', '/tmp/project ')).assertTrue();
      expect(shouldApplyGitDiffForTarget(true, 'host-L', '/tmp/project', 'host-L', '/tmp/project ')).assertFalse();
      expect(shouldApplyGitDiffForTarget(true, 'host-L', '/tmp/project ', 'host-L', '/tmp/project')).assertFalse();
      expect(gitTargetPathsEqual('/tmp/project /', '/tmp/project ')).assertTrue();
      // DirectoryBrowser currentPath & parentPath MUST preserve trailing spaces verbatim (only trim for emptiness)
      expect(resolveDirectoryBrowserCurrentPath('/tmp/project ', '/tmp/fallback')).assertEqual('/tmp/project ');
      expect(resolveDirectoryBrowserCurrentPath('   ', '/tmp/project ')).assertEqual('/tmp/project ');
      expect(resolveDirectoryBrowserCurrentPath(null, '   ')).assertEqual('~');
      expect(resolveDirectoryBrowserParentPath('/tmp/parent ')).assertEqual('/tmp/parent ');
      expect(resolveDirectoryBrowserParentPath('   ')).assertEqual(null);
      // Windows UNC (`\\server\share\p` / `//server/share/p`) & drive roots (`C:\` / `C:/` vs `C:`)
      // MUST normalize idempotently!
      const normUncBack = normalizeGitTargetPath('\\\\server\\share\\p');
      expect(normUncBack).assertEqual('//server/share/p');
      expect(normalizeGitTargetPath(normUncBack)).assertEqual('//server/share/p');
      expect(gitTargetPathsEqual('\\\\server\\share\\p', '//server/share/p')).assertTrue();
      expect(gitTargetPathsEqual('\\\\server\\share\\p\\', '//server//share/p/')).assertTrue();
      expect(normalizeGitTargetPath('C:\\')).assertEqual('C:/');
      expect(normalizeGitTargetPath('C:/')).assertEqual('C:/');
      expect(normalizeGitTargetPath(normalizeGitTargetPath('C:\\'))).assertEqual('C:/');
      expect(normalizeGitTargetPath('C:')).assertEqual('C:');
      expect(gitTargetPathsEqual('C:\\', 'C:/')).assertTrue();
      expect(gitTargetPathsEqual('C:\\', 'C:')).assertFalse();

      // Nit-2: Trilingual compact relative time & countdown formatting (en, zh-CN, ja)
      const zhTemplates: CompactTimeTemplates = {
        justNow: '刚刚',
        secondsShort: '%d秒',
        minutesShort: '%d分钟',
        hoursShort: '%d小时',
        daysShort: '%d天',
        weeksShort: '%d周',
        minutesSecondsShort: '%1$d分%2$d秒',
        hoursMinutesShort: '%1$d小时%2$d分'
      };
      const jaTemplates: CompactTimeTemplates = {
        justNow: 'たった今',
        secondsShort: '%d秒',
        minutesShort: '%d分',
        hoursShort: '%d時間',
        daysShort: '%d日',
        weeksShort: '%d週間',
        minutesSecondsShort: '%1$d分%2$d秒',
        hoursMinutesShort: '%1$d時間%2$d分'
      };
      const nowMs = 1700000000000;
      const fiveMinSpec = resolveRelativeTimeSpec(nowMs - 5 * 60 * 1000, nowMs);
      expect(formatCompactTimeWithTemplates(fiveMinSpec, EN_COMPACT_TIME_TEMPLATES)).assertEqual('5m');
      expect(formatCompactTimeWithTemplates(fiveMinSpec, zhTemplates)).assertEqual('5分钟');
      expect(formatCompactTimeWithTemplates(fiveMinSpec, jaTemplates)).assertEqual('5分');

      const countdownSpec = resolveCountdownTimeSpec(nowMs + (3 * 60 + 15) * 1000, nowMs);
      expect(formatCompactTimeWithTemplates(countdownSpec, EN_COMPACT_TIME_TEMPLATES)).assertEqual('3m 15s');
      expect(formatCompactTimeWithTemplates(countdownSpec, zhTemplates)).assertEqual('3分15秒');
      expect(formatCompactTimeWithTemplates(countdownSpec, jaTemplates)).assertEqual('3分15秒');

      // Nit-4: Entry fetch before entering PA visit (openPersonalAgentHome flow) is adopted
      // deterministically so panel mount (even 1650ms+ later) stays strictly 1 RPC.
      const paBridge = new MockPersonalAgentBridge();
      const paStore = new OrbitPersonalAgentStore();
      paStore.bindBridge(paBridge);
      paStore.ensureFreshBriefing(); // idle fetch during openPersonalAgentHome()
      expect(paBridge.briefingCalls).assertEqual(1);
      paStore.enterPersonalAgentVisit('host-1', 'pa-1');
      paStore.ensureFreshBriefing(); // panel mount reuse (regardless of elapsed wall time)
      paStore.ensureFreshBriefing(); // toggle/resize reuse
      expect(paBridge.briefingCalls).assertEqual(1);
      // Explicit invalidation refreshes cleanly
      paStore.refreshBriefing(true);
      expect(paBridge.briefingCalls).assertEqual(2);
      expect(paStore.isBriefingDirty()).assertFalse();

      // Failed explicit refresh MUST preserve previous snapshot AND keep dirty=true so next
      // ensureFreshBriefing() in the SAME visit retries and loads version 2 once bridge recovers!
      paBridge.failNextBriefing = true;
      paStore.refreshBriefing(true);
      expect(paBridge.briefingCalls).assertEqual(3);
      expect(paStore.briefing !== null).assertTrue();
      expect(paStore.briefing?.counts.done_recent).assertEqual(0);
      expect(paStore.isBriefingDirty()).assertTrue();

      paBridge.briefingPayload =
        '{"personal_agent":{"host_id":"host-1","agent_id":"pa-1","title":"Personal Agent","host_name":"Mac"},' +
        '"needs_you":[],"missions":[],"other_active":[],"waiting":[],"hosts":[],' +
        '"counts":{"needs_you":0,"running":0,"done_recent":7,"hosts_online":1,"hosts_total":1},' +
        '"since_last_visit":{"completed":0,"auto_handled":0}}';
      paStore.ensureFreshBriefing();
      expect(paBridge.briefingCalls).assertEqual(4);
      expect(paStore.briefing?.counts.done_recent).assertEqual(7);
      expect(paStore.isBriefingDirty()).assertFalse();

      // Leaving and starting a brand-new visit fetches once again
      paStore.clearActiveVisitKey();
      paStore.enterPersonalAgentVisit('host-1', 'pa-1');
      paStore.ensureFreshBriefing();
      expect(paBridge.briefingCalls).assertEqual(5);
      paStore.dispose();
    });
  });
}
