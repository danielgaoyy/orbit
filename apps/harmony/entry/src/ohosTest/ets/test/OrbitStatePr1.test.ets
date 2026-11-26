import { describe, expect, it } from '@ohos/hypium';
import {
  AgentStageDeltaDto,
  AgentStageDeltaRequestDto,
  DiffFileDto,
  PendingPlanApprovalDto,
  PersistedDraftPayloadV1Dto,
  SessionInsightsDto,
  StageTurnDto,
  TimelineItemDto,
  TurnLiveSegmentDto,
  TurnWorkCountsDto
} from '../../../main/ets/model/OrbitTypes';
import {
  OrbitComposerDraftBridge,
  OrbitStageBridge
} from '../../../main/ets/model/OrbitCoreBridge';
import {
  OrbitEventFlushHandler,
  OrbitEventFrameScheduler
} from '../../../main/ets/model/OrbitEventFrameScheduler';
import {
  ORBIT_DRAFTS_STORAGE_KEY,
  OrbitComposerStore
} from '../../../main/ets/model/stores/OrbitComposerStore';
import {
  OrbitStageSession,
  OrbitStageStore
} from '../../../main/ets/model/stores/OrbitStageStore';

function emptyWorkCounts(): TurnWorkCountsDto {
  return {
    edited_file_count: 0,
    created_file_count: 0,
    command_count: 0,
    read_file_count: 0,
    search_count: 0,
    subagent_count: 0,
    mcp_server_count: 0,
    other_tool_count: 0,
    thought_count: 0,
    narration_count: 0,
    action_steps: 0,
    total_steps: 0,
    failed_count: 0,
    denied_count: 0,
    unrecovered_failure_count: 0,
    additions: 0,
    deletions: 0
  };
}

function makePermissionItem(seq: number, reqId: string, title: string): TimelineItemDto {
  return {
    item_type: 'permission_request',
    seq: seq,
    request_id: reqId,
    title: title,
    allowed_decisions: ['allow_once', 'allow_always', 'deny']
  };
}

function makeQuestionItem(seq: number, reqId: string, questionText: string): TimelineItemDto {
  return {
    item_type: 'user_question',
    seq: seq,
    request_id: reqId,
    title: 'Clarification',
    questions: [
      {
        id: 'q1',
        header: 'Scope',
        question: questionText,
        options: ['Option A', 'Option B'],
        is_multi_select: false
      }
    ]
  };
}

function makePlanApproval(seq: number, reqId: string, title: string): PendingPlanApprovalDto {
  return {
    seq: seq,
    request_id: reqId,
    title: title,
    plan_markdown: '# Plan\n1. Verify'
  };
}

function makeDiffFile(path: string, additions: number): DiffFileDto {
  return {
    path: path,
    additions: additions,
    deletions: 0,
    lines: [
      {
        kind: 'add',
        old_line: null,
        new_line: 1,
        content: '+const ok = true;'
      }
    ]
  };
}

function makeToolItem(seq: number, callId: string, status: string, title: string): TimelineItemDto {
  return {
    item_type: 'tool_call',
    seq: seq,
    call_id: callId,
    status: status,
    summary_title: title
  };
}

function makeTurn(
  turnKey: string,
  turnIndex: number,
  segments: TurnLiveSegmentDto[],
  hasActiveItem: boolean
): StageTurnDto {
  return {
    key: turnKey,
    turn_index: turnIndex,
    anchor_seq: (turnIndex + 1) * 10,
    user_prompt: {
      item_type: 'user_prompt',
      seq: (turnIndex + 1) * 10,
      text: `Prompt ${turnIndex}`
    },
    is_compaction_command: false,
    folded_steps: [],
    work_counts: emptyWorkCounts(),
    work_summary_label: 'Worked',
    work_summary_live_label: 'Working',
    work_summary_includes_failure: false,
    thought_duration_ms: null,
    turn_duration_ms: null,
    terminal_reply: null,
    notices: [],
    live_segments: segments,
    turn_diffs: [],
    has_error: false,
    has_unrecovered_error: false,
    has_active_item: hasActiveItem,
    started_at_ms: 1700000000000,
    completed_at_ms: null
  };
}

