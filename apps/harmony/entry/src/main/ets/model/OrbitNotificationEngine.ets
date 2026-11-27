import {
  AgentCardDto,
  HostDetailDto,
  PersonalAgentBriefingDto,
  PushPreferencesDto,
  TimelineItemDto
} from './OrbitTypes';

const MAX_PREVIEW_CHARS: number = 120;

export interface LocalNotificationCopyStrings {
  readonly permissionTitle: string;
  readonly permissionBody: string;
  readonly turnCompletedTitle: string;
  readonly turnCompletedBody: string;
  readonly agentErrorTitle: string;
  readonly agentErrorBody: string;
  readonly goalFinishedTitle: string;
  readonly goalFinishedBody: string;
  readonly timerFiredTitle: string;
  readonly timerFiredBody: string;
}

export const DEFAULT_LOCAL_NOTIFICATION_COPY: LocalNotificationCopyStrings = {
  permissionTitle: 'Approval required',
  permissionBody: 'An agent is waiting for your approval.',
  turnCompletedTitle: 'Turn completed',
  turnCompletedBody: 'An agent finished its background turn.',
  agentErrorTitle: 'Agent error',
  agentErrorBody: 'An agent stopped with an error.',
  goalFinishedTitle: 'Goal finished',
  goalFinishedBody: 'An autonomous goal reached a terminal state.',
  timerFiredTitle: 'Timer fired',
  timerFiredBody: 'A scheduled timer triggered.'
};

export interface NotificationVisibilityContext {
  readonly isAppInBackground: boolean;
  readonly isNewSessionDraft: boolean;
  readonly showPersonalAgentSetup: boolean;
  readonly selectedHostId: string;
  readonly selectedAgentId: string;
}

export interface OrbitLocalNotificationPayload {
  readonly hostId: string;
  readonly agentId: string;
  readonly eventKind: string;
  readonly title: string;
  readonly body: string;
  readonly requestId: string;
  readonly seq: number;
}

export interface BuiltNotificationCopy {
  readonly title: string;
  readonly body: string;
}

export function defaultPushPreferences(): PushPreferencesDto {
  return {
    enabled: true,
    notify_permission_required: true,
    notify_turn_completed: true,
    notify_agent_error: true,
    notify_goal_finished: true,
    notify_timer_fired: true,
    include_message_preview: false
  };
}

export function clonePushPreferences(src: PushPreferencesDto): PushPreferencesDto {
  return {
    enabled: src.enabled === true,
    notify_permission_required: src.notify_permission_required === true,
    notify_turn_completed: src.notify_turn_completed === true,
    notify_agent_error: src.notify_agent_error === true,
    notify_goal_finished: src.notify_goal_finished === true,
    notify_timer_fired: src.notify_timer_fired === true,
    include_message_preview: src.include_message_preview === true
  };
}

export function isGoalTerminalStatus(status: string): boolean {
  const lower = status.trim().toLowerCase();
  return lower === 'succeeded' ||
    lower === 'failed' ||
    lower === 'canceled' ||
    lower === 'cancelled';
}

function sanitizePreviewText(raw: string): string {
  const collapsed = raw.replace(/\s+/g, ' ').trim();
  if (collapsed.length <= MAX_PREVIEW_CHARS) {
    return collapsed;
  }
  return `${collapsed.substring(0, MAX_PREVIEW_CHARS)}…`;
}

function normalizeErrorSummaryForDedup(raw: string): string {
  const collapsed = raw.replace(/\s+/g, ' ').trim().toLowerCase();
  const withoutEllipsis = collapsed.replace(/(\.{3}|…)+$/, '').trim();
  if (withoutEllipsis.length <= 64) {
    return withoutEllipsis;
  }
  return withoutEllipsis.substring(0, 64).trim();
}

/**
 * Centralized zero-knowledge notification copy builder matching `crates/orbit-daemon/src/push.rs`.
 * When `includeMessagePreview === false`, `optionalPreview` is completely ignored and NEVER
 * appears anywhere in `title` or `body`.
 */
export function buildLocalNotificationCopy(
  eventKind: string,
  hostName: string,
  includeMessagePreview: boolean,
  optionalPreview: string,
  copy: LocalNotificationCopyStrings = DEFAULT_LOCAL_NOTIFICATION_COPY
): BuiltNotificationCopy {
  const cleanHost = hostName.trim().length > 0 ? hostName.trim() : 'Orbit';
  let prefix = copy.turnCompletedTitle;
  let minimalBody = copy.turnCompletedBody;

  if (eventKind === 'permission_required') {
    prefix = copy.permissionTitle;
    minimalBody = copy.permissionBody;
  } else if (eventKind === 'agent_error') {
    prefix = copy.agentErrorTitle;
    minimalBody = copy.agentErrorBody;
  } else if (eventKind === 'goal_finished') {
    prefix = copy.goalFinishedTitle;
    minimalBody = copy.goalFinishedBody;
  } else if (eventKind === 'timer_fired') {
    prefix = copy.timerFiredTitle;
    minimalBody = copy.timerFiredBody;
  }

  const title = `${prefix} · ${cleanHost}`;
  let body = minimalBody;
  if (includeMessagePreview) {
    const cleanPreview = sanitizePreviewText(optionalPreview);
    if (cleanPreview.length > 0) {
      body = cleanPreview;
    }
  }
  return { title: title, body: body };
}

/**
 * Canonical conversation visibility check shared across ALL notification event kinds.
 */
