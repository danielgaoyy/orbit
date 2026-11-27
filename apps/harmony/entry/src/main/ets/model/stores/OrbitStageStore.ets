import {
  AgentCardDto,
  AgentStageDeltaDto,
  AgentStageDeltaRequestDto,
  DiffFileDto,
  PendingPlanApprovalDto,
  SessionInsightsDto,
  StageTurnDto,
  TimelineItemDto
} from '../OrbitTypes';
import { OrbitStageBridge } from '../OrbitCoreBridge';
import { orbitLogWarn } from '../OrbitLogger';
import { OrbitPerfProbe } from '../OrbitPerfProbe';
import {
  OrbitStageTurnModel,
  OrbitTimelineItemModel,
  deriveTimelineItemKey
} from '../viewmodels/OrbitStageModels';

export const ORBIT_STAGE_TAIL_LIMIT_COMPACT: number = 24;
export const ORBIT_STAGE_TAIL_LIMIT_WIDE: number = 40;
export const ORBIT_STAGE_OLDER_PAGE_SIZE: number = 20;
const MAX_CACHED_STAGE_SESSIONS: number = 4;

export interface OrbitStageReloadOptions {
  limit: number;
}

export type OrbitStageFetchReason = 'initial' | 'incremental' | 'reload' | 'resync' | 'gap_recovery';

export function resolveStageTailLimit(windowWidthVp: number): number {
  return windowWidthVp >= 680 ? ORBIT_STAGE_TAIL_LIMIT_WIDE : ORBIT_STAGE_TAIL_LIMIT_COMPACT;
}

export function stageSessionKey(hostId: string, agentId: string): string {
  return `${hostId}::${agentId}`;
}

@ObservedV2
export class OrbitStageSession {
  readonly key: string;
  readonly hostId: string;
  readonly agentId: string;
  @Trace public revision: number = 0;
  @Trace public commitRevision: number = 0;
  @Trace public auxRevision: number = 0;
  @Trace turns: OrbitStageTurnModel[] = [];
  @Trace totalTurnCount: number = 0;
  @Trace windowStartTurnIndex: number = 0;
  @Trace hasOlderTurns: boolean = false;
  @Trace hasOlderHistory: boolean = false;
  @Trace latestSeq: number = 0;
  @Trace agent: AgentCardDto | null = null;
  @Trace pendingPermissions: OrbitTimelineItemModel[] = [];
  @Trace pendingQuestions: OrbitTimelineItemModel[] = [];
  @Trace pendingPlanApproval: PendingPlanApprovalDto | null = null;
  @Trace sessionDiffs: DiffFileDto[] = [];
  @Trace insights: SessionInsightsDto | null = null;
  @Trace activeTurnKey: string = '';
  @Trace loadingOlder: boolean = false;
  generation: number = 0;
  lastAccessOrder: number = 0;

  private turnByKey: Map<string, OrbitStageTurnModel> = new Map<string, OrbitStageTurnModel>();
  private permByKey: Map<string, OrbitTimelineItemModel> = new Map<string, OrbitTimelineItemModel>();
  private questionByKey: Map<string, OrbitTimelineItemModel> = new Map<string, OrbitTimelineItemModel>();

  constructor(hostId: string, agentId: string) {
    this.hostId = hostId;
    this.agentId = agentId;
    this.key = stageSessionKey(hostId, agentId);
  }

  public getTurnByKey(turnKey: string): OrbitStageTurnModel | undefined {
    return this.turnByKey.get(turnKey);
  }

  public registerTurn(turn: OrbitStageTurnModel): void {
    this.turnByKey.set(turn.turnKey, turn);
  }

  public clearTurnsAndIndex(): void {
    if (this.turns.length > 0) {
      this.turns.splice(0, this.turns.length);
    }
    this.turnByKey.clear();
  }

  public pruneTurnIndexToCurrentTurns(): void {
    const liveKeys: Set<string> = new Set<string>();
    for (let i = 0; i < this.turns.length; i++) {
      liveKeys.add(this.turns[i].turnKey);
    }
    const staleKeys: string[] = [];
    this.turnByKey.forEach((_val: OrbitStageTurnModel, k: string): void => {
      if (!liveKeys.has(k)) {
        staleKeys.push(k);
      }
    });
    for (let i = 0; i < staleKeys.length; i++) {
      this.turnByKey.delete(staleKeys[i]);
    }
  }

