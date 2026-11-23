import type {
  GitReviewView,
  StageTurnView,
  TimelineItemView,
  ToolCallDetail,
  TurnLiveSegmentView,
} from "@/lib/orbit-ipc";

export const MAX_CACHED_STAGE_VIEWS = 14;

export interface StageCachePinTab {
  id?: string;
  kind: string;
  title?: string;
  hostId?: string;
  subagentCallId?: string;
  subagentRole?: string;
  subagentPrompt?: string;
  childAgentId?: string | null;
  peerAgentId?: string;
  parentAgentId?: string | null;
  providerSubagentId?: string | null;
}

export interface OpenSubagentTabSpec {
  callId: string;
  role: string;
  promptPreview: string;
  tabHostId: string | null;
  parentHostId: string | null;
  parentAgentId: string | null;
  effectiveChildAgentId: string | null;
  effectiveProviderSubagentId: string | null;
}

export function resolveSubagentTabId(spec: OpenSubagentTabSpec): string {
  if (spec.effectiveChildAgentId) {
    return `tab:subagent:agent:${spec.tabHostId ?? "local"}:${spec.effectiveChildAgentId}`;
  }
  if (spec.effectiveProviderSubagentId) {
    return `tab:subagent:provider:${spec.parentHostId ?? "local"}:${spec.parentAgentId ?? ""}:${spec.effectiveProviderSubagentId}`;
  }
  return `tab:subagent:call:${spec.callId}`;
}

/**
 * Inserts a distinct subagent tab or updates & focuses the matching existing subagent tab in place
 * without overwriting other open subagent tabs.
 */
export function upsertSubagentSidePanelTab<T extends StageCachePinTab & { id: string; title: string }>(
  tabs: readonly T[],
  spec: OpenSubagentTabSpec,
): { tabs: T[]; activeTabId: string; created: boolean } {
  const canonicalId = resolveSubagentTabId(spec);
  const normalizedHostId = spec.tabHostId ?? undefined;
  const normalizedParentId = spec.parentAgentId ?? null;

  const existingIdx = tabs.findIndex((t) => {
    if (t.kind !== "subagent") return false;
    if (t.id === canonicalId) return true;
    if (
      spec.effectiveChildAgentId &&
      t.childAgentId === spec.effectiveChildAgentId &&
      (t.hostId ?? undefined) === normalizedHostId
    ) {
      return true;
    }
    if (
      spec.effectiveProviderSubagentId &&
      t.providerSubagentId === spec.effectiveProviderSubagentId &&
      (t.parentAgentId ?? null) === normalizedParentId &&
      (t.hostId ?? undefined) === normalizedHostId
    ) {
      return true;
    }
    if (
      spec.callId &&
      t.subagentCallId === spec.callId &&
      (t.parentAgentId ?? null) === normalizedParentId
    ) {
      return true;
    }
    return false;
  });

  if (existingIdx >= 0) {
    const existing = tabs[existingIdx];
    const updated: T = {
      ...existing,
      title: spec.role || existing.title || "Subagent",
      hostId: normalizedHostId,
      subagentCallId: spec.callId || existing.subagentCallId,
      subagentRole: spec.role || existing.subagentRole,
      subagentPrompt: spec.promptPreview || existing.subagentPrompt,
      childAgentId: spec.effectiveChildAgentId,
      providerSubagentId: spec.effectiveProviderSubagentId ?? null,
      parentAgentId: normalizedParentId,
    };
    const nextTabs = tabs.slice();
    nextTabs[existingIdx] = updated;
    return { tabs: nextTabs, activeTabId: existing.id, created: false };
  }

  const newTab = {
    id: canonicalId,
    kind: "subagent",
    title: spec.role || "Subagent",
    hostId: normalizedHostId,
    subagentCallId: spec.callId,
    subagentRole: spec.role,
    subagentPrompt: spec.promptPreview,
    childAgentId: spec.effectiveChildAgentId,
    providerSubagentId: spec.effectiveProviderSubagentId ?? null,
    parentAgentId: normalizedParentId,
  } as unknown as T;

  return {
    tabs: [...tabs, newTab],
    activeTabId: canonicalId,
    created: true,
  };
}

export interface StageCachePinSource {
  selectedHostId: string | null;
  selectedAgentId: string | null;
  isDraftNewSession?: boolean;
  sidePanelTabs: readonly StageCachePinTab[];
}