export function isConversationVisiblyFocused(
  ctx: NotificationVisibilityContext,
  hostId: string,
  agentId: string
): boolean {
  if (ctx.isAppInBackground || ctx.isNewSessionDraft || ctx.showPersonalAgentSetup) {
    return false;
  }
  if (hostId.length === 0 || agentId.length === 0) {
    return false;
  }
  return ctx.selectedHostId === hostId && ctx.selectedAgentId === agentId;
}

function normalizeAgentState(ag: AgentCardDto): string {
  const raw = (ag.status?.state ?? '').trim().toLowerCase();
  if (raw.length > 0) {
    return raw;
  }
  if (ag.is_awaiting_permission) {
    return 'awaiting_permission';
  }
  if (ag.is_running) {
    return 'running';
  }
  return 'idle';
}

function isPriorActiveTurnState(state: string): boolean {
  // Strictly matches `crates/orbit-daemon/src/push.rs:580`:
  // only Running | AutonomousWakeup | AwaitingPermission -> Idle triggers `turn_completed`.
  // `initializing -> idle` MUST NOT trigger `turn_completed`.
  return state === 'running' ||
    state === 'autonomous_wakeup' ||
    state === 'awaiting_permission';
}

function parseTimestampMsSafe(rfc3339: string | null | undefined): number {
  if (typeof rfc3339 !== 'string' || rfc3339.trim().length === 0) {
    return 0;
  }
  const parsed = Date.parse(rfc3339);
  return Number.isFinite(parsed) && parsed > 0 ? parsed : 0;
}

interface AgentObservedVersion {
  readonly latestSeq: number;
  readonly updatedAtMs: number;
  readonly state: string;
}

interface NonErrorBaseline {
  readonly updatedAtMs: number;
  readonly seq: number;
  readonly fromBriefing: boolean;
}

interface AgentErrorEpisodeState {
  inError: boolean;
  notified: boolean;
  updatedAtMs: number;
  latestSeq: number;
  errorUpdatedAtMs: number;
  errorBriefingMs: number;
  errorSeq: number;
  minHostSeqForRecovery: number;
  minHostMsForRecovery: number;
  recoveredAtMs: number;
  recoveredByBriefing: boolean;
  lastErrorSeq: number;
  lastErrorUpdatedAtMs: number;
}

/**
 * Unified local notification transition engine mirroring `crates/orbit-daemon/src/push.rs:580`.
 * Eliminates duplicate state machines between HostDetail, StageDelta, and Briefing; enforces
 * monotonic version advancement across sources, zero-knowledge previews, and consistent
 * `(hostId, agentId)` visibility & event-ID deduplication.
 */
export class OrbitNotificationEngine {
  private readonly initializedHosts: Set<string> = new Set<string>();
  private hasBriefingBaseline: boolean = false;
  private readonly agentStateByKey: Map<string, string> = new Map<string, string>();
  private readonly agentVersionByKey: Map<string, AgentObservedVersion> = new Map<string, AgentObservedVersion>();
  private readonly lastCompletedSigByKey: Map<string, string> = new Map<string, string>();
  private readonly lastCompletedSeqByKey: Map<string, number> = new Map<string, number>();
  private readonly lastCompletedUpdatedMsByKey: Map<string, number> = new Map<string, number>();
  private readonly notifiedHostPermKeys: Set<string> = new Set<string>();
  private readonly notifiedBriefingPermKeys: Set<string> = new Set<string>();
  private readonly errorEpisodeByKey: Map<string, AgentErrorEpisodeState> =
    new Map<string, AgentErrorEpisodeState>();
  private readonly observedNonErrorByKey: Map<string, NonErrorBaseline> =
    new Map<string, NonErrorBaseline>();
  private readonly activeBriefingErrorKeys: Set<string> = new Set<string>();
  private readonly agentErrorSigByKey: Map<string, string> = new Map<string, string>();
  private readonly goalStatusByKey: Map<string, string> = new Map<string, string>();
  private readonly notifiedGoalTransitions: Set<string> = new Set<string>();

  private threadKey(hostId: string, agentId: string): string {
    return `${hostId}::${agentId}`;
  }

  private recordNonErrorBaseline(
    key: string,
    updatedAtMs: number,
    seq: number,
    fromBriefing: boolean
  ): void {
    const cleanMs = Number.isFinite(updatedAtMs) && updatedAtMs > 0 ? updatedAtMs : 0;
    const cleanSeq = !fromBriefing && Number.isFinite(seq) && seq > 0 ? Math.floor(seq) : 0;
    const prev = this.observedNonErrorByKey.get(key);
    if (prev === undefined) {
      this.observedNonErrorByKey.set(key, {
        updatedAtMs: cleanMs,
        seq: cleanSeq,
        fromBriefing: fromBriefing
      });
      return;
    }
    if (cleanMs > prev.updatedAtMs) {
      this.observedNonErrorByKey.set(key, {
        updatedAtMs: cleanMs,
        seq: cleanSeq,
        fromBriefing: fromBriefing
      });
      return;
    }
    if (cleanMs < prev.updatedAtMs) {
      return;
    }
    if (!fromBriefing && cleanSeq > 0) {
      if (prev.fromBriefing || cleanSeq > prev.seq) {
        this.observedNonErrorByKey.set(key, {
          updatedAtMs: cleanMs,
          seq: cleanSeq,
          fromBriefing: false
        });
      }
      return;
    }
    if (fromBriefing && !prev.fromBriefing && prev.seq === 0) {
      this.observedNonErrorByKey.set(key, {
        updatedAtMs: cleanMs,
        seq: 0,
        fromBriefing: true
      });
    }
  }