  public applyPendingPermissions(dtos: TimelineItemDto[]): void {
    const nextList: OrbitTimelineItemModel[] = [];
    const nextKeys: Set<string> = new Set<string>();
    for (let i = 0; i < dtos.length; i++) {
      const dto = dtos[i];
      const key = deriveTimelineItemKey(dto);
      nextKeys.add(key);
      let model = this.permByKey.get(key);
      if (model !== undefined) {
        model.patchFromDto(dto);
      } else {
        model = new OrbitTimelineItemModel(dto);
        this.permByKey.set(key, model);
      }
      nextList.push(model);
    }
    const stale: string[] = [];
    this.permByKey.forEach((_v: OrbitTimelineItemModel, k: string): void => {
      if (!nextKeys.has(k)) {
        stale.push(k);
      }
    });
    for (let i = 0; i < stale.length; i++) {
      this.permByKey.delete(stale[i]);
    }
    this.pendingPermissions = nextList;
  }

  public applyPendingQuestions(dtos: TimelineItemDto[]): void {
    const nextList: OrbitTimelineItemModel[] = [];
    const nextKeys: Set<string> = new Set<string>();
    for (let i = 0; i < dtos.length; i++) {
      const dto = dtos[i];
      const key = deriveTimelineItemKey(dto);
      nextKeys.add(key);
      let model = this.questionByKey.get(key);
      if (model !== undefined) {
        model.patchFromDto(dto);
      } else {
        model = new OrbitTimelineItemModel(dto);
        this.questionByKey.set(key, model);
      }
      nextList.push(model);
    }
    const stale: string[] = [];
    this.questionByKey.forEach((_v: OrbitTimelineItemModel, k: string): void => {
      if (!nextKeys.has(k)) {
        stale.push(k);
      }
    });
    for (let i = 0; i < stale.length; i++) {
      this.questionByKey.delete(stale[i]);
    }
    this.pendingQuestions = nextList;
  }
}

export interface OrbitStageDeltaAppliedCallback {
  onDeltaApplied(session: OrbitStageSession, delta: AgentStageDeltaDto): void;
}

/**
 * Domain store for active & LRU-cached (up to 4) semantic Stage sessions.
 * Implements the 8-step in-place `@ObservedV2` `applyDelta` pipeline, strict
 * 3-condition aux guard, generation-based stale response rejection, and tail pagination.
 */
@ObservedV2
export class OrbitStageStore {
  @Trace public activeSession: OrbitStageSession | null = null;
  @Trace public activeRevision: number = 0;
  @Trace public activePendingPlanApproval: PendingPlanApprovalDto | null = null;
  @Trace public activeTurnKey: string = '';

  private bridge: OrbitStageBridge | null = null;
  private readonly sessionByKey: Map<string, OrbitStageSession> = new Map<string, OrbitStageSession>();
  private accessClock: number = 0;
  private currentTailLimit: number = ORBIT_STAGE_TAIL_LIMIT_COMPACT;
  private inFlightGenByKey: Map<string, number> = new Map<string, number>();
  private inFlightPromiseByKey: Map<string, Promise<boolean>> = new Map<string, Promise<boolean>>();
  private pendingRefetchByKey: Set<string> = new Set<string>();
  private appliedCallback: OrbitStageDeltaAppliedCallback | null = null;

  public bindBridge(bridge: OrbitStageBridge | null): void {
    this.bridge = bridge;
  }

  public setDeltaAppliedCallback(cb: OrbitStageDeltaAppliedCallback | null): void {
    this.appliedCallback = cb;
  }

  public setTailLimit(limit: number): void {
    if (limit > 0) {
      this.currentTailLimit = limit;
    }
  }

  public getTailLimit(): number {
    return this.currentTailLimit;
  }

  public getOrCreateSession(hostId: string, agentId: string): OrbitStageSession {
    const key = stageSessionKey(hostId, agentId);
    const existing = this.sessionByKey.get(key);
    this.accessClock += 1;
    if (existing !== undefined) {
      existing.lastAccessOrder = this.accessClock;
      return existing;
    }
    const created = new OrbitStageSession(hostId, agentId);
    created.lastAccessOrder = this.accessClock;
    this.sessionByKey.set(key, created);
    this.evictOldestSessionsIfNeeded();
    return created;
  }

  public getSession(hostId: string, agentId: string): OrbitStageSession | null {
    const found = this.sessionByKey.get(stageSessionKey(hostId, agentId));
    return found !== undefined ? found : null;
  }

  public activateSession(hostId: string, agentId: string): OrbitStageSession | null {
    if (hostId.length === 0 || agentId.length === 0) {
      this.clearActiveSession();
      return null;
    }
    const prev = this.activeSession;
    const next = this.getOrCreateSession(hostId, agentId);
    if (prev !== null && prev.key !== next.key) {
      // Bump previous session generation so any in-flight responses for the old session drop cleanly.
      prev.generation += 1;
    }
    this.activeSession = next;
    this.syncActiveSessionMirrors(next);
    return next;
  }