function makeDelta(
  revision: number,
  auxRevision: number,
  auxUnchanged: boolean,
  reset: boolean,
  replaceFrom: number,
  changedTurns: StageTurnDto[],
  totalTurnCount: number,
  perms: TimelineItemDto[],
  questions: TimelineItemDto[],
  plan: PendingPlanApprovalDto | null,
  diffs: DiffFileDto[],
  insights: SessionInsightsDto | undefined
): AgentStageDeltaDto {
  return {
    revision: revision,
    host_id: 'host-1',
    agent: null,
    replace_from_turn_index: replaceFrom,
    window_start_turn_index: 0,
    changed_turns: changedTurns,
    total_turn_count: totalTurnCount,
    pending_permissions: perms,
    pending_questions: questions,
    pending_plan_approval: plan,
    session_turn_diffs: diffs,
    insights: insights,
    latest_seq: revision * 10,
    reset: reset,
    aux_revision: auxRevision,
    aux_unchanged: auxUnchanged,
    has_older_turns: false,
    has_older_history: false
  };
}

class FakeDraftBridge implements OrbitComposerDraftBridge {
  public readonly storage: Map<string, string> = new Map<string, string>();
  public setCalls: number = 0;
  public deleteCalls: number = 0;

  getComposerDraftJson(draftKey: string): string | null {
    const val = this.storage.get(draftKey);
    return val !== undefined ? val : null;
  }

  setComposerDraft(draftKey: string, draftJson: string): void {
    this.setCalls += 1;
    this.storage.set(draftKey, draftJson);
  }

  deleteComposerDraft(draftKey: string): void {
    this.deleteCalls += 1;
    this.storage.delete(draftKey);
  }
}

class FakeStageBridge implements OrbitStageBridge {
  public readonly requests: AgentStageDeltaRequestDto[] = [];
  private nextRevision: number = 10;

  async agentStageDeltaByRequestJsonAsync(reqJson: string): Promise<string> {
    const parsed = JSON.parse(reqJson) as AgentStageDeltaRequestDto;
    this.requests.push(parsed);
    const rev = this.nextRevision;
    this.nextRevision += 1;
    const isFull = parsed.since_revision === 0;
    const turn = makeTurn(
      'turn-0',
      0,
      [
        {
          kind: 'step_group',
          key: 'seg-work-0',
          items: [makeToolItem(rev, 'call-1', 'running', `Step ${rev}`)],
          has_running_step: true,
          has_following_content: false
        }
      ],
      true
    );
    const delta = makeDelta(
      rev,
      1,
      !isFull,
      isFull,
      0,
      [turn],
      1,
      isFull ? [makePermissionItem(100, 'perm-1', 'Allow shell')] : [],
      [],
      null,
      [],
      undefined
    );
    return JSON.stringify(delta);
  }

  agentStageTurnsSliceJson(
    _hostId: string,
    _agentId: string,
    _startTurnIndex: number,
    _count: number
  ): string {
    return '[]';
  }

  loadOlderAgentTimeline(_hostId: string, _agentId: string, _limitEvents: number): boolean {
    return false;
  }
}

class TestEventFlushHandler implements OrbitEventFlushHandler {
  private readonly stageStore: OrbitStageStore;
  public fleetDirtyCount: number = 0;
  public resyncCallCount: number = 0;
  public pendingFetch: Promise<boolean> = Promise.resolve(false);

  constructor(stageStore: OrbitStageStore) {
    this.stageStore = stageStore;
  }

  getSelectedHostId(): string {
    return 'host-1';
  }

  isStageScopeVisible(hostId: string, agentId: string): boolean {
    return hostId === 'host-1' && agentId === 'agent-1';
  }

  isGitScopeVisible(_hostId: string, _cwd: string): boolean {
    return false;
  }

  isTerminalScopeVisible(_hostId: string, _slot: number): boolean {
    return false;
  }