  private isHostErrorStaleAgainstNonErrorBaseline(
    key: string,
    incomingSeq: number,
    incomingUpdatedMs: number
  ): boolean {
    const baseline = this.observedNonErrorByKey.get(key);
    if (baseline === undefined || baseline.updatedAtMs <= 0 || incomingUpdatedMs <= 0) {
      return false;
    }
    if (incomingUpdatedMs < baseline.updatedAtMs) {
      return true;
    }
    if (incomingUpdatedMs === baseline.updatedAtMs) {
      if (baseline.fromBriefing) {
        return true;
      }
      if (incomingSeq === 0 || baseline.seq === 0 || incomingSeq <= baseline.seq) {
        return true;
      }
    }
    return false;
  }

  private isBriefingErrorStaleAgainstNonErrorBaseline(
    key: string,
    itemMs: number
  ): boolean {
    const baseline = this.observedNonErrorByKey.get(key);
    if (baseline === undefined || baseline.updatedAtMs <= 0 || itemMs <= 0) {
      return false;
    }
    // Briefing carries no monotonic seq: require strictly newer timestamp (`itemMs > baseline.updatedAtMs`)
    // against any known non-error baseline (from HostDetail or Briefing), symmetrically matching
    // `isHostErrorStaleAgainstNonErrorBaseline`.
    return itemMs <= baseline.updatedAtMs;
  }

  private clearPermKeysForAgent(store: Set<string>, hostId: string, agentId: string): void {
    const prefix = `perm:${hostId}::${agentId}`;
    const keys = Array.from(store.values());
    for (let i = 0; i < keys.length; i += 1) {
      if (keys[i] === prefix || keys[i].startsWith(`${prefix}::`)) {
        store.delete(keys[i]);
      }
    }
  }

  /**
   * Returns `true` when an incoming agent snapshot is stale relative to a previously observed
   * higher-version snapshot (e.g., a late StageDelta `running` with lower seq or unversioned
   * `seq === 0` at the same/earlier millisecond arriving after HostDetail already observed
   * `idle` at a positive `latestSeq`). Prevents state regression & duplicate `turn_completed`.
   */
  private isStaleAgentSnapshot(
    key: string,
    prevVer: AgentObservedVersion | undefined,
    incomingSeq: number,
    incomingUpdatedMs: number,
    nextState: string
  ): boolean {
    if (prevVer === undefined) {
      return false;
    }
    if (incomingSeq > 0 && prevVer.latestSeq > 0 && incomingSeq < prevVer.latestSeq) {
      return true;
    }
    if (incomingSeq === 0 && prevVer.latestSeq > 0) {
      // Unversioned snapshot (`seq === 0`) MUST NEVER regress a versioned snapshot (`latestSeq > 0`)
      // unless its timestamp is strictly newer; identical or missing timestamps are treated as stale.
      if (
        incomingUpdatedMs <= 0 ||
        prevVer.updatedAtMs <= 0 ||
        incomingUpdatedMs <= prevVer.updatedAtMs
      ) {
        return true;
      }
    }
    if (incomingSeq === prevVer.latestSeq) {
      if (incomingUpdatedMs > 0 && prevVer.updatedAtMs > 0 && incomingUpdatedMs < prevVer.updatedAtMs) {
        return true;
      }
      if (
        prevVer.state === 'idle' &&
        isPriorActiveTurnState(nextState) &&
        (incomingUpdatedMs === 0 || prevVer.updatedAtMs === 0 || incomingUpdatedMs <= prevVer.updatedAtMs)
      ) {
        return true;
      }
    }
    // Never let an unversioned or same-timestamp `running` snapshot re-arm completion after a
    // versioned (`seq > 0`) turn completion has already been recorded.
    const completedSeq = this.lastCompletedSeqByKey.get(key) ?? 0;
    const completedMs = this.lastCompletedUpdatedMsByKey.get(key) ?? 0;
    if (completedSeq > 0 && isPriorActiveTurnState(nextState)) {
      if (incomingSeq === 0 && (incomingUpdatedMs <= 0 || incomingUpdatedMs <= completedMs)) {
        return true;
      }
      if (incomingSeq > 0 && incomingSeq <= completedSeq && incomingUpdatedMs <= completedMs) {
        return true;
      }
    }
    if (incomingSeq === 0 && prevVer.updatedAtMs > 0 && incomingUpdatedMs > 0 && incomingUpdatedMs < prevVer.updatedAtMs) {
      return true;
    }
    return false;
  }