export function collectPinnedStageKeys(source: StageCachePinSource): Set<string> {
  const pinned = new Set<string>();
  const selectedHostId = source.selectedHostId;

  if (selectedHostId && source.selectedAgentId && !source.isDraftNewSession) {
    pinned.add(`${selectedHostId}::${source.selectedAgentId}`);
  }

  for (const tab of source.sidePanelTabs) {
    const tabHostId = tab.hostId ?? selectedHostId;
    if (tab.childAgentId && tabHostId) {
      pinned.add(`${tabHostId}::${tab.childAgentId}`);
    }
    if (tab.peerAgentId && tabHostId) {
      pinned.add(`${tabHostId}::${tab.peerAgentId}`);
    }
    if (tab.parentAgentId) {
      if (selectedHostId) {
        pinned.add(`${selectedHostId}::${tab.parentAgentId}`);
      }
      if (tab.hostId && tab.hostId !== selectedHostId) {
        pinned.add(`${tab.hostId}::${tab.parentAgentId}`);
      }
    }
  }

  return pinned;
}

export function recordStageAccess(accessOrder: readonly string[], key: string): string[] {
  if (!key) return accessOrder.slice();
  if (accessOrder.length > 0 && accessOrder[accessOrder.length - 1] === key) {
    return accessOrder.slice();
  }
  const next = accessOrder.filter((k) => k !== key);
  next.push(key);
  return next;
}

export function removeStageAccessKeys(
  accessOrder: readonly string[],
  removedKeys: ReadonlySet<string>,
): string[] {
  if (removedKeys.size === 0 || accessOrder.length === 0) {
    return accessOrder.slice();
  }
  return accessOrder.filter((k) => !removedKeys.has(k));
}

export interface StageEvictionResult<T> {
  stages: Record<string, T>;
  accessOrder: string[];
  evictedKeys: string[];
}

/**
 * Enforces an LRU cap on cached renderer stage views while never evicting pinned entries
 * (currently selected thread, open managed subagent / peer split tabs, or parent threads of
 * open provider subagent tabs).
 */
export function evictInactiveStageEntries<T>(
  stages: Record<string, T>,
  accessOrder: readonly string[],
  pinnedKeys: ReadonlySet<string>,
  maxEntries: number = MAX_CACHED_STAGE_VIEWS,
): StageEvictionResult<T> {
  const cap = Math.max(0, Math.floor(maxEntries));
  const presentKeys = Object.keys(stages);
  const presentSet = new Set(presentKeys);

  // Preserve known recency order (oldest -> newest); prepend any untracked present keys as oldest.
  const trackedSet = new Set<string>();
  const knownInOrder: string[] = [];
  for (const key of accessOrder) {
    if (presentSet.has(key) && !trackedSet.has(key)) {
      trackedSet.add(key);
      knownInOrder.push(key);
    }
  }
  const untrackedOldest: string[] = [];
  for (const key of presentKeys) {
    if (!trackedSet.has(key)) {
      untrackedOldest.push(key);
    }
  }
  const normalizedOrder =
    untrackedOldest.length > 0 ? [...untrackedOldest, ...knownInOrder] : knownInOrder;

  if (normalizedOrder.length <= cap) {
    return {
      stages,
      accessOrder: normalizedOrder,
      evictedKeys: [],
    };
  }

  let remaining = normalizedOrder.length;
  const evictedKeys: string[] = [];
  const evictedSet = new Set<string>();

  for (const key of normalizedOrder) {
    if (remaining <= cap) break;
    if (pinnedKeys.has(key)) continue;
    evictedKeys.push(key);
    evictedSet.add(key);
    remaining -= 1;
  }

  if (evictedKeys.length === 0) {
    return {
      stages,
      accessOrder: normalizedOrder,
      evictedKeys: [],
    };
  }

  const nextStages: Record<string, T> = {};
  for (const key of presentKeys) {
    if (!evictedSet.has(key)) {
      nextStages[key] = stages[key];
    }
  }
  const nextOrder = normalizedOrder.filter((key) => !evictedSet.has(key));

  return {
    stages: nextStages,
    accessOrder: nextOrder,
    evictedKeys,
  };
}

function fingerprintToolDetail(detail: ToolCallDetail): string {
  switch (detail.kind) {
    case "shell":
      return `sh:${detail.command.length}:${detail.exit_code ?? ""}:${detail.output_preview?.length ?? 0}`;
    case "file_read":
      return `fr:${detail.path.length}:${detail.start_line ?? ""}:${detail.end_line ?? ""}`;
    case "file_edit":
      return `fe:${detail.path.length}:${detail.unified_diff?.length ?? 0}`;
    case "search":
      return `se:${detail.query.length}:${detail.scope?.length ?? 0}`;
    case "subagent":
      return `sa:${detail.role.length}:${detail.prompt_preview.length}:${detail.child_agent_id ?? ""}`;
    case "generic":
      return `gn:${detail.tool_name.length}:${detail.arguments_json.length}`;
  }
}