  public clearActiveSession(): void {
    if (this.activeSession !== null) {
      this.activeSession.generation += 1;
    }
    this.activeSession = null;
    this.activeRevision = 0;
    this.activePendingPlanApproval = null;
    this.activeTurnKey = '';
  }

  public dropSession(hostId: string, agentId: string): void {
    const key = stageSessionKey(hostId, agentId);
    const existing = this.sessionByKey.get(key);
    if (existing !== undefined) {
      existing.generation += 1;
      this.sessionByKey.delete(key);
      if (this.activeSession !== null && this.activeSession.key === key) {
        this.activeSession = null;
        this.activeRevision = 0;
        this.activePendingPlanApproval = null;
        this.activeTurnKey = '';
      }
    }
  }

  public trimBackgroundCaches(): void {
    const activeKey = this.activeSession !== null ? this.activeSession.key : '';
    const keysToDrop: string[] = [];
    this.sessionByKey.forEach((_sess: OrbitStageSession, k: string): void => {
      if (k !== activeKey) {
        keysToDrop.push(k);
      }
    });
    for (let i = 0; i < keysToDrop.length; i++) {
      this.sessionByKey.delete(keysToDrop[i]);
    }
  }

  /**
   * 8-step in-place `applyDelta` specification (§6.2-C):
   * 1. Reject mismatched generation.
   * 2. Compute 3-condition `mustApplyAux` guard.
   * 3. Update all 5 aux fields + `auxRevision` ONLY when `mustApplyAux === true`.
   * 4. Clear `turns` & `turnByKey` when `delta.reset === true`.
   * 5. If `replace_from_turn_index > turns.length`, trigger `reloadFull` (bounded) and return.
   * 6. Patch/reuse `OrbitStageTurnModel` instances in place from `replace_from_turn_index` + splice tail.
   * 7. Update `revision`, `totalTurnCount`, `windowStartTurnIndex`, `hasOlderTurns`, `latestSeq`.
   * 8. Recompute `activeTurnKey` as the last turn with `hasActiveItem`.
   */
  public applyDelta(
    session: OrbitStageSession,
    delta: AgentStageDeltaDto,
    generation: number
  ): boolean {
    // Step 1: Reject stale responses after session switch / reload.
    if (generation !== session.generation) {
      return false;
    }

    const startUs = OrbitPerfProbe.nowUs();
    try {
      // Step 2: Strict 3-condition aux guard (root fix for aux_unchanged wiping approvals).
      const incomingAuxRev = delta.aux_revision !== undefined ? delta.aux_revision : session.auxRevision;
      const mustApplyAux: boolean =
        !delta.aux_unchanged ||
        delta.reset === true ||
        incomingAuxRev !== session.auxRevision;

      // Step 3: Only mutate aux fields when mustApplyAux is true.
      if (mustApplyAux) {
        session.applyPendingPermissions(delta.pending_permissions ?? []);
        session.applyPendingQuestions(delta.pending_questions ?? []);
        session.pendingPlanApproval = delta.pending_plan_approval ?? null;
        session.sessionDiffs = delta.session_turn_diffs ?? [];
        session.insights = delta.insights ?? null;
        session.auxRevision = incomingAuxRev;
      }

      // Step 4: On reset, clear turns & turn lookup index.
      if (delta.reset) {
        session.clearTurnsAndIndex();
      }

      // Step 5: Gap guard — never attempt out-of-bounds local patch; do a bounded reloadFull.
      const replaceFrom = delta.replace_from_turn_index;
      if (replaceFrom > session.turns.length) {
        this.reloadFull(session, { limit: this.currentTailLimit }, 'gap_recovery');
        return false;
      }

      // Step 6: In-place patch reusing OrbitStageTurnModel instances by turnKey + tail splice.
      const changedTurns = delta.changed_turns ?? [];
      for (let i = 0; i < changedTurns.length; i++) {
        const turnDto = changedTurns[i];
        const targetIdx = replaceFrom + i;
        let model = session.getTurnByKey(turnDto.key);
        if (model !== undefined) {
          model.patchFromDto(turnDto);
        } else {
          model = new OrbitStageTurnModel(turnDto);
          session.registerTurn(model);
        }
        if (targetIdx < session.turns.length) {
          if (session.turns[targetIdx] !== model) {
            session.turns[targetIdx] = model;
          }
        } else {
          session.turns.push(model);
        }
      }

      const nextLen = replaceFrom + changedTurns.length;
      if (session.turns.length > nextLen) {
        session.turns.splice(nextLen, session.turns.length - nextLen);
      }
      session.pruneTurnIndexToCurrentTurns();

      // Step 7: Update revision & pagination metadata.
      if (delta.window_start_turn_index !== undefined) {
        session.windowStartTurnIndex = delta.window_start_turn_index;
      }
      session.revision = delta.revision;
      session.totalTurnCount = delta.total_turn_count;
      session.hasOlderTurns = delta.has_older_turns === true || session.windowStartTurnIndex > 0;
      session.hasOlderHistory = delta.has_older_history === true || session.hasOlderTurns;
      session.latestSeq = delta.latest_seq;
      session.loadingOlder = false;
      if (delta.agent !== null) {
        session.agent = delta.agent;
      }

      // Step 8: activeTurnKey = last turn with hasActiveItem (or empty string).
      let lastActiveKey = '';
      for (let i = session.turns.length - 1; i >= 0; i--) {
        if (session.turns[i].hasActiveItem) {
          lastActiveKey = session.turns[i].turnKey;
          break;
        }
      }
      session.activeTurnKey = lastActiveKey;
      session.commitRevision += 1;

      if (this.activeSession !== null && this.activeSession.key === session.key) {
        this.syncActiveSessionMirrors(session);
      }
      if (this.appliedCallback !== null) {
        this.appliedCallback.onDeltaApplied(session, delta);
      }
      return true;
    } finally {
      OrbitPerfProbe.tick('stage.tick', Math.max(0, OrbitPerfProbe.nowUs() - startUs));
    }
  }