  private evaluateAgentStateTransitions(
    hostId: string,
    hostName: string,
    ag: AgentCardDto,
    isBaseline: boolean,
    ctx: NotificationVisibilityContext,
    prefs: PushPreferencesDto,
    copy: LocalNotificationCopyStrings,
    explicitPermissions: TimelineItemDto[] | null,
    out: OrbitLocalNotificationPayload[]
  ): void {
    const key = this.threadKey(hostId, ag.agent_id);
    const viewed = isConversationVisiblyFocused(ctx, hostId, ag.agent_id);
    const nextState = normalizeAgentState(ag);
    const incomingSeq = typeof ag.latest_seq === 'number' && Number.isFinite(ag.latest_seq)
      ? Math.max(0, Math.floor(ag.latest_seq))
      : 0;
    const incomingUpdatedMs = parseTimestampMsSafe(ag.updated_at_rfc3339);
    const prevVer = this.agentVersionByKey.get(key);
    if (this.isStaleAgentSnapshot(key, prevVer, incomingSeq, incomingUpdatedMs, nextState)) {
      return;
    }

    const prevState = this.agentStateByKey.get(key) ?? '';
    this.agentStateByKey.set(key, nextState);
    this.agentVersionByKey.set(key, {
      latestSeq: Math.max(incomingSeq, prevVer !== undefined ? prevVer.latestSeq : 0),
      updatedAtMs: Math.max(incomingUpdatedMs, prevVer !== undefined ? prevVer.updatedAtMs : 0),
      state: nextState
    });

    // (a) Permission required
    const hasPendingPerm = ag.is_awaiting_permission ||
      nextState === 'awaiting_permission' ||
      (explicitPermissions !== null && explicitPermissions.length > 0);
    if (hasPendingPerm) {
      if (explicitPermissions !== null && explicitPermissions.length > 0) {
        for (let i = 0; i < explicitPermissions.length; i += 1) {
          const item = explicitPermissions[i];
          const reqId = (item.request_id ?? '').trim();
          const suffix = reqId.length > 0 ? reqId : `seq_${item.seq}`;
          const permKey = `perm:${hostId}::${ag.agent_id}::${suffix}`;
          if (!this.notifiedHostPermKeys.has(permKey)) {
            this.notifiedHostPermKeys.add(permKey);
            if (!isBaseline && prefs.enabled && prefs.notify_permission_required && !viewed) {
              const previewText = (item.title ?? '').trim().length > 0
                ? (item.title ?? '').trim()
                : ag.title.trim();
              const built = buildLocalNotificationCopy(
                'permission_required',
                hostName,
                prefs.include_message_preview,
                previewText,
                copy
              );
              out.push({
                hostId: hostId,
                agentId: ag.agent_id,
                eventKind: 'permission_required',
                title: built.title,
                body: built.body,
                requestId: reqId,
                seq: item.seq
              });
            }
          }
        }
      } else {
        const reqId = (ag.status?.request_id ?? '').trim();
        const permKey = reqId.length > 0
          ? `perm:${hostId}::${ag.agent_id}::${reqId}`
          : `perm:${hostId}::${ag.agent_id}`;
        if (!this.notifiedHostPermKeys.has(permKey)) {
          this.notifiedHostPermKeys.add(permKey);
          if (!isBaseline && prefs.enabled && prefs.notify_permission_required && !viewed) {
            const built = buildLocalNotificationCopy(
              'permission_required',
              hostName,
              prefs.include_message_preview,
              ag.title.trim(),
              copy
            );
            // HostDetail only knows `agent.latest_seq` (not necessarily the approval item's seq),
            // so pass `seq: 0` here to avoid mismatched seq targeting when `requestId` is absent.
            out.push({
              hostId: hostId,
              agentId: ag.agent_id,
              eventKind: 'permission_required',
              title: built.title,
              body: built.body,
              requestId: reqId,
              seq: 0
            });
          }
        }
      }
    } else {
      this.clearPermKeysForAgent(this.notifiedHostPermKeys, hostId, ag.agent_id);
    }

    // (b) Turn completed — aligns strictly with `crates/orbit-daemon/src/push.rs:580`:
    // fires ONLY on `(Running | AutonomousWakeup | AwaitingPermission) -> Idle`.
    // Never fires on `initializing -> idle`, `running -> closed`, or `running -> error`.
    if (
      !isBaseline &&
      isPriorActiveTurnState(prevState) &&
      nextState === 'idle' &&
      !hasPendingPerm
    ) {
      const completionSig = `${incomingSeq}:${incomingUpdatedMs}`;
      const alreadyNotifiedSig = this.lastCompletedSigByKey.get(key) ?? '';
      if (completionSig !== alreadyNotifiedSig) {
        this.lastCompletedSigByKey.set(key, completionSig);
        if (incomingSeq > 0) {
          this.lastCompletedSeqByKey.set(key, Math.max(incomingSeq, this.lastCompletedSeqByKey.get(key) ?? 0));
        }
        if (incomingUpdatedMs > 0) {
          this.lastCompletedUpdatedMsByKey.set(
            key,
            Math.max(incomingUpdatedMs, this.lastCompletedUpdatedMsByKey.get(key) ?? 0)
          );
        }
        if (prefs.enabled && prefs.notify_turn_completed && !viewed) {
          const trimmedTitle = ag.title.trim();
          const preview = trimmedTitle.length > 0 ? `Completed: ${trimmedTitle}` : '';
          const built = buildLocalNotificationCopy(
            'turn_completed',
            hostName,
            prefs.include_message_preview,
            preview,
            copy
          );
          out.push({
            hostId: hostId,
            agentId: ag.agent_id,
            eventKind: 'turn_completed',
            title: built.title,
            body: built.body,
            requestId: '',
            seq: 0
          });
        }
      }
    }

    // (c) Agent error — shared versioned error/recovery state machine across HostDetail,
    // StageDelta, and Briefing:
    // - Never lets a stale/baseline-identical `running` snapshot clear an active error episode.
    // - Never lets a stale old `error` snapshot re-trigger after a newer recovery.
    // - Keeps `agentStateByKey` strictly decoupled from Briefing so `turn_completed` stays pure.
    const existingEp = this.errorEpisodeByKey.get(key);
    if (nextState === 'error') {
      const errMsg = (ag.status?.message ?? '').trim();
      const errSig = `error:${normalizeErrorSummaryForDedup(errMsg)}`;
      // Strictly discard errors dated before a known recovery boundary `recoveredAtMs` regardless
      // of HostDetail seq (since Briefing recovery carries no seq); only allow same-ms HostDetail
      // transitions when recovery itself came from HostDetail and seq strictly advanced.
      const isStaleErrorAfterRecovery =
        existingEp !== undefined &&
        !existingEp.inError &&
        (
          (
            existingEp.recoveredAtMs > 0 &&
            incomingUpdatedMs > 0 &&
            (
              incomingUpdatedMs < existingEp.recoveredAtMs ||
              (
                incomingUpdatedMs === existingEp.recoveredAtMs &&
                (
                  existingEp.recoveredByBriefing ||
                  incomingSeq === 0 ||
                  existingEp.latestSeq === 0 ||
                  incomingSeq <= existingEp.latestSeq
                )
              )
            )
          ) ||
          (
            existingEp.lastErrorUpdatedAtMs > 0 &&
            incomingUpdatedMs > 0 &&
            incomingUpdatedMs <= existingEp.lastErrorUpdatedAtMs &&
            (existingEp.lastErrorSeq === 0 || incomingSeq === 0 || incomingSeq <= existingEp.lastErrorSeq)
          ) ||
          (
            existingEp.lastErrorSeq > 0 &&
            incomingSeq > 0 &&
            incomingSeq <= existingEp.lastErrorSeq &&
            (incomingUpdatedMs <= 0 || existingEp.lastErrorUpdatedAtMs <= 0 || incomingUpdatedMs <= existingEp.lastErrorUpdatedAtMs)
          )
        );
      const isStaleErrorBeforeBaseline = this.isHostErrorStaleAgainstNonErrorBaseline(
        key,
        incomingSeq,
        incomingUpdatedMs
      );
      if (!isStaleErrorAfterRecovery && !isStaleErrorBeforeBaseline) {
        const alreadyInErrorOrNotified =
          existingEp !== undefined && (existingEp.inError || existingEp.notified);
        const isErrorBaselineForAgent =
          isBaseline &&
          existingEp === undefined &&
          !this.observedNonErrorByKey.has(key);
        const shouldNotify = !isErrorBaselineForAgent && !alreadyInErrorOrNotified;
        const nextEp: AgentErrorEpisodeState = {
          inError: true,
          notified: true,
          updatedAtMs: Math.max(incomingUpdatedMs, existingEp !== undefined ? existingEp.updatedAtMs : 0),
          latestSeq: Math.max(incomingSeq, existingEp !== undefined ? existingEp.latestSeq : 0),
          errorUpdatedAtMs: Math.max(
            incomingUpdatedMs,
            existingEp !== undefined && existingEp.inError ? existingEp.errorUpdatedAtMs : 0
          ),
          errorBriefingMs: existingEp !== undefined && existingEp.inError ? existingEp.errorBriefingMs : 0,
          errorSeq: Math.max(
            incomingSeq,
            existingEp !== undefined && existingEp.inError ? existingEp.errorSeq : 0
          ),
          minHostSeqForRecovery: Math.max(
            incomingSeq,
            existingEp !== undefined && existingEp.inError ? existingEp.minHostSeqForRecovery : 0
          ),
          minHostMsForRecovery: Math.max(
            incomingUpdatedMs,
            existingEp !== undefined && existingEp.inError ? existingEp.minHostMsForRecovery : 0
          ),
          recoveredAtMs: existingEp !== undefined ? existingEp.recoveredAtMs : 0,
          recoveredByBriefing: existingEp !== undefined ? existingEp.recoveredByBriefing : false,
          lastErrorSeq: Math.max(incomingSeq, existingEp !== undefined ? existingEp.lastErrorSeq : 0),
          lastErrorUpdatedAtMs: Math.max(
            incomingUpdatedMs,
            existingEp !== undefined ? existingEp.lastErrorUpdatedAtMs : 0
          )
        };
        this.errorEpisodeByKey.set(key, nextEp);
        this.agentErrorSigByKey.set(key, errSig);
        if (shouldNotify && prefs.enabled && prefs.notify_agent_error && !viewed) {
          const trimmedTitle = ag.title.trim();
          const preview = trimmedTitle.length > 0 ? `Error in ${trimmedTitle}` : errMsg;
          const built = buildLocalNotificationCopy(
            'agent_error',
            hostName,
            prefs.include_message_preview,
            preview,
            copy
          );
          out.push({
            hostId: hostId,
            agentId: ag.agent_id,
            eventKind: 'agent_error',
            title: built.title,
            body: built.body,
            requestId: '',
            seq: 0
          });
        }
      }
    } else {
      if (existingEp !== undefined && existingEp.inError) {
        // Only clear an active error episode when this non-error HostDetail/StageDelta snapshot is
        // strictly newer than BOTH the pre-error HostDetail baseline AND the error snapshot itself.
        const atOrBeforePreErrorBaseline =
          (existingEp.minHostSeqForRecovery > 0 || existingEp.minHostMsForRecovery > 0) &&
          incomingSeq <= existingEp.minHostSeqForRecovery &&
          incomingUpdatedMs <= existingEp.minHostMsForRecovery;
        const atOrBeforeHostError =
          existingEp.errorSeq > 0 &&
          incomingSeq > 0 &&
          incomingSeq <= existingEp.errorSeq &&
          incomingUpdatedMs <= existingEp.errorUpdatedAtMs;
        const atOrBeforeBriefingError =
          existingEp.errorBriefingMs > 0 &&
          incomingUpdatedMs > 0 &&
          incomingUpdatedMs <= existingEp.errorBriefingMs &&
          (existingEp.errorSeq === 0 || incomingSeq <= existingEp.errorSeq);
        const isStaleRecovery =
          atOrBeforePreErrorBaseline || atOrBeforeHostError || atOrBeforeBriefingError;
        if (!isStaleRecovery) {
          this.recordNonErrorBaseline(key, incomingUpdatedMs, incomingSeq, false);
          this.errorEpisodeByKey.set(key, {
            inError: false,
            notified: false,
            updatedAtMs: Math.max(incomingUpdatedMs, existingEp.updatedAtMs),
            latestSeq: Math.max(incomingSeq, existingEp.latestSeq),
            errorUpdatedAtMs: 0,
            errorBriefingMs: 0,
            errorSeq: 0,
            minHostSeqForRecovery: 0,
            minHostMsForRecovery: 0,
            recoveredAtMs: Math.max(incomingUpdatedMs, existingEp.recoveredAtMs),
            recoveredByBriefing: false,
            lastErrorSeq: existingEp.lastErrorSeq,
            lastErrorUpdatedAtMs: existingEp.lastErrorUpdatedAtMs
          });
          this.agentErrorSigByKey.delete(key);
        }
      } else {
        this.recordNonErrorBaseline(key, incomingUpdatedMs, incomingSeq, false);
      }
    }
  }

