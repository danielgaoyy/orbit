import type { DiffFileView, TimelineItemView } from "@/lib/orbit-ipc";

export interface TurnFileDiffSummary {
  path: string;
  additions: number;
  deletions: number;
}

export interface TurnWorkCounts {
  editedFileCount: number;
  commandCount: number;
  readFileCount: number;
  searchCount: number;
  subagentCount: number;
  otherToolCount: number;
  thoughtCount: number;
  narrationCount: number;
  totalSteps: number;
}

export type TurnLiveSegment =
  | {
      kind: "step_group";
      key: string;
      items: TimelineItemView[];
    }
  | {
      kind: "assistant_message";
      key: string;
      item: Extract<TimelineItemView, { item_type: "assistant_message" }>;
    }
  | {
      kind: "notice";
      key: string;
      item: Extract<TimelineItemView, { item_type: "notice" }>;
    }
  | {
      kind: "compaction";
      key: string;
      label: string;
    };

export interface StageTurn {
  key: string;
  turnIndex: number;
  anchorSeq: number;
  userPrompt: Extract<TimelineItemView, { item_type: "user_prompt" }> | null;
  isCompactionCommand: boolean;
  /** Intermediate steps prior to `terminalReply` (eligible for completed-turn auto-fold). */
  foldedSteps: TimelineItemView[];
  /** Counts of work performed inside `foldedSteps` (or entire turn). */
  workCounts: TurnWorkCounts;
  /** Human-readable Claude / Paseo style summary sentence (e.g. `Edited 2 files and ran 1 command`). */
  workSummaryLabel: string;
  /** Final synthesis reply of the turn, kept visible when the completed turn folds. */
  terminalReply: Extract<TimelineItemView, { item_type: "assistant_message" }> | null;
  /** Notices (warnings/errors) surfaced unconditionally so failures are never buried. */
  notices: Extract<TimelineItemView, { item_type: "notice" }>[];
  /** Chronological segments used while the turn is actively streaming or unfolded without a reply. */
  liveSegments: TurnLiveSegment[];
  /** Deduplicated per-file diff stats touched during this turn. */
  turnDiffs: TurnFileDiffSummary[];
  /** Whether any tool call in this turn failed or was denied. */
  hasError: boolean;
  /** Whether any tool call or message in this turn is currently in flight. */
  hasActiveItem: boolean;
}

export interface ProcessedTimelineStage {
  turns: StageTurn[];
  pendingPermissions: Extract<TimelineItemView, { item_type: "permission_request" }>[];
  pendingQuestions: Extract<TimelineItemView, { item_type: "question_request" }>[];
  sessionTurnDiffs: DiffFileView[];
}

export function stripWorkspacePrefix(filePath: string, cwd?: string | null): string {
  if (!filePath) return filePath;
  const cleanCwd = (cwd ?? "").replace(/\/+$/, "");
  if (cleanCwd && cleanCwd !== "~" && filePath.startsWith(`${cleanCwd}/`)) {
    return filePath.slice(cleanCwd.length + 1);
  }
  return filePath;
}

export function formatDurationCompact(durationMs: number): string {
  const totalSeconds = Math.max(1, Math.round(durationMs / 1000));
  if (totalSeconds < 60) {
    return `${totalSeconds}s`;
  }
  const minutes = Math.floor(totalSeconds / 60);
  const seconds = totalSeconds % 60;
  if (minutes >= 60) {
    const hours = Math.floor(minutes / 60);
    const remMin = minutes % 60;
    return remMin > 0 ? `${hours}h ${remMin}m` : `${hours}h`;
  }
  return seconds > 0 ? `${minutes}m ${seconds}s` : `${minutes}m`;
}

function joinNaturalParts(parts: string[]): string {
  if (parts.length === 0) return "";
  if (parts.length === 1) {
    return parts[0].charAt(0).toUpperCase() + parts[0].slice(1);
  }
  if (parts.length === 2) {
    const joined = `${parts[0]} and ${parts[1]}`;
    return joined.charAt(0).toUpperCase() + joined.slice(1);
  }
  const joined = `${parts.slice(0, -1).join(", ")}, and ${parts[parts.length - 1]}`;
  return joined.charAt(0).toUpperCase() + joined.slice(1);
}