  public async fetchIncremental(
    session: OrbitStageSession,
    limit: number = this.currentTailLimit
  ): Promise<boolean> {
    const reason: OrbitStageFetchReason = session.revision === 0 ? 'initial' : 'incremental';
    return await this.fetchDeltaInternal(session, limit, false, reason);
  }

  public async reloadFull(
    session: OrbitStageSession,
    opts: OrbitStageReloadOptions,
    reason: OrbitStageFetchReason = 'reload'
  ): Promise<boolean> {
    const boundedLimit = opts.limit > 0 ? opts.limit : this.currentTailLimit;
    session.generation += 1;
    return await this.fetchDeltaInternal(session, boundedLimit, true, reason);
  }

  public async resyncActiveSession(limit: number = this.currentTailLimit): Promise<boolean> {
    const active = this.activeSession;
    if (active === null) {
      return false;
    }
    return await this.reloadFull(active, { limit: limit }, 'resync');
  }

  public loadOlder(session: OrbitStageSession): number {
    if (this.bridge === null || session.loadingOlder) {
      return 0;
    }
    if (session.windowStartTurnIndex > 0) {
      session.loadingOlder = true;
      try {
        const start = Math.max(0, session.windowStartTurnIndex - ORBIT_STAGE_OLDER_PAGE_SIZE);
        const count = session.windowStartTurnIndex - start;
        if (count <= 0) {
          session.loadingOlder = false;
          session.hasOlderTurns = false;
          return 0;
        }
        const rawJson = this.bridge.agentStageTurnsSliceJson(
          session.hostId,
          session.agentId,
          start,
          count
        );
        const sliceDtos = JSON.parse(rawJson) as StageTurnDto[];
        const prepended: OrbitStageTurnModel[] = [];
        for (let i = 0; i < sliceDtos.length; i++) {
          const turnDto = sliceDtos[i];
          let model = session.getTurnByKey(turnDto.key);
          if (model !== undefined) {
            model.patchFromDto(turnDto);
          } else {
            model = new OrbitStageTurnModel(turnDto);
            session.registerTurn(model);
          }
          prepended.push(model);
        }
        for (let i = prepended.length - 1; i >= 0; i--) {
          session.turns.unshift(prepended[i]);
        }
        session.windowStartTurnIndex = start;
        session.hasOlderTurns = start > 0;
        if (!session.hasOlderTurns && !session.hasOlderHistory) {
          session.hasOlderHistory = false;
        }
        session.loadingOlder = false;
        session.commitRevision += 1;
        if (this.activeSession !== null && this.activeSession.key === session.key) {
          this.syncActiveSessionMirrors(session);
        }
        return prepended.length;
      } catch (err) {
        session.loadingOlder = false;
        orbitLogWarn('OrbitStageStore', 'agentStageTurnsSliceJson failed', err as Object);
        return 0;
      }
    }

    if (session.hasOlderHistory) {
      session.loadingOlder = true;
      try {
        const requested = this.bridge.loadOlderAgentTimeline(session.hostId, session.agentId, 120);
        if (!requested) {
          session.loadingOlder = false;
          session.hasOlderHistory = false;
        }
      } catch (err) {
        session.loadingOlder = false;
        orbitLogWarn('OrbitStageStore', 'loadOlderAgentTimeline failed', err as Object);
      }
    }
    return 0;
  }