  public observeHostDetail(
    hostId: string,
    detail: HostDetailDto,
    ctx: NotificationVisibilityContext,
    prefs: PushPreferencesDto,
    copy: LocalNotificationCopyStrings = DEFAULT_LOCAL_NOTIFICATION_COPY
  ): OrbitLocalNotificationPayload[] {
    const out: OrbitLocalNotificationPayload[] = [];
    const isBaseline = !this.initializedHosts.has(hostId);
    this.initializedHosts.add(hostId);
    const hostLabel = detail.host.display_name.trim().length > 0
      ? detail.host.display_name.trim()
      : hostId;

    for (let i = 0; i < detail.agents.length; i += 1) {
      this.evaluateAgentStateTransitions(
        hostId,
        hostLabel,
        detail.agents[i],
        isBaseline,
        ctx,
        prefs,
        copy,
        null,
        out
      );
    }

    // (d) Goal finished — shares a unified per-(hostId, goalId) baseline with Briefing goal_snapshots.
    const goals = detail.goal_runs ?? [];
    for (let i = 0; i < goals.length; i += 1) {
      const goal = goals[i];
      this.observeGoalTransition(
        hostId,
        hostLabel,
        goal.goal_id,
        goal.agent_id,
        goal.status,
        goal.objective,
        ctx,
        prefs,
        copy,
        out
      );
    }

    return out;
  }

