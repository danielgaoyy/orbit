import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
} from "react";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import {
  AlertTriangle,
  ArrowDown,
  Check,
  ChevronRight,
  Copy,
  FileCode2,
  FileDiff,
  FileImage,
  FilePlus2,
  FileSearch,
  FileSymlink,
  FileText,
  FoldVertical,
  GitCompareArrows,
  Globe,
  Layers,
  ListTodo,
  Loader2,
  MessageSquareText,
  Quote,
  RotateCcw,
  Scissors,
  Search,
  Sparkles,
  SquareTerminal,
  Terminal as TerminalIcon,
  Undo2,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import {
  type DiffLineView,
  type PromptBadge,
  type TimelineItemView,
} from "@/lib/orbit-ipc";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import {
  type StageTurn,
  type TurnFileDiffSummary,
  type WorkSummaryResult,
  classifySearchTool,
  describeToolStep,
  extractThinkingHeadline,
  formatDurationCompact,
  formatThoughtSummaryLabel,
  processTimelineForStage,
  stripWorkspacePrefix,
} from "@/lib/timeline-coalesce";
import { OmniComposer } from "@/components/composer/omni-composer";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { StickyApprovalDock } from "@/components/stage/stage-approval-dock";
import { ChatOutlineRail } from "@/components/stage/stage-outline-rail";
import {
  AgentMessageCard,
  ParentSubagentBanner,
  SubagentsTrack,
} from "@/components/stage/subagents-track";
import { StageOverviewPanel } from "@/components/stage/stage-overview-panel";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const USER_PROMPT_PREVIEW_MAX_PX = 124;

interface TurnTimingRecord {
  startedAtMs: number;
  completedAtMs: number | null;
  durationMs: number | null;
  workDurationMs: number | null;
}

type ViewportAnchorHoldFn = (
  triggerEl: HTMLElement | null,
  apply: () => void,
  edge?: "top" | "bottom",
) => void;

type ThinkingDurationLookupFn = (thinkKey: string) => number | null;

// Singleton Intl formatters & tooltip cache so N turns allocate zero formatter instances per render
const SHORT_DATE_SAME_YEAR_FMT =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric" })
    : null;
const SHORT_DATE_WITH_YEAR_FMT =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat("en-US", { month: "short", day: "numeric", year: "numeric" })
    : null;
const FULL_TIMESTAMP_FMT =
  typeof Intl !== "undefined"
    ? new Intl.DateTimeFormat("en-US", {
        year: "numeric",
        month: "short",
        day: "numeric",
        hour: "2-digit",
        minute: "2-digit",
        hour12: false,
      })
    : null;
const fullTimestampTooltipCache = new Map<number, string>();

// Shared singleton 15s relative-time clock so N rendered turns share 1 timer (0 timers when idle/empty)
let sharedRelativeNowMs = Date.now();
let sharedRelativeTimerId: number | null = null;
const relativeClockListeners = new Set<() => void>();

function subscribeRelativeClock(listener: () => void): () => void {
  relativeClockListeners.add(listener);
  if (sharedRelativeTimerId === null && typeof window !== "undefined") {
    sharedRelativeNowMs = Date.now();
    sharedRelativeTimerId = window.setInterval(() => {
      sharedRelativeNowMs = Date.now();
      for (const fn of relativeClockListeners) {
        fn();
      }
    }, 15_000);
  }
  return () => {
    relativeClockListeners.delete(listener);
    if (relativeClockListeners.size === 0 && sharedRelativeTimerId !== null) {
      window.clearInterval(sharedRelativeTimerId);
      sharedRelativeTimerId = null;
    }
  };
}

function useSharedRelativeNow(): number {
  return useSyncExternalStore(
    subscribeRelativeClock,
    () => sharedRelativeNowMs,
    () => sharedRelativeNowMs,
  );
}

function formatRelativeTimeLabel(timestampMs: number, nowMs: number): string {
  const diffSec = Math.max(0, Math.floor((nowMs - timestampMs) / 1000));
  if (diffSec < 60) {
    return "just now";
  }
  const mins = Math.floor(diffSec / 60);
  if (mins < 60) {
    return `${mins}m ago`;
  }
  const hours = Math.floor(mins / 60);
  if (hours < 24) {
    return `${hours}h ago`;
  }
  const days = Math.floor(hours / 24);
  if (days < 30) {
    return `${days}d ago`;
  }
  try {
    const d = new Date(timestampMs);
    const sameYear = d.getFullYear() === new Date(nowMs).getFullYear();
    const fmt = sameYear ? SHORT_DATE_SAME_YEAR_FMT : SHORT_DATE_WITH_YEAR_FMT;
    return fmt ? fmt.format(d) : `${days}d ago`;
  } catch {
    return `${days}d ago`;
  }
}

function formatFullTimestampTooltip(timestampMs: number): string {
  const cached = fullTimestampTooltipCache.get(timestampMs);
  if (cached !== undefined) return cached;
  try {
    const formatted = FULL_TIMESTAMP_FMT ? FULL_TIMESTAMP_FMT.format(new Date(timestampMs)) : "";
    if (fullTimestampTooltipCache.size > 1000) {
      fullTimestampTooltipCache.clear();
    }
    fullTimestampTooltipCache.set(timestampMs, formatted);
    return formatted;
  } catch {
    return "";
  }
}

function formatJsonPreview(rawJson: string): string {
  const trimmed = rawJson.trim();
  if (!trimmed) return "";
  try {
    return JSON.stringify(JSON.parse(trimmed), null, 2);
  } catch {
    return trimmed;
  }
}

function parseUnifiedDiffFallback(unifiedDiff: string): DiffLineView[] {
  const out: DiffLineView[] = [];
  for (const raw of unifiedDiff.split(/\r?\n/)) {
    if (
      raw.startsWith("diff --git") ||
      raw.startsWith("index ") ||
      raw.startsWith("--- ") ||
      raw.startsWith("+++ ")
    ) {
      continue;
    }
    if (raw.startsWith("@@")) {
      out.push({ kind: "hunk_header", old_line: null, new_line: null, content: raw });
    } else if (raw.startsWith("+")) {
      out.push({ kind: "added", old_line: null, new_line: null, content: raw.slice(1) });
    } else if (raw.startsWith("-")) {
      out.push({ kind: "removed", old_line: null, new_line: null, content: raw.slice(1) });
    } else {
      out.push({
        kind: "context",
        old_line: null,
        new_line: null,
        content: raw.startsWith(" ") ? raw.slice(1) : raw,
      });
    }
  }
  return out;
}

/** Inline compact diff preview used when expanding an `Edited` / `Created` step inside a turn. */
function InlineStepDiffPreview({ lines }: { lines: DiffLineView[] }) {
  const capped = lines.slice(0, 80);
  return (
    <ScrollArea
      orientation="both"
      className="border-border bg-muted/45 dark:bg-muted/25 rounded-lg border"
      viewportClassName="selectable-text max-h-56 overscroll-contain py-1.5 font-mono text-[12px] leading-[1.55]"
    >
      {capped.map((ln, idx) => {
        if (ln.kind === "meta") return null;
        return (
          <div
            key={idx}
            className={cn(
              "flex min-w-fit px-3 whitespace-pre",
              ln.kind === "added" &&
                "bg-emerald-500/[0.10] text-emerald-900 dark:bg-emerald-500/[0.14] dark:text-emerald-200",
              ln.kind === "removed" &&
                "bg-rose-500/[0.10] text-rose-900 dark:bg-rose-500/[0.14] dark:text-rose-200",
              ln.kind === "hunk_header" && "text-muted-foreground/75 py-0.5 text-[11px] select-none",
              ln.kind === "context" && "text-foreground/80",
            )}
          >
            <span className="w-4 shrink-0 opacity-55 select-none">
              {ln.kind === "added" ? "+" : ln.kind === "removed" ? "-" : " "}
            </span>
            <span>{ln.content}</span>
          </div>
        );
      })}
      {lines.length > 80 && (
        <div className="text-muted-foreground px-3 pt-1 text-[11px] select-none">
          … {lines.length - 80} more diff lines (open in Diff Review for full file)
        </div>
      )}
    </ScrollArea>
  );
}

function resolveToolStepIcon(
  item: Extract<TimelineItemView, { item_type: "tool_call" }>,
  descriptorKind: ReturnType<typeof describeToolStep>["kind"],
): LucideIcon {
  switch (descriptorKind) {
    case "file_read":
      return FileText;
    case "file_create":
      return FilePlus2;
    case "file_edit":
      return FileCode2;
    case "search": {
      if (item.detail.kind === "search") {
        const sub = classifySearchTool(item.detail.query, item.summary_title);
        if (sub === "web") return Globe;
        if (sub === "glob") return FileSearch;
      }
      return Search;
    }
    case "shell":
      return TerminalIcon;
    case "subagent":
      return Layers;
    case "generic":
      return Wrench;
  }
}

/**
 * Borderless step row for reasoning, intermediate notes, permissions, and tool calls:
 * - Zero box/border clutter at rest; tight vertical grouping.
 * - Semantic muted icon swaps cleanly in-place to `ChevronRight` on hover/expand.
 * - Active thinking/tool step sweeps a calm linear-gradient text shimmer.
 */
const StepItemRow = memo(function StepItemRow({
  item,
  cwd,
  holdViewport,
  isActivelyThinking = false,
  thoughtDurationMs = null,
  autoExpandError = false,
}: {
  item: TimelineItemView;
  cwd: string;
  holdViewport: ViewportAnchorHoldFn;
  isActivelyThinking?: boolean;
  thoughtDurationMs?: number | null;
  autoExpandError?: boolean;
}) {
  const defaultOpen =
    autoExpandError &&
    item.item_type === "tool_call" &&
    (item.status === "failed" || item.status === "denied");
  const [expanded, setExpanded] = useState(defaultOpen);
  const [hovered, setHovered] = useState(false);
  const triggerRef = useRef<HTMLButtonElement | null>(null);

  if (item.item_type === "assistant_thinking") {
    const trimmedThinking = item.text.trim();
    const headline = extractThinkingHeadline(trimmedThinking, isActivelyThinking);
    const canExpand = trimmedThinking.length > 0;
    const showChevron = canExpand && (hovered || expanded);
    const thoughtLabel = isActivelyThinking
      ? "Thinking"
      : formatThoughtSummaryLabel(thoughtDurationMs, 1);

    return (
      <div>
        <button
          ref={triggerRef}
          type="button"
          disabled={!canExpand}
          onMouseEnter={() => setHovered(true)}
          onMouseLeave={() => setHovered(false)}
          onClick={() => {
            if (!canExpand) return;
            holdViewport(triggerRef.current, () => setExpanded((v) => !v));
          }}
          className="group/step hover:bg-accent/55 -ml-1.5 flex w-full max-w-full cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-left text-[13px] transition-colors disabled:cursor-default"
        >
          <span className="text-muted-foreground group-hover/step:text-foreground/80 flex size-4 shrink-0 items-center justify-center">
            {showChevron ? (
              <ChevronRight
                className={cn(
                  "size-3.5 stroke-[1.9] transition-transform duration-150",
                  expanded && "rotate-90",
                )}
              />
            ) : (
              <Sparkles className="size-3.5 stroke-[1.75]" />
            )}
          </span>
          <span
            className={cn(
              "shrink-0 font-normal",
              isActivelyThinking
                ? "agent-text-shimmer"
                : "text-foreground/72 group-hover/step:text-foreground",
            )}
          >
            {thoughtLabel}
          </span>
          {!expanded && headline && (
            <span
              className={cn(
                "truncate text-[12.5px]",
                isActivelyThinking ? "agent-text-shimmer" : "text-muted-foreground/80",
              )}
            >
              {headline}
            </span>
          )}
        </button>

        {expanded && (
          <ScrollArea
            className="border-border my-1 ml-4 border-l"
            viewportClassName="max-h-56 overscroll-contain pl-3"
          >
            <pre className="selectable-text text-muted-foreground m-0 font-mono text-[12px] leading-relaxed whitespace-pre-wrap">
              {trimmedThinking}
            </pre>
          </ScrollArea>
        )}
      </div>
    );
  }

  if (item.item_type === "assistant_message") {
    const trimmed = item.text.trim();
    if (!trimmed) return null;
    const singleLinePreview = trimmed.replace(/\s+/g, " ");
    const isCompactSingleLine = !trimmed.includes("\n") && singleLinePreview.length <= 120;

    if (isCompactSingleLine) {
      return (
        <div className="text-muted-foreground flex items-start gap-2 py-1 text-[13px] leading-[1.5]">
          <span className="text-muted-foreground/75 mt-[2px] flex size-4 shrink-0 items-center justify-center">
            <MessageSquareText className="size-3.5 stroke-[1.75]" />
          </span>
          {item.blocks.length > 0 ? (
            <div className="min-w-0 flex-1">
              <MarkdownBlocksView blocks={item.blocks} streaming={item.streaming} compact />
            </div>
          ) : (
            <span className="selectable-text text-foreground/78">{trimmed}</span>
          )}
        </div>
      );
    }

    const showChevron = hovered || expanded;
    return (
      <div>
        <button
          ref={triggerRef}
          type="button"
          onMouseEnter={() => setHovered(true)}
          onMouseLeave={() => setHovered(false)}
          onClick={() => holdViewport(triggerRef.current, () => setExpanded((v) => !v))}
          className="group/note hover:bg-accent/55 -ml-1.5 flex w-full max-w-full cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-left text-[13px] transition-colors"
        >
          <span className="text-muted-foreground group-hover/note:text-foreground/80 flex size-4 shrink-0 items-center justify-center">
            {showChevron ? (
              <ChevronRight
                className={cn(
                  "size-3.5 stroke-[1.9] transition-transform duration-150",
                  expanded && "rotate-90",
                )}
              />
            ) : (
              <MessageSquareText className="size-3.5 stroke-[1.75]" />
            )}
          </span>
          <span className="text-foreground/72 group-hover/note:text-foreground truncate">
            {singleLinePreview}
          </span>
        </button>

        {expanded && (
          <div className="border-border my-1 ml-4 border-l pl-3 text-[13.5px]">
            <MarkdownBlocksView blocks={item.blocks} streaming={item.streaming} />
          </div>
        )}
      </div>
    );
  }

  if (item.item_type === "permission_request") {
    const denied = /deny|reject|decline|cancel/i.test(item.resolved_decision ?? "");
    return (
      <div className="text-muted-foreground flex items-center gap-2 py-0.5 text-[12.5px]">
        <span className="flex size-4 shrink-0 items-center justify-center">
          {denied ? (
            <AlertTriangle className="size-3.5 text-rose-500" />
          ) : (
            <Check className="size-3.5 text-emerald-600 dark:text-emerald-400" />
          )}
        </span>
        <span className="truncate">
          {item.title} ·{" "}
          <span className="text-foreground/75">
            {(item.resolved_decision ?? "resolved").replace(/_/g, " ")}
          </span>
        </span>
      </div>
    );
  }

  if (item.item_type === "question_request") {
    return (
      <div className="text-muted-foreground flex items-center gap-2 py-0.5 text-[12.5px]">
        <span className="flex size-4 shrink-0 items-center justify-center">
          <Check className="size-3.5 text-sky-600 dark:text-sky-400" />
        </span>
        <span className="truncate">
          Answered {item.questions.length} clarifying{" "}
          {item.questions.length === 1 ? "question" : "questions"}
        </span>
      </div>
    );
  }

  if (item.item_type !== "tool_call") {
    return null;
  }

  const desc = describeToolStep(item, cwd);
  const isError = desc.isFailed || desc.isDenied;
  const IconComp = resolveToolStepIcon(item, desc.kind);

  const diffLines =
    item.diff_files[0]?.lines && item.diff_files[0].lines.length > 0
      ? item.diff_files[0].lines
      : item.detail.kind === "file_edit" && item.detail.unified_diff
        ? parseUnifiedDiffFallback(item.detail.unified_diff)
        : [];

  const hasExpandableDetail =
    desc.hasExpandableDetail &&
    (item.detail.kind !== "file_edit" || diffLines.length > 0);
  const showChevron = hasExpandableDetail && (hovered || expanded);
  const openFilePath = desc.openPath;
  const canOpenFileInDiff =
    Boolean(openFilePath) && (desc.kind === "file_edit" || desc.kind === "file_create");
  const canOpenFileDirect = Boolean(openFilePath) && desc.kind === "file_read";
  const isSubagentStep = item.detail.kind === "subagent";

  return (
    <div>
      <div
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className={cn(
          "group/tool -ml-1.5 flex items-center justify-between gap-2 rounded-md px-1.5 py-1 text-[13px] transition-colors",
          (hasExpandableDetail || canOpenFileInDiff || canOpenFileDirect || isSubagentStep) &&
            "hover:bg-accent/55",
        )}
      >
        <button
          ref={triggerRef}
          type="button"
          disabled={!hasExpandableDetail}
          onClick={() => {
            if (!hasExpandableDetail) return;
            holdViewport(triggerRef.current, () => setExpanded((v) => !v));
          }}
          className={cn(
            "flex min-w-0 flex-1 items-center gap-2 text-left",
            hasExpandableDetail ? "cursor-pointer" : "cursor-default",
          )}
        >
          <span
            className={cn(
              "flex size-4 shrink-0 items-center justify-center",
              isError
                ? "text-rose-500"
                : "text-muted-foreground group-hover/tool:text-foreground/80",
            )}
          >
            {showChevron ? (
              <ChevronRight
                className={cn(
                  "size-3.5 stroke-[1.9] transition-transform duration-150",
                  expanded && "rotate-90",
                )}
              />
            ) : isError ? (
              <AlertTriangle className="size-3.5 stroke-[1.85]" />
            ) : (
              <IconComp className="size-3.5 stroke-[1.75]" />
            )}
          </span>

          <span
            className={cn(
              "shrink-0 font-normal",
              isError
                ? "text-rose-600 dark:text-rose-400"
                : desc.isRunning
                  ? "agent-text-shimmer"
                  : "text-foreground/75 group-hover/tool:text-foreground",
            )}
          >
            {desc.verb}
          </span>

          {desc.target && (
            <span
              onClick={
                canOpenFileInDiff && !hasExpandableDetail && openFilePath
                  ? (e) => {
                      e.stopPropagation();
                      orbitActions.openInspectorForFile(openFilePath, "session_turns");
                    }
                  : canOpenFileDirect && openFilePath
                    ? (e) => {
                        e.stopPropagation();
                        orbitActions.openFileInSidePanel(openFilePath, {
                          focusLine:
                            item.detail.kind === "file_read" ? item.detail.start_line : null,
                        });
                      }
                    : undefined
              }
              className={cn(
                "truncate font-mono text-[12px]",
                desc.isRunning
                  ? "agent-text-shimmer"
                  : "text-muted-foreground group-hover/tool:text-foreground/70",
                ((canOpenFileInDiff && !hasExpandableDetail) || canOpenFileDirect) &&
                  "cursor-pointer hover:underline",
              )}
            >
              {desc.target}
            </span>
          )}

          {desc.secondaryMeta && (
            <span className="text-muted-foreground/70 shrink-0 font-mono text-[11.5px]">
              {desc.secondaryMeta}
            </span>
          )}
        </button>

        <div className="tabular-nums flex shrink-0 items-center gap-2 font-mono text-[11.5px]">
          {(desc.additions > 0 || desc.deletions > 0) && (
            <span className="flex items-center gap-1.5">
              {desc.additions > 0 && (
                <span className="text-emerald-600 dark:text-emerald-400">
                  +{desc.additions}
                </span>
              )}
              {desc.deletions > 0 && (
                <span className="text-rose-600 dark:text-rose-400">-{desc.deletions}</span>
              )}
            </span>
          )}

          {desc.kind === "shell" &&
            !desc.isRunning &&
            desc.exitCode !== null &&
            desc.exitCode !== 0 && (
              <span className="text-rose-500">exit {desc.exitCode}</span>
            )}

          {canOpenFileInDiff && openFilePath && (
            <button
              type="button"
              title="Open in Diff Review"
              onClick={(e) => {
                e.stopPropagation();
                orbitActions.openInspectorForFile(openFilePath, "session_turns");
              }}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded p-0.5 opacity-0 transition-opacity group-hover/tool:opacity-100"
            >
              <FileSymlink className="size-3.5" />
            </button>
          )}

          {canOpenFileDirect && openFilePath && (
            <button
              type="button"
              title="Open File in Side Panel"
              onClick={(e) => {
                e.stopPropagation();
                orbitActions.openFileInSidePanel(openFilePath, {
                  focusLine: item.detail.kind === "file_read" ? item.detail.start_line : null,
                });
              }}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded p-0.5 opacity-0 transition-opacity group-hover/tool:opacity-100"
            >
              <FileSymlink className="size-3.5" />
            </button>
          )}

          {item.detail.kind === "subagent" && (
            <button
              type="button"
              title="Inspect Subagent in Side Panel"
              onClick={(e) => {
                e.stopPropagation();
                if (item.detail.kind !== "subagent") return;
                orbitActions.openSubagentTab({
                  callId: item.call_id,
                  role: item.detail.role || "Subagent",
                  promptPreview: item.detail.prompt_preview || "",
                  childAgentId: item.detail.child_agent_id,
                });
              }}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded p-0.5 opacity-0 transition-opacity group-hover/tool:opacity-100"
            >
              <FileSymlink className="size-3.5" />
            </button>
          )}
        </div>
      </div>

      {expanded && hasExpandableDetail && (
        <div className="orbit-stage-enter my-1.5 ml-4 pl-3">
          {item.detail.kind === "shell" && (
            <ScrollArea
              className={cn(
                "rounded-lg border",
                isError
                  ? "border-rose-500/30 bg-rose-500/[0.04]"
                  : "border-border bg-muted/45 dark:bg-muted/25",
              )}
              viewportClassName="max-h-56 overscroll-contain px-3 py-2"
            >
              <pre
                className={cn(
                  "selectable-text font-mono text-[12px] leading-relaxed whitespace-pre-wrap",
                  isError ? "text-foreground/90" : "text-foreground/88",
                )}
              >
                <span className="text-muted-foreground select-none">$ </span>
                {item.detail.command}
                {item.detail.output_preview ? (
                  `\n\n${item.detail.output_preview}`
                ) : !desc.isRunning ? (
                  <span className="text-muted-foreground select-none">{"\n\n(no output)"}</span>
                ) : (
                  ""
                )}
              </pre>
            </ScrollArea>
          )}

          {item.detail.kind === "file_edit" && diffLines.length > 0 && (
            <InlineStepDiffPreview lines={diffLines} />
          )}

          {item.detail.kind === "subagent" && item.detail.prompt_preview && (
            <ScrollArea
              className="border-border bg-muted/45 dark:bg-muted/25 rounded-lg border"
              viewportClassName="max-h-44 overscroll-contain px-3 py-2"
            >
              <pre className="selectable-text text-muted-foreground font-mono text-[12px] whitespace-pre-wrap">
                {item.detail.prompt_preview}
              </pre>
            </ScrollArea>
          )}

          {item.detail.kind === "generic" && item.detail.arguments_json && (
            <ScrollArea
              className="border-border bg-muted/45 dark:bg-muted/25 rounded-lg border"
              viewportClassName="max-h-44 overscroll-contain px-3 py-2"
            >
              <pre className="selectable-text text-muted-foreground font-mono text-[12px] whitespace-pre-wrap">
                {formatJsonPreview(item.detail.arguments_json)}
              </pre>
            </ScrollArea>
          )}
        </div>
      )}
    </div>
  );
});

/** Vertical step connector rail for expanded turn activity steps. */
const StepTimelineRail = memo(function StepTimelineRail({
  items,
  cwd,
  holdViewport,
  indented = false,
  isLiveActive = false,
  autoExpandErrors = false,
  agentId,
  thinkingSignature: _thinkingSignature,
  getThinkingDurationMs,
}: {
  items: TimelineItemView[];
  cwd: string;
  holdViewport: ViewportAnchorHoldFn;
  indented?: boolean;
  isLiveActive?: boolean;
  autoExpandErrors?: boolean;
  agentId: string;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
}) {
  if (items.length === 0) return null;

  const lastSeq = items[items.length - 1]?.seq;

  return (
    <div
      className={cn(
        "space-y-0.5",
        indented && "border-border mt-1 ml-[7px] border-l pl-3.5",
      )}
    >
      {items.map((sub, i) => {
        const isLastInLiveGroup = isLiveActive && sub.seq === lastSeq;
        const thinkingKey =
          sub.item_type === "assistant_thinking"
            ? `${agentId}::think::${sub.item_id || sub.seq}`
            : null;
        const thoughtDurationMs = thinkingKey ? getThinkingDurationMs(thinkingKey) : null;

        return (
          <StepItemRow
            key={`${sub.item_type}-${sub.seq}-${i}`}
            item={sub}
            cwd={cwd}
            holdViewport={holdViewport}
            isActivelyThinking={
              sub.item_type === "assistant_thinking" && isLastInLiveGroup
            }
            thoughtDurationMs={thoughtDurationMs}
            autoExpandError={autoExpandErrors}
          />
        );
      })}
    </div>
  );
});

/**
 * Unified All-or-Nothing Turn Activity Disclosure:
 * - Folds ALL intermediate steps (thinking, tool calls, resolved gates, interim narration)
 *   behind one clickable summary header (`Reading stream-stage.tsx… · 4 steps · 14s ›` ->
 *   `Read 3 files, edited stream-stage.tsx · 14s ›`).
 * - Strictly respects manual user toggle during and after streaming.
 */
const TurnActivityDisclosure = memo(function TurnActivityDisclosure({
  items,
  isWorkActive,
  liveLabel,
  liveDetail,
  settledLabel,
  summaryCounts,
  labelIncludesFailure,
  defaultCollapsed,
  autoOpenForError,
  liveStartedAtMs,
  latestActivityMs,
  isNetworkOffline = false,
  settledDurationMs,
  cwd,
  agentId,
  thinkingSignature,
  getThinkingDurationMs,
  holdViewport,
}: {
  items: TimelineItemView[];
  isWorkActive: boolean;
  liveLabel: string;
  liveDetail: string | null;
  settledLabel: string;
  summaryCounts: WorkSummaryResult["counts"];
  labelIncludesFailure: boolean;
  defaultCollapsed: boolean;
  autoOpenForError: boolean;
  liveStartedAtMs: number | null;
  latestActivityMs?: number | null;
  isNetworkOffline?: boolean;
  settledDurationMs: number | null;
  cwd: string;
  agentId: string;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
}) {
  const [userExpanded, setUserExpanded] = useState<boolean | null>(null);
  const foldBtnRef = useRef<HTMLButtonElement | null>(null);
  const fallbackStartRef = useRef<number>(Date.now());
  const effectiveLiveStartedAtMs = liveStartedAtMs ?? fallbackStartRef.current;

  if (items.length === 0) {
    if (!isWorkActive) return null;
    return (
      <div className="text-muted-foreground flex max-w-full items-center gap-1.5 py-0.5 text-[13.5px] font-normal">
        <span className="agent-text-shimmer truncate">{liveLabel || "Working…"}</span>
        <InlineLiveElapsed
          startedAtMs={effectiveLiveStartedAtMs}
          lastActivityMs={latestActivityMs}
          isNetworkOffline={isNetworkOffline}
        />
      </div>
    );
  }

  const defaultOpen = !defaultCollapsed || autoOpenForError;
  const isExpanded = userExpanded !== null ? userExpanded : defaultOpen;
  const showFailureBadge =
    !isWorkActive && summaryCounts.unrecoveredFailureCount > 0 && !labelIncludesFailure;
  const showDiffNumbers =
    isWorkActive && (summaryCounts.additions > 0 || summaryCounts.deletions > 0);
  const showSettledDuration =
    !isWorkActive &&
    settledDurationMs !== null &&
    settledDurationMs >= 1000 &&
    !settledLabel.startsWith("Thought for ");

  return (
    <div>
      <button
        ref={foldBtnRef}
        type="button"
        aria-expanded={isExpanded}
        onClick={() =>
          holdViewport(foldBtnRef.current, () =>
            setUserExpanded((prev) => !(prev !== null ? prev : defaultOpen)),
          )
        }
        className="group/fold text-muted-foreground hover:text-foreground hover:bg-accent/55 -ml-1.5 inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[13.5px] font-normal transition-colors"
      >
        {isWorkActive ? (
          <>
            <span className={cn("agent-text-shimmer", liveDetail ? "shrink-0" : "truncate")}>
              {liveLabel}
            </span>
            {liveDetail && (
              <span className="agent-text-shimmer truncate text-[13px] opacity-85">
                {liveDetail}
              </span>
            )}
            {items.length >= 2 && (
              <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[12.5px]">
                · {items.length} steps
              </span>
            )}
            <InlineLiveElapsed
              startedAtMs={effectiveLiveStartedAtMs}
              lastActivityMs={latestActivityMs}
              isNetworkOffline={isNetworkOffline}
            />
            {showDiffNumbers && (
              <span className="tabular-nums flex shrink-0 items-center gap-1 font-mono text-[11.5px]">
                {summaryCounts.additions > 0 && (
                  <span className="text-emerald-600 dark:text-emerald-400">
                    +{summaryCounts.additions}
                  </span>
                )}
                {summaryCounts.deletions > 0 && (
                  <span className="text-rose-600 dark:text-rose-400">
                    -{summaryCounts.deletions}
                  </span>
                )}
              </span>
            )}
          </>
        ) : (
          <>
            <span
              className={cn(
                "truncate",
                labelIncludesFailure && "text-rose-600 dark:text-rose-400",
              )}
            >
              {settledLabel}
            </span>
            {showFailureBadge && (
              <span className="shrink-0 text-[12.5px] text-rose-600 dark:text-rose-400">
                · {summaryCounts.unrecoveredFailureCount} failed
              </span>
            )}
            {showSettledDuration && (
              <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[12.5px]">
                · {formatDurationCompact(settledDurationMs)}
              </span>
            )}
          </>
        )}

        <ChevronRight
          className={cn(
            "size-3.5 shrink-0 opacity-70 transition-transform duration-150 group-hover/fold:opacity-100",
            isExpanded && "rotate-90",
          )}
        />
      </button>

      {isExpanded && (
        <div className="orbit-stage-enter">
          <StepTimelineRail
            items={items}
            cwd={cwd}
            holdViewport={holdViewport}
            indented
            isLiveActive={isWorkActive}
            autoExpandErrors={
              !isWorkActive && (summaryCounts.failedCount > 0 || summaryCounts.deniedCount > 0)
            }
            agentId={agentId}
            thinkingSignature={thinkingSignature}
            getThinkingDurationMs={getThinkingDurationMs}
          />
        </div>
      )}
    </div>
  );
});

const LEGACY_PLAN_PREFIX =
  "Analyze the relevant codebase and propose a concise step-by-step plan before making edits:";
const LEGACY_REVIEW_PROMPT =
  "Review the uncommitted working tree changes for bugs, edge cases, and architectural consistency.";

function resolvePromptBadgeIcon(badge: PromptBadge): LucideIcon {
  switch (badge.kind) {
    case "slash":
      if (badge.label === "/plan") return ListTodo;
      if (badge.label === "/review") return FileSearch;
      if (badge.label === "/compact") return FoldVertical;
      return Sparkles;
    case "file":
      return resolveSemanticFileIcon(badge.label);
    case "git_diff":
      return /staged/i.test(badge.label) ? GitCompareArrows : FileDiff;
    case "terminal":
      return SquareTerminal;
    case "image":
      return FileImage;
    case "quote":
      return Quote;
    case "paste": {
      const cleanName = badge.label.split("·")[0]?.trim() ?? "";
      return cleanName.includes(".") ? resolveSemanticFileIcon(cleanName) : FileText;
    }
    default:
      return FileText;
  }
}

interface UserPromptPresentation {
  cleanText: string;
  badges: PromptBadge[];
  sanitizedLegacy: boolean;
}

const promptPresentationCache = new WeakMap<
  Extract<TimelineItemView, { item_type: "user_prompt" }>,
  UserPromptPresentation
>();

/**
 * Extracts clean user-visible text and semantic chips from `user_prompt`,
 * including backwards-compatible cleanup for older turns stored before structured `badges`.
 * Memoized by `WeakMap` so `UserPromptBubble` and `outlinePrompts` run regexes at most once per item.
 */
function extractUserPromptPresentation(
  item: Extract<TimelineItemView, { item_type: "user_prompt" }>,
): UserPromptPresentation {
  const cached = promptPresentationCache.get(item);
  if (cached) return cached;

  if (item.badges && item.badges.length > 0) {
    const res: UserPromptPresentation = {
      cleanText: item.text.trim(),
      badges: item.badges,
      sanitizedLegacy: false,
    };
    promptPresentationCache.set(item, res);
    return res;
  }

  let working = item.text;
  const inferredBadges: PromptBadge[] = [];
  let sanitized = false;

  working = working.replace(/^Attached image file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "image", label: name });
    return "";
  });

  working = working.replace(/^Attached file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "file", label: name });
    return "";
  });

  working = working.replace(/^Referenced workspace files:\s*(.+)$/gm, (_, listStr: string) => {
    sanitized = true;
    const matches = Array.from(listStr.matchAll(/`@?([^`]+)`/g));
    for (const m of matches) {
      const name = m[1].split("/").pop() || m[1];
      inferredBadges.push({ kind: "file", label: name });
    }
    return "";
  });

  working = working.replace(
    /^>\s*(Working Tree Diff|Staged Diff):\n```diff\n[\s\S]*?\n```\s*$/gm,
    (_, label: string) => {
      sanitized = true;
      inferredBadges.push({ kind: "git_diff", label });
      return "";
    },
  );
  working = working.replace(
    /^>\s*Context:\s*Inspect (staged|working tree) git changes\.\s*$/gim,
    (_, kind: string) => {
      sanitized = true;
      inferredBadges.push({
        kind: "git_diff",
        label: kind.toLowerCase() === "staged" ? "Staged Diff" : "Working Tree Diff",
      });
      return "";
    },
  );
  working = working.replace(
    /^>\s*([^:\n]*lines):\n```text\n[\s\S]*?\n```\s*$/gm,
    (_, label: string) => {
      sanitized = true;
      inferredBadges.push({ kind: "terminal", label: label.trim() });
      return "";
    },
  );
  working = working.replace(
    /^>\s*Reference\s*`([^`]+)`:\n```[\s\S]*?\n```\s*$/gm,
    (_, loc: string) => {
      sanitized = true;
      const shortLoc = loc.split("/").pop() || loc;
      inferredBadges.push({ kind: "quote", label: shortLoc });
      return "";
    },
  );

  working = working.trim();

  if (working.startsWith(LEGACY_PLAN_PREFIX)) {
    sanitized = true;
    inferredBadges.unshift({ kind: "slash", label: "/plan" });
    working = working.slice(LEGACY_PLAN_PREFIX.length).trim();
  } else if (working === LEGACY_REVIEW_PROMPT) {
    sanitized = true;
    inferredBadges.unshift({ kind: "slash", label: "/review" });
    working = "";
  }

  const res: UserPromptPresentation = {
    cleanText: working,
    badges: inferredBadges,
    sanitizedLegacy: sanitized,
  };
  promptPresentationCache.set(item, res);
  return res;
}

/**
 * Orbit Native User Prompt Bubble:
 * - Crisp cool-neutral Orbit surface aligned flush right.
 * - Renders semantic context & slash workflow tokens (`/plan`, `/review`, `@file`, `Diff`)
 *   without exposing hidden wire scaffolding.
 * - Long messages preview smoothly with a bottom fade gradient; click anywhere on the bubble to
 *   expand, and click outside while expanded to collapse back.
 * - Quiet bottom-right hover toolbar (`timestamp` + `Copy` + `Undo to this point`) with zero layout shift.
 */
const UserPromptBubble = memo(function UserPromptBubble({
  item,
  holdViewport,
  animateEntrance = false,
  onRequestRewind,
}: {
  item: Extract<TimelineItemView, { item_type: "user_prompt" }>;
  holdViewport: ViewportAnchorHoldFn;
  animateEntrance?: boolean;
  onRequestRewind?: (seq: number, promptText: string) => void;
}) {
  const [expanded, setExpanded] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const [copied, setCopied] = useState(false);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const bubbleRef = useRef<HTMLDivElement | null>(null);
  const bodyRef = useRef<HTMLDivElement | null>(null);

  const { cleanText, badges, sanitizedLegacy } = useMemo(
    () => extractUserPromptPresentation(item),
    [item],
  );

  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    const measure = () => {
      const nextOverflow = el.scrollHeight > USER_PROMPT_PREVIEW_MAX_PX + 10;
      setOverflowing((prev) => (prev === nextOverflow ? prev : nextOverflow));
    };
    measure();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(measure);
    ro.observe(el);
    return () => ro.disconnect();
  }, [cleanText, badges.length]);

  useEffect(() => {
    if (!expanded) return;
    const handleClickOutside = (event: MouseEvent) => {
      if (!(event.target instanceof Element)) return;
      if (containerRef.current?.contains(event.target)) return;
      if (event.target.closest('[data-slot="dialog-content"], .orbit-dialog-overlay')) {
        return;
      }
      const sel = window.getSelection();
      if (
        sel &&
        !sel.isCollapsed &&
        sel.toString().trim().length > 0 &&
        bubbleRef.current &&
        sel.anchorNode &&
        bubbleRef.current.contains(sel.anchorNode)
      ) {
        return;
      }
      holdViewport(bubbleRef.current, () => setExpanded(false), "top");
    };
    document.addEventListener("click", handleClickOutside);
    return () => {
      document.removeEventListener("click", handleClickOutside);
    };
  }, [expanded, holdViewport]);

  const copyPayload = cleanText || badges.map((b) => b.label).join(" ");

  const handleCopy = async () => {
    await navigator.clipboard.writeText(copyPayload);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
  };

  const handleBubbleClick = (event: React.MouseEvent<HTMLDivElement>) => {
    if (!overflowing || expanded) return;
    const target = event.target as HTMLElement | null;
    if (target?.closest("a, button")) return;
    const sel = window.getSelection();
    if (sel && !sel.isCollapsed && sel.toString().trim().length > 0) return;
    holdViewport(bubbleRef.current, () => setExpanded(true), "top");
  };

  const slashBadge = useMemo(
    () => badges.find((b) => b.kind === "slash") ?? null,
    [badges],
  );
  const attachmentBadges = useMemo(
    () => badges.filter((b) => b.kind !== "slash"),
    [badges],
  );
  const SlashIcon = slashBadge ? resolvePromptBadgeIcon(slashBadge) : null;
  const slashLabel = slashBadge ? slashBadge.label.replace(/^\//, "") : "";
  const promptTimestampMs =
    typeof item.timestamp_ms === "number" &&
    Number.isFinite(item.timestamp_ms) &&
    item.timestamp_ms > 0
      ? item.timestamp_ms
      : null;

  return (
    <div
      ref={containerRef}
      className={cn(
        "group/user flex flex-col items-end",
        animateEntrance && "orbit-bubble-enter",
      )}
    >
      <div
        ref={bubbleRef}
        onClick={handleBubbleClick}
        className={cn(
          "prompt-bubble-surface text-foreground relative min-w-0 max-w-[80%] overflow-hidden rounded-[20px] px-4 py-2.5",
          overflowing && !expanded && "cursor-pointer",
        )}
      >
        <div
          ref={bodyRef}
          style={
            !expanded && overflowing
              ? { maxHeight: `${USER_PROMPT_PREVIEW_MAX_PX}px`, overflow: "hidden" }
              : undefined
          }
          className="relative space-y-1.5"
        >
          {attachmentBadges.length > 0 && (
            <div className="flex flex-wrap items-center gap-1.5 select-none">
              {attachmentBadges.map((badge, idx) => {
                const BadgeIcon = resolvePromptBadgeIcon(badge);
                const isMono = badge.kind === "file" || badge.kind === "quote";
                return (
                  <span
                    key={`${badge.kind}-${badge.label}-${idx}`}
                    className="text-foreground/85 inline-flex items-center gap-1.5 rounded-lg border border-black/[0.075] bg-black/[0.03] px-2 py-0.5 text-[12px] dark:border-white/[0.10] dark:bg-white/[0.055]"
                  >
                    <BadgeIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
                    <span className={cn("leading-normal font-medium", isMono && "font-mono")}>
                      {badge.label}
                    </span>
                  </span>
                );
              })}
            </div>
          )}

          {slashBadge && SlashIcon ? (
            <p className="selectable-text text-foreground text-[14.5px] leading-[1.64] whitespace-pre-wrap">
              <span className="mr-1.5 inline-flex items-center gap-1.5 align-baseline select-none">
                <SlashIcon className="text-muted-foreground relative top-[2px] size-[14px] shrink-0 stroke-[1.75]" />
                <span className="text-foreground font-medium">{slashLabel}</span>
              </span>
              {cleanText}
            </p>
          ) : (
            cleanText.length > 0 &&
            (sanitizedLegacy ? (
              <p className="selectable-text text-foreground text-[14.5px] leading-[1.64] whitespace-pre-wrap">
                {cleanText}
              </p>
            ) : (
              <MarkdownBlocksView blocks={item.blocks} bubble />
            ))
          )}
        </div>

        {!expanded && overflowing && (
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-x-0 bottom-0 h-14 rounded-b-[19px]"
            style={{
              background:
                "linear-gradient(to top, var(--surface-elevated) 14%, color-mix(in oklab, var(--surface-elevated) 82%, transparent) 52%, transparent 100%)",
            }}
          />
        )}
      </div>

      <div className="mt-1 -mb-1.5 flex h-[22px] shrink-0 items-center justify-end gap-1 pr-1 whitespace-nowrap opacity-0 transition-opacity duration-150 group-focus-within/user:opacity-100 group-hover/user:opacity-100">
        {promptTimestampMs !== null && (
          <RelativeTimestampPill timestampMs={promptTimestampMs} />
        )}
        <CopyActionButton
          copied={copied}
          label="Copy"
          onClick={() => void handleCopy()}
        />
        {onRequestRewind && item.seq > 0 && (
          <UndoActionButton
            label="Undo to this point"
            onClick={() => onRequestRewind(item.seq, copyPayload)}
          />
        )}
      </div>
    </div>
  );
});

/** Turn-end aggregated diff summary pill (`N files changed · +A -D · Review ▸`). */
const TurnChangesBar = memo(function TurnChangesBar({
  diffs,
  cwd,
}: {
  diffs: TurnFileDiffSummary[];
  cwd: string;
}) {
  if (diffs.length === 0) return null;
  const totalAdd = diffs.reduce((acc, d) => acc + d.additions, 0);
  const totalDel = diffs.reduce((acc, d) => acc + d.deletions, 0);
  const primaryPath = diffs[0]?.path ?? null;

  const label =
    diffs.length === 1 && primaryPath
      ? `Edited ${stripWorkspacePrefix(primaryPath, cwd)}`
      : `${diffs.length} files changed`;

  return (
    <div className="pt-1.5">
      <button
        type="button"
        onClick={() => orbitActions.openInspectorForFile(primaryPath, "session_turns")}
        className="border-border bg-elevated hover:bg-accent/65 inline-flex max-w-full cursor-pointer items-center gap-2.5 rounded-full border px-3 py-1 text-[12.5px] transition-colors"
      >
        <FileCode2 className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
        <span className="text-foreground/88 truncate font-medium">{label}</span>
        {(totalAdd > 0 || totalDel > 0) && (
          <span className="tabular-nums flex shrink-0 items-center gap-1.5 font-mono text-[11.5px]">
            {totalAdd > 0 && (
              <span className="text-emerald-600 dark:text-emerald-400">+{totalAdd}</span>
            )}
            {totalDel > 0 && <span className="text-rose-600 dark:text-rose-400">-{totalDel}</span>}
          </span>
        )}
        <span className="text-muted-foreground shrink-0 text-[12px]">Review ▸</span>
      </button>
    </div>
  );
});

/** Shared pill-styled Copy icon button (`13px` icon in `22×22px` hit target, balanced with `14.5px` prose). */
function CopyActionButton({
  copied,
  label,
  onClick,
  className,
}: {
  copied: boolean;
  label: string;
  onClick: () => void;
  className?: string;
}) {
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger
        delay={240}
        render={
          <button
            type="button"
            aria-label={copied ? "Copied" : label}
            onClick={onClick}
            className={cn(
              "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
              className,
            )}
          />
        }
      >
        <Copy
          className={cn(
            "size-[13px] stroke-[1.8] transition-all duration-150 ease-[var(--ease-out-expo)]",
            copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100",
          )}
        />
        <Check
          className={cn(
            "absolute size-[13px] stroke-[2] text-emerald-600 transition-all duration-150 ease-[var(--ease-out-expo)] dark:text-emerald-400",
            copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0",
          )}
        />
      </BaseTooltip.Trigger>
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner side="top" sideOffset={6} className="pointer-events-none z-50">
          <BaseTooltip.Popup className="bg-elevated text-foreground orbit-tooltip-content rounded-full border border-black/[0.08] px-2.5 py-0.5 text-[11.5px] leading-normal font-normal shadow-[0_4px_14px_rgba(0,0,0,0.08)] select-none dark:border-white/[0.11] dark:shadow-[0_4px_16px_rgba(0,0,0,0.36)]">
            {copied ? "Copied" : label}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

/** Semantic Undo / Rewind icon button matching `CopyActionButton` (`13px` icon in `22×22px` hit target). */
function UndoActionButton({
  label,
  onClick,
  className,
}: {
  label: string;
  onClick: () => void;
  className?: string;
}) {
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger
        delay={240}
        render={
          <button
            type="button"
            aria-label={label}
            onClick={onClick}
            className={cn(
              "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
              className,
            )}
          />
        }
      >
        <Undo2 className="size-[13px] stroke-[1.8]" />
      </BaseTooltip.Trigger>
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner side="top" sideOffset={6} className="pointer-events-none z-50">
          <BaseTooltip.Popup className="bg-elevated text-foreground orbit-tooltip-content rounded-full border border-black/[0.08] px-2.5 py-0.5 text-[11.5px] leading-normal font-normal shadow-[0_4px_14px_rgba(0,0,0,0.08)] select-none dark:border-white/[0.11] dark:shadow-[0_4px_16px_rgba(0,0,0,0.36)]">
            {label}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}

interface RewindFileEntry {
  displayPath: string;
  basename: string;
  additions: number;
  deletions: number;
}

interface RewindRequestTarget {
  seq: number;
  promptText: string;
  affectedTurnsCount: number;
  editedFiles: RewindFileEntry[];
}

function isRevertibleWorkspaceFile(rawPath: string, cwd: string): boolean {
  const trimmed = rawPath.trim();
  if (!trimmed || trimmed === "Working Tree" || trimmed === "Staged Diff") {
    return false;
  }
  const cleanCwd = cwd.replace(/\/+$/, "");
  if (trimmed.startsWith("/") && cleanCwd && cleanCwd !== "~") {
    return trimmed === cleanCwd || trimmed.startsWith(`${cleanCwd}/`);
  }
  return true;
}

function RewindTurnDialog({
  target,
  isAgentRunning,
  submitting,
  onOpenChange,
  onConfirm,
}: {
  target: RewindRequestTarget | null;
  isAgentRunning: boolean;
  submitting: boolean;
  onOpenChange: (open: boolean) => void;
  onConfirm: (restoreFiles: boolean) => void;
}) {
  const [restoreFiles, setRestoreFiles] = useState(true);
  const files = target?.editedFiles ?? [];
  const hasFiles = files.length > 0;

  useEffect(() => {
    if (target) {
      setRestoreFiles(target.editedFiles.length > 0);
    }
  }, [target]);

  useEffect(() => {
    if (!target || submitting) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key !== "Enter" || e.metaKey || e.ctrlKey || e.altKey || e.shiftKey) return;
      const activeTag = (document.activeElement as HTMLElement | null)?.tagName;
      if (activeTag === "BUTTON" || activeTag === "INPUT") return;
      e.preventDefault();
      onConfirm(hasFiles ? restoreFiles : false);
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [target, submitting, hasFiles, restoreFiles, onConfirm]);

  const basenameCounts = useMemo(() => {
    const counts = new Map<string, number>();
    for (const f of files) {
      counts.set(f.basename, (counts.get(f.basename) ?? 0) + 1);
    }
    return counts;
  }, [files]);

  const effectiveRestore = hasFiles && restoreFiles;

  return (
    <Dialog open={target !== null} onOpenChange={(open) => !submitting && onOpenChange(open)}>
      <DialogContent className="max-w-[440px] gap-3.5 p-5">
        <DialogHeader className="gap-1.5 pr-6">
          <DialogTitle className="text-[15.5px] font-semibold">Confirm Undo</DialogTitle>
          <DialogDescription className="text-[13px] leading-snug">
            {hasFiles
              ? "Confirming this undo action will make the following changes:"
              : `Confirming this undo action will rewind ${
                  target && target.affectedTurnsCount > 1
                    ? `${target.affectedTurnsCount} turns`
                    : "this turn"
                } and restore the prompt to the composer.`}
          </DialogDescription>
        </DialogHeader>

        {hasFiles && (
          <div className="min-w-0 space-y-2.5">
            <label className="inline-flex cursor-pointer items-center gap-2.5 select-none">
              <button
                type="button"
                role="checkbox"
                aria-checked={restoreFiles}
                disabled={submitting}
                onClick={() => setRestoreFiles((v) => !v)}
                className={cn(
                  "flex size-4 shrink-0 cursor-pointer items-center justify-center rounded-[4px] border transition-colors disabled:opacity-60",
                  restoreFiles
                    ? "border-primary bg-primary text-primary-foreground"
                    : "border-muted-foreground/45 bg-background hover:border-foreground/50",
                )}
              >
                {restoreFiles && <Check className="size-3 stroke-[2.5]" />}
              </button>
              <span className="text-foreground text-[13.5px] font-medium">
                Revert file changes
              </span>
            </label>

            <div className="relative min-w-0">
              <div
                className={cn(
                  "max-h-[216px] space-y-1.5 overflow-y-auto pr-1 transition-opacity duration-150",
                  !restoreFiles && "opacity-45",
                )}
              >
                {files.map((file) => {
                  const FileIcon = resolveSemanticFileIcon(file.displayPath);
                  const showRelPath = (basenameCounts.get(file.basename) ?? 0) > 1;
                  return (
                    <div
                      key={file.displayPath}
                      title={file.displayPath}
                      className="flex min-w-0 items-center gap-2 py-0.5 text-[13px]"
                    >
                      <FileIcon className="text-muted-foreground/85 size-3.5 shrink-0 stroke-[1.75]" />
                      <span
                        className={cn(
                          "text-foreground/90 min-w-0 truncate",
                          !restoreFiles && "line-through",
                        )}
                      >
                        {showRelPath ? file.displayPath : file.basename}
                      </span>
                      {(file.additions > 0 || file.deletions > 0) && (
                        <span className="tabular-nums flex shrink-0 items-center gap-1.5 font-mono text-[12px]">
                          <span className="text-emerald-600 dark:text-emerald-400">
                            +{file.additions}
                          </span>
                          <span className="text-rose-600 dark:text-rose-400">
                            -{file.deletions}
                          </span>
                        </span>
                      )}
                    </div>
                  );
                })}
              </div>
              {files.length > 7 && (
                <div
                  className="pointer-events-none absolute inset-x-0 bottom-0 h-7"
                  style={{
                    background:
                      "linear-gradient(to top, var(--surface-elevated) 0%, transparent 100%)",
                  }}
                />
              )}
            </div>
          </div>
        )}

        {isAgentRunning && (
          <div className="flex items-center gap-2 rounded-lg border border-amber-500/25 bg-amber-500/[0.07] px-3 py-2 text-[12px] text-amber-700 dark:text-amber-300">
            <AlertTriangle className="size-3.5 shrink-0" />
            <span>Active turn will be stopped first.</span>
          </div>
        )}

        <div className="flex items-center justify-end gap-2 pt-1">
          <Button
            type="button"
            variant="ghost"
            size="sm"
            disabled={submitting}
            onClick={() => onOpenChange(false)}
          >
            Cancel
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={submitting}
            onClick={() => onConfirm(effectiveRestore)}
            className="gap-1.5"
          >
            {submitting && <Loader2 className="size-3.5 animate-spin" />}
            <span>Confirm</span>
            {!submitting && <span className="text-[11px] opacity-75">↵</span>}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}

/** Auto-refreshing relative timestamp (`just now` -> `2m ago`) with full-date floating pill tooltip on hover. */
const RelativeTimestampPill = memo(function RelativeTimestampPill({
  timestampMs,
}: {
  timestampMs: number;
}) {
  const nowMs = useSharedRelativeNow();
  const relativeText = formatRelativeTimeLabel(timestampMs, nowMs);
  const fullText = formatFullTimestampTooltip(timestampMs);

  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger
        delay={140}
        render={
          <span className="text-muted-foreground hover:text-foreground/85 tabular-nums inline-flex cursor-default items-center rounded px-1 py-0.5 text-[12px] leading-none transition-colors select-none" />
        }
      >
        {relativeText}
      </BaseTooltip.Trigger>
      {fullText && (
        <BaseTooltip.Portal>
          <BaseTooltip.Positioner side="top" sideOffset={6} className="pointer-events-none z-50">
            <BaseTooltip.Popup className="bg-elevated text-foreground orbit-tooltip-content tabular-nums rounded-full border border-black/[0.08] px-3 py-1 text-[12px] leading-normal font-normal shadow-[0_4px_14px_rgba(0,0,0,0.08)] select-none dark:border-white/[0.11] dark:shadow-[0_4px_16px_rgba(0,0,0,0.36)]">
              {fullText}
            </BaseTooltip.Popup>
          </BaseTooltip.Positioner>
        </BaseTooltip.Portal>
      )}
    </BaseTooltip.Root>
  );
});

/** Isolated 1-second live elapsed ticker with offline & silent-stall hints. */
const InlineLiveElapsed = memo(function InlineLiveElapsed({
  startedAtMs,
  lastActivityMs,
  isNetworkOffline = false,
}: {
  startedAtMs: number;
  lastActivityMs?: number | null;
  isNetworkOffline?: boolean;
}) {
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [startedAtMs]);

  const elapsedMs = Math.max(0, nowMs - startedAtMs);
  const silentMs = Math.max(0, nowMs - (lastActivityMs ?? startedAtMs));
  const isStalled = !isNetworkOffline && silentMs >= 45_000;

  if (elapsedMs < 1000 && !isNetworkOffline) return null;

  return (
    <>
      {elapsedMs >= 1000 && (
        <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[12.5px]">
          · {formatDurationCompact(elapsedMs)}
        </span>
      )}
      {isNetworkOffline ? (
        <span className="shrink-0 text-[12px] text-amber-600 dark:text-amber-400">
          · offline
        </span>
      ) : isStalled ? (
        <span className="text-muted-foreground/65 shrink-0 text-[12px]">
          · waiting for response
        </span>
      ) : null}
    </>
  );
});

/**
 * Completed Turn Footer (`[Copy · 2m ago]`):
 * Hidden while a turn is active since live elapsed duration lives inline in `TurnActivityDisclosure`.
 */
const TurnFooter = memo(function TurnFooter({
  isTurnActive,
  timing,
  completedAtMs,
  replyText,
}: {
  isTurnActive: boolean;
  timing?: TurnTimingRecord;
  completedAtMs: number | null;
  replyText: string | null;
}) {
  const [copied, setCopied] = useState(false);

  const resolvedCompletedAtMs = timing?.completedAtMs ?? completedAtMs;

  if (isTurnActive || (!replyText && resolvedCompletedAtMs === null)) {
    return null;
  }

  const handleCopy = async () => {
    if (!replyText) return;
    await navigator.clipboard.writeText(replyText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="mt-0.5 flex h-[22px] items-center gap-1.5">
      {replyText && (
        <CopyActionButton
          copied={copied}
          label="Copy"
          onClick={() => void handleCopy()}
          className="-ml-1"
        />
      )}

      {resolvedCompletedAtMs !== null && (
        <RelativeTimestampPill timestampMs={resolvedCompletedAtMs} />
      )}
    </div>
  );
});

const CompactionDivider = memo(function CompactionDivider({ label }: { label?: string }) {
  return (
    <div className="text-muted-foreground flex items-center gap-3 px-[10px] py-2 text-[12px]">
      <div className="border-border flex-1 border-t" />
      <span className="inline-flex items-center gap-1.5">
        <Scissors className="size-3 opacity-75" />
        <span>{label || "Context compacted"}</span>
      </span>
      <div className="border-border flex-1 border-t" />
    </div>
  );
});

const WakeupDivider = memo(function WakeupDivider({ label }: { label?: string }) {
  return (
    <div className="text-muted-foreground/80 flex items-center gap-3 py-1.5 text-[11.5px]">
      <div className="border-border/75 flex-1 border-t" />
      <span className="inline-flex items-center gap-1.5">
        <Sparkles className="size-3 opacity-70" />
        <span>{label || "Resumed from background task"}</span>
      </span>
      <div className="border-border/75 flex-1 border-t" />
    </div>
  );
});

const EMPTY_TIMELINE_ITEMS: TimelineItemView[] = [];

function computeTotalThinkingDurationMs(
  items: TimelineItemView[],
  agentId: string,
  getThinkingDurationMs: ThinkingDurationLookupFn,
): number | null {
  let total = 0;
  let foundAny = false;
  for (const it of items) {
    if (it.item_type !== "assistant_thinking") continue;
    const dur = getThinkingDurationMs(`${agentId}::think::${it.item_id || it.seq}`);
    if (dur !== null) {
      total += dur;
      foundAny = true;
    }
  }
  return foundAny ? total : null;
}

function buildTurnThinkingSignature(
  turn: StageTurn,
  agentId: string,
  thinkingTimings: Record<string, TurnTimingRecord>,
): string {
  let sig = "";
  for (const seg of turn.liveSegments) {
    if (seg.kind !== "step_group") continue;
    for (const it of seg.items) {
      if (it.item_type !== "assistant_thinking") continue;
      const key = `${agentId}::think::${it.item_id || it.seq}`;
      const dur = thinkingTimings[key]?.durationMs;
      if (dur !== null && dur !== undefined) {
        sig += `${it.seq}:${dur};`;
      }
    }
  }
  return sig;
}

const EMPTY_WORK_COUNTS: WorkSummaryResult["counts"] = {
  editedFileCount: 0,
  createdFileCount: 0,
  commandCount: 0,
  readFileCount: 0,
  searchCount: 0,
  subagentCount: 0,
  mcpServerCount: 0,
  otherToolCount: 0,
  thoughtCount: 0,
  narrationCount: 0,
  actionSteps: 0,
  totalSteps: 0,
  failedCount: 0,
  deniedCount: 0,
  unrecoveredFailureCount: 0,
  additions: 0,
  deletions: 0,
};

/**
 * Renders a complete Conversation Turn as chronological interleaved segments:
 * - Consecutive thinking, tool calls, and resolved gates fold cleanly inside per-batch
 *   `TurnActivityDisclosure` disclosures with live elapsed / stall / offline indicators.
 * - Every assistant reply (`assistant_message`) renders inline as top-level Markdown prose and
 *   is never swallowed into step disclosures — including interim updates and replies prior to
 *   post-turn autonomous wakeups.
 * - Notices (API warnings, stream interruptions, errors) and wakeup boundaries render in place.
 */
const TurnStageSection = memo(function TurnStageSection({
  turn,
  isTurnActive,
  isNetworkOffline,
  animatePromptEntrance = false,
  autoFoldCompleted,
  cwd,
  hostId,
  agentId,
  timing,
  thinkingSignature,
  getThinkingDurationMs,
  holdViewport,
  registerTurnEl,
  onRequestRewind,
}: {
  turn: StageTurn;
  isTurnActive: boolean;
  isNetworkOffline: boolean;
  animatePromptEntrance?: boolean;
  autoFoldCompleted: boolean;
  cwd: string;
  hostId: string;
  agentId: string;
  timing?: TurnTimingRecord;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
  registerTurnEl: (seq: number, el: HTMLDivElement | null) => void;
  onRequestRewind: (seq: number, promptText: string) => void;
}) {
  const setRef = useCallback(
    (el: HTMLDivElement | null) => {
      registerTurnEl(turn.anchorSeq, el);
    },
    [registerTurnEl, turn.anchorSeq],
  );

  if (
    turn.isCompactionCommand &&
    turn.foldedSteps.length === 0 &&
    turn.terminalReply === null &&
    turn.notices.length === 0
  ) {
    return (
      <div ref={setRef}>
        <CompactionDivider label={turn.compactions[0]?.label || "Context compacted"} />
      </div>
    );
  }

  const segments = turn.liveSegments;
  const lastSeg = segments[segments.length - 1] ?? null;
  const hasBody = segments.length > 0 || isTurnActive;
  const liveStartedAtMs = timing?.startedAtMs ?? turn.startedAtMs ?? null;

  // Show a lightweight trailing live indicator when the turn is active, but the last rendered
  // segment is already settled (e.g., after an interim reply, notice, or wakeup divider).
  const needsTrailingActivity =
    isTurnActive &&
    (lastSeg === null ||
      (lastSeg.kind === "assistant_message" && !lastSeg.item.streaming) ||
      lastSeg.kind === "notice" ||
      lastSeg.kind === "wakeup" ||
      lastSeg.kind === "compaction");

  return (
    <div
      ref={setRef}
      style={!isTurnActive ? { contain: "layout style" } : undefined}
      className="space-y-4"
    >
      {turn.userPrompt && (
        <UserPromptBubble
          item={turn.userPrompt}
          holdViewport={holdViewport}
          animateEntrance={animatePromptEntrance}
          onRequestRewind={onRequestRewind}
        />
      )}

      {turn.agentMessage && <AgentMessageCard hostId={hostId} item={turn.agentMessage} />}

      {hasBody && (
        <div className="px-[10px]">
          <div className="space-y-3">
            {segments.map((seg, idx) => {
              const isLastSeg = idx === segments.length - 1;

              if (seg.kind === "compaction") {
                return <CompactionDivider key={seg.key} label={seg.label} />;
              }

              if (seg.kind === "wakeup") {
                return <WakeupDivider key={seg.key} label={seg.label} />;
              }

              if (seg.kind === "notice") {
                const n = seg.item;
                const isError = n.level === "error";
                const isWarning = n.level === "warning";
                return (
                  <div
                    key={seg.key}
                    className={cn(
                      "flex items-start gap-2 rounded-xl border px-3.5 py-2 text-[13px] leading-[1.45]",
                      isError
                        ? "border-rose-500/30 bg-rose-500/[0.07] text-rose-700 dark:text-rose-300"
                        : isWarning
                          ? "border-amber-500/30 bg-amber-500/[0.07] text-amber-800 dark:text-amber-200"
                          : "text-muted-foreground border-border bg-muted/50",
                    )}
                  >
                    <AlertTriangle
                      className={cn(
                        "mt-[2px] size-4 shrink-0",
                        isError
                          ? "text-rose-600 dark:text-rose-400"
                          : isWarning
                            ? "text-amber-600 dark:text-amber-400"
                            : "text-muted-foreground",
                      )}
                    />
                    <span className="selectable-text min-w-0 whitespace-pre-wrap break-words">
                      {n.message}
                    </span>
                  </div>
                );
              }

              if (seg.kind === "assistant_message") {
                return (
                  <MarkdownBlocksView
                    key={seg.key}
                    blocks={seg.item.blocks}
                    streaming={seg.item.streaming}
                  />
                );
              }

              // `step_group`
              // Never render an already-failed trailing segment (with no running step) as active
              // `Running ... waiting for response` — keep its failure visible while any new turn work
              // shows cleanly in the trailing indicator below.
              const isTrailingFailedGroup =
                !seg.hasRunningStep && seg.summary.counts.unrecoveredFailureCount > 0;
              const isGroupActive =
                isTurnActive &&
                (seg.hasRunningStep ||
                  (isLastSeg && !seg.hasFollowingContent && !isTrailingFailedGroup));
              const groupThoughtMs =
                computeTotalThinkingDurationMs(seg.items, agentId, getThinkingDurationMs) ??
                seg.thoughtDurationMs;
              const groupSettledLabel =
                seg.summary.counts.actionSteps === 0 && seg.summary.counts.thoughtCount > 0
                  ? formatThoughtSummaryLabel(groupThoughtMs, seg.summary.counts.thoughtCount)
                  : seg.summary.label;
              const groupSettledDurationMs =
                seg.workDurationMs ??
                (segments.length === 1
                  ? (timing?.workDurationMs ?? turn.workDurationMs ?? timing?.durationMs ?? null)
                  : null);
              const groupLiveStartMs = seg.startedAtMs ?? liveStartedAtMs;

              return (
                <TurnActivityDisclosure
                  key={seg.key}
                  items={seg.items}
                  isWorkActive={isGroupActive}
                  liveLabel={seg.summary.liveLabel}
                  liveDetail={seg.summary.liveDetail}
                  settledLabel={groupSettledLabel}
                  summaryCounts={seg.summary.counts}
                  labelIncludesFailure={seg.summary.labelIncludesFailure}
                  defaultCollapsed={autoFoldCompleted}
                  autoOpenForError={
                    !isGroupActive &&
                    !seg.hasFollowingContent &&
                    seg.summary.counts.unrecoveredFailureCount > 0
                  }
                  liveStartedAtMs={groupLiveStartMs}
                  latestActivityMs={seg.latestActivityMs ?? groupLiveStartMs}
                  isNetworkOffline={isNetworkOffline}
                  settledDurationMs={groupSettledDurationMs}
                  cwd={cwd}
                  agentId={agentId}
                  thinkingSignature={thinkingSignature}
                  getThinkingDurationMs={getThinkingDurationMs}
                  holdViewport={holdViewport}
                />
              );
            })}

            {needsTrailingActivity && (
              <TurnActivityDisclosure
                items={EMPTY_TIMELINE_ITEMS}
                isWorkActive
                liveLabel={lastSeg === null ? "Working…" : "Thinking…"}
                liveDetail={null}
                settledLabel=""
                summaryCounts={EMPTY_WORK_COUNTS}
                labelIncludesFailure={false}
                defaultCollapsed={autoFoldCompleted}
                autoOpenForError={false}
                liveStartedAtMs={liveStartedAtMs}
                latestActivityMs={turn.completedAtMs ?? liveStartedAtMs}
                isNetworkOffline={isNetworkOffline}
                settledDurationMs={null}
                cwd={cwd}
                agentId={agentId}
                thinkingSignature={thinkingSignature}
                getThinkingDurationMs={getThinkingDurationMs}
                holdViewport={holdViewport}
              />
            )}

            {!isTurnActive && <TurnChangesBar diffs={turn.turnDiffs} cwd={cwd} />}

            <TurnFooter
              isTurnActive={isTurnActive}
              timing={timing}
              completedAtMs={turn.completedAtMs}
              replyText={!isTurnActive ? (turn.lastAssistantReply?.text ?? null) : null}
            />
          </div>
        </div>
      )}
    </div>
  );
}, (prev, next) =>
  prev.turn === next.turn &&
  prev.isTurnActive === next.isTurnActive &&
  prev.isNetworkOffline === next.isNetworkOffline &&
  prev.animatePromptEntrance === next.animatePromptEntrance &&
  prev.autoFoldCompleted === next.autoFoldCompleted &&
  prev.cwd === next.cwd &&
  prev.agentId === next.agentId &&
  prev.thinkingSignature === next.thinkingSignature &&
  prev.timing?.startedAtMs === next.timing?.startedAtMs &&
  prev.timing?.durationMs === next.timing?.durationMs &&
  prev.timing?.workDurationMs === next.timing?.workDurationMs &&
  prev.timing?.completedAtMs === next.timing?.completedAtMs &&
  prev.onRequestRewind === next.onRequestRewind,
);

function NewSessionLaunchpad() {
  return (
    <div className="orbit-stage-enter flex h-full w-full flex-col justify-center px-6 py-12 pb-[10vh]">
      <div className="mx-auto w-full max-w-[768px] space-y-6">
        <h1 className="text-foreground/90 px-1 text-center text-[26px] font-normal tracking-[-0.018em]">
          What would you like to build?
        </h1>

        <OmniComposer variant="hero" />
      </div>
    </div>
  );
}

export function StreamStage() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const sessionScrollNonce = useOrbitSelector((s) => s.sessionScrollNonce);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const overviewDesktopOpenPref = useOrbitSelector((s) => s.overviewDesktopOpenPref);
  const isNetworkOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const cachedTimeline = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? (s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`] ?? null)
      : null,
  );
  const wasSessionRewound = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? Boolean(s.rewoundSessions[`${s.selectedHostId}::${s.selectedAgentId}`])
      : false,
  );

  const stageContainerRef = useRef<HTMLDivElement | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const contentRef = useRef<HTMLDivElement | null>(null);
  const composerShelfRef = useRef<HTMLDivElement | null>(null);
  const prevComposerHeightRef = useRef<number>(0);
  const lastObservedScrollTopRef = useRef<number>(0);
  const stickToBottomRef = useRef<boolean>(true);
  const programmaticScrollRef = useRef<boolean>(false);
  const programmaticScrollTimerRef = useRef<number | null>(null);
  const scrollRafRef = useRef<number | null>(null);
  const lastOutlineSyncAtRef = useRef<number>(0);
  const turnElementMapRef = useRef<Map<number, HTMLDivElement>>(new Map());
  const turnTimingsRef = useRef<Record<string, TurnTimingRecord>>({});
  const thinkingTimingsRef = useRef<Record<string, TurnTimingRecord>>({});
  const mountedBaselineRef = useRef<{ agentId: string | null; maxSeq: number }>({
    agentId: null,
    maxSeq: 0,
  });

  const [isOccludingText, setIsOccludingText] = useState(false);
  const [showJumpLatest, setShowJumpLatest] = useState(false);
  const [stageWidth, setStageWidth] = useState(0);
  const [canDockOverview, setCanDockOverview] = useState(false);
  const canDockOverviewRef = useRef(false);
  const [activePromptSeq, setActivePromptSeq] = useState<number | null>(null);
  const [, setTimingTick] = useState(0);
  const [rewindTarget, setRewindTarget] = useState<RewindRequestTarget | null>(null);
  const [rewindSubmitting, setRewindSubmitting] = useState(false);

  const getThinkingDurationMs = useCallback<ThinkingDurationLookupFn>((thinkKey) => {
    return thinkingTimingsRef.current[thinkKey]?.durationMs ?? null;
  }, []);

  const currentAgent = useMemo(
    () =>
      hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ??
      cachedTimeline?.agent ??
      null,
    [hostDetail, selectedAgentId, cachedTimeline?.agent],
  );
  const isAgentRunning = Boolean(currentAgent?.is_running);

  const stage = useMemo(
    () =>
      processTimelineForStage(
        cachedTimeline?.items ?? EMPTY_TIMELINE_ITEMS,
        coalesceExploration,
      ),
    [cachedTimeline?.items, coalesceExploration],
  );

  const latestTurn = stage.turns[stage.turns.length - 1] ?? null;
  // Single source of truth: trust authoritative backend `currentAgent.is_running` (which covers
  // Initializing, Running, background-task waiting, and AutonomousWakeup) so stale historical
  // running tool items can never hold an Idle/Error/Closed turn in a phantom Running state.
  const isLatestTurnActive = Boolean(latestTurn && isAgentRunning);

  // Record live turn & per-thinking-block start/completion durations for the active session
  useEffect(() => {
    if (!selectedAgentId || !latestTurn) return;
    let changed = false;
    const now = Date.now();

    const timingKey = `${selectedAgentId}::${latestTurn.key}`;
    const existing = turnTimingsRef.current[timingKey];
    const seededStartMs =
      latestTurn.startedAtMs !== null && latestTurn.startedAtMs <= now
        ? latestTurn.startedAtMs
        : now;

    if (isLatestTurnActive) {
      if (!existing || existing.completedAtMs !== null) {
        turnTimingsRef.current[timingKey] = {
          startedAtMs: seededStartMs,
          completedAtMs: null,
          durationMs: null,
          workDurationMs: null,
        };
        changed = true;
      } else if (latestTurn.terminalReply !== null && existing.workDurationMs === null) {
        turnTimingsRef.current[timingKey] = {
          ...existing,
          workDurationMs: Math.max(0, now - existing.startedAtMs),
        };
        changed = true;
      } else if (latestTurn.terminalReply === null && existing.workDurationMs !== null) {
        turnTimingsRef.current[timingKey] = {
          ...existing,
          workDurationMs: null,
        };
        changed = true;
      }
    } else if (existing && existing.completedAtMs === null) {
      const totalMs = Math.max(0, now - existing.startedAtMs);
      turnTimingsRef.current[timingKey] = {
        ...existing,
        completedAtMs: now,
        durationMs: totalMs,
        workDurationMs: existing.workDurationMs ?? totalMs,
      };
      changed = true;
    }

    const rawItems = cachedTimeline?.items ?? EMPTY_TIMELINE_ITEMS;
    const lastRawItem = rawItems[rawItems.length - 1] ?? null;

    for (let idx = rawItems.length - 1; idx >= 0; idx -= 1) {
      const item = rawItems[idx];
      if (item.seq < latestTurn.anchorSeq) break;
      if (item.item_type !== "assistant_thinking") continue;

      const thinkKey = `${selectedAgentId}::think::${item.item_id || item.seq}`;
      const isCurrentlyThinking =
        isLatestTurnActive && lastRawItem?.seq === item.seq;
      const prevThink = thinkingTimingsRef.current[thinkKey];

      if (isCurrentlyThinking) {
        if (!prevThink) {
          thinkingTimingsRef.current[thinkKey] = {
            startedAtMs: now,
            completedAtMs: null,
            durationMs: null,
            workDurationMs: null,
          };
          changed = true;
        }
      } else if (prevThink && prevThink.completedAtMs === null) {
        thinkingTimingsRef.current[thinkKey] = {
          ...prevThink,
          completedAtMs: now,
          durationMs: Math.max(0, now - prevThink.startedAtMs),
          workDurationMs: null,
        };
        changed = true;
      }
    }

    if (changed) {
      setTimingTick((n) => n + 1);
    }
  }, [selectedAgentId, latestTurn, isLatestTurnActive, cachedTimeline?.items]);

  // Hysteresis-guarded ResizeObserver for Stage Overview docking (`>= 1040px` dock, `< 1000px` collapse)
  // and unified stage width tracking for the peripheral outline rail.
  // Synchronous `useLayoutEffect` prevents first-paint layout shift when entering an active session.
  useLayoutEffect(() => {
    const stageEl = stageContainerRef.current;
    if (!stageEl || typeof ResizeObserver === "undefined") {
      orbitActions.setStageCanDockOverview(false);
      return;
    }

    const syncStageWidth = (w: number) => {
      setStageWidth((prev) => (Math.abs(prev - w) < 0.5 ? prev : w));
      const nextCanDock = canDockOverviewRef.current ? w >= 1000 : w >= 1040;
      if (nextCanDock !== canDockOverviewRef.current) {
        canDockOverviewRef.current = nextCanDock;
        setCanDockOverview(nextCanDock);
        orbitActions.setStageCanDockOverview(nextCanDock);
      }
    };

    const initialW = stageEl.getBoundingClientRect().width;
    const initialDock = initialW >= 1040;
    setStageWidth(initialW);
    canDockOverviewRef.current = initialDock;
    setCanDockOverview(initialDock);
    orbitActions.setStageCanDockOverview(initialDock);

    const roStage = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0;
      syncStageWidth(w);
    });
    roStage.observe(stageEl);

    return () => {
      roStage.disconnect();
    };
  }, [isDraft, selectedAgentId]);

  const isOverviewDockedVisible = canDockOverview && overviewDesktopOpenPref;
  const isStageWide = stageWidth - (isOverviewDockedVisible ? 324 : 0) >= 820;

  const outlinePrompts = useMemo(
    () =>
      stage.turns
        .filter((t) => t.userPrompt && !t.isCompactionCommand)
        .map((t) => {
          const pres = extractUserPromptPresentation(t.userPrompt!);
          const summary =
            pres.cleanText || pres.badges.map((b) => b.label).join(" ") || "Prompt";
          return {
            seq: t.anchorSeq,
            preview: summary.replace(/\s+/g, " ").trim().slice(0, 110),
          };
        }),
    [stage.turns],
  );

  // Refs for `syncScrollMetrics` so its callback identity (and `holdViewport`) stays 100% stable forever
  const outlinePromptsRef = useRef(outlinePrompts);
  outlinePromptsRef.current = outlinePrompts;
  const stageTurnsRef = useRef(stage.turns);
  stageTurnsRef.current = stage.turns;
  const turnsCountRef = useRef(stage.turns.length);
  turnsCountRef.current = stage.turns.length;

  const effectiveCwdRef = useRef("");
  effectiveCwdRef.current = selectedCwd || currentAgent?.cwd || "";

  const handleRequestRewind = useCallback((seq: number, promptText: string) => {
    const activeCwd = effectiveCwdRef.current;
    const affected = stageTurnsRef.current.filter((t) => t.anchorSeq >= seq);
    const byPath = new Map<string, RewindFileEntry>();

    const recordFile = (rawPath: string, additions: number, deletions: number) => {
      if (!isRevertibleWorkspaceFile(rawPath, activeCwd)) return;
      const displayPath = stripWorkspacePrefix(rawPath, activeCwd).replace(/^\.\//, "");
      if (!displayPath) return;
      const prev = byPath.get(displayPath);
      if (prev) {
        prev.additions += additions;
        prev.deletions += deletions;
      } else {
        const basename = displayPath.split("/").pop() || displayPath;
        byPath.set(displayPath, {
          displayPath,
          basename,
          additions,
          deletions,
        });
      }
    };

    for (const turn of affected) {
      for (const diff of turn.turnDiffs) {
        if (diff.path) recordFile(diff.path, diff.additions, diff.deletions);
      }
      for (const seg of turn.liveSegments) {
        if (seg.kind !== "step_group") continue;
        for (const step of seg.items) {
          if (
            step.item_type === "tool_call" &&
            step.detail.kind === "file_edit" &&
            step.detail.path &&
            step.diff_files.length === 0
          ) {
            recordFile(step.detail.path, 0, 0);
          }
        }
      }
    }

    setRewindTarget({
      seq,
      promptText,
      affectedTurnsCount: Math.max(1, affected.length),
      editedFiles: Array.from(byPath.values()),
    });
  }, []);

  const handleConfirmRewind = useCallback(
    async (restoreFiles: boolean) => {
      if (!rewindTarget || !selectedHostId || !selectedAgentId || rewindSubmitting) return;
      setRewindSubmitting(true);
      try {
        const ok = await orbitActions.rewindAgent(
          selectedHostId,
          selectedAgentId,
          rewindTarget.seq,
          restoreFiles,
          rewindTarget.promptText,
        );
        if (ok) {
          setRewindTarget(null);
        }
      } finally {
        setRewindSubmitting(false);
      }
    },
    [rewindTarget, selectedHostId, selectedAgentId, rewindSubmitting],
  );
  const isStageWideRef = useRef(isStageWide);
  isStageWideRef.current = isStageWide;

  useEffect(() => {
    return () => {
      if (programmaticScrollTimerRef.current !== null) {
        window.clearTimeout(programmaticScrollTimerRef.current);
      }
      if (scrollRafRef.current !== null) {
        window.cancelAnimationFrame(scrollRafRef.current);
      }
    };
  }, []);

  const syncScrollMetrics = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const currentTop = el.scrollTop;
    const prevTop = lastObservedScrollTopRef.current;
    lastObservedScrollTopRef.current = currentTop;

    const distanceFromBottom = Math.max(0, el.scrollHeight - currentTop - el.clientHeight);
    const hasTurns = turnsCountRef.current > 0;

    if (programmaticScrollRef.current) {
      if (distanceFromBottom < 8) {
        programmaticScrollRef.current = false;
        if (programmaticScrollTimerRef.current !== null) {
          window.clearTimeout(programmaticScrollTimerRef.current);
          programmaticScrollTimerRef.current = null;
        }
      }
      stickToBottomRef.current = true;
      setIsOccludingText((prev) => (prev ? false : prev));
      setShowJumpLatest((prev) => (prev ? false : prev));
    } else {
      const nearBottom = distanceFromBottom < 36;
      const scrolledUp = currentTop < prevTop - 1.5;
      if (nearBottom) {
        stickToBottomRef.current = true;
      } else if (scrolledUp) {
        stickToBottomRef.current = false;
      }
      const pinned = stickToBottomRef.current;
      const nextOccluding = !pinned && distanceFromBottom > 4 && hasTurns;
      const nextJump = !pinned && !nearBottom && hasTurns;
      setIsOccludingText((prev) => (prev === nextOccluding ? prev : nextOccluding));
      setShowJumpLatest((prev) => (prev === nextJump ? prev : nextJump));
    }

    // Update active turn in peripheral outline rail strictly when visible (throttled to avoid layout thrash during fast momentum scrolling)
    const prompts = outlinePromptsRef.current;
    if (isStageWideRef.current && prompts.length >= 2) {
      const now = performance.now();
      if (now - lastOutlineSyncAtRef.current >= 90) {
        lastOutlineSyncAtRef.current = now;
        const viewportAnchorY = el.getBoundingClientRect().top + 120;
        let currentSeq = prompts[0].seq;
        for (const p of prompts) {
          const node = turnElementMapRef.current.get(p.seq);
          if (node && node.getBoundingClientRect().top <= viewportAnchorY) {
            currentSeq = p.seq;
          }
        }
        setActivePromptSeq((prev) => (prev === currentSeq ? prev : currentSeq));
      }
    }
  }, []);

  const snapScrollerToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    lastObservedScrollTopRef.current = el.scrollTop;
  }, []);

  const handleScroll = useCallback(() => {
    if (scrollRafRef.current !== null) return;
    scrollRafRef.current = window.requestAnimationFrame(() => {
      scrollRafRef.current = null;
      syncScrollMetrics();
    });
  }, [syncScrollMetrics]);

  // Use a strictly passive native wheel listener so WebKit's scrolling thread never blocks on JS during trackpad gestures
  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    const onPassiveWheel = (e: WheelEvent) => {
      if (e.deltaY < -0.5) {
        stickToBottomRef.current = false;
        if (programmaticScrollRef.current) {
          programmaticScrollRef.current = false;
          if (programmaticScrollTimerRef.current !== null) {
            window.clearTimeout(programmaticScrollTimerRef.current);
            programmaticScrollTimerRef.current = null;
          }
        }
      }
    };
    el.addEventListener("wheel", onPassiveWheel, { passive: true });
    return () => el.removeEventListener("wheel", onPassiveWheel);
  }, [isDraft, selectedHostId, selectedAgentId]);

  // Keep conversation bottom anchored cleanly when inner content grows (async timeline load, markdown/code layout, images)
  useEffect(() => {
    const contentEl = contentRef.current;
    const scrollerEl = scrollRef.current;
    if (!contentEl || !scrollerEl || typeof ResizeObserver === "undefined") return;

    const ro = new ResizeObserver(() => {
      if (stickToBottomRef.current && !programmaticScrollRef.current) {
        snapScrollerToBottom();
      }
      syncScrollMetrics();
    });
    ro.observe(contentEl);
    ro.observe(scrollerEl);
    return () => ro.disconnect();
  }, [isDraft, selectedHostId, selectedAgentId, snapScrollerToBottom, syncScrollMetrics]);

  // Keep conversation bottom anchored cleanly above the Composer when the Composer grows (multiline / attachments)
  useEffect(() => {
    const shelfEl = composerShelfRef.current;
    if (!shelfEl || typeof ResizeObserver === "undefined") return;
    prevComposerHeightRef.current = shelfEl.getBoundingClientRect().height;

    const ro = new ResizeObserver((entries) => {
      const nextH =
        entries[0]?.borderBoxSize?.[0]?.blockSize ??
        entries[0]?.contentRect.height ??
        shelfEl.getBoundingClientRect().height;
      const prevH = prevComposerHeightRef.current;
      prevComposerHeightRef.current = nextH;

      const deltaH = nextH - prevH;
      if (Math.abs(deltaH) < 0.5) return;

      const scroller = scrollRef.current;
      if (scroller && stickToBottomRef.current) {
        if (deltaH > 0) {
          scroller.scrollTop += deltaH;
        }
        lastObservedScrollTopRef.current = scroller.scrollTop;
      }
      syncScrollMetrics();
    });
    ro.observe(shelfEl);
    return () => ro.disconnect();
  }, [isDraft, selectedAgentId, syncScrollMetrics]);

  const scrollToLatest = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    programmaticScrollRef.current = true;
    stickToBottomRef.current = true;
    setIsOccludingText(false);
    setShowJumpLatest(false);

    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
    }
    programmaticScrollTimerRef.current = window.setTimeout(() => {
      programmaticScrollRef.current = false;
      programmaticScrollTimerRef.current = null;
      syncScrollMetrics();
    }, 800);

    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }, [syncScrollMetrics]);

  /**
   * Disclosure Scroll-Anchor Hold:
   * Keeps the clicked disclosure trigger stationary on screen when expanding/collapsing,
   * and ensures newly expanded details right above the Composer scroll into clear view.
   */
  const holdViewport: ViewportAnchorHoldFn = useCallback(
    (triggerEl, apply, edge = "top") => {
      const scroller = scrollRef.current;
      if (!scroller || !triggerEl) {
        apply();
        return;
      }
      const beforeRect = triggerEl.getBoundingClientRect();
      const beforeAnchor = edge === "bottom" ? beforeRect.bottom : beforeRect.top;
      const wasPinnedBottom = stickToBottomRef.current;
      stickToBottomRef.current = false;

      apply();

      requestAnimationFrame(() => {
        const curScroller = scrollRef.current;
        if (!curScroller || !triggerEl.isConnected) return;
        const afterRect = triggerEl.getBoundingClientRect();
        const afterAnchor = edge === "bottom" ? afterRect.bottom : afterRect.top;
        const delta = afterAnchor - beforeAnchor;
        if (Math.abs(delta) > 1) {
          curScroller.scrollTop += delta;
        }
        lastObservedScrollTopRef.current = curScroller.scrollTop;

        // If an item near the bottom expanded below the scroller's bottom edge, nudge into view
        const parentWrapper = triggerEl.parentElement;
        if (parentWrapper) {
          const wrapperRect = parentWrapper.getBoundingClientRect();
          const scrollerRect = curScroller.getBoundingClientRect();
          const overflowBelow = wrapperRect.bottom - scrollerRect.bottom;
          if (overflowBelow > 8 && wrapperRect.height < scrollerRect.height * 0.75) {
            curScroller.scrollBy({
              top: Math.min(overflowBelow + 12, 220),
              behavior: "smooth",
            });
          }
        }

        if (wasPinnedBottom) {
          const dist =
            curScroller.scrollHeight - curScroller.scrollTop - curScroller.clientHeight;
          stickToBottomRef.current = dist < 48;
        }
        syncScrollMetrics();
      });
    },
    [syncScrollMetrics],
  );

  const registerTurnEl = useCallback((seq: number, el: HTMLDivElement | null) => {
    if (el) {
      turnElementMapRef.current.set(seq, el);
    } else {
      turnElementMapRef.current.delete(seq);
    }
  }, []);

  const jumpToPromptSeq = useCallback((seq: number) => {
    const scroller = scrollRef.current;
    const target = turnElementMapRef.current.get(seq);
    if (!scroller || !target) return;
    stickToBottomRef.current = false;
    setActivePromptSeq(seq);
    const offsetTop =
      target.getBoundingClientRect().top -
      scroller.getBoundingClientRect().top +
      scroller.scrollTop -
      16;
    scroller.scrollTo({ top: Math.max(0, offsetTop), behavior: "smooth" });
  }, []);

  const activeSessionScope =
    !isDraft && selectedHostId && selectedAgentId
      ? `${selectedHostId}::${selectedAgentId}`
      : null;

  // Always reset bottom-pinning cleanly before paint whenever switching into a session
  useLayoutEffect(() => {
    if (!activeSessionScope) return;
    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
      programmaticScrollTimerRef.current = null;
    }
    programmaticScrollRef.current = false;
    stickToBottomRef.current = true;
    lastObservedScrollTopRef.current = 0;
    setIsOccludingText(false);
    setShowJumpLatest(false);
    setActivePromptSeq(null);
    setRewindTarget(null);
    snapScrollerToBottom();
  }, [activeSessionScope, sessionScrollNonce, snapScrollerToBottom]);

  // Synchronously pin to bottom before paint as timeline items arrive or turns append
  useLayoutEffect(() => {
    if (!activeSessionScope) return;
    if (stickToBottomRef.current && !programmaticScrollRef.current) {
      snapScrollerToBottom();
    }
    syncScrollMetrics();
  }, [
    activeSessionScope,
    cachedTimeline?.latestSeq,
    stage.turns.length,
    snapScrollerToBottom,
    syncScrollMetrics,
  ]);

  if (!selectedHostId) {
    return (
      <div className="text-muted-foreground flex flex-1 items-center justify-center text-[13.5px]">
        Pair or select an Orbit host to begin.
      </div>
    );
  }

  if (isDraft || !selectedAgentId) {
    mountedBaselineRef.current = { agentId: null, maxSeq: 0 };
    return (
      <ScrollArea className="min-h-0 flex-1" viewportClassName="[&>div]:h-full">
        <NewSessionLaunchpad />
      </ScrollArea>
    );
  }

  if (mountedBaselineRef.current.agentId !== selectedAgentId) {
    const isFreshFirstTurn = stage.turns.length === 1 && isLatestTurnActive;
    mountedBaselineRef.current = {
      agentId: selectedAgentId,
      maxSeq: isFreshFirstTurn
        ? 0
        : (cachedTimeline?.latestSeq ?? Number.MAX_SAFE_INTEGER),
    };
  } else if (
    cachedTimeline &&
    cachedTimeline.latestSeq < mountedBaselineRef.current.maxSeq
  ) {
    mountedBaselineRef.current = {
      agentId: selectedAgentId,
      maxSeq: cachedTimeline.latestSeq,
    };
  }
  const baselineMaxSeq = mountedBaselineRef.current.maxSeq;

  const effectiveCwd = selectedCwd || currentAgent?.cwd || "";

  return (
    <div
      ref={stageContainerRef}
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
    >
      {/* Peripheral Megathread Outline Rail (spacious conversation column >= 820px only) */}
      {isStageWide && outlinePrompts.length >= 2 && (
        <ChatOutlineRail
          prompts={outlinePrompts}
          activeSeq={activePromptSeq ?? outlinePrompts[outlinePrompts.length - 1]?.seq ?? null}
          onJumpToSeq={jumpToPromptSeq}
        />
      )}

      {/* Parent Back Banner when inspecting a managed subagent */}
      {selectedHostId && currentAgent?.parent_agent_id && (
        <div className="shrink-0 px-6">
          <ParentSubagentBanner hostId={selectedHostId} agent={currentAgent} />
        </div>
      )}

      {/* 1. Unified Scrollable Conversation Viewport — scrollbar stays on far right edge */}
      <ScrollArea
        className="min-h-0 flex-1"
        viewportRef={scrollRef}
        onViewportScroll={handleScroll}
        viewportClassName={cn(
          "pt-6 pb-11 pl-6 transition-[padding-right]",
          isOverviewDockedVisible
            ? "pr-[348px] duration-[240ms] ease-[var(--ease-panel)]"
            : "pr-6 duration-[200ms] ease-[var(--ease-panel-out)]",
        )}
      >
        <div
          key={selectedAgentId}
          ref={contentRef}
          className="orbit-stage-enter mx-auto w-full max-w-[768px] space-y-8"
        >
          {stage.turns.length === 0 &&
            wasSessionRewound &&
            cachedTimeline !== null &&
            (cachedTimeline.agent?.latest_seq ?? 0) === 0 &&
            !isAgentRunning && (
              <div className="text-muted-foreground flex flex-col items-center justify-center py-20 text-center">
                <RotateCcw className="mb-2.5 size-5 opacity-45" />
                <p className="text-[13.5px]">
                  Rewound to the start of this session. Edit your prompt below to continue.
                </p>
              </div>
            )}

          {stage.turns.map((turn, idx) => {
            const isLast = idx === stage.turns.length - 1;
            const turnActive = isLast && isLatestTurnActive;
            const isFreshLivePrompt =
              turn.userPrompt !== null && turn.userPrompt.seq > baselineMaxSeq;
            const timing = turnTimingsRef.current[`${selectedAgentId}::${turn.key}`];
            const thinkingSignature = buildTurnThinkingSignature(
              turn,
              selectedAgentId,
              thinkingTimingsRef.current,
            );
            return (
              <TurnStageSection
                key={turn.key}
                turn={turn}
                isTurnActive={turnActive}
                isNetworkOffline={isNetworkOffline}
                animatePromptEntrance={isFreshLivePrompt}
                autoFoldCompleted={coalesceExploration}
                cwd={effectiveCwd}
                hostId={selectedHostId ?? ""}
                agentId={selectedAgentId}
                timing={timing}
                thinkingSignature={thinkingSignature}
                getThinkingDurationMs={getThinkingDurationMs}
                holdViewport={holdViewport}
                registerTurnEl={registerTurnEl}
                onRequestRewind={handleRequestRewind}
              />
            );
          })}
        </div>
      </ScrollArea>

      {/* 2. Docked Composer Shelf — normal flex sibling below scrollRef (scrollbar never enters here) */}
      <div ref={composerShelfRef} className="bg-canvas relative z-20 shrink-0">
        {/* Upward Occlusion Fade & `38×38` [↓] Button */}
        <div className="pointer-events-none absolute inset-x-0 bottom-full h-7">
          <div
            className={cn(
              "h-full w-full transition-opacity duration-150",
              isOccludingText ? "opacity-100" : "opacity-0",
            )}
            style={{
              background:
                "linear-gradient(to top, var(--surface-canvas) 0%, color-mix(in oklab, var(--surface-canvas) 66%, transparent) 48%, transparent 100%)",
            }}
          />
          <div
            className={cn(
              "pointer-events-none absolute inset-y-0 left-0 transition-[right]",
              isOverviewDockedVisible
                ? "right-[324px] duration-[240ms] ease-[var(--ease-panel)]"
                : "right-0 duration-[200ms] ease-[var(--ease-panel-out)]",
            )}
          >
            <button
              type="button"
              aria-label="Scroll to latest"
              aria-hidden={!showJumpLatest}
              tabIndex={showJumpLatest ? 0 : -1}
              onClick={scrollToLatest}
              className={cn(
                "border-border bg-elevated text-muted-foreground hover:text-foreground hover:border-foreground/20 absolute bottom-2 left-1/2 inline-flex size-9.5 -translate-x-1/2 cursor-pointer items-center justify-center rounded-full border shadow-[0_2px_8px_rgba(0,0,0,0.08)] transition-all duration-200 ease-[var(--ease-out-expo)] dark:shadow-[0_2px_12px_rgba(0,0,0,0.36)]",
                showJumpLatest
                  ? "pointer-events-auto translate-y-0 scale-100 opacity-100"
                  : "pointer-events-none translate-y-1 scale-95 opacity-0",
              )}
            >
              <ArrowDown className="size-[18px] stroke-[1.9]" />
            </button>
          </div>
        </div>

        <div
          className={cn(
            "transition-[padding-right]",
            isOverviewDockedVisible
              ? "pr-[324px] duration-[240ms] ease-[var(--ease-panel)]"
              : "pr-0 duration-[200ms] ease-[var(--ease-panel-out)]",
          )}
        >
          <SubagentsTrack hostId={selectedHostId} agentId={selectedAgentId} />
          <StickyApprovalDock
            hostId={selectedHostId}
            agentId={selectedAgentId}
            cwd={effectiveCwd}
            permissions={stage.pendingPermissions}
            questions={stage.pendingQuestions}
          />
          <OmniComposer variant="docked" occluding={isOccludingText} />
        </div>
      </div>

      {/* 3. Top-Right Floating Overview Card Overlay (to the left of the far-right scrollbar) */}
      <StageOverviewPanel
        canDock={canDockOverview}
        hostId={selectedHostId}
        agentId={selectedAgentId}
        cwd={effectiveCwd}
        items={cachedTimeline?.items ?? EMPTY_TIMELINE_ITEMS}
        sessionTurnDiffs={stage.sessionTurnDiffs}
        currentAgent={currentAgent}
      />

      <RewindTurnDialog
        target={rewindTarget}
        isAgentRunning={isAgentRunning}
        submitting={rewindSubmitting}
        onOpenChange={(open) => {
          if (!open) setRewindTarget(null);
        }}
        onConfirm={(restoreFiles) => void handleConfirmRewind(restoreFiles)}
      />
    </div>
  );
}