  private async fetchDeltaInternal(
    session: OrbitStageSession,
    limit: number,
    forceFull: boolean,
    reason: OrbitStageFetchReason
  ): Promise<boolean> {
    if (this.bridge === null) {
      return false;
    }
    if (!forceFull && this.inFlightGenByKey.has(session.key)) {
      this.pendingRefetchByKey.add(session.key);
      const existingPromise = this.inFlightPromiseByKey.get(session.key);
      if (existingPromise !== undefined) {
        return await existingPromise;
      }
      return false;
    }

    const reqGeneration = session.generation;
    const sinceRevision = forceFull ? 0 : session.revision;
    const boundedLimit = limit > 0 ? limit : this.currentTailLimit;
    const req: AgentStageDeltaRequestDto = {
      host_id: session.hostId,
      agent_id: session.agentId,
      since_revision: sinceRevision,
      since_aux_revision: (forceFull || session.revision === 0) ? null : session.auxRevision,
      turn_limit: boundedLimit,
      since_window_start_turn_index: (forceFull || session.revision === 0)
        ? null
        : session.windowStartTurnIndex
    };

    if (sinceRevision === 0) {
      if (reason === 'reload' || reason === 'resync' || reason === 'gap_recovery' || reason === 'initial') {
        OrbitPerfProbe.count('stage.delta.since_zero.reload');
      } else {
        OrbitPerfProbe.count('stage.delta.since_zero.unexpected');
      }
    }

    this.inFlightGenByKey.set(session.key, reqGeneration);
    const runPromise = (async (): Promise<boolean> => {
      try {
        if (this.bridge === null) {
          return false;
        }
        const rawJson = await this.bridge.agentStageDeltaByRequestJsonAsync(JSON.stringify(req));
        if (reqGeneration !== session.generation) {
          return false;
        }
        const delta = JSON.parse(rawJson) as AgentStageDeltaDto;
        if (delta.aux_unchanged === true) {
          OrbitPerfProbe.recordBytes('stage.delta.aux_unchanged', rawJson.length);
        }
        return this.applyDelta(session, delta, reqGeneration);
      } catch (err) {
        orbitLogWarn('OrbitStageStore', 'agentStageDeltaByRequestJsonAsync failed', err as Object);
        return false;
      } finally {
        if (this.inFlightGenByKey.get(session.key) === reqGeneration) {
          this.inFlightGenByKey.delete(session.key);
          this.inFlightPromiseByKey.delete(session.key);
        }
        if (this.pendingRefetchByKey.has(session.key) && !this.inFlightGenByKey.has(session.key)) {
          this.pendingRefetchByKey.delete(session.key);
          if (this.sessionByKey.has(session.key) && reqGeneration === session.generation) {
            this.fetchIncremental(session, boundedLimit).catch((err: Object) => {
              orbitLogWarn('OrbitStageStore', 'Coalesced stage refetch failed', err);
            });
          }
        }
      }
    })();
    this.inFlightPromiseByKey.set(session.key, runPromise);
    return await runPromise;
  }

  private syncActiveSessionMirrors(session: OrbitStageSession): void {
    if (this.activeRevision !== session.revision) {
      this.activeRevision = session.revision;
    }
    if (this.activePendingPlanApproval !== session.pendingPlanApproval) {
      this.activePendingPlanApproval = session.pendingPlanApproval;
    }
    if (this.activeTurnKey !== session.activeTurnKey) {
      this.activeTurnKey = session.activeTurnKey;
    }
  }

  private evictOldestSessionsIfNeeded(): void {
    while (this.sessionByKey.size > MAX_CACHED_STAGE_SESSIONS) {
      const activeKey = this.activeSession !== null ? this.activeSession.key : '';
      let oldestKey = '';
      let oldestOrder = Number.MAX_SAFE_INTEGER;
      this.sessionByKey.forEach((sess: OrbitStageSession, k: string): void => {
        if (k !== activeKey && sess.lastAccessOrder < oldestOrder) {
          oldestOrder = sess.lastAccessOrder;
          oldestKey = k;
        }
      });
      if (oldestKey.length === 0) {
        break;
      }
      this.sessionByKey.delete(oldestKey);
    }
  }
}