  private observeGoalTransition(
    hostId: string,
    hostLabel: string,
    goalId: string,
    agentId: string,
    rawStatus: string,
    objective: string,
    ctx: NotificationVisibilityContext,
    prefs: PushPreferencesDto,
    copy: LocalNotificationCopyStrings,
    out: OrbitLocalNotificationPayload[]
  ): void {
    const gKey = `${hostId}::${goalId}`;
    const nextStatus = (rawStatus ?? '').trim().toLowerCase();
    const prevStatus = this.goalStatusByKey.get(gKey) ?? '';
    const nowTerminal = isGoalTerminalStatus(nextStatus);
    const wasTerminal = isGoalTerminalStatus(prevStatus);
    if (wasTerminal && !nowTerminal) {
      // Never allow an older non-terminal snapshot from another source to regress a terminal goal.
      return;
    }
    this.goalStatusByKey.set(gKey, nextStatus);
    const transitionKey = `${gKey}::${nextStatus}`;

    if (!nowTerminal) {
      return;
    }
    if (prevStatus.length === 0) {
      // First-ever sighting of this (hostId, goalId) across ALL sources is already terminal:
      // baseline silently so neither Briefing nor HostDetail notifies for pre-existing terminal goals.
      this.notifiedGoalTransitions.add(transitionKey);
      return;
    }
    if (!wasTerminal && !this.notifiedGoalTransitions.has(transitionKey)) {
      this.notifiedGoalTransitions.add(transitionKey);
      const goalViewed = isConversationVisiblyFocused(ctx, hostId, agentId);
      if (prefs.enabled && prefs.notify_goal_finished && !goalViewed) {
        const obj = objective.trim();
        const preview = obj.length > 0 ? `Goal ${nextStatus}: ${obj}` : '';
        const built = buildLocalNotificationCopy(
          'goal_finished',
          hostLabel,
          prefs.include_message_preview,
          preview,
          copy
        );
        out.push({
          hostId: hostId,
          agentId: agentId,
          eventKind: 'goal_finished',
          title: built.title,
          body: built.body,
          requestId: '',
          seq: 0
        });
      }
    }
  }

  public observeStageDelta(
    hostId: string,
    hostName: string,
    agent: AgentCardDto,
    pendingPermissions: TimelineItemDto[],
    ctx: NotificationVisibilityContext,
    prefs: PushPreferencesDto,
    copy: LocalNotificationCopyStrings = DEFAULT_LOCAL_NOTIFICATION_COPY
  ): OrbitLocalNotificationPayload[] {
    const out: OrbitLocalNotificationPayload[] = [];
    const key = this.threadKey(hostId, agent.agent_id);
    const isBaseline = !this.initializedHosts.has(hostId) && !this.agentStateByKey.has(key);
    this.evaluateAgentStateTransitions(
      hostId,
      hostName,
      agent,
      isBaseline,
      ctx,
      prefs,
      copy,
      pendingPermissions,
      out
    );
    return out;
  }

