import type {
  AgentCardView,
  ArtifactSummary,
  ConditionalTimerRecord,
  DiffFileView,
  GitReviewView,
  GoalRunRecord,
  HostDetailView,
  PendingPermissionSummary,
  SessionBackgroundTaskView,
  SessionLinkView,
  SessionPullRequestView,
  SessionSubagentEntryView,
  SessionTouchedFileView,
  TerminalSummary,
  TimelineItemView,
} from "../../lib/orbit-ipc.ts";
import { normalizeCwdForCompare } from "../../lib/stage-cache.ts";
import {
  type ResolvedSubagentItem,
  filterVisibleSubagents,
  isSubagentActiveStatus,
  resolveSessionSubagents,
} from "../../lib/subagents.ts";
import {
  type SubagentCharacterSpec,
  assignSubagentCharacters,
} from "../../lib/subagent-avatar.ts";
import type { OrbitUiState, TerminalTabSession } from "../store-types.ts";

function isAgentBackgroundWaiting(
  agent: Pick<AgentCardView, "has_background_tasks" | "status_label"> | null | undefined,
): boolean {
  if (!agent) return false;
  if (typeof agent.has_background_tasks === "boolean") {
    return agent.has_background_tasks;
  }
  const lbl = agent.status_label.trim();
  return /^running\s*\(.*background task/i.test(lbl) || /^background task$/i.test(lbl);
}

export interface SessionSubagentWithAvatar extends ResolvedSubagentItem {
  readonly character: SubagentCharacterSpec;
}

export interface SessionSubagentsSummary {
  readonly all: readonly SessionSubagentWithAvatar[];
  readonly visible: readonly SessionSubagentWithAvatar[];
  readonly activeCount: number;
  readonly awaitingCount: number;
  readonly settledCount: number;
  readonly characterByKey: ReadonlyMap<string, SubagentCharacterSpec>;
}

const EMPTY_SUBAGENT_LIST: readonly SessionSubagentWithAvatar[] = Object.freeze([]);
const EMPTY_SUBAGENT_SUMMARY: SessionSubagentsSummary = Object.freeze({
  all: EMPTY_SUBAGENT_LIST,
  visible: EMPTY_SUBAGENT_LIST,
  activeCount: 0,
  awaitingCount: 0,
  settledCount: 0,
  characterByKey: new Map(),
});

const EMPTY_INSIGHT_SUBAGENTS: readonly SessionSubagentEntryView[] = Object.freeze([]);

/**
 * Single source of truth for a conversation's subagents (managed + provider-native + cross-host),
 * including collision-free animal avatar assignment and archive filtering.
 */
export function selectSessionSubagents(
  state: Pick<OrbitUiState, "hostDetails" | "fleet" | "archivedSubagents"> &
    Partial<Pick<OrbitUiState, "stages">>,
  hostId: string | null,
  agentId: string | null,
  insightsSubagentsOverride?: readonly SessionSubagentEntryView[],
): SessionSubagentsSummary {
  if (!hostId || !agentId) return EMPTY_SUBAGENT_SUMMARY;

  const insightsSubagents =
    insightsSubagentsOverride ??
    state.stages?.[`${hostId}::${agentId}`]?.insights?.subagents ??
    EMPTY_INSIGHT_SUBAGENTS;

  const resolved = resolveSessionSubagents({
    hostId,
    agentId,
    insightsSubagents,
    hostDetails: state.hostDetails,
    fleetHosts: state.fleet.hosts,
  });

  if (resolved.length === 0) return EMPTY_SUBAGENT_SUMMARY;

  const characterByKey = assignSubagentCharacters(
    resolved,
    (sa) => sa.childAgentId || sa.providerSubagentId || sa.callId || sa.key,
  );

  const all: SessionSubagentWithAvatar[] = resolved.map((sa) => ({
    ...sa,
    character: characterByKey.get(sa.key)!,
  }));

  const visible = filterVisibleSubagents(all, state.archivedSubagents, hostId, agentId);

  let activeCount = 0;
  let awaitingCount = 0;
  let settledCount = 0;
  for (const sa of visible) {
    if (sa.status === "awaiting_permission") {
      awaitingCount += 1;
      activeCount += 1;
    } else if (isSubagentActiveStatus(sa.status)) {
      activeCount += 1;
    } else {
      settledCount += 1;
    }
  }

  return {
    all,
    visible,
    activeCount,
    awaitingCount,
    settledCount,
    characterByKey,
  };
}

export type PendingPermissionTimelineItem = Extract<
  TimelineItemView,
  { item_type: "permission_request" }
>;
export type PendingQuestionTimelineItem = Extract<
  TimelineItemView,
  { item_type: "question_request" }
>;

export interface UnifiedPendingSubagentPermission {
  readonly kind: "subagent_permission";
  readonly key: string;
  readonly hostId: string;
  readonly agentId: string;
  readonly subagent: SessionSubagentWithAvatar;
  readonly permission: PendingPermissionSummary;
}

export interface UnifiedPendingDecisions {
  readonly parentPermissions: readonly PendingPermissionTimelineItem[];
  readonly parentQuestions: readonly PendingQuestionTimelineItem[];
  readonly subagentPermissions: readonly UnifiedPendingSubagentPermission[];
  readonly totalCount: number;
}

const EMPTY_DECISIONS: UnifiedPendingDecisions = Object.freeze({
  parentPermissions: Object.freeze([]),
  parentQuestions: Object.freeze([]),
  subagentPermissions: Object.freeze([]),
  totalCount: 0,
});

/**
 * Single source of truth for all pending approvals and clarifications belonging to
 * the active conversation AND any of its managed child subagents across all hosts.
 */
export function selectPendingDecisions(
  state: Pick<OrbitUiState, "stages" | "hostDetails" | "fleet" | "archivedSubagents">,
  hostId: string | null,
  agentId: string | null,
): UnifiedPendingDecisions {
  if (!hostId || !agentId) return EMPTY_DECISIONS;

  const stage = state.stages[`${hostId}::${agentId}`];
  const parentPermissions = (stage?.pending_permissions ?? []).filter(
    (item): item is PendingPermissionTimelineItem =>
      item.item_type === "permission_request" && item.resolved_decision === null,
  );
  const parentQuestions = (stage?.pending_questions ?? []).filter(
    (item): item is PendingQuestionTimelineItem =>
      item.item_type === "question_request" && item.resolved_answers === null,
  );

  const subagents = selectSessionSubagents(state, hostId, agentId);
  const subagentPermissions: UnifiedPendingSubagentPermission[] = [];
  const seenReqIds = new Set<string>();

  for (const sa of subagents.all) {
    if (!sa.isManaged || !sa.childAgentId) continue;
    const childDetail = state.hostDetails[sa.childHostId];
    const pendingList = childDetail?.pending_permissions ?? [];
    for (const perm of pendingList) {
      if (perm.agent_id !== sa.childAgentId) continue;
      const dedupeKey = `${sa.childHostId}::${sa.childAgentId}::${perm.request_id}`;
      if (seenReqIds.has(dedupeKey)) continue;
      seenReqIds.add(dedupeKey);
      subagentPermissions.push({
        kind: "subagent_permission",
        key: dedupeKey,
        hostId: sa.childHostId,
        agentId: sa.childAgentId,
        subagent: sa,
        permission: perm,
      });
    }
  }

  const totalCount =
    parentPermissions.length + parentQuestions.length + subagentPermissions.length;

  if (totalCount === 0) return EMPTY_DECISIONS;

  return {
    parentPermissions,
    parentQuestions,
    subagentPermissions,
    totalCount,
  };
}

export interface WorkspaceToolSummary {
  readonly reviewScope: "working_tree" | "session_turns";
  readonly reviewCount: number;
  readonly reviewAdditions: number;
  readonly reviewDeletions: number;
  readonly artifactCount: number;
  readonly terminalCount: number;
  readonly showGroupNotes: boolean;
  readonly groupBoardRevision: number;
  readonly groupTitle: string | null;
  readonly workspaceId: string | null;
  readonly touchedFiles: readonly SessionTouchedFileView[];
  readonly touchedAdditions: number;
  readonly touchedDeletions: number;
}

const EMPTY_TOUCHED_FILES: readonly SessionTouchedFileView[] = [];

/**
 * Resolves the single GitReviewView matching `(hostId, cwd)` by exact key or normalized CWD.
 * Safe to invoke directly inside `useOrbitSelector` because it returns the exact stored object reference (or null).
 */
export function findGitReviewForCwd(
  gitReviews: Record<string, GitReviewView>,
  hostId: string | null,
  cwd: string,
): GitReviewView | null {
  if (!hostId || !cwd) return null;
  const exact = gitReviews[`${hostId}::${cwd}`];
  if (exact) return exact;
  const norm = normalizeCwdForCompare(cwd);
  if (!norm) return null;
  const prefix = `${hostId}::`;
  for (const [key, val] of Object.entries(gitReviews)) {
    if (key.startsWith(prefix) && normalizeCwdForCompare(key.slice(prefix.length)) === norm) {
      return val;
    }
  }
  return null;
}

/**
 * Computes the project-scoped terminal count for `(hostId, cwd)` (deduplicating open tabs and detached terminals).
 * Safe to invoke directly inside `useOrbitSelector` because it returns a primitive number.
 */
export function selectScopedTerminalCount(
  terminalTabs: readonly TerminalTabSession[],
  hostTerminals: readonly TerminalSummary[] | undefined,
  hostId: string | null,
  cwd: string,
): number {
  if (!hostId) return 0;
  const normalizedCwd = cwd ? normalizeCwdForCompare(cwd) : "";
  const matchesProjectCwd = (termCwd: string | null | undefined): boolean => {
    if (!normalizedCwd) return true;
    const normTerm = termCwd ? normalizeCwdForCompare(termCwd) : "";
    if (!normTerm) return false;
    return normTerm === normalizedCwd || normTerm.startsWith(`${normalizedCwd}/`);
  };

  const openTerminalsForHost = terminalTabs.filter(
    (t) => t.hostId === hostId && matchesProjectCwd(t.cwd),
  );
  const openIds = new Set(openTerminalsForHost.map((t) => t.terminalId).filter(Boolean));
  const detachedTerminals = (hostTerminals ?? []).filter(
    (t) => !openIds.has(t.terminal_id) && matchesProjectCwd(t.cwd),
  ).length;
  return openTerminalsForHost.length + detachedTerminals;
}

/**
 * Returns true if ANY remote host (other than `hostId`) currently holds a managed child agent
 * or dispatched workspace peer belonging to `(hostId, agentId)`. Returning a boolean lets
 * `useOrbitSelector` short-circuit remote host heartbeats with zero re-renders for the 99%
 * local-only session case.
 */
export function hasCrossHostChildAgents(
  hostDetails: Record<string, HostDetailView>,
  hostId: string | null,
  agentId: string | null,
  insightsSubagents?: readonly SessionSubagentEntryView[],
): boolean {
  if (!hostId || !agentId) return false;
  let dispatchedIds: Set<string> | null = null;
  if (insightsSubagents && insightsSubagents.length > 0) {
    for (const sa of insightsSubagents) {
      if (sa.child_agent_id?.startsWith("ag_")) {
        dispatchedIds ??= new Set();
        dispatchedIds.add(sa.child_agent_id);
      }
    }
  }
  for (const [hid, detail] of Object.entries(hostDetails)) {
    if (hid === hostId) continue;
    for (const ag of detail.agents) {
      if (
        (ag.parent_agent_id === agentId &&
          (!ag.parent_host_id || ag.parent_host_id === hostId)) ||
        (dispatchedIds !== null && dispatchedIds.has(ag.agent_id))
      ) {
        return true;
      }
    }
  }
  return false;
}

/**
 * Returns true if the given host currently has any Orbit managed child agents, dispatched
 * workspace peers, or provider-native subagent descriptors belonging to `(hostId, agentId)`.
 */
export function hasLocalSubagentRecords(
  hostDetail: HostDetailView | null | undefined,
  hostId: string | null,
  agentId: string | null,
  insightsSubagents?: readonly SessionSubagentEntryView[],
): boolean {
  if (!hostDetail || !hostId || !agentId) return false;
  if ((hostDetail.provider_subagents?.[agentId]?.length ?? 0) > 0) {
    return true;
  }
  let dispatchedIds: Set<string> | null = null;
  if (insightsSubagents && insightsSubagents.length > 0) {
    for (const sa of insightsSubagents) {
      if (sa.child_agent_id?.startsWith("ag_")) {
        dispatchedIds ??= new Set();
        dispatchedIds.add(sa.child_agent_id);
      }
    }
  }
  for (const ag of hostDetail.agents) {
    if (
      (ag.parent_agent_id === agentId &&
        (!ag.parent_host_id || ag.parent_host_id === hostId)) ||
      (dispatchedIds !== null && dispatchedIds.has(ag.agent_id))
    ) {
      return true;
    }
  }
  return false;
}

/**
 * Returns true if an open Side Panel file tab path matches a Touched File entry
 * across absolute paths, workspace-relative display paths, `./` prefixes, and separator styles.
 */
export function doesTouchedFileMatchOpenPath(
  openFilePath: string | null | undefined,
  file: Pick<SessionTouchedFileView, "path" | "display_path">,
  cwd: string,
): boolean {
  if (!openFilePath) return false;
  const clean = (p: string) => p.replace(/\\/g, "/").replace(/^\.\//, "");
  const normOpen = clean(openFilePath);
  const normPath = clean(file.path);
  const normDisplay = clean(file.display_path);
  if (normOpen === normPath || normOpen === normDisplay) return true;

  const normCwd = cwd ? normalizeCwdForCompare(cwd) : "";
  if (normCwd) {
    const prefix = `${normCwd}/`;
    const stripPrefix = (p: string) => (p.startsWith(prefix) ? p.slice(prefix.length) : p);
    const openRel = stripPrefix(normOpen);
    if (openRel === stripPrefix(normPath) || openRel === stripPrefix(normDisplay)) {
      return true;
    }
  }
  return false;
}

const ACTIVE_GOAL_STATUSES_LOOKUP = new Set<GoalRunRecord["status"]>([
  "running",
  "verifying",
  "paused",
]);

/**
 * Finds the active goal record for `agentId` on `hostDetail`, returning the exact stored object reference or null.
 */
export function selectActiveGoalForAgent(
  hostDetail: HostDetailView | null | undefined,
  agentId: string | null,
): GoalRunRecord | null {
  if (!hostDetail || !agentId) return null;
  const goals = hostDetail.goal_runs;
  if (!goals || goals.length === 0) return null;
  for (const g of goals) {
    if (g.agent_id === agentId && ACTIVE_GOAL_STATUSES_LOOKUP.has(g.status)) {
      return g;
    }
  }
  return null;
}

/**
 * Returns true if `agentId` currently has any armed conditional timers on `hostDetail`.
 */
export function hasArmedTimersForAgent(
  hostDetail: HostDetailView | null | undefined,
  agentId: string | null,
): boolean {
  if (!hostDetail || !agentId) return false;
  const timers = hostDetail.conditional_timers;
  if (!timers || timers.length === 0) return false;
  for (const timer of timers) {
    if (timer.agent_id === agentId && timer.state === "armed") {
      return true;
    }
  }
  return false;
}

/**
 * Single source of truth for workspace tool counters (Review Diff, Artifacts, Terminal,
 * Group Notes, Touched Files) consumed by Overview HUD without duplicate calculations.
 */
export function selectWorkspaceToolSummary(
  state: Pick<OrbitUiState, "hostDetails"> &
    Partial<Pick<OrbitUiState, "gitReviews" | "terminalTabs" | "stages">>,
  hostId: string | null,
  agentId: string | null,
  cwd: string,
  sessionTurnDiffs: readonly DiffFileView[] = [],
  currentAgent?: AgentCardView | null,
  insightsTouchedFilesOverride?: readonly SessionTouchedFileView[],
  overrides?: {
    gitReview?: GitReviewView | null;
    terminalCount?: number;
    artifactCount?: number;
    showGroupNotes?: boolean;
    groupBoardRevision?: number;
    groupTitle?: string | null;
    workspaceId?: string | null;
  },
): WorkspaceToolSummary {
  const hostDetail = hostId ? (state.hostDetails[hostId] ?? null) : null;
  const resolvedAgent =
    currentAgent ??
    (hostId && agentId
      ? (hostDetail?.agents.find((a) => a.agent_id === agentId) ??
        state.stages?.[`${hostId}::${agentId}`]?.agent ??
        null)
      : null);

  // Resolve git review by exact or normalized CWD
  const gitReview =
    overrides && "gitReview" in overrides
      ? (overrides.gitReview ?? null)
      : state.gitReviews
        ? findGitReviewForCwd(state.gitReviews, hostId, cwd)
        : null;

  let reviewScope: "working_tree" | "session_turns" = "working_tree";
  let reviewCount = 0;
  let reviewAdditions = 0;
  let reviewDeletions = 0;

  const gitFiles = gitReview?.diff_files ?? [];
  const statusFiles = gitReview?.status?.files ?? [];
  if (statusFiles.length > 0 || gitFiles.length > 0) {
    const diffByPath = new Map(gitFiles.map((f) => [f.path, f]));
    const mergedMap = new Map<string, { additions: number; deletions: number }>();
    for (const sf of statusFiles) {
      const df = diffByPath.get(sf.path);
      if (!mergedMap.has(sf.path)) {
        mergedMap.set(sf.path, {
          additions: df?.additions ?? 0,
          deletions: df?.deletions ?? 0,
        });
      }
    }
    for (const df of gitFiles) {
      if (!mergedMap.has(df.path)) {
        mergedMap.set(df.path, { additions: df.additions, deletions: df.deletions });
      }
    }
    reviewCount = mergedMap.size;
    for (const f of mergedMap.values()) {
      reviewAdditions += f.additions;
      reviewDeletions += f.deletions;
    }
  } else if (sessionTurnDiffs.length > 0) {
    reviewScope = "session_turns";
    reviewCount = sessionTurnDiffs.length;
    for (const f of sessionTurnDiffs) {
      reviewAdditions += f.additions;
      reviewDeletions += f.deletions;
    }
  }

  const allArtifacts = hostDetail?.artifacts ?? [];
  const artifactCount =
    typeof overrides?.artifactCount === "number"
      ? overrides.artifactCount
      : allArtifacts.length === 0
        ? 0
        : resolvedAgent?.workspace_id
          ? allArtifacts.filter((a) => a.workspace_id === resolvedAgent.workspace_id).length
          : allArtifacts.length;

  const terminalCount =
    typeof overrides?.terminalCount === "number"
      ? overrides.terminalCount
      : selectScopedTerminalCount(
          state.terminalTabs ?? [],
          hostDetail?.terminals,
          hostId,
          cwd,
        );

  const currentWorkspace =
    hostDetail && resolvedAgent
      ? (hostDetail.workspaces.find((w) => w.workspace_id === resolvedAgent.workspace_id) ?? null)
      : null;
  const groupBoardRevision =
    typeof overrides?.groupBoardRevision === "number"
      ? overrides.groupBoardRevision
      : ((resolvedAgent
          ? hostDetail?.workspace_boards?.[resolvedAgent.workspace_id]?.revision
          : 0) ??
        currentWorkspace?.board_revision ??
        0);
  const showGroupNotes =
    typeof overrides?.showGroupNotes === "boolean"
      ? overrides.showGroupNotes
      : Boolean(
          resolvedAgent &&
            !resolvedAgent.parent_agent_id &&
            (currentWorkspace?.is_curated || groupBoardRevision > 0),
        );
  const groupTitle =
    overrides && "groupTitle" in overrides
      ? (overrides.groupTitle ?? null)
      : (currentWorkspace?.title ?? null);
  const workspaceId =
    overrides && "workspaceId" in overrides
      ? (overrides.workspaceId ?? null)
      : (resolvedAgent?.workspace_id ?? null);

  const touchedFiles =
    insightsTouchedFilesOverride ??
    (hostId && agentId
      ? state.stages?.[`${hostId}::${agentId}`]?.insights?.touched_files
      : undefined) ??
    EMPTY_TOUCHED_FILES;
  let touchedAdditions = 0;
  let touchedDeletions = 0;
  for (const tf of touchedFiles) {
    touchedAdditions += tf.additions;
    touchedDeletions += tf.deletions;
  }

  return {
    reviewScope,
    reviewCount,
    reviewAdditions,
    reviewDeletions,
    artifactCount,
    terminalCount,
    showGroupNotes,
    groupBoardRevision,
    groupTitle,
    workspaceId,
    touchedFiles,
    touchedAdditions,
    touchedDeletions,
  };
}

export interface FormattedTouchedFile {
  readonly file: SessionTouchedFileView;
  readonly basename: string;
  readonly dirHint: string | null;
}

function touchedActionRank(action: SessionTouchedFileView["action"]): number {
  return action === "created" || action === "edited" ? 0 : 1;
}

function splitDisplayPath(displayPath: string): { basename: string; dirParts: string[] } {
  const normalized = displayPath.replace(/\\/g, "/").replace(/^\/+/, "");
  const parts = normalized.split("/").filter(Boolean);
  if (parts.length === 0) return { basename: displayPath, dirParts: [] };
  return {
    basename: parts[parts.length - 1]!,
    dirParts: parts.slice(0, -1),
  };
}

/**
 * Orders touched files with modified (`created`/`edited`) files ahead of pure `read` files
 * (preserving most-recent `last_seq` descending within each tier) and computes minimal
 * disambiguating parent directory hints whenever multiple files share the same basename.
 */
export function formatTouchedFilesForOverview(
  files: readonly SessionTouchedFileView[],
): readonly FormattedTouchedFile[] {
  if (files.length === 0) return [];

  const sorted = [...files].sort((a, b) => {
    const rankDiff = touchedActionRank(a.action) - touchedActionRank(b.action);
    if (rankDiff !== 0) return rankDiff;
    return b.last_seq - a.last_seq;
  });

  const parsed = sorted.map((file) => ({ file, ...splitDisplayPath(file.display_path) }));
  const byBasename = new Map<string, typeof parsed>();
  for (const item of parsed) {
    const bucket = byBasename.get(item.basename);
    if (bucket) bucket.push(item);
    else byBasename.set(item.basename, [item]);
  }

  const dirHintByPath = new Map<string, string | null>();
  for (const bucket of byBasename.values()) {
    if (bucket.length <= 1) {
      dirHintByPath.set(bucket[0]!.file.path, null);
      continue;
    }
    for (const item of bucket) {
      if (item.dirParts.length === 0) {
        dirHintByPath.set(item.file.path, null);
        continue;
      }
      let chosen = item.dirParts.join("/");
      for (let depth = 1; depth <= item.dirParts.length; depth += 1) {
        const candidate = item.dirParts.slice(-depth).join("/");
        const collides = bucket.some(
          (other) => other !== item && other.dirParts.slice(-depth).join("/") === candidate,
        );
        if (!collides) {
          chosen = candidate;
          break;
        }
      }
      dirHintByPath.set(item.file.path, chosen);
    }
  }

  return parsed.map((item) => ({
    file: item.file,
    basename: item.basename,
    dirHint: dirHintByPath.get(item.file.path) ?? null,
  }));
}

export interface SplitFormattedTouchedFiles {
  readonly modified: readonly FormattedTouchedFile[];
  readonly read: readonly FormattedTouchedFile[];
}

export function splitFormattedTouchedFiles(
  files: readonly FormattedTouchedFile[],
): SplitFormattedTouchedFiles {
  const modified: FormattedTouchedFile[] = [];
  const read: FormattedTouchedFile[] = [];
  for (const item of files) {
    if (item.file.action === "created" || item.file.action === "edited") {
      modified.push(item);
    } else {
      read.push(item);
    }
  }
  return { modified, read };
}

/**
 * Merges timeline-projected pull requests with PRs created directly from the Review tab,
 * deduplicating by canonical URL while preferring richer titles and latest sequence.
 */
export function mergeSessionPullRequests(
  insightPrs: readonly SessionPullRequestView[] | undefined,
  createdPrs: readonly SessionPullRequestView[] | undefined,
): readonly SessionPullRequestView[] {
  if ((!insightPrs || insightPrs.length === 0) && (!createdPrs || createdPrs.length === 0)) {
    return [];
  }
  const byKey = new Map<string, SessionPullRequestView>();
  const ingest = (pr: SessionPullRequestView) => {
    const key = pr.url.trim().toLowerCase().replace(/\/+$/, "");
    if (!key) return;
    const prev = byKey.get(key);
    if (!prev) {
      byKey.set(key, pr);
      return;
    }
    byKey.set(key, {
      url: prev.url,
      number: prev.number || pr.number,
      repo: prev.repo || pr.repo,
      title: prev.title?.trim() ? prev.title : (pr.title ?? null),
      last_seq: Math.max(prev.last_seq, pr.last_seq),
    });
  };
  for (const pr of createdPrs ?? []) ingest(pr);
  for (const pr of insightPrs ?? []) ingest(pr);
  return [...byKey.values()].sort(
    (a, b) => b.last_seq - a.last_seq || b.number - a.number,
  );
}

export interface OverviewLinkItem {
  readonly kind: "local" | "external";
  readonly url: string;
  readonly label: string;
  readonly host: string;
}

function formatLocalPreviewLabel(rawUrl: string): { label: string; host: string } {
  try {
    const parsed = new URL(rawUrl);
    const hostPort = parsed.port ? `${parsed.hostname}:${parsed.port}` : parsed.hostname;
    const cleanPath = parsed.pathname === "/" ? "" : parsed.pathname.replace(/\/+$/, "");
    return {
      label: cleanPath ? `${hostPort}${cleanPath}` : hostPort,
      host: hostPort,
    };
  } catch {
    return { label: rawUrl, host: rawUrl };
  }
}

/**
 * Combines detected localhost preview URLs (prioritized first) with extracted external links,
 * deduplicating normalized URLs cleanly.
 */
export function buildOverviewLinkItems(
  localUrls: readonly string[] | undefined,
  externalLinks: readonly SessionLinkView[] | undefined,
): readonly OverviewLinkItem[] {
  if (
    (!localUrls || localUrls.length === 0) &&
    (!externalLinks || externalLinks.length === 0)
  ) {
    return [];
  }
  const seen = new Set<string>();
  const out: OverviewLinkItem[] = [];
  for (const raw of localUrls ?? []) {
    const trimmed = raw.trim();
    if (!trimmed) continue;
    const key = trimmed.toLowerCase().replace(/\/+$/, "");
    if (seen.has(key)) continue;
    seen.add(key);
    const formatted = formatLocalPreviewLabel(trimmed);
    out.push({
      kind: "local",
      url: trimmed,
      label: formatted.label,
      host: formatted.host,
    });
  }
  for (const link of externalLinks ?? []) {
    const trimmed = link.url.trim();
    if (!trimmed) continue;
    const key = trimmed.toLowerCase().replace(/\/+$/, "");
    if (seen.has(key)) continue;
    seen.add(key);
    out.push({
      kind: "external",
      url: trimmed,
      label: link.label || link.host || trimmed,
      host: link.host,
    });
  }
  return out;
}

/**
 * Normalizes URLs for matching active built-in browser tabs against local preview links
 * (case-insensitive origin, ignoring trailing slash differences).
 */
export function doesUrlMatchOpenBrowserUrl(
  openUrl: string | null | undefined,
  targetUrl: string,
): boolean {
  if (!openUrl) return false;
  const norm = (raw: string) => raw.trim().toLowerCase().replace(/\/+$/, "");
  return norm(openUrl) === norm(targetUrl);
}

/**
 * Selects and orders artifacts belonging to the active conversation (`agentId`) or unattached
 * workspace-level shared artifacts, sorted most recently updated first.
 */
export function selectWorkspaceArtifactsList(
  allArtifacts: readonly ArtifactSummary[],
  workspaceId: string | null,
  agentId?: string | null,
): readonly ArtifactSummary[] {
  if (allArtifacts.length === 0) return [];
  const filtered = allArtifacts.filter((a) => {
    if (agentId) {
      if (a.agent_id) return a.agent_id === agentId;
      return workspaceId ? a.workspace_id === workspaceId : true;
    }
    return workspaceId ? a.workspace_id === workspaceId : true;
  });
  if (filtered.length <= 1) return filtered;
  return [...filtered].sort(
    (a, b) => Date.parse(b.updated_at) - Date.parse(a.updated_at) || b.version - a.version,
  );
}

export type OverviewIndicatorTone = "none" | "approval" | "running" | "goal";

export type OverviewIndicatorReason =
  | "none"
  | "approval"
  | "subagent"
  | "background_task"
  | "goal"
  | "timer";

export interface SessionRuntimeActivity {
  readonly activeGoal: GoalRunRecord | null;
  readonly armedTimers: readonly ConditionalTimerRecord[];
  readonly backgroundTasks: readonly SessionBackgroundTaskView[];
  readonly hasRunningBgTask: boolean;
  readonly overviewIndicatorTone: OverviewIndicatorTone;
  readonly overviewIndicatorReason: OverviewIndicatorReason;
}

const EMPTY_TIMERS: readonly ConditionalTimerRecord[] = [];
const EMPTY_BG_TASKS: readonly SessionBackgroundTaskView[] = [];

const ACTIVE_GOAL_STATUSES = new Set<GoalRunRecord["status"]>([
  "running",
  "verifying",
  "paused",
]);

/**
 * Single source of truth for active Goal, armed Timers, Background Tasks, and
 * Overview toggle indicator tone/reason (used by both TopBar & Overview HUD).
 */
export function selectSessionRuntimeActivity(
  state: Pick<OrbitUiState, "hostDetails" | "fleet" | "archivedSubagents"> &
    Partial<Pick<OrbitUiState, "stages">>,
  hostId: string | null,
  agentId: string | null,
  options?: {
    subagents?: SessionSubagentsSummary;
    backgroundTasks?: readonly SessionBackgroundTaskView[];
  },
): SessionRuntimeActivity {
  if (!hostId || !agentId) {
    return {
      activeGoal: null,
      armedTimers: EMPTY_TIMERS,
      backgroundTasks: EMPTY_BG_TASKS,
      hasRunningBgTask: false,
      overviewIndicatorTone: "none",
      overviewIndicatorReason: "none",
    };
  }

  const hostDetail = state.hostDetails[hostId] ?? null;
  const ag = hostDetail?.agents.find((a) => a.agent_id === agentId) ?? null;

  const goalsForAgent = (hostDetail?.goal_runs ?? []).filter((g) => g.agent_id === agentId);
  const activeGoal =
    goalsForAgent.find((g) => ACTIVE_GOAL_STATUSES.has(g.status)) ?? null;

  const armedTimers = (hostDetail?.conditional_timers ?? []).filter(
    (t) => t.agent_id === agentId && t.state === "armed",
  );

  const backgroundTasks =
    options?.backgroundTasks ??
    state.stages?.[`${hostId}::${agentId}`]?.insights?.background_tasks ??
    EMPTY_BG_TASKS;
  const hasRunningBgTask =
    isAgentBackgroundWaiting(ag) || backgroundTasks.some((t) => t.status === "running");

  const subagents = options?.subagents ?? selectSessionSubagents(state, hostId, agentId);
  let overviewIndicatorTone: OverviewIndicatorTone = "none";
  let overviewIndicatorReason: OverviewIndicatorReason = "none";
  if (subagents.awaitingCount > 0) {
    overviewIndicatorTone = "approval";
    overviewIndicatorReason = "approval";
  } else if (subagents.activeCount > 0) {
    overviewIndicatorTone = "running";
    overviewIndicatorReason = "subagent";
  } else if (hasRunningBgTask) {
    overviewIndicatorTone = "running";
    overviewIndicatorReason = "background_task";
  } else if (activeGoal) {
    overviewIndicatorTone = "goal";
    overviewIndicatorReason = "goal";
  } else if (armedTimers.length > 0) {
    overviewIndicatorTone = "goal";
    overviewIndicatorReason = "timer";
  }

  return {
    activeGoal,
    armedTimers,
    backgroundTasks,
    hasRunningBgTask,
    overviewIndicatorTone,
    overviewIndicatorReason,
  };
}

export interface FleetHealthSummary {
  readonly totalHosts: number;
  readonly onlineDirectCount: number;
  readonly onlineRelayCount: number;
  readonly connectingCount: number;
  readonly offlineCount: number;
  readonly totalPendingApprovals: number;
}

export function selectFleetHealth(
  state: Pick<OrbitUiState, "fleet">,
): FleetHealthSummary {
  let onlineDirectCount = 0;
  let onlineRelayCount = 0;
  let connectingCount = 0;
  let offlineCount = 0;

  for (const host of state.fleet.hosts) {
    switch (host.connection_state) {
      case "online_direct":
        onlineDirectCount += 1;
        break;
      case "online_relay":
        onlineRelayCount += 1;
        break;
      case "connecting_direct":
      case "connecting_relay":
        connectingCount += 1;
        break;
      default:
        offlineCount += 1;
        break;
    }
  }

  return {
    totalHosts: state.fleet.hosts.length,
    onlineDirectCount,
    onlineRelayCount,
    connectingCount,
    offlineCount,
    totalPendingApprovals: state.fleet.total_pending_approvals,
  };
}