  onFleetDirty(): void {
    this.fleetDirtyCount += 1;
  }

  onBriefingDirty(): void {
  }

  onSelectedHostDirty(_hostId: string): void {
  }

  onBackgroundHostDirty(_hostId: string): void {
  }

  onVisibleTimelineDirty(hostId: string, agentId: string): void {
    const sess = this.stageStore.getOrCreateSession(hostId, agentId);
    this.pendingFetch = this.stageStore.fetchIncremental(sess, 24);
  }

  onBackgroundTimelineDirty(_hostId: string, _agentId: string): void {
  }

  onVisibleGitDirty(_hostId: string, _cwd: string): void {
  }

  onBackgroundGitDirty(_hostId: string, _cwd: string): void {
  }

  onVisibleTerminalDirty(_hostId: string, _slot: number): void {
  }

  onResync(): void {
    this.resyncCallCount += 1;
    this.pendingFetch = this.stageStore.resyncActiveSession(24);
  }
}

export default function orbitStatePr1Test(): void {
  describe('OrbitStatePr1', () => {
    it('auxUnchangedKeepsPending', 0, () => {
      const store = new OrbitStageStore();
      const session = store.activateSession('host-1', 'agent-1') as OrbitStageSession;

      const turn1 = makeTurn('turn-0', 0, [], true);
      const initialDelta = makeDelta(
        1,
        1,
        false,
        true,
        0,
        [turn1],
        1,
        [makePermissionItem(11, 'perm-1', 'Run cargo test')],
        [makeQuestionItem(12, 'q-1', 'Continue?')],
        makePlanApproval(13, 'plan-1', 'Refactor plan'),
        [makeDiffFile('src/main.rs', 5)],
        { detected_local_urls: ['http://127.0.0.1:6868'] }
      );
      expect(store.applyDelta(session, initialDelta, session.generation)).assertTrue();

      const prevPerms = session.pendingPermissions;
      const prevQuestions = session.pendingQuestions;
      const prevPlan = session.pendingPlanApproval;
      const prevDiffs = session.sessionDiffs;
      const prevInsights = session.insights;

      expect(prevPerms.length).assertEqual(1);
      expect(prevQuestions.length).assertEqual(1);
      expect(prevPlan !== null).assertTrue();
      expect(prevDiffs.length).assertEqual(1);
      expect(prevInsights !== null).assertTrue();
      expect(session.auxRevision).assertEqual(1);

      // Streaming delta with aux_unchanged=true and empty aux payloads MUST NOT wipe pending items.
      const streamingDelta = makeDelta(
        2,
        1,
        true,
        false,
        0,
        [makeTurn('turn-0', 0, [], true)],
        1,
        [],
        [],
        null,
        [],
        undefined
      );
      expect(store.applyDelta(session, streamingDelta, session.generation)).assertTrue();

      expect(session.pendingPermissions === prevPerms).assertTrue();
      expect(session.pendingPermissions.length).assertEqual(1);
      expect(session.pendingPermissions[0].requestId).assertEqual('perm-1');
      expect(session.pendingQuestions === prevQuestions).assertTrue();
      expect(session.pendingQuestions.length).assertEqual(1);
      expect(session.pendingPlanApproval === prevPlan).assertTrue();
      expect(session.sessionDiffs === prevDiffs).assertTrue();
      expect(session.insights === prevInsights).assertTrue();
      expect(session.auxRevision).assertEqual(1);
    });

    it('auxChangedAppliesAll', 0, () => {
      const store = new OrbitStageStore();
      const session = store.activateSession('host-1', 'agent-1') as OrbitStageSession;

      const delta1 = makeDelta(
        1,
        1,
        false,
        true,
        0,
        [makeTurn('turn-0', 0, [], true)],
        1,
        [makePermissionItem(11, 'perm-1', 'First approval')],
        [makeQuestionItem(12, 'q-1', 'First question')],
        makePlanApproval(13, 'plan-1', 'Plan 1'),
        [makeDiffFile('a.ets', 2)],
        { detected_local_urls: ['http://localhost:3000'] }
      );
      expect(store.applyDelta(session, delta1, session.generation)).assertTrue();

      // aux_unchanged=false updates all 5 aux fields and auxRevision.
      const delta2 = makeDelta(
        2,
        2,
        false,
        false,
        0,
        [makeTurn('turn-0', 0, [], true)],
        1,
        [
          makePermissionItem(11, 'perm-1', 'First approval'),
          makePermissionItem(21, 'perm-2', 'Second approval')
        ],
        [makeQuestionItem(22, 'q-2', 'Second question')],
        makePlanApproval(23, 'plan-2', 'Plan 2'),
        [makeDiffFile('a.ets', 4), makeDiffFile('b.ets', 7)],
        { detected_local_urls: ['http://localhost:8080'] }
      );
      expect(store.applyDelta(session, delta2, session.generation)).assertTrue();

      expect(session.auxRevision).assertEqual(2);
      expect(session.pendingPermissions.length).assertEqual(2);
      expect(session.pendingPermissions[1].requestId).assertEqual('perm-2');
      expect(session.pendingQuestions.length).assertEqual(1);
      expect(session.pendingQuestions[0].requestId).assertEqual('q-2');
      expect(session.pendingPlanApproval !== null).assertTrue();
      expect(session.pendingPlanApproval?.request_id).assertEqual('plan-2');
      expect(store.activePendingPlanApproval?.request_id).assertEqual('plan-2');
      expect(session.sessionDiffs.length).assertEqual(2);
      expect(session.sessionDiffs[1].path).assertEqual('b.ets');
      expect(session.insights?.detected_local_urls?.[0]).assertEqual('http://localhost:8080');

      // 3rd guard condition: aux_revision mismatch still applies even if aux_unchanged=true.
      const delta3 = makeDelta(
        3,
        3,
        true,
        false,
        0,
        [makeTurn('turn-0', 0, [], false)],
        1,
        [],
        [],
        null,
        [],
        { detected_local_urls: [] }
      );
      expect(store.applyDelta(session, delta3, session.generation)).assertTrue();
      expect(session.auxRevision).assertEqual(3);
      expect(session.pendingPermissions.length).assertEqual(0);
      expect(session.pendingQuestions.length).assertEqual(0);
      expect(session.pendingPlanApproval === null).assertTrue();
      expect(session.sessionDiffs.length).assertEqual(0);
    });

    it('patchReusesTurnIdentity', 0, () => {
      const store = new OrbitStageStore();
      const session = store.activateSession('host-1', 'agent-1') as OrbitStageSession;

      const delta1 = makeDelta(
        1,
        1,
        false,
        true,
        0,
        [
          makeTurn(
            'turn-0',
            0,
            [
              {
                kind: 'step_group',
                key: 'seg-work-0',
                items: [makeToolItem(11, 'call-1', 'running', 'Read file')],
                has_running_step: true,
                has_following_content: false
              }
            ],
            true
          )
        ],
        1,
        [],
        [],
        null,
        [],
        undefined
      );
      expect(store.applyDelta(session, delta1, session.generation)).assertTrue();

      const firstTurnRef = session.turns[0];
      const firstItemRef = session.turns[0].liveSegments[0].items[0];
      firstTurnRef.stepsExpanded = true;
      firstItemRef.detailExpanded = true;

      const delta2 = makeDelta(
        2,
        1,
        true,
        false,
        0,
        [
          makeTurn(
            'turn-0',
            0,
            [
              {
                kind: 'step_group',
                key: 'seg-work-0',
                items: [
                  makeToolItem(11, 'call-1', 'completed', 'Read file'),
                  makeToolItem(12, 'call-2', 'running', 'Edit file')
                ],
                has_running_step: true,
                has_following_content: false
              }
            ],
            true
          )
        ],
        1,
        [],
        [],
        null,
        [],
        undefined
      );
      expect(store.applyDelta(session, delta2, session.generation)).assertTrue();

      expect(session.turns.length).assertEqual(1);
      expect(session.turns[0] === firstTurnRef).assertTrue();
      expect(session.turns[0].stepsExpanded).assertTrue();
      expect(session.turns[0].liveSegments[0].items.length).assertEqual(2);
      expect(session.turns[0].liveSegments[0].items[0] === firstItemRef).assertTrue();
      expect(session.turns[0].liveSegments[0].items[0].status).assertEqual('completed');
      expect(session.turns[0].liveSegments[0].items[0].detailExpanded).assertTrue();
      expect(session.turns[0].liveSegments[0].items[1].callId).assertEqual('call-2');
    });

    it('segmentKeyStable', 0, () => {
      const store = new OrbitStageStore();
      const session = store.activateSession('host-1', 'agent-1') as OrbitStageSession;

      const delta1 = makeDelta(
        1,
        1,
        false,
        true,
        0,
        [
          makeTurn(
            'turn-0',
            0,
            [
              {
                kind: 'step_group',
                key: 'seg-work-0',
                items: [makeToolItem(11, 'call-1', 'running', 'Step 1')],
                has_running_step: true,
                has_following_content: false
              }
            ],
            true
          )
        ],
        1,
        [],
        [],
        null,
        [],
        undefined
      );
      store.applyDelta(session, delta1, session.generation);

      const firstSegRef = session.turns[0].liveSegments[0];
      const firstKey = firstSegRef.segKey;
      expect(firstKey).assertEqual('seg-work-0');
      firstSegRef.stepsExpanded = true;

      const delta2 = makeDelta(
        2,
        1,
        true,
        false,
        0,
        [
          makeTurn(
            'turn-0',
            0,
            [
              {
                kind: 'step_group',
                key: 'seg-work-0',
                items: [
                  makeToolItem(11, 'call-1', 'completed', 'Step 1'),
                  makeToolItem(12, 'call-2', 'completed', 'Step 2'),
                  makeToolItem(13, 'call-3', 'completed', 'Step 3')
                ],
                has_running_step: false,
                has_following_content: true
              }
            ],
            false
          )
        ],
        1,
        [],
        [],
        null,
        [],
        undefined
      );
      store.applyDelta(session, delta2, session.generation);

      const updatedSeg = session.turns[0].liveSegments[0];
      expect(updatedSeg.segKey).assertEqual(firstKey);
      expect(updatedSeg === firstSegRef).assertTrue();
      expect(updatedSeg.items.length).assertEqual(3);
      expect(updatedSeg.isRunning).assertFalse();
      expect(updatedSeg.hasFollowingContent).assertTrue();
      expect(updatedSeg.stepsExpanded).assertTrue();
    });

    it('draftScopeIsolation', 0, () => {
      const fakeBridge = new FakeDraftBridge();
      const composer = new OrbitComposerStore();
      composer.bindBridge(fakeBridge);
      composer.hydrate(1700000000000);

      composer.setActiveScope('host-a', 'agent-a', 1700000001000);
      composer.setText('Draft for session A');
      composer.setSlashDirective('/plan');

      // Switching to session B must clear active editor state and flush A.
      composer.setActiveScope('host-a', 'agent-b', 1700000002000);
      expect(composer.activeScopeKey).assertEqual('host-a::agent-b');
      expect(composer.text).assertEqual('');
      expect(composer.slashDirective).assertEqual('');

      composer.setText('Draft for session B');

      // New-session draft scope uses draft::<hostId>.
      composer.setActiveScope('host-a', '', 1700000003000);
      expect(composer.activeScopeKey).assertEqual('draft::host-a');
      expect(composer.text).assertEqual('');
      composer.setText('New session draft');

      // Switching back restores exact per-scope drafts without bleeding across sessions.
      composer.setActiveScope('host-a', 'agent-a', 1700000004000);
      expect(composer.text).assertEqual('Draft for session A');
      expect(composer.slashDirective).assertEqual('/plan');

      composer.setActiveScope('host-a', 'agent-b', 1700000005000);
      expect(composer.text).assertEqual('Draft for session B');
      expect(composer.slashDirective).assertEqual('');

      composer.setActiveScope('host-a', '', 1700000006000);
      expect(composer.text).assertEqual('New session draft');

      composer.dispose();
    });

    it('draftSurvivesRestart', 0, () => {
      const fakeBridge = new FakeDraftBridge();
      const nowMs = 1700000010000;

      const store1 = new OrbitComposerStore();
      store1.bindBridge(fakeBridge);
      store1.hydrate(nowMs);
      store1.setActiveScope('host-1', 'agent-42', nowMs);
      store1.setText('Persisted across cold restart');
      store1.setSlashDirective('/review');
      store1.setSideQuestionDraft('Check edge cases');
      expect(store1.flushNow(nowMs + 500)).assertTrue();
      store1.dispose();

      const rawPersisted = fakeBridge.getComposerDraftJson(ORBIT_DRAFTS_STORAGE_KEY);
      expect(rawPersisted !== null).assertTrue();
      const parsed = JSON.parse(rawPersisted as string) as PersistedDraftPayloadV1Dto;
      expect(parsed.version).assertEqual(1);
      expect(parsed.drafts['host-1::agent-42'] !== undefined).assertTrue();

      // Simulate cold restart with brand new OrbitComposerStore over same SQLite backing map.
      const store2 = new OrbitComposerStore();
      store2.bindBridge(fakeBridge);
      store2.hydrate(nowMs + 1000);
      store2.setActiveScope('host-1', 'agent-42', nowMs + 1000);

      expect(store2.text).assertEqual('Persisted across cold restart');
      expect(store2.slashDirective).assertEqual('/review');
      expect(store2.sideQuestionDraft).assertEqual('Check edge cases');

      store2.dispose();
    });

    it('resyncForcesFullRefetch', 0, async (): Promise<void> => {
      const fakeBridge = new FakeStageBridge();
      const stageStore = new OrbitStageStore();
      stageStore.bindBridge(fakeBridge);
      stageStore.setTailLimit(24);
      const session = stageStore.activateSession('host-1', 'agent-1') as OrbitStageSession;

      const handler = new TestEventFlushHandler(stageStore);
      const scheduler = new OrbitEventFrameScheduler(handler);

      // 1. Initial load establishes revision=10.
      expect(await stageStore.fetchIncremental(session, 24)).assertTrue();
      expect(fakeBridge.requests.length).assertEqual(1);
      expect(fakeBridge.requests[0].since_revision).assertEqual(0);
      expect(session.revision).assertEqual(10);

      // 2. fleet_changed MUST NOT trigger any stage delta refetch.
      scheduler.enqueue('{"kind":"fleet_changed","revision":11}');
      scheduler.flushNow();
      expect(handler.fleetDirtyCount).assertEqual(1);
      expect(fakeBridge.requests.length).assertEqual(1);

      // 3. Normal timeline_changed uses incremental since_revision=10.
      scheduler.enqueue('{"kind":"timeline_changed","revision":11,"host_id":"host-1","agent_id":"agent-1"}');
      scheduler.flushNow();
      expect(await handler.pendingFetch).assertTrue();
      expect(fakeBridge.requests.length).assertEqual(2);
      expect(fakeBridge.requests[1].since_revision).assertEqual(10);
      expect(session.revision).assertEqual(11);

      // 4. resync_required forces full refetch with since_revision=0 and bounded turn_limit.
      scheduler.enqueue('{"kind":"resync_required","revision":12}');
      scheduler.flushNow();
      expect(await handler.pendingFetch).assertTrue();
      expect(handler.resyncCallCount).assertEqual(1);
      expect(fakeBridge.requests.length).assertEqual(3);
      expect(fakeBridge.requests[2].since_revision).assertEqual(0);
      expect(fakeBridge.requests[2].since_aux_revision).assertEqual(null);
      expect(fakeBridge.requests[2].turn_limit).assertEqual(24);

      scheduler.dispose();
    });
  });
}