  public observeBriefing(
    briefing: PersonalAgentBriefingDto,
    ctx: NotificationVisibilityContext,
    getPrefsForHost: (hostId: string) => PushPreferencesDto,
    copy: LocalNotificationCopyStrings = DEFAULT_LOCAL_NOTIFICATION_COPY
  ): OrbitLocalNotificationPayload[] {
    const out: OrbitLocalNotificationPayload[] = [];
    const isBaseline = !this.hasBriefingBaseline;
    const activePermKeys = new Set<string>();
    const currentBriefingErrorKeys = new Set<string>();
    const decisions = briefing.needs_you ?? [];

    // Collect per-agent Briefing worker state/timestamps across missions + other_active ONLY for
    // versioned error-recovery tracking (NEVER used to infer `turn_completed`).
    const briefingWorkerStateByKey = new Map<string, string>();
    const briefingWorkerUpdatedMsByKey = new Map<string, number>();
    const recordBriefingWorkerMeta = (hostId: string, agentId: string, state: string, updatedAtMs: number): void => {
      const wKey = this.threadKey(hostId, agentId);
      const normState = (state ?? '').trim().toLowerCase();
      const ts = typeof updatedAtMs === 'number' && Number.isFinite(updatedAtMs) ? Math.max(0, updatedAtMs) : 0;
      if (normState.length > 0 && normState !== 'error') {
        const curEp = this.errorEpisodeByKey.get(wKey);
        const curErrRefMs = curEp !== undefined && curEp.inError
          ? Math.max(curEp.errorBriefingMs, curEp.errorUpdatedAtMs)
          : 0;
        const isStaleWorkerDuringError =
          curEp !== undefined && curEp.inError && curErrRefMs > 0 && (ts === 0 || ts <= curErrRefMs);
        if (!isStaleWorkerDuringError) {
          this.recordNonErrorBaseline(wKey, ts, 0, true);
        }
      }
      const prevTs = briefingWorkerUpdatedMsByKey.get(wKey) ?? -1;
      if (ts >= prevTs) {
        briefingWorkerStateByKey.set(wKey, normState);
        briefingWorkerUpdatedMsByKey.set(wKey, ts);
      }
    };
    const missions = briefing.missions ?? [];
    for (let i = 0; i < missions.length; i += 1) {
      const m = missions[i];
      recordBriefingWorkerMeta(m.root.host_id, m.root.agent_id, m.root.state, m.root.updated_at_ms);
      for (let j = 0; j < m.workers.length; j += 1) {
        const w = m.workers[j];
        recordBriefingWorkerMeta(w.host_id, w.agent_id, w.state, w.updated_at_ms);
      }
    }
    const otherActive = briefing.other_active ?? [];
    for (let i = 0; i < otherActive.length; i += 1) {
      const w = otherActive[i];
      recordBriefingWorkerMeta(w.host_id, w.agent_id, w.state, w.updated_at_ms);
    }

    for (let i = 0; i < decisions.length; i += 1) {
      const item = decisions[i];
      if (item.kind === 'error') {
        const key = this.threadKey(item.host_id, item.agent_id);
        const itemMs = typeof item.updated_at_ms === 'number' && Number.isFinite(item.updated_at_ms)
          ? Math.max(0, item.updated_at_ms)
          : 0;
        const errSig = `error:${normalizeErrorSummaryForDedup(item.summary)}`;
        const existingEp = this.errorEpisodeByKey.get(key);
        const isStaleBriefingError =
          (
            existingEp !== undefined &&
            !existingEp.inError &&
            existingEp.recoveredAtMs > 0 &&
            itemMs > 0 &&
            itemMs <= existingEp.recoveredAtMs
          ) ||
          this.isBriefingErrorStaleAgainstNonErrorBaseline(key, itemMs);
        if (isStaleBriefingError) {
          continue;
        }
        currentBriefingErrorKeys.add(key);
        const alreadyInErrorOrNotified =
          existingEp !== undefined && (existingEp.inError || existingEp.notified);
        const prevHostVer = this.agentVersionByKey.get(key);
        const isErrorBaseline =
          existingEp === undefined &&
          isBaseline &&
          !this.initializedHosts.has(item.host_id) &&
          !this.agentStateByKey.has(key) &&
          !this.observedNonErrorByKey.has(key);
        const nextEp: AgentErrorEpisodeState = {
          inError: true,
          notified: true,
          updatedAtMs: Math.max(itemMs, existingEp !== undefined ? existingEp.updatedAtMs : 0),
          latestSeq: existingEp !== undefined ? existingEp.latestSeq : (prevHostVer !== undefined ? prevHostVer.latestSeq : 0),
          errorUpdatedAtMs: Math.max(
            itemMs,
            existingEp !== undefined && existingEp.inError ? existingEp.errorUpdatedAtMs : 0
          ),
          errorBriefingMs: Math.max(
            itemMs,
            existingEp !== undefined && existingEp.inError ? existingEp.errorBriefingMs : 0
          ),
          errorSeq: existingEp !== undefined && existingEp.inError ? existingEp.errorSeq : 0,
          minHostSeqForRecovery: Math.max(
            prevHostVer !== undefined ? prevHostVer.latestSeq : 0,
            existingEp !== undefined && existingEp.inError ? existingEp.minHostSeqForRecovery : 0
          ),
          minHostMsForRecovery: Math.max(
            prevHostVer !== undefined ? prevHostVer.updatedAtMs : 0,
            existingEp !== undefined && existingEp.inError ? existingEp.minHostMsForRecovery : 0
          ),
          recoveredAtMs: existingEp !== undefined ? existingEp.recoveredAtMs : 0,
          recoveredByBriefing: existingEp !== undefined ? existingEp.recoveredByBriefing : false,
          lastErrorSeq: existingEp !== undefined ? existingEp.lastErrorSeq : 0,
          lastErrorUpdatedAtMs: Math.max(
            itemMs,
            existingEp !== undefined ? existingEp.lastErrorUpdatedAtMs : 0
          )
        };
        this.errorEpisodeByKey.set(key, nextEp);
        this.agentErrorSigByKey.set(key, errSig);
        if (!isErrorBaseline && !alreadyInErrorOrNotified) {
          const prefs = getPrefsForHost(item.host_id);
          const viewed = isConversationVisiblyFocused(ctx, item.host_id, item.agent_id);
          if (prefs.enabled && prefs.notify_agent_error && !viewed) {
            const preview = item.summary.trim().length > 0 ? item.summary.trim() : item.agent_title.trim();
            const built = buildLocalNotificationCopy(
              'agent_error',
              item.host_name,
              prefs.include_message_preview,
              preview,
              copy
            );
            out.push({
              hostId: item.host_id,
              agentId: item.agent_id,
              eventKind: 'agent_error',
              title: built.title,
              body: built.body,
              requestId: '',
              seq: 0
            });
          }
        }
        continue;
      }
      if (item.kind !== 'permission') {
        continue;
      }
      const reqId = (item.request_id ?? '').trim();
      const suffix = reqId.length > 0 ? reqId : item.key;
      const permKey = `perm:${item.host_id}::${item.agent_id}::${suffix}`;
      activePermKeys.add(permKey);

      if (isBaseline) {
        this.notifiedBriefingPermKeys.add(permKey);
        continue;
      }
      if (!this.notifiedBriefingPermKeys.has(permKey) && !this.notifiedHostPermKeys.has(permKey)) {
        this.notifiedBriefingPermKeys.add(permKey);
        this.notifiedHostPermKeys.add(permKey);
        const prefs = getPrefsForHost(item.host_id);
        const viewed = isConversationVisiblyFocused(ctx, item.host_id, item.agent_id);
        if (prefs.enabled && prefs.notify_permission_required && !viewed) {
          const preview = item.summary.trim().length > 0 ? item.summary.trim() : item.agent_title.trim();
          const built = buildLocalNotificationCopy(
            'permission_required',
            item.host_name,
            prefs.include_message_preview,
            preview,
            copy
          );
          out.push({
            hostId: item.host_id,
            agentId: item.agent_id,
            eventKind: 'permission_required',
            title: built.title,
            body: built.body,
            requestId: reqId,
            seq: 0
          });
        }
      }
    }

    // Observe lightweight zero-IO `goal_snapshots` across ALL hosts (including unselected hosts and
    // non-PA regular sessions) using the shared per-(hostId, goalId) baseline. NOTE: Briefing's
    // coarse `missions`/`other_active` display states are NEVER used to infer `turn_completed`.
    const goalSnaps = briefing.goal_snapshots ?? [];
    for (let i = 0; i < goalSnaps.length; i += 1) {
      const gs = goalSnaps[i];
      const hostLabel = gs.host_name.trim().length > 0 ? gs.host_name.trim() : gs.host_id;
      this.observeGoalTransition(
        gs.host_id,
        hostLabel,
        gs.goal_id,
        gs.agent_id,
        gs.status,
        gs.objective,
        ctx,
        getPrefsForHost(gs.host_id),
        copy,
        out
      );
    }

    this.hasBriefingBaseline = true;
    const prevBriefingKeys = Array.from(this.notifiedBriefingPermKeys.values());
    for (let i = 0; i < prevBriefingKeys.length; i += 1) {
      if (!activePermKeys.has(prevBriefingKeys[i])) {
        this.notifiedBriefingPermKeys.delete(prevBriefingKeys[i]);
      }
    }

    // Advance shared error episodes to recovered when Briefing observes either:
    // (a) a strictly newer non-error worker state for that agent in missions/other_active, OR
    // (b) an error previously tracked in Briefing's `needs_you` has disappeared from `needs_you`
    //     without any stale worker entry contradicting recovery (independent of stale HostDetail).
    const allErrorKeys = Array.from(this.errorEpisodeByKey.keys());
    for (let i = 0; i < allErrorKeys.length; i += 1) {
      const errKey = allErrorKeys[i];
      const ep = this.errorEpisodeByKey.get(errKey);
      if (ep === undefined || !ep.inError || currentBriefingErrorKeys.has(errKey)) {
        continue;
      }
      const workerState = briefingWorkerStateByKey.get(errKey);
      const workerMs = briefingWorkerUpdatedMsByKey.get(errKey) ?? 0;
      const errRefMs = Math.max(ep.errorBriefingMs, ep.errorUpdatedAtMs);
      let briefingRecovered = false;
      let recoveredMs = ep.recoveredAtMs;

      if (workerState !== undefined) {
        if (workerState !== 'error') {
          const isStrictlyNewerWorker =
            errRefMs === 0 || (workerMs > 0 && workerMs > errRefMs);
          if (isStrictlyNewerWorker) {
            briefingRecovered = true;
            recoveredMs = Math.max(recoveredMs, workerMs);
          }
        }
      } else if (this.activeBriefingErrorKeys.has(errKey)) {
        // Error was previously reported in Briefing `needs_you` and is now gone from Briefing.
        briefingRecovered = true;
      }

      if (briefingRecovered) {
        this.errorEpisodeByKey.set(errKey, {
          inError: false,
          notified: false,
          updatedAtMs: Math.max(ep.updatedAtMs, recoveredMs),
          latestSeq: ep.latestSeq,
          errorUpdatedAtMs: 0,
          errorBriefingMs: 0,
          errorSeq: 0,
          minHostSeqForRecovery: 0,
          minHostMsForRecovery: 0,
          recoveredAtMs: recoveredMs,
          recoveredByBriefing: true,
          lastErrorSeq: ep.lastErrorSeq,
          lastErrorUpdatedAtMs: ep.lastErrorUpdatedAtMs
        });
        this.agentErrorSigByKey.delete(errKey);
      }
    }
    this.activeBriefingErrorKeys.clear();
    currentBriefingErrorKeys.forEach((errKey: string): void => {
      this.activeBriefingErrorKeys.add(errKey);
    });
    return out;
  }
}