function fingerprintTimelineItem(item: TimelineItemView | null | undefined): string {
  if (!item) return "none";
  switch (item.item_type) {
    case "assistant_message":
      return `am:${item.seq}:${item.text.length}:${item.streaming ? 1 : 0}:${item.blocks.length}`;
    case "assistant_thinking":
      return `at:${item.seq}:${item.text.length}`;
    case "tool_call":
      return `tc:${item.seq}:${item.status}:${item.summary_title.length}:${item.diff_files.length}:${fingerprintToolDetail(item.detail)}`;
    case "permission_request":
      return `pr:${item.seq}:${item.resolved_decision ?? ""}`;
    case "question_request":
      return `qr:${item.seq}:${item.resolved_answers?.length ?? 0}`;
    case "notice":
      return `nt:${item.seq}:${item.level}:${item.message.length}`;
    case "user_prompt":
      return `up:${item.seq}:${item.text.length}`;
    case "agent_message":
      return `ag:${item.seq}:${item.text.length}`;
  }
}

function fingerprintLiveSegment(seg: TurnLiveSegmentView | undefined): string {
  if (!seg) return "none";
  switch (seg.kind) {
    case "assistant_message":
      return `msg:${seg.key}:${fingerprintTimelineItem(seg.item)}`;
    case "agent_message":
      return `agm:${seg.key}:${fingerprintTimelineItem(seg.item)}`;
    case "notice":
      return `not:${seg.key}:${fingerprintTimelineItem(seg.item)}`;
    case "compaction":
      return `cmp:${seg.key}:${seg.label.length}`;
    case "wakeup":
      return `wkp:${seg.key}:${seg.label.length}`;
    case "step_group": {
      let thinkingLen = 0;
      let runningTools = 0;
      let failedTools = 0;
      let detailLenSum = 0;
      for (const it of seg.items) {
        if (it.item_type === "assistant_thinking") {
          thinkingLen += it.text.length;
        } else if (it.item_type === "tool_call") {
          if (it.status === "running") runningTools += 1;
          else if (it.status === "failed" || it.status === "denied") failedTools += 1;
          detailLenSum +=
            it.summary_title.length +
            (it.detail.kind === "shell"
              ? (it.detail.output_preview?.length ?? 0) + it.detail.command.length
              : it.detail.kind === "file_edit"
                ? (it.detail.unified_diff?.length ?? 0)
                : it.detail.kind === "generic"
                  ? it.detail.arguments_json.length
                  : 0);
        }
      }
      const lastItem = seg.items[seg.items.length - 1];
      return `sg:${seg.key}:${seg.items.length}:${seg.has_running_step ? 1 : 0}:${seg.summary.counts.total_steps}:${seg.summary.live_label.length}:${thinkingLen}:${runningTools}:${failedTools}:${detailLenSum}:${fingerprintTimelineItem(lastItem)}`;
    }
  }
}

/**
 * Computes a streaming-sensitive fingerprint for provider subagent turns so live text deltas,
 * thinking deltas, tool status/output updates, terminal reply streaming, notices, and diffs
 * all trigger reactive UI updates immediately.
 */
export function computeSubagentTurnsFingerprint(turns: readonly StageTurnView[]): string {
  const lastTurn = turns[turns.length - 1];
  if (!lastTurn) return "0";
  const allSegmentsSig = lastTurn.live_segments.map(fingerprintLiveSegment).join(",");
  let diffAddSum = 0;
  let diffDelSum = 0;
  for (const d of lastTurn.turn_diffs) {
    diffAddSum += d.additions;
    diffDelSum += d.deletions;
  }
  const termReply = lastTurn.terminal_reply;
  const termReplySig =
    termReply && termReply.item_type === "assistant_message"
      ? `${termReply.text.length}:${termReply.streaming ? 1 : 0}:${termReply.blocks.length}`
      : "0:0:0";
  return [
    turns.length,
    lastTurn.key,
    lastTurn.live_segments.length,
    allSegmentsSig,
    termReplySig,
    lastTurn.notices.length,
    `${lastTurn.turn_diffs.length}:${diffAddSum}:${diffDelSum}`,
    lastTurn.has_active_item ? 1 : 0,
    lastTurn.has_error ? 1 : 0,
    lastTurn.work_counts.total_steps,
    lastTurn.folded_steps.length,
    lastTurn.work_summary_live_label.length,
    lastTurn.completed_at_ms ?? 0,
  ].join("|");
}