export function summarizeWorkItems(items: TimelineItemView[]): {
  counts: TurnWorkCounts;
  label: string;
} {
  const editedPaths = new Set<string>();
  let commandCount = 0;
  let readFileCount = 0;
  let searchCount = 0;
  let subagentCount = 0;
  let otherToolCount = 0;
  let thoughtCount = 0;
  let narrationCount = 0;

  for (const it of items) {
    if (it.item_type === "assistant_thinking") {
      thoughtCount += 1;
    } else if (it.item_type === "assistant_message") {
      narrationCount += 1;
    } else if (it.item_type === "tool_call") {
      switch (it.detail.kind) {
        case "file_edit":
          editedPaths.add(it.detail.path);
          break;
        case "shell":
          commandCount += 1;
          break;
        case "file_read":
          readFileCount += 1;
          break;
        case "search":
          searchCount += 1;
          break;
        case "subagent":
          subagentCount += 1;
          break;
        case "generic":
          otherToolCount += 1;
          break;
      }
    }
  }

  const editedFileCount = editedPaths.size;
  const actionSteps =
    editedFileCount + commandCount + readFileCount + searchCount + subagentCount + otherToolCount;
  const totalSteps = actionSteps + thoughtCount;

  const parts: string[] = [];
  if (editedFileCount > 0) {
    parts.push(`edited ${editedFileCount} ${editedFileCount === 1 ? "file" : "files"}`);
  }
  if (commandCount > 0) {
    parts.push(`ran ${commandCount} ${commandCount === 1 ? "command" : "commands"}`);
  }
  if (readFileCount > 0) {
    parts.push(`read ${readFileCount} ${readFileCount === 1 ? "file" : "files"}`);
  }
  if (searchCount > 0) {
    parts.push(`searched ${searchCount} ${searchCount === 1 ? "query" : "queries"}`);
  }
  if (subagentCount > 0) {
    parts.push(`delegated ${subagentCount} ${subagentCount === 1 ? "subagent" : "subagents"}`);
  }
  if (otherToolCount > 0) {
    parts.push(`used ${otherToolCount} ${otherToolCount === 1 ? "tool" : "tools"}`);
  }

  // Reasoning stays inside details unless reasoning was the only work in the turn
  let label = joinNaturalParts(parts);
  if (!label) {
    if (thoughtCount > 0) {
      label = "Thought through approach";
    } else if (narrationCount > 0) {
      label = `${narrationCount} intermediate ${narrationCount === 1 ? "note" : "notes"}`;
    } else {
      label = "Completed steps";
    }
  }

  return {
    counts: {
      editedFileCount,
      commandCount,
      readFileCount,
      searchCount,
      subagentCount,
      otherToolCount,
      thoughtCount,
      narrationCount,
      totalSteps,
    },
    label,
  };
}

function isStepRowItem(item: TimelineItemView): boolean {
  return (
    item.item_type === "assistant_thinking" ||
    item.item_type === "tool_call" ||
    item.item_type === "permission_request" ||
    item.item_type === "question_request"
  );
}

function buildLiveSegments(bodyItems: TimelineItemView[]): TurnLiveSegment[] {
  const segments: TurnLiveSegment[] = [];
  let stepBuf: TimelineItemView[] = [];

  const flushSteps = () => {
    if (stepBuf.length === 0) return;
    segments.push({
      kind: "step_group",
      key: `steps-${stepBuf[0].seq}-${stepBuf.length}`,
      items: stepBuf,
    });
    stepBuf = [];
  };

  for (const it of bodyItems) {
    if (isStepRowItem(it)) {
      stepBuf.push(it);
      continue;
    }
    flushSteps();
    if (it.item_type === "assistant_message") {
      segments.push({
        kind: "assistant_message",
        key: `msg-${it.item_id}-${it.seq}`,
        item: it,
      });
    } else if (it.item_type === "notice") {
      if (/compact/i.test(it.message)) {
        segments.push({
          kind: "compaction",
          key: `compact-${it.seq}`,
          label: it.message,
        });
      } else {
        segments.push({
          kind: "notice",
          key: `notice-${it.seq}`,
          item: it,
        });
      }
    }
  }

  flushSteps();
  return segments;
}