/**
 * Lightweight signature for a cached `GitReviewView` (branch + status files + diff file totals)
 * used to detect real git state changes and avoid infinite `git_changed` -> `ListWorkspaceFiles` loops.
 */
export function computeGitReviewSignature(review: GitReviewView | null | undefined): string {
  if (!review) return "";
  const statusPart = review.status
    ? `${review.status.is_git_repo ? 1 : 0}:${review.status.branch ?? ""}:${review.status.files
        .map((f) => `${f.path}:${f.status}:${f.staged ? 1 : 0}:${f.untracked ? 1 : 0}`)
        .join(",")}`
    : "nostatus";
  const diffPart = review.diff_files
    .map((f) => `${f.path}:${f.additions}:${f.deletions}`)
    .join(",");
  return `${statusPart}|${diffPart}`;
}

/**
 * Strips Windows extended-length verbatim path prefixes (`\\?\C:\...` -> `C:\...`,
 * `\\?\UNC\server\share` -> `\\server\share`) and normalizes Windows drive-letter casing
 * (`c:\...` -> `C:\...`) while leaving Unix paths untouched.
 */
export function stripWindowsVerbatimPrefix(rawPath: string): string {
  let out = rawPath;
  if (/^\\\\\?\\UNC\\/i.test(out)) {
    out = `\\\\${out.slice(8)}`;
  } else if (out.startsWith("\\\\?\\") && /^[A-Za-z]:/.test(out.slice(4))) {
    out = out.slice(4);
  }
  if (/^[a-z]:(?:[\\/]|$)/.test(out)) {
    out = `${out[0].toUpperCase()}${out.slice(1)}`;
  }
  return out;
}

/**
 * Normalizes a workspace CWD for equality comparison by trimming whitespace, stripping
 * Windows verbatim `\\?\` prefixes, normalizing Windows drive-letter casing, and stripping
 * redundant trailing slashes while preserving root paths (`/`, `C:\`, `C:/`).
 */
export function normalizeCwdForCompare(cwd: string | null | undefined): string {
  if (!cwd) return "";
  const trimmed = stripWindowsVerbatimPrefix(cwd.trim());
  if (!trimmed) return "";
  if (trimmed === "/" || trimmed === "\\") return trimmed;
  if (/^[A-Za-z]:[\\/]+$/.test(trimmed)) {
    return `${trimmed.slice(0, 2)}${trimmed[2]}`;
  }
  return trimmed.replace(/[\\/]+$/, "");
}

/**
 * Merges an incoming `GitReviewView` snapshot into the cached workspace snapshot so a single-file
 * diff fetch (`selected_path !== null`) never clobbers workspace-wide `diff_files` totals used by
 * the Overview panel (`+added -deleted`) and Review file list.
 */
export function mergeGitReviewSnapshot(
  prev: GitReviewView | undefined,
  incoming: GitReviewView,
): GitReviewView {
  if (incoming.selected_path === null || !prev || prev.diff_files.length === 0) {
    return incoming;
  }

  const incomingStatus = incoming.status ?? prev.status;
  const activeStatusPaths = incomingStatus
    ? new Set(incomingStatus.files.map((f) => f.path))
    : null;

  const incomingByPath = new Map(incoming.diff_files.map((f) => [f.path, f]));
  const mergedFiles = [];
  const seen = new Set<string>();

  for (const prevFile of prev.diff_files) {
    if (prevFile.path === incoming.selected_path) {
      const updated = incomingByPath.get(prevFile.path);
      if (updated) {
        mergedFiles.push(updated);
        seen.add(updated.path);
      }
      continue;
    }
    if (incomingByPath.has(prevFile.path)) {
      const updated = incomingByPath.get(prevFile.path)!;
      mergedFiles.push(updated);
      seen.add(updated.path);
      continue;
    }
    if (activeStatusPaths && !activeStatusPaths.has(prevFile.path)) {
      continue;
    }
    mergedFiles.push(prevFile);
    seen.add(prevFile.path);
  }

  for (const incFile of incoming.diff_files) {
    if (!seen.has(incFile.path)) {
      mergedFiles.push(incFile);
      seen.add(incFile.path);
    }
  }

  return {
    ...incoming,
    status: incomingStatus,
    selected_path: prev.selected_path === null ? null : incoming.selected_path,
    diff_files: mergedFiles,
  };
}