function finalizeTurn(
  turnIndex: number,
  userPrompt: Extract<TimelineItemView, { item_type: "user_prompt" }> | null,
  bodyItems: TimelineItemView[],
): StageTurn {
  const anchorSeq = userPrompt?.seq ?? bodyItems[0]?.seq ?? turnIndex;
  const key = userPrompt ? `turn-${userPrompt.message_id}` : `turn-init-${anchorSeq}`;
  const isCompactionCommand = Boolean(
    userPrompt &&
      (/^\s*\/compact\b/i.test(userPrompt.text.trim()) ||
        (userPrompt.badges?.some((b) => b.kind === "slash" && b.label === "/compact") &&
          userPrompt.text.trim().length === 0)),
  );

  const notices: Extract<TimelineItemView, { item_type: "notice" }>[] = [];
  const nonNoticeBody: TimelineItemView[] = [];
  let hasError = false;
  let hasActiveItem = false;

  const diffByPath = new Map<string, TurnFileDiffSummary>();

  for (const it of bodyItems) {
    if (it.item_type === "tool_call") {
      if (it.status === "running") hasActiveItem = true;
      if (it.status === "failed" || it.status === "denied") hasError = true;
      for (const df of it.diff_files) {
        const prev = diffByPath.get(df.path);
        if (prev) {
          prev.additions += df.additions;
          prev.deletions += df.deletions;
        } else {
          diffByPath.set(df.path, {
            path: df.path,
            additions: df.additions,
            deletions: df.deletions,
          });
        }
      }
      if (it.detail.kind === "file_edit" && !diffByPath.has(it.detail.path)) {
        diffByPath.set(it.detail.path, {
          path: it.detail.path,
          additions: 0,
          deletions: 0,
        });
      }
    } else if (it.item_type === "assistant_message" && it.streaming) {
      hasActiveItem = true;
    }

    if (it.item_type === "notice") {
      if (!/compact/i.test(it.message)) {
        notices.push(it);
      }
      continue;
    }

    nonNoticeBody.push(it);
  }

  // Identify terminal assistant reply: the trailing `assistant_message` when no step follows it
  let terminalReplyIndex = -1;
  for (let i = nonNoticeBody.length - 1; i >= 0; i -= 1) {
    const candidate = nonNoticeBody[i];
    if (candidate.item_type === "assistant_message") {
      terminalReplyIndex = i;
      break;
    }
    // If a tool call or thinking step occurred *after* the last assistant message,
    // there is no final synthesis reply yet — keep steps in live/open view.
    if (isStepRowItem(candidate)) {
      break;
    }
  }

  const terminalReply =
    terminalReplyIndex >= 0
      ? (nonNoticeBody[terminalReplyIndex] as Extract<
          TimelineItemView,
          { item_type: "assistant_message" }
        >)
      : null;

  const foldedSteps =
    terminalReplyIndex > 0
      ? nonNoticeBody.slice(0, terminalReplyIndex)
      : terminalReplyIndex === -1
        ? nonNoticeBody
        : [];

  const { counts: workCounts, label: workSummaryLabel } = summarizeWorkItems(foldedSteps);

  return {
    key,
    turnIndex,
    anchorSeq,
    userPrompt,
    isCompactionCommand,
    foldedSteps,
    workCounts,
    workSummaryLabel,
    terminalReply,
    notices,
    liveSegments: buildLiveSegments(bodyItems),
    turnDiffs: Array.from(diffByPath.values()),
    hasError,
    hasActiveItem,
  };
}

/**
 * Structures the flat timeline into Claude.app-style semantic Turns,
 * enabling completed-turn auto-folding (`Worked for ... · Edited N files`),
 * live streaming vertical step rails, turn-end diff summaries, and sticky HITL gates.
 */
export function processTimelineForStage(
  items: TimelineItemView[],
  _enableCoalescing = true,
): ProcessedTimelineStage {
  const turns: StageTurn[] = [];
  const pendingPermissions: Extract<TimelineItemView, { item_type: "permission_request" }>[] = [];
  const pendingQuestions: Extract<TimelineItemView, { item_type: "question_request" }>[] = [];
  const sessionTurnDiffs: DiffFileView[] = [];

  let currentPrompt: Extract<TimelineItemView, { item_type: "user_prompt" }> | null = null;
  let currentBody: TimelineItemView[] = [];
  let hasOpenTurn = false;

  const flushTurn = () => {
    if (!hasOpenTurn) return;
    turns.push(finalizeTurn(turns.length, currentPrompt, currentBody));
    currentPrompt = null;
    currentBody = [];
    hasOpenTurn = false;
  };

  for (const item of items) {
    if (item.item_type === "tool_call" && item.diff_files.length > 0) {
      for (const df of item.diff_files) {
        sessionTurnDiffs.push(df);
      }
    }

    // Active unresolved human-in-the-loop items anchor to the Sticky Dock above the Composer
    if (item.item_type === "permission_request" && !item.resolved_decision) {
      pendingPermissions.push(item);
      continue;
    }

    if (item.item_type === "question_request" && !item.resolved_answers) {
      pendingQuestions.push(item);
      continue;
    }

    if (item.item_type === "user_prompt") {
      flushTurn();
      currentPrompt = item;
      currentBody = [];
      hasOpenTurn = true;
      continue;
    }

    if (!hasOpenTurn) {
      hasOpenTurn = true;
      currentPrompt = null;
    }
    currentBody.push(item);
  }

  flushTurn();

  return {
    turns,
    pendingPermissions,
    pendingQuestions,
    sessionTurnDiffs,
  };
}
