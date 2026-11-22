import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  useSyncExternalStore,
  type CSSProperties,
  type ReactElement,
} from "react";
import { createPortal } from "react-dom";
import {
  observeElementOffset,
  useVirtualizer,
  type Range,
  type ReactVirtualizer,
  type Virtualizer,
} from "@tanstack/react-virtual";
import {
  AlertTriangle,
  ArrowDown,
  Blocks,
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
  Film,
  FoldVertical,
  GitBranch,
  GitCompareArrows,
  GitFork,
  Globe,
  Layers,
  ListTodo,
  Loader2,
  MessageSquareText,
  Music,
  Quote,
  RotateCcw,
  RotateCw,
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
  type AgentCardView,
  type ArtifactSummary,
  type DiffFileView,
  type DiffLineView,
  type InlineSpanView,
  type PromptBadge,
  type RichBlockView,
  type SessionInsightsView,
  type StageTurnView,
  type TimelineItemView,
  type TurnFileDiffSummaryView,
  type WorkSummaryResultView,
  isAgentForegroundRunning,
} from "@/lib/orbit-ipc";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { detectRichMediaKind } from "@/lib/media-utils";
import {
  classifySearchTool,
  describeToolStep,
  extractThinkingHeadline,
  formatDurationCompact,
  formatThoughtSummaryLabel,
  stripWorkspacePrefix,
} from "@/lib/timeline-coalesce";
import { OmniComposer } from "@/components/composer/omni-composer";
import { extractInlineArtifactDirectives } from "@/components/artifacts/mcp-app-bridge";
import { MarkdownBlocksView, MarkdownLinkProvider } from "@/components/markdown-blocks";
import { openOrbitLink } from "@/lib/file-links";
import {
  getCachedPromptImage,
  loadPromptImage,
  lookupRecentSentTurnImage,
  type PromptImageEntry,
} from "@/lib/prompt-image-cache";
import { useWorkspaceMotion } from "@/components/layout/workspace-motion";
import { StickyApprovalDock } from "@/components/stage/stage-approval-dock";
import { ChatOutlineRail } from "@/components/stage/stage-outline-rail";
import {
  AgentMessageCard,
  ParentSubagentBanner,
  SubagentApprovalBanner,
} from "@/components/stage/subagents-track";
import { StageOverviewPanel } from "@/components/stage/stage-overview-panel";
import { Button } from "@/components/ui/button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { isImeKeyEvent } from "@/lib/ime";
import { isAgentInGitRepo } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const USER_PROMPT_PREVIEW_MAX_PX = 124;

const StageHydratingFallback = memo(function StageHydratingFallback({
  bootstrapError,
}: {
  bootstrapError: string | null;
}) {
  const [showIndicator, setShowIndicator] = useState(false);
  const [showSlowRetry, setShowSlowRetry] = useState(false);
  const [retrying, setRetrying] = useState(false);

  useEffect(() => {
    const showTimer = window.setTimeout(() => setShowIndicator(true), 350);
    const slowTimer = window.setTimeout(() => setShowSlowRetry(true), 5000);
    return () => {
      window.clearTimeout(showTimer);
      window.clearTimeout(slowTimer);
    };
  }, []);

  if (!bootstrapError && !showIndicator) {
    return <div className="flex-1" />;
  }

  const needsRetry = Boolean(bootstrapError) || showSlowRetry;

  return (
    <div className="orbit-stage-enter text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2.5 px-6 text-[13px] select-none">
      <div className="flex items-center gap-2">
        {!bootstrapError && (
          <OrbitSpinner size={13} className="text-muted-foreground/75 shrink-0" />
        )}
        <span className="text-muted-foreground/85">
          {bootstrapError
            ? "Unable to load workspace"
            : showSlowRetry
              ? "Taking longer than usual to connect…"
              : "Connecting to workspace…"}
        </span>
      </div>
      {needsRetry && (
        <button
          type="button"
          disabled={retrying}
          onClick={() => {
            setRetrying(true);
            setShowSlowRetry(false);
            void orbitActions.retryBootstrap().finally(() => {
              setRetrying(false);
            });
          }}
          className="border-border/80 text-foreground/80 hover:bg-accent hover:text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
        >
          <RotateCw className={cn("size-3", retrying && "animate-spin")} />
          <span>{retrying ? "Retrying…" : "Retry"}</span>
        </button>
      )}
    </div>
  );
});

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

// Shared singleton relative-time clocks (15s for recent < 1h, 60s for older >= 1h)
const RECENT_CLOCK_THRESHOLD_MS = 3_600_000;
let sharedRelativeNowMs = Date.now();
let sharedSlowRelativeNowMs = Date.now();
let sharedRelativeTimerId: number | null = null;
let sharedSlowRelativeTimerId: number | null = null;
const relativeClockListeners = new Set<() => void>();
const slowRelativeClockListeners = new Set<() => void>();

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

function subscribeSlowRelativeClock(listener: () => void): () => void {
  slowRelativeClockListeners.add(listener);
  if (sharedSlowRelativeTimerId === null && typeof window !== "undefined") {
    sharedSlowRelativeNowMs = Date.now();
    sharedSlowRelativeTimerId = window.setInterval(() => {
      sharedSlowRelativeNowMs = Date.now();
      for (const fn of slowRelativeClockListeners) {
        fn();
      }
    }, 60_000);
  }
  return () => {
    slowRelativeClockListeners.delete(listener);
    if (slowRelativeClockListeners.size === 0 && sharedSlowRelativeTimerId !== null) {
      window.clearInterval(sharedSlowRelativeTimerId);
      sharedSlowRelativeTimerId = null;
    }
  };
}

function useSharedRelativeNow(timestampMs: number): number {
  const isRecent = Date.now() - timestampMs < RECENT_CLOCK_THRESHOLD_MS;
  return useSyncExternalStore(
    isRecent ? subscribeRelativeClock : subscribeSlowRelativeClock,
    () => (isRecent ? sharedRelativeNowMs : sharedSlowRelativeNowMs),
    () => (isRecent ? sharedRelativeNowMs : sharedSlowRelativeNowMs),
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

function stableStepItemKey(item: TimelineItemView, idx: number): string {
  switch (item.item_type) {
    case "assistant_thinking":
      return item.item_id ? `think-${item.item_id}` : `think-seq-${item.seq}-${idx}`;
    case "assistant_message":
      return item.item_id ? `msg-${item.item_id}` : `msg-seq-${item.seq}-${idx}`;
    case "tool_call":
      return item.call_id ? `tool-${item.call_id}` : `tool-seq-${item.seq}-${idx}`;
    case "permission_request":
      return item.request_id ? `perm-${item.request_id}` : `perm-seq-${item.seq}-${idx}`;
    case "question_request":
      return item.request_id ? `q-${item.request_id}` : `q-seq-${item.seq}-${idx}`;
    case "user_prompt":
      return item.message_id ? `prompt-${item.message_id}` : `prompt-seq-${item.seq}-${idx}`;
    case "agent_message":
      return item.message_id ? `am-${item.message_id}` : `am-seq-${item.seq}-${idx}`;
    case "notice":
      return `notice-${item.seq}-${idx}`;
  }
}

/**
 * Borderless step row for reasoning, intermediate notes, permissions, and tool calls:
 * - Zero box/border clutter at rest; tight vertical grouping.
 * - Pure-CSS `group-hover` swap between semantic icon and `ChevronRight` (0 React hover re-renders during scroll).
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
  const triggerRef = useRef<HTMLButtonElement | null>(null);
  const toolRowRef = useRef<HTMLDivElement | null>(null);

  if (item.item_type === "assistant_thinking") {
    const trimmedThinking = item.text.trim();
    const headline = extractThinkingHeadline(trimmedThinking, isActivelyThinking);
    const canExpand = trimmedThinking.length > 0;
    const thoughtLabel = isActivelyThinking
      ? "Thinking"
      : formatThoughtSummaryLabel(thoughtDurationMs, 1);

    return (
      <div>
        <button
          ref={triggerRef}
          type="button"
          disabled={!canExpand}
          onClick={() => {
            if (!canExpand) return;
            holdViewport(triggerRef.current, () => setExpanded((v) => !v));
          }}
          className="group/step hover:bg-accent/55 -ml-1.5 flex w-full max-w-full cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-left text-[13px] transition-colors disabled:cursor-default"
        >
          <span className="text-muted-foreground group-hover/step:text-foreground/80 relative flex size-4 shrink-0 items-center justify-center">
            {canExpand ? (
              expanded ? (
                <ChevronRight className="size-3.5 rotate-90 stroke-[1.9] transition-transform duration-150" />
              ) : (
                <>
                  <Sparkles className="size-3.5 stroke-[1.75] transition-opacity duration-100 group-hover/step:opacity-0" />
                  <ChevronRight className="absolute size-3.5 stroke-[1.9] opacity-0 transition-opacity duration-100 group-hover/step:opacity-100" />
                </>
              )
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

    return (
      <div>
        <button
          ref={triggerRef}
          type="button"
          onClick={() => holdViewport(triggerRef.current, () => setExpanded((v) => !v))}
          className="group/note hover:bg-accent/55 -ml-1.5 flex w-full max-w-full cursor-pointer items-center gap-2 rounded-md px-1.5 py-1 text-left text-[13px] transition-colors"
        >
          <span className="text-muted-foreground group-hover/note:text-foreground/80 relative flex size-4 shrink-0 items-center justify-center">
            {expanded ? (
              <ChevronRight className="size-3.5 rotate-90 stroke-[1.9] transition-transform duration-150" />
            ) : (
              <>
                <MessageSquareText className="size-3.5 stroke-[1.75] transition-opacity duration-100 group-hover/note:opacity-0" />
                <ChevronRight className="absolute size-3.5 stroke-[1.9] opacity-0 transition-opacity duration-100 group-hover/note:opacity-100" />
              </>
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
  const openFilePath = desc.openPath;
  const canOpenFileInDiff =
    Boolean(openFilePath) && (desc.kind === "file_edit" || desc.kind === "file_create");
  const canOpenFileDirect = Boolean(openFilePath) && desc.kind === "file_read";
  const isSubagentStep = item.detail.kind === "subagent";
  const readStartLine = item.detail.kind === "file_read" ? item.detail.start_line : null;
  const readEndLine = item.detail.kind === "file_read" ? item.detail.end_line : null;
  const canOpenTargetDirectly = Boolean(
    openFilePath && ((canOpenFileInDiff && !hasExpandableDetail) || canOpenFileDirect),
  );
  const handleOpenTargetFile = () => {
    if (!openFilePath) return;
    if (canOpenFileInDiff) {
      orbitActions.openInspectorForFile(openFilePath, "session_turns");
      return;
    }
    if (canOpenFileDirect) {
      orbitActions.openFileInSidePanel(openFilePath, {
        cwd,
        focusLine: readStartLine,
        focusEndLine: readEndLine,
      });
    }
  };

  return (
    <div>
      <div
        className={cn(
          "group/tool -ml-1.5 flex items-center justify-between gap-2 rounded-md px-1.5 py-1 text-[13px] transition-colors",
          (hasExpandableDetail || canOpenFileInDiff || canOpenFileDirect || isSubagentStep) &&
            "hover:bg-accent/55",
        )}
      >
        <div
          ref={toolRowRef}
          onClick={
            hasExpandableDetail
              ? () => holdViewport(toolRowRef.current, () => setExpanded((v) => !v))
              : undefined
          }
          className={cn(
            "flex min-w-0 flex-1 items-center gap-2 text-left",
            hasExpandableDetail ? "cursor-pointer" : "cursor-default",
          )}
        >
          <span
            className={cn(
              "relative flex size-4 shrink-0 items-center justify-center",
              isError
                ? "text-rose-500"
                : "text-muted-foreground group-hover/tool:text-foreground/80",
            )}
          >
            {hasExpandableDetail ? (
              expanded ? (
                <ChevronRight className="size-3.5 rotate-90 stroke-[1.9] transition-transform duration-150" />
              ) : (
                <>
                  {isError ? (
                    <AlertTriangle className="size-3.5 stroke-[1.85] transition-opacity duration-100 group-hover/tool:opacity-0" />
                  ) : (
                    <IconComp className="size-3.5 stroke-[1.75] transition-opacity duration-100 group-hover/tool:opacity-0" />
                  )}
                  <ChevronRight className="absolute size-3.5 stroke-[1.9] opacity-0 transition-opacity duration-100 group-hover/tool:opacity-100" />
                </>
              )
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

          {desc.target &&
            (canOpenTargetDirectly ? (
              <button
                type="button"
                title={
                  canOpenFileInDiff
                    ? `Open ${desc.target} diff`
                    : `Open ${desc.target} in Side Panel`
                }
                onClick={(e) => {
                  e.stopPropagation();
                  handleOpenTargetFile();
                }}
                className={cn(
                  "min-w-0 cursor-pointer truncate text-left font-mono text-[12px] hover:underline",
                  desc.isRunning
                    ? "agent-text-shimmer"
                    : "text-muted-foreground group-hover/tool:text-foreground/70 hover:text-foreground",
                )}
              >
                {desc.target}
              </button>
            ) : (
              <span
                className={cn(
                  "truncate font-mono text-[12px]",
                  desc.isRunning
                    ? "agent-text-shimmer"
                    : "text-muted-foreground group-hover/tool:text-foreground/70",
                )}
              >
                {desc.target}
              </span>
            ))}

          {desc.secondaryMeta &&
            (canOpenFileDirect && openFilePath ? (
              <button
                type="button"
                title={`Jump to ${desc.target ?? openFilePath} (${desc.secondaryMeta})`}
                onClick={(e) => {
                  e.stopPropagation();
                  handleOpenTargetFile();
                }}
                className="text-muted-foreground/75 hover:text-foreground shrink-0 cursor-pointer font-mono text-[11.5px] hover:underline"
              >
                {desc.secondaryMeta}
              </button>
            ) : (
              <span className="text-muted-foreground/70 shrink-0 font-mono text-[11.5px]">
                {desc.secondaryMeta}
              </span>
            ))}
        </div>

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
                  cwd,
                  focusLine: readStartLine,
                  focusEndLine: readEndLine,
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
            key={stableStepItemKey(sub, i)}
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
  isBackgroundWait = false,
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
  summaryCounts: WorkSummaryResultView["counts"];
  labelIncludesFailure: boolean;
  defaultCollapsed: boolean;
  autoOpenForError: boolean;
  liveStartedAtMs: number | null;
  latestActivityMs?: number | null;
  isNetworkOffline?: boolean;
  isBackgroundWait?: boolean;
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
          stallLabel={isBackgroundWait ? null : undefined}
        />
      </div>
    );
  }

  const defaultOpen = !defaultCollapsed || autoOpenForError;
  const isExpanded = userExpanded !== null ? userExpanded : defaultOpen;
  const showFailureBadge =
    !isWorkActive && summaryCounts.unrecovered_failure_count > 0 && !labelIncludesFailure;
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
              stallLabel={isBackgroundWait ? "running in background" : undefined}
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
                · {summaryCounts.unrecovered_failure_count} failed
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
              !isWorkActive && (summaryCounts.failed_count > 0 || summaryCounts.denied_count > 0)
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
    case "pdf":
      return FileText;
    case "video":
      return Film;
    case "audio":
      return Music;
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
    inferredBadges.push({ kind: "image", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached PDF file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "pdf", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached audio file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "audio", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached video file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "video", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Attached file:\s*`([^`]+)`\s*$/gm, (_, rawPath: string) => {
    sanitized = true;
    const name = rawPath.split("/").pop() || rawPath;
    inferredBadges.push({ kind: "file", label: name, path: rawPath });
    return "";
  });

  working = working.replace(/^Referenced workspace files:\s*(.+)$/gm, (_, listStr: string) => {
    sanitized = true;
    const matches = Array.from(listStr.matchAll(/`@?([^`]+)`/g));
    for (const m of matches) {
      const rawPath = m[1];
      const name = rawPath.split("/").pop() || rawPath;
      inferredBadges.push({ kind: "file", label: name, path: rawPath });
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
      inferredBadges.push({ kind: "quote", label: shortLoc, path: loc });
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

interface PromptImagePreviewTarget {
  url: string;
  label: string;
  path: string | null;
  hostId: string;
  cwd: string;
}

const PromptImageThumbnail = memo(function PromptImageThumbnail({
  badge,
  cleanText,
  imageIndex,
  hostId,
  agentId,
  cwd,
  onPreviewImage,
}: {
  badge: PromptBadge;
  cleanText: string;
  imageIndex: number;
  hostId: string;
  agentId: string;
  cwd: string;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
}) {
  const recent = useMemo(
    () =>
      !badge.path
        ? lookupRecentSentTurnImage(hostId, agentId || null, cleanText, imageIndex, badge.label)
        : null,
    [badge.path, badge.label, cleanText, imageIndex, hostId, agentId],
  );
  const effectivePath = badge.path?.trim() || recent?.path?.trim() || null;

  const resolveInitialState = (): {
    status: "loading" | "ready" | "fallback";
    url: string | null;
    resolvedPath: string | null;
  } => {
    if (effectivePath && hostId) {
      const cached = getCachedPromptImage(hostId, effectivePath);
      if (cached?.status === "ready") {
        return { status: "ready", url: cached.url, resolvedPath: cached.resolvedPath };
      }
      if (cached?.status === "error") {
        return recent?.previewUrl
          ? { status: "ready", url: recent.previewUrl, resolvedPath: effectivePath }
          : { status: "fallback", url: null, resolvedPath: effectivePath };
      }
      return recent?.previewUrl
        ? { status: "ready", url: recent.previewUrl, resolvedPath: effectivePath }
        : { status: "loading", url: null, resolvedPath: effectivePath };
    }
    if (recent?.previewUrl) {
      return { status: "ready", url: recent.previewUrl, resolvedPath: effectivePath };
    }
    return { status: "fallback", url: null, resolvedPath: null };
  };

  const [thumbState, setThumbState] = useState(resolveInitialState);

  useEffect(() => {
    const initial = resolveInitialState();
    setThumbState(initial);
    if (initial.status !== "loading" || !effectivePath || !hostId) {
      return;
    }
    let cancelled = false;
    void loadPromptImage(hostId, cwd || "~", effectivePath).then((entry: PromptImageEntry) => {
      if (cancelled) return;
      if (entry.status === "ready") {
        setThumbState({ status: "ready", url: entry.url, resolvedPath: entry.resolvedPath });
      } else {
        setThumbState({ status: "fallback", url: null, resolvedPath: effectivePath });
      }
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [hostId, agentId, cwd, badge.path, badge.label, effectivePath]);

  if (thumbState.status === "loading") {
    return (
      <div
        title={badge.label}
        className="border-border bg-muted/55 size-[56px] shrink-0 animate-pulse rounded-[12px] border"
      />
    );
  }

  if (thumbState.status === "ready" && thumbState.url) {
    const imgUrl = thumbState.url;
    const imgPath = thumbState.resolvedPath ?? effectivePath;
    return (
      <button
        type="button"
        title={`Preview ${badge.label}`}
        onClick={() =>
          onPreviewImage({
            url: imgUrl,
            label: badge.label,
            path: imgPath,
            hostId,
            cwd,
          })
        }
        className="group/img border-border bg-muted/40 relative size-[56px] shrink-0 cursor-pointer overflow-hidden rounded-[12px] border shadow-[0_1px_3px_rgba(0,0,0,0.06)] transition-[border-color,box-shadow] duration-150 hover:border-foreground/25 dark:shadow-[0_1px_6px_rgba(0,0,0,0.26)]"
      >
        <img
          src={imgUrl}
          alt={badge.label}
          decoding="async"
          draggable={false}
          onError={() =>
            setThumbState({ status: "fallback", url: null, resolvedPath: imgPath })
          }
          className="size-full object-cover transition-transform duration-200 ease-[var(--ease-out-expo)] group-hover/img:scale-[1.03]"
        />
      </button>
    );
  }

  return (
    <span
      title={badge.label}
      className="text-foreground/85 inline-flex items-center gap-1.5 rounded-xl border border-black/[0.075] bg-black/[0.03] px-2.5 py-1 text-[12px] dark:border-white/[0.10] dark:bg-white/[0.055]"
    >
      <FileImage className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
      <span className="max-w-[180px] truncate font-medium">{badge.label}</span>
    </span>
  );
});

const PromptImageLightbox = memo(function PromptImageLightbox({
  target,
  onClose,
}: {
  target: PromptImagePreviewTarget | null;
  onClose: () => void;
}) {
  return (
    <Dialog open={target !== null} onOpenChange={(open) => !open && onClose()}>
      <DialogContent className="w-fit max-w-[92vw] gap-3 p-4 sm:p-5">
        <DialogHeader className="gap-1 pr-8">
          <DialogTitle className="truncate font-mono text-[13.5px] font-medium">
            {target?.label ?? ""}
          </DialogTitle>
          {target?.path && (
            <DialogDescription className="truncate font-mono text-[11.5px]">
              {target.path}
            </DialogDescription>
          )}
        </DialogHeader>

        {target && (
          <div className="flex flex-col items-end gap-3">
            <div className="border-border bg-muted/30 flex max-h-[76vh] max-w-[86vw] items-center justify-center overflow-hidden rounded-xl border p-1.5">
              <img
                src={target.url}
                alt={target.label}
                className="max-h-[72vh] max-w-[84vw] rounded-lg object-contain"
              />
            </div>

            {target.path && target.hostId && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  const p = target.path;
                  const h = target.hostId;
                  const c = target.cwd;
                  onClose();
                  if (p) {
                    orbitActions.openFileInSidePanel(p, { hostId: h, cwd: c });
                  }
                }}
                className="gap-1.5 text-[12px]"
              >
                <FileSymlink className="size-3.5" />
                Open in Side Panel
              </Button>
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
});

/**
 * Orbit Native User Prompt Bubble:
 * - Crisp cool-neutral Orbit surface aligned flush right.
 * - Renders image attachments as right-aligned rounded thumbnails cleanly above the text bubble.
 * - Renders semantic context & slash workflow tokens (`/plan`, `/review`, `@file`, `Diff`)
 *   without exposing hidden wire scaffolding.
 * - Long messages preview smoothly with a bottom fade gradient; click anywhere on the bubble to
 *   expand, and click outside while expanded to collapse back.
 * - Quiet bottom-right hover toolbar (`timestamp` + `Copy` + `Undo to this point`) with zero layout shift.
 */
const UserPromptBubble = memo(function UserPromptBubble({
  item,
  hostId,
  agentId,
  cwd,
  holdViewport,
  animateEntrance = false,
  onRequestRewind,
  onPreviewImage,
}: {
  item: Extract<TimelineItemView, { item_type: "user_prompt" }>;
  hostId: string;
  agentId: string;
  cwd?: string;
  holdViewport: ViewportAnchorHoldFn;
  animateEntrance?: boolean;
  onRequestRewind?: (seq: number, promptText: string) => void;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
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
    let rafId = 0;
    const ro = new ResizeObserver(() => {
      if (rafId) return;
      rafId = window.requestAnimationFrame(() => {
        rafId = 0;
        measure();
      });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (rafId) window.cancelAnimationFrame(rafId);
    };
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

  const imageBadges = useMemo(
    () => badges.filter((b) => b.kind === "image"),
    [badges],
  );
  const slashBadge = useMemo(
    () => badges.find((b) => b.kind === "slash") ?? null,
    [badges],
  );
  const attachmentBadges = useMemo(
    () => badges.filter((b) => b.kind !== "slash" && b.kind !== "image"),
    [badges],
  );
  const hasBubbleContent =
    attachmentBadges.length > 0 || Boolean(slashBadge) || cleanText.length > 0;
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
      {imageBadges.length > 0 && (
        <div
          className={cn(
            "flex max-w-[80%] flex-wrap justify-end gap-1.5 select-none",
            hasBubbleContent && "mb-1.5",
          )}
        >
          {imageBadges.map((badge, idx) => (
            <PromptImageThumbnail
              key={`uimg-${badge.path ?? badge.label}-${idx}`}
              badge={badge}
              cleanText={cleanText}
              imageIndex={idx}
              hostId={hostId}
              agentId={agentId}
              cwd={cwd || "~"}
              onPreviewImage={onPreviewImage}
            />
          ))}
        </div>
      )}

      {hasBubbleContent && (
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
                  const mediaKind =
                    badge.kind === "pdf" || badge.kind === "audio" || badge.kind === "video"
                      ? badge.kind
                      : badge.kind === "file"
                        ? detectRichMediaKind(badge.path || badge.label)
                        : null;
                  const isMono =
                    badge.kind === "file" || badge.kind === "quote" || mediaKind !== null;
                  const fileTarget = badge.path?.trim() || badge.label;
                  const isClickableMediaBadge = mediaKind !== null && fileTarget.trim().length > 0;
                  const isClickableFileBadge = isMono && fileTarget.trim().length > 0;
                  if (isClickableMediaBadge && mediaKind) {
                    return (
                      <button
                        key={`${badge.kind}-${badge.label}-${idx}`}
                        type="button"
                        title={`Preview ${badge.label}`}
                        onClick={(e) => {
                          e.stopPropagation();
                          orbitActions.openMediaPreview({
                            kind: mediaKind,
                            filename: badge.label,
                            path: fileTarget,
                            hostId,
                            cwd: cwd || "~",
                          });
                        }}
                        className="text-foreground/85 hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-black/[0.075] bg-black/[0.03] px-2 py-0.5 text-[12px] transition-colors hover:border-black/[0.14] hover:bg-black/[0.055] dark:border-white/[0.10] dark:bg-white/[0.055] dark:hover:border-white/[0.18] dark:hover:bg-white/[0.095]"
                      >
                        <BadgeIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
                        <span className="font-mono leading-normal font-medium hover:underline">
                          {badge.label}
                        </span>
                      </button>
                    );
                  }
                  if (isClickableFileBadge) {
                    return (
                      <button
                        key={`${badge.kind}-${badge.label}-${idx}`}
                        type="button"
                        title={`Open ${badge.label} in Side Panel`}
                        onClick={(e) => {
                          e.stopPropagation();
                          openOrbitLink(fileTarget, { hostId, cwd });
                        }}
                        className="text-foreground/85 hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border border-black/[0.075] bg-black/[0.03] px-2 py-0.5 text-[12px] transition-colors hover:border-black/[0.14] hover:bg-black/[0.055] dark:border-white/[0.10] dark:bg-white/[0.055] dark:hover:border-white/[0.18] dark:hover:bg-white/[0.095]"
                      >
                        <BadgeIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
                        <span className="font-mono leading-normal font-medium hover:underline">
                          {badge.label}
                        </span>
                      </button>
                    );
                  }
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
      )}

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
  diffs: TurnFileDiffSummaryView[];
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

/**
 * Zero-rest-overhead hover pill tooltip:
 * Avoids mounting hundreds of persistent tooltip context/portal providers per conversation turn.
 * Reads trigger geometry only after hover/focus delay elapses and mounts a single lightweight portal.
 */
function HoverPillTooltip({
  label,
  delay = 220,
  tabularNums = false,
  renderTrigger,
}: {
  label: string;
  delay?: number;
  tabularNums?: boolean;
  renderTrigger: (handlers: {
    onMouseEnter: (e: React.MouseEvent<HTMLElement>) => void;
    onMouseLeave: () => void;
    onFocus: (e: React.FocusEvent<HTMLElement>) => void;
    onBlur: () => void;
  }) => ReactElement;
}) {
  const [anchorPos, setAnchorPos] = useState<{ x: number; y: number } | null>(null);
  const [shiftX, setShiftX] = useState(0);
  const timerRef = useRef<number | null>(null);
  const tooltipRef = useRef<HTMLDivElement | null>(null);

  const clearTimer = useCallback(() => {
    if (timerRef.current !== null) {
      window.clearTimeout(timerRef.current);
      timerRef.current = null;
    }
  }, []);

  useEffect(() => clearTimer, [clearTimer]);

  useLayoutEffect(() => {
    if (!anchorPos || !tooltipRef.current) {
      setShiftX(0);
      return;
    }
    const half = tooltipRef.current.getBoundingClientRect().width / 2;
    const pad = 8;
    const vw = window.innerWidth;
    let nextShift = 0;
    if (anchorPos.x + half > vw - pad) {
      nextShift = vw - pad - (anchorPos.x + half);
    } else if (anchorPos.x - half < pad) {
      nextShift = pad - (anchorPos.x - half);
    }
    setShiftX(nextShift);
  }, [anchorPos, label]);

  const scheduleShow = useCallback(
    (el: HTMLElement | null) => {
      clearTimer();
      if (!el || !label) return;
      timerRef.current = window.setTimeout(() => {
        timerRef.current = null;
        if (!el.isConnected) return;
        const rect = el.getBoundingClientRect();
        setShiftX(0);
        setAnchorPos({ x: rect.left + rect.width / 2, y: rect.top - 6 });
      }, delay);
    },
    [clearTimer, delay, label],
  );

  const hide = useCallback(() => {
    clearTimer();
    setAnchorPos(null);
    setShiftX(0);
  }, [clearTimer]);

  return (
    <>
      {renderTrigger({
        onMouseEnter: (e) => scheduleShow(e.currentTarget),
        onMouseLeave: hide,
        onFocus: (e) => scheduleShow(e.currentTarget),
        onBlur: hide,
      })}
      {anchorPos &&
        label &&
        createPortal(
          <div
            ref={tooltipRef}
            role="tooltip"
            style={{
              left: `${anchorPos.x + shiftX}px`,
              top: `${anchorPos.y}px`,
            }}
            className="pointer-events-none fixed z-50 -translate-x-1/2 -translate-y-full whitespace-nowrap"
          >
            <div
              className={cn(
                "bg-elevated text-foreground rounded-full border border-black/[0.08] px-2.5 py-0.5 text-[11.5px] leading-normal font-normal whitespace-nowrap shadow-[0_4px_14px_rgba(0,0,0,0.08)] select-none dark:border-white/[0.11] dark:shadow-[0_4px_16px_rgba(0,0,0,0.36)]",
                tabularNums && "tabular-nums px-3 py-1 text-[12px]",
              )}
            >
              {label}
            </div>
          </div>,
          document.body,
        )}
    </>
  );
}

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
    <HoverPillTooltip
      label={copied ? "Copied" : label}
      delay={240}
      renderTrigger={(handlers) => (
        <button
          type="button"
          aria-label={copied ? "Copied" : label}
          onClick={onClick}
          {...handlers}
          className={cn(
            "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
            className,
          )}
        >
          <Copy
            className={cn(
              "size-[13px] stroke-[1.8] transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)]",
              copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100",
            )}
          />
          <Check
            className={cn(
              "absolute size-[13px] stroke-[2] text-emerald-600 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] dark:text-emerald-400",
              copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0",
            )}
          />
        </button>
      )}
    />
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
    <HoverPillTooltip
      label={label}
      delay={240}
      renderTrigger={(handlers) => (
        <button
          type="button"
          aria-label={label}
          onClick={onClick}
          {...handlers}
          className={cn(
            "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
            className,
          )}
        >
          <Undo2 className="size-[13px] stroke-[1.8]" />
        </button>
      )}
    />
  );
}

/**
 * Restrained Fork icon button matching `CopyActionButton` / `UndoActionButton` (`13px` icon in `22×22px` hit target).
 * Directly forks in place for non-git directories; opens a tiny 2-option popover when worktree branching is available.
 */
function ForkActionButton({
  canUseWorktree,
  onFork,
  align = "start",
  className,
}: {
  canUseWorktree: boolean;
  onFork: (worktree: boolean) => void;
  align?: "start" | "end";
  className?: string;
}) {
  const [menuOpen, setMenuOpen] = useState(false);

  if (!canUseWorktree) {
    return (
      <HoverPillTooltip
        label="Fork from here"
        delay={240}
        renderTrigger={(handlers) => (
          <button
            type="button"
            aria-label="Fork from here"
            onClick={() => onFork(false)}
            {...handlers}
            className={cn(
              "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
              className,
            )}
          >
            <GitFork className="size-[13px] stroke-[1.8]" />
          </button>
        )}
      />
    );
  }

  return (
    <DropdownMenu open={menuOpen} onOpenChange={setMenuOpen}>
      <HoverPillTooltip
        label={menuOpen ? "" : "Fork from here"}
        delay={240}
        renderTrigger={(handlers) => (
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              aria-label="Fork from here"
              {...handlers}
              className={cn(
                "text-muted-foreground hover:text-foreground hover:bg-accent/65 orbit-press relative inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md",
                menuOpen && "bg-accent/65 text-foreground",
                className,
              )}
            >
              <GitFork className="size-[13px] stroke-[1.8]" />
            </button>
          </DropdownMenuTrigger>
        )}
      />
      <DropdownMenuContent
        align={align}
        sideOffset={6}
        className="w-max min-w-[168px] rounded-[11px] p-1"
      >
        <DropdownMenuItem
          onSelect={() => onFork(false)}
          className="cursor-pointer rounded-[8px] px-2.5 py-1.5 text-[12.5px]"
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <GitFork className="text-muted-foreground size-[13px] shrink-0 stroke-[1.8]" />
            <span>Fork here</span>
          </span>
        </DropdownMenuItem>
        <DropdownMenuItem
          onSelect={() => onFork(true)}
          className="cursor-pointer rounded-[8px] px-2.5 py-1.5 text-[12.5px]"
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <GitBranch className="text-muted-foreground size-[13px] shrink-0 stroke-[1.8]" />
            <span>Fork in new worktree</span>
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/** Super subtle, restrained 1-line provenance chip at the top of a forked thread. */
const ForkedFromBanner = memo(function ForkedFromBanner({
  hostId,
  agent,
}: {
  hostId: string;
  agent: AgentCardView | null;
}) {
  const sourceId = agent?.forked_from_agent_id ?? null;
  const parentAgent = useOrbitSelector((s) =>
    sourceId
      ? (s.hostDetails[hostId]?.agents.find((a) => a.agent_id === sourceId) ?? null)
      : null,
  );
  const customTitle = useOrbitSelector((s) =>
    sourceId ? s.threadCustomTitles[`${hostId}::${sourceId}`] : undefined,
  );
  const isLocallyArchived = useOrbitSelector((s) =>
    sourceId ? Boolean(s.archivedThreads[`${hostId}::${sourceId}`]) : false,
  );

  if (!sourceId) return null;

  const parentTitle =
    customTitle?.trim() ||
    parentAgent?.title?.trim() ||
    (sourceId.length > 14 ? `${sourceId.slice(0, 12)}…` : sourceId);
  const canJump = Boolean(
    parentAgent && !parentAgent.archived_at_rfc3339 && !isLocallyArchived,
  );

  return (
    <div className="mx-auto flex w-full max-w-[var(--orbit-stage-max-w,768px)] items-center pt-2 pb-0.5">
      {canJump ? (
        <button
          type="button"
          onClick={() => orbitActions.selectAgent(hostId, sourceId)}
          className="text-muted-foreground/85 hover:text-foreground border-border/65 bg-muted/35 hover:bg-muted/65 inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11.5px] leading-tight transition-colors"
        >
          <GitFork className="size-3 shrink-0 stroke-[1.75] opacity-75" />
          <span className="truncate">
            Forked from <span className="text-foreground/85 font-medium">{parentTitle}</span>
          </span>
        </button>
      ) : (
        <span className="text-muted-foreground/75 border-border/55 bg-muted/25 inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[11.5px] leading-tight">
          <GitFork className="size-3 shrink-0 stroke-[1.75] opacity-65" />
          <span className="truncate">Forked from {parentTitle}</span>
        </span>
      )}
    </div>
  );
});

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
      if (isImeKeyEvent(e)) return;
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
                  "max-h-[216px] space-y-1.5 overflow-y-auto pr-3.5 transition-opacity duration-150",
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
                  className="pointer-events-none absolute right-3.5 bottom-0 left-0 h-7"
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
  const nowMs = useSharedRelativeNow(timestampMs);
  const relativeText = formatRelativeTimeLabel(timestampMs, nowMs);
  const fullText = formatFullTimestampTooltip(timestampMs);

  return (
    <HoverPillTooltip
      label={fullText}
      delay={140}
      tabularNums
      renderTrigger={(handlers) => (
        <span
          {...handlers}
          className="text-muted-foreground hover:text-foreground/85 tabular-nums inline-flex cursor-default items-center rounded px-1 py-0.5 text-[12px] leading-none transition-colors select-none"
        >
          {relativeText}
        </span>
      )}
    />
  );
});

/** Isolated 1-second live elapsed ticker with offline & silent-stall hints. */
const InlineLiveElapsed = memo(function InlineLiveElapsed({
  startedAtMs,
  lastActivityMs,
  isNetworkOffline = false,
  stallLabel = "waiting for response",
}: {
  startedAtMs: number;
  lastActivityMs?: number | null;
  isNetworkOffline?: boolean;
  stallLabel?: string | null;
}) {
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [startedAtMs]);

  const elapsedMs = Math.max(0, nowMs - startedAtMs);
  const silentMs = Math.max(0, nowMs - (lastActivityMs ?? startedAtMs));
  const isStalled = !isNetworkOffline && stallLabel !== null && silentMs >= 45_000;

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
          · {stallLabel}
        </span>
      ) : null}
    </>
  );
});

function resolveTurnForkSeq(turn: StageTurnView): number | null {
  let maxSeq = 0;
  const record = (seq: number | undefined | null) => {
    if (typeof seq === "number" && seq > maxSeq) maxSeq = seq;
  };
  record(turn.anchor_seq);
  record(turn.user_prompt?.seq);
  record(turn.agent_message?.seq);
  record(turn.terminal_reply?.seq);
  record(turn.last_assistant_reply?.seq);
  for (const item of turn.folded_steps) record(item.seq);
  for (const item of turn.notices) record(item.seq);
  for (const item of turn.compactions) record(item.seq);
  for (const seg of turn.live_segments) {
    if (seg.kind === "step_group") {
      for (const item of seg.items) record(item.seq);
    } else if (seg.kind === "assistant_message" || seg.kind === "notice") {
      record(seg.item.seq);
    }
  }
  return maxSeq > 0 ? maxSeq : null;
}

/**
 * Completed Turn Footer (`[Copy · Fork · 2m ago]`):
 * Hidden while a turn is active since live elapsed duration lives inline in `TurnActivityDisclosure`.
 */
const TurnFooter = memo(function TurnFooter({
  isTurnActive,
  timing,
  completedAtMs,
  replyText,
  forkUpToSeq,
  isLastTurn = false,
  canForkWorktree = false,
  onRequestFork,
}: {
  isTurnActive: boolean;
  timing?: TurnTimingRecord;
  completedAtMs: number | null;
  replyText: string | null;
  forkUpToSeq?: number | null;
  isLastTurn?: boolean;
  canForkWorktree?: boolean;
  onRequestFork?: (upToSeq: number | null, worktree: boolean) => void;
}) {
  const [copied, setCopied] = useState(false);

  const resolvedCompletedAtMs = timing?.completedAtMs ?? completedAtMs;
  const showFork = Boolean(onRequestFork && (isLastTurn || (forkUpToSeq !== null && forkUpToSeq !== undefined && forkUpToSeq > 0)));

  if (isTurnActive || (!replyText && resolvedCompletedAtMs === null && !showFork)) {
    return null;
  }

  const handleCopy = async () => {
    if (!replyText) return;
    await navigator.clipboard.writeText(replyText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  return (
    <div className="mt-0.5 flex h-[22px] items-center gap-1">
      {replyText && (
        <CopyActionButton
          copied={copied}
          label="Copy"
          onClick={() => void handleCopy()}
          className="-ml-1"
        />
      )}

      {showFork && onRequestFork && (
        <ForkActionButton
          canUseWorktree={canForkWorktree}
          align="start"
          onFork={(worktree) => onRequestFork(isLastTurn ? null : (forkUpToSeq ?? null), worktree)}
          className={replyText ? undefined : "-ml-1"}
        />
      )}

      {resolvedCompletedAtMs !== null && (
        <RelativeTimestampPill timestampMs={resolvedCompletedAtMs} />
      )}
    </div>
  );
});

/**
 * Returns true when a `wakeup` segment represents a transient "waiting for background task(s)"
 * state rather than a permanent timeline milestone.
 */
function isWaitingWakeupLabel(label: string | undefined | null): boolean {
  return Boolean(label && /^waiting for\b/i.test(label.trim()));
}

/**
 * Returns true when a `wakeup` segment should be hidden from the main conversation stream:
 * both transient "Waiting for ..." states (surfaced in Overview) and internal
 * "Resumed from background task" runtime scheduling transitions (which mature agents
 * fold silently into the normal continuation of the turn).
 */
function isHiddenWakeupLabel(label: string | undefined | null): boolean {
  if (!label) return true;
  const trimmed = label.trim();
  return (
    /^waiting for\b/i.test(trimmed) ||
    /^resumed from background task\b/i.test(trimmed)
  );
}

const CompactionMilestoneRow = memo(function CompactionMilestoneRow({
  label,
}: {
  label?: string;
}) {
  return (
    <div className="py-0.5 select-none">
      <span className="text-muted-foreground/85 border-border/75 bg-muted/45 dark:bg-muted/30 inline-flex max-w-full items-center gap-1.5 rounded-full border px-2.5 py-0.5 text-[12px] font-normal">
        <Scissors className="size-3 shrink-0 opacity-75" />
        <span className="truncate">{label || "Context compacted"}</span>
      </span>
    </div>
  );
});

const WakeupEventRow = memo(function WakeupEventRow({ label }: { label?: string }) {
  return (
    <div className="text-muted-foreground/80 flex items-center gap-1.5 py-0.5 text-[12.5px] font-normal select-none">
      <RotateCw className="size-3 shrink-0 opacity-75" />
      <span className="truncate">{label || "Resumed from background task"}</span>
    </div>
  );
});

const EMPTY_TIMELINE_ITEMS: TimelineItemView[] = [];
const EMPTY_TURNS: StageTurnView[] = [];
const EMPTY_DIFF_FILES: DiffFileView[] = [];
const EMPTY_INSIGHTS: SessionInsightsView = {
  touched_files: [],
  background_tasks: [],
  subagents: [],
  detected_local_urls: [],
};

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
  turn: StageTurnView,
  agentId: string,
  thinkingTimings: Record<string, TurnTimingRecord>,
): string {
  let sig = "";
  for (const seg of turn.live_segments) {
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

const EMPTY_WORK_COUNTS: WorkSummaryResultView["counts"] = {
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
interface InlineArtifactRef {
  key: string;
  artifactId?: string;
  title: string;
  version?: number;
  kindLabel?: string;
}

function parseArtifactRefsFromMessage(
  text: string,
  knownArtifacts: readonly ArtifactSummary[],
): InlineArtifactRef[] {
  const directives = extractInlineArtifactDirectives(text);
  if (directives.length === 0) {
    return [];
  }
  const refs: InlineArtifactRef[] = [];
  const seen = new Set<string>();
  for (const dir of directives) {
    const explicitId = dir.id;
    const rawTitle = dir.title;
    const rawUri = dir.resourceUri;

    const matched =
      (explicitId ? knownArtifacts.find((a) => a.artifact_id === explicitId) : undefined) ??
      (rawUri ? knownArtifacts.find((a) => a.resource_uri === rawUri) : undefined) ??
      (rawTitle
        ? knownArtifacts.find((a) => a.title.toLowerCase() === rawTitle.toLowerCase())
        : undefined);

    const key = matched?.artifact_id || explicitId || rawTitle || rawUri || `idx_${dir.index}`;
    if (seen.has(key)) continue;
    seen.add(key);

    refs.push({
      key,
      artifactId: matched?.artifact_id ?? explicitId,
      title: matched?.title ?? rawTitle ?? "Artifact",
      version: matched?.version,
      kindLabel: matched?.kind,
    });
  }
  return refs;
}

function parseArtifactRefsFromTurnTools(
  turn: StageTurnView,
  knownArtifacts: readonly ArtifactSummary[],
): InlineArtifactRef[] {
  const refs: InlineArtifactRef[] = [];
  const seen = new Set<string>();
  const inspectItems = (items: readonly TimelineItemView[]) => {
    for (const item of items) {
      if (item.item_type !== "tool_call") continue;
      if (item.detail.kind !== "generic") continue;
      const toolName = item.detail.tool_name.toLowerCase();
      if (!toolName.endsWith("publish_artifact")) continue;
      try {
        const parsed = JSON.parse(item.detail.arguments_json) as Record<string, unknown>;
        const explicitId =
          typeof parsed.artifact_id === "string" && parsed.artifact_id.trim()
            ? parsed.artifact_id.trim()
            : undefined;
        const rawTitle =
          typeof parsed.title === "string" && parsed.title.trim()
            ? parsed.title.trim()
            : undefined;
        const matched =
          (explicitId ? knownArtifacts.find((a) => a.artifact_id === explicitId) : undefined) ??
          (rawTitle
            ? knownArtifacts.find((a) => a.title.toLowerCase() === rawTitle.toLowerCase())
            : undefined);
        const key = matched?.artifact_id || explicitId || rawTitle || item.call_id;
        if (seen.has(key)) continue;
        seen.add(key);
        refs.push({
          key,
          artifactId: matched?.artifact_id ?? explicitId,
          title: matched?.title ?? rawTitle ?? "Artifact",
          version: matched?.version,
          kindLabel: matched?.kind ?? (typeof parsed.kind === "string" ? parsed.kind : undefined),
        });
      } catch {
        // Ignore unparseable partial tool arguments while streaming
      }
    }
  };
  inspectItems(turn.folded_steps);
  for (const seg of turn.live_segments) {
    if (seg.kind === "step_group") {
      inspectItems(seg.items);
    }
  }
  return refs;
}

const InlineArtifactCard = memo(function InlineArtifactCard({
  hostId,
  artifact,
}: {
  hostId: string;
  artifact: InlineArtifactRef;
}) {
  return (
    <button
      type="button"
      onClick={() =>
        orbitActions.openArtifactsTab({
          hostId,
          artifactId: artifact.artifactId,
        })
      }
      className="border-border/80 bg-elevated/45 hover:bg-elevated/85 group inline-flex max-w-full cursor-pointer items-center gap-2 rounded-xl border px-3 py-1.5 text-left text-[12.5px] transition-colors"
    >
      <Blocks className="text-muted-foreground group-hover:text-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
      <span className="text-muted-foreground shrink-0 text-[11.5px]">Artifact</span>
      <span className="text-muted-foreground/55 shrink-0">·</span>
      <span className="text-foreground truncate font-medium">{artifact.title}</span>
      {artifact.version != null && (
        <>
          <span className="text-muted-foreground/55 shrink-0">·</span>
          <span className="text-muted-foreground shrink-0 font-mono text-[11.5px]">
            v{artifact.version}
          </span>
        </>
      )}
      <ChevronRight className="text-muted-foreground/55 group-hover:text-foreground h-[13px] w-[13px] shrink-0 transition-transform group-hover:translate-x-0.5" />
    </button>
  );
});

const EMPTY_ARTIFACTS: readonly ArtifactSummary[] = [];

export const TurnStageSection = memo(function TurnStageSection({
  turn,
  isTurnActive,
  isLastTurn = false,
  agentStatusLabel = null,
  hasLiveBackgroundTasks = false,
  isNetworkOffline,
  animatePromptEntrance = false,
  autoFoldCompleted,
  hostId,
  cwd,
  agentId,
  timing,
  thinkingSignature: _thinkingSignature,
  getThinkingDurationMs,
  holdViewport,
  onRequestRewind,
  onRequestFork,
  canForkWorktree = false,
  onPreviewImage,
}: {
  turn: StageTurnView;
  isTurnActive: boolean;
  isLastTurn?: boolean;
  agentStatusLabel?: string | null;
  hasLiveBackgroundTasks?: boolean;
  isNetworkOffline: boolean;
  animatePromptEntrance?: boolean;
  autoFoldCompleted: boolean;
  hostId: string;
  cwd: string;
  agentId: string;
  timing?: TurnTimingRecord;
  thinkingSignature: string;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
  onRequestRewind: (seq: number, promptText: string) => void;
  onRequestFork?: (upToSeq: number | null, worktree: boolean) => void;
  canForkWorktree?: boolean;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
}) {
  const rawSegments = turn.live_segments;
  const hostArtifacts = useOrbitSelector(
    (s) => s.hostDetails[hostId]?.artifacts ?? EMPTY_ARTIFACTS,
  );
  const turnToolArtifactRefs = useMemo(
    () => parseArtifactRefsFromTurnTools(turn, hostArtifacts),
    [turn, hostArtifacts],
  );
  const lastRawSeg = rawSegments[rawSegments.length - 1] ?? null;

  // Exclude transient `Waiting for ...` and internal `Resumed from background task` wakeup rows
  // from permanent timeline history, and collapse consecutive duplicate external wakeup events.
  const segments = useMemo(() => {
    const out: typeof rawSegments = [];
    for (const seg of rawSegments) {
      if (seg.kind === "wakeup") {
        if (isHiddenWakeupLabel(seg.label)) continue;
        const prev = out[out.length - 1];
        if (prev?.kind === "wakeup" && prev.label === seg.label) continue;
      }
      out.push(seg);
    }
    return out;
  }, [rawSegments]);

  if (
    turn.is_compaction_command &&
    turn.folded_steps.length === 0 &&
    turn.terminal_reply === null &&
    turn.notices.length === 0
  ) {
    return (
      <div className="min-w-0 px-[10px]">
        <CompactionMilestoneRow label={turn.compactions[0]?.label || "Context compacted"} />
      </div>
    );
  }

  const isWaitingOnlyOnBackgroundTasks =
    hasLiveBackgroundTasks &&
    ((lastRawSeg?.kind === "wakeup" && isWaitingWakeupLabel(lastRawSeg.label)) ||
      (Boolean(agentStatusLabel && /background task/i.test(agentStatusLabel)) &&
        (lastRawSeg === null ||
          (lastRawSeg.kind === "assistant_message" &&
            lastRawSeg.item.item_type === "assistant_message" &&
            !lastRawSeg.item.streaming))));

  // Show a lightweight trailing live indicator when the turn is active, but the last rendered
  // segment is already settled (unless `LiveBackgroundTasksBar` already owns the waiting state).
  const needsTrailingActivity =
    isTurnActive &&
    !isWaitingOnlyOnBackgroundTasks &&
    (lastRawSeg === null ||
      (lastRawSeg.kind === "assistant_message" &&
        lastRawSeg.item.item_type === "assistant_message" &&
        !lastRawSeg.item.streaming) ||
      lastRawSeg.kind === "notice" ||
      lastRawSeg.kind === "wakeup" ||
      lastRawSeg.kind === "compaction");

  const hasBody = segments.length > 0 || needsTrailingActivity;
  const liveStartedAtMs = timing?.startedAtMs ?? turn.started_at_ms ?? null;
  const promptItem =
    turn.user_prompt?.item_type === "user_prompt" ? turn.user_prompt : null;
  const lastReplyText =
    !isTurnActive && turn.last_assistant_reply?.item_type === "assistant_message"
      ? turn.last_assistant_reply.text
      : null;
  const turnForkSeq = useMemo(() => resolveTurnForkSeq(turn), [turn]);

  return (
    <div
      style={!isTurnActive ? { contain: "layout style" } : undefined}
      className="min-w-0 space-y-4"
    >
      {promptItem && (
        <UserPromptBubble
          item={promptItem}
          hostId={hostId}
          agentId={agentId}
          cwd={cwd}
          holdViewport={holdViewport}
          animateEntrance={animatePromptEntrance}
          onRequestRewind={onRequestRewind}
          onPreviewImage={onPreviewImage}
        />
      )}

      {turn.agent_message?.item_type === "agent_message" && (
        <AgentMessageCard hostId={hostId} item={turn.agent_message} />
      )}

      {hasBody && (
        <div className="min-w-0 px-[10px]">
          <div className="min-w-0 space-y-3">
            {segments.map((seg, idx) => {
              const isLastSeg = idx === segments.length - 1;

              if (seg.kind === "compaction") {
                return <CompactionMilestoneRow key={seg.key} label={seg.label} />;
              }

              if (seg.kind === "wakeup") {
                return <WakeupEventRow key={seg.key} label={seg.label} />;
              }

              if (seg.kind === "notice") {
                const n = seg.item;
                if (n.item_type !== "notice") return null;
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
                if (seg.item.item_type !== "assistant_message") return null;
                const msgArtifacts = !seg.item.streaming
                  ? parseArtifactRefsFromMessage(seg.item.text, hostArtifacts)
                  : [];
                return (
                  <div key={seg.key} className="space-y-2">
                    <MarkdownLinkProvider cwd={cwd}>
                      <MarkdownBlocksView
                        blocks={seg.item.blocks}
                        streaming={seg.item.streaming}
                      />
                    </MarkdownLinkProvider>
                    {msgArtifacts.length > 0 && (
                      <div className="flex flex-wrap items-center gap-2 pt-0.5">
                        {msgArtifacts.map((ref) => (
                          <InlineArtifactCard key={ref.key} hostId={hostId} artifact={ref} />
                        ))}
                      </div>
                    )}
                  </div>
                );
              }

              // `step_group`
              // Never render an already-failed trailing segment (with no running step) as active
              // `Running ... waiting for response` — keep its failure visible while any new turn work
              // shows cleanly in the trailing indicator below.
              const isTrailingFailedGroup =
                !seg.has_running_step && seg.summary.counts.unrecovered_failure_count > 0;
              const hasRunningBgTask = seg.items.some(
                (it) =>
                  it.item_type === "tool_call" &&
                  it.status === "running" &&
                  Boolean(it.background_task),
              );
              const hasRunningNonBgTool = seg.items.some(
                (it) =>
                  it.item_type === "tool_call" &&
                  it.status === "running" &&
                  !it.background_task,
              );
              const isBackgroundWait = hasRunningBgTask && !hasRunningNonBgTool;
              // Once prose, a notice, or a wakeup divider follows a step group, settle the
              // pre-reply disclosure cleanly into its concise summary instead of keeping an
              // extra shimmering `Running ... · running in background` line hovering above
              // the reply (active background tasks stay visible in the trailing wait status
              // and the Overview `Background Tasks` list).
              const isGroupActive =
                isTurnActive &&
                !seg.has_following_content &&
                (seg.has_running_step || (isLastSeg && !isTrailingFailedGroup));
              const groupThoughtMs =
                computeTotalThinkingDurationMs(seg.items, agentId, getThinkingDurationMs) ??
                seg.thought_duration_ms ??
                null;
              const groupSettledLabel =
                seg.summary.counts.action_steps === 0 && seg.summary.counts.thought_count > 0
                  ? formatThoughtSummaryLabel(groupThoughtMs, seg.summary.counts.thought_count)
                  : seg.summary.label;
              const groupSettledDurationMs =
                seg.work_duration_ms ??
                (segments.length === 1
                  ? (timing?.workDurationMs ?? turn.work_duration_ms ?? timing?.durationMs ?? null)
                  : null);
              const groupLiveStartMs = seg.started_at_ms ?? liveStartedAtMs;

              return (
                <TurnActivityDisclosure
                  key={seg.key}
                  items={seg.items}
                  isWorkActive={isGroupActive}
                  liveLabel={seg.summary.live_label}
                  liveDetail={seg.summary.live_detail ?? null}
                  settledLabel={groupSettledLabel}
                  summaryCounts={seg.summary.counts}
                  labelIncludesFailure={seg.summary.label_includes_failure}
                  defaultCollapsed={autoFoldCompleted}
                  autoOpenForError={
                    !isGroupActive &&
                    !seg.has_following_content &&
                    seg.summary.counts.unrecovered_failure_count > 0
                  }
                  liveStartedAtMs={groupLiveStartMs}
                  latestActivityMs={seg.latest_activity_ms ?? groupLiveStartMs}
                  isNetworkOffline={isNetworkOffline}
                  isBackgroundWait={isBackgroundWait}
                  settledDurationMs={groupSettledDurationMs}
                  cwd={cwd}
                  agentId={agentId}
                  thinkingSignature={_thinkingSignature}
                  getThinkingDurationMs={getThinkingDurationMs}
                  holdViewport={holdViewport}
                />
              );
            })}

            {turnToolArtifactRefs.length > 0 && (
              <div className="flex flex-wrap items-center gap-2 pt-0.5">
                {turnToolArtifactRefs.map((ref) => (
                  <InlineArtifactCard key={ref.key} hostId={hostId} artifact={ref} />
                ))}
              </div>
            )}

            {needsTrailingActivity && (() => {
              const trailingLabel =
                lastRawSeg === null
                  ? "Working…"
                  : lastRawSeg.kind === "wakeup" &&
                      !isHiddenWakeupLabel(lastRawSeg.label) &&
                      lastRawSeg.label.trim()
                    ? lastRawSeg.label.trim().endsWith("…")
                      ? lastRawSeg.label.trim()
                      : `${lastRawSeg.label.trim()}…`
                    : agentStatusLabel &&
                        /waking/i.test(agentStatusLabel) &&
                        !/background[_\s]task/i.test(agentStatusLabel)
                      ? agentStatusLabel.trim().endsWith("…")
                        ? agentStatusLabel.trim()
                        : `${agentStatusLabel.trim()}…`
                      : "Thinking…";
              const isTrailingBackgroundWait = /background task/i.test(trailingLabel);
              return (
                <TurnActivityDisclosure
                  items={EMPTY_TIMELINE_ITEMS}
                  isWorkActive
                  liveLabel={trailingLabel}
                  liveDetail={null}
                  settledLabel=""
                  summaryCounts={EMPTY_WORK_COUNTS}
                  labelIncludesFailure={false}
                  defaultCollapsed={autoFoldCompleted}
                  autoOpenForError={false}
                  liveStartedAtMs={liveStartedAtMs}
                  latestActivityMs={turn.completed_at_ms ?? liveStartedAtMs}
                  isNetworkOffline={isNetworkOffline}
                  isBackgroundWait={isTrailingBackgroundWait}
                  settledDurationMs={null}
                  cwd={cwd}
                  agentId={agentId}
                  thinkingSignature={_thinkingSignature}
                  getThinkingDurationMs={getThinkingDurationMs}
                  holdViewport={holdViewport}
                />
              );
            })()}

            {!isTurnActive && <TurnChangesBar diffs={turn.turn_diffs} cwd={cwd} />}

            <TurnFooter
              isTurnActive={isTurnActive}
              timing={timing}
              completedAtMs={turn.completed_at_ms}
              replyText={lastReplyText}
              forkUpToSeq={turnForkSeq}
              isLastTurn={isLastTurn}
              canForkWorktree={canForkWorktree}
              onRequestFork={onRequestFork}
            />
          </div>
        </div>
      )}
    </div>
  );
}, (prev, next) =>
  prev.turn === next.turn &&
  prev.isTurnActive === next.isTurnActive &&
  prev.isLastTurn === next.isLastTurn &&
  prev.agentStatusLabel === next.agentStatusLabel &&
  prev.hasLiveBackgroundTasks === next.hasLiveBackgroundTasks &&
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
  prev.onRequestRewind === next.onRequestRewind &&
  prev.onRequestFork === next.onRequestFork &&
  prev.canForkWorktree === next.canForkWorktree,
);

function NewSessionLaunchpad() {
  return (
    <div className="orbit-stage-enter flex h-full w-full flex-col justify-center px-[24px] py-12 pb-[10vh]">
      <div className="mx-auto w-full max-w-[var(--orbit-stage-max-w,768px)] space-y-6">
        <h1 className="text-foreground/90 px-1 text-center text-[26px] font-normal tracking-[-0.018em]">
          What would you like to build?
        </h1>

        <OmniComposer variant="hero" />
      </div>
    </div>
  );
}

interface StageScrollUiSnapshot {
  isOccludingText: boolean;
  showJumpLatest: boolean;
  activePromptSeq: number | null;
}

interface StageScrollUiStore {
  getSnapshot: () => StageScrollUiSnapshot;
  subscribe: (listener: () => void) => () => void;
  update: (patch: Partial<StageScrollUiSnapshot>) => void;
}

function createStageScrollUiStore(): StageScrollUiStore {
  let snap: StageScrollUiSnapshot = {
    isOccludingText: false,
    showJumpLatest: false,
    activePromptSeq: null,
  };
  const listeners = new Set<() => void>();
  return {
    getSnapshot: () => snap,
    subscribe: (listener) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    update: (patch) => {
      const nextOcclusion = patch.isOccludingText ?? snap.isOccludingText;
      const nextJump = patch.showJumpLatest ?? snap.showJumpLatest;
      const nextActive =
        patch.activePromptSeq !== undefined ? patch.activePromptSeq : snap.activePromptSeq;
      if (
        nextOcclusion === snap.isOccludingText &&
        nextJump === snap.showJumpLatest &&
        nextActive === snap.activePromptSeq
      ) {
        return;
      }
      snap = {
        isOccludingText: nextOcclusion,
        showJumpLatest: nextJump,
        activePromptSeq: nextActive,
      };
      for (const listener of listeners) {
        listener();
      }
    },
  };
}

interface OutlinePromptWithIndex {
  seq: number;
  turnIndex: number;
  preview: string;
}

const StageOutlineRailBridge = memo(function StageOutlineRailBridge({
  scrollUiStore,
  prompts,
  onJumpToSeq,
  visible,
  offsetPx,
  offsetTransition,
  isAnimating,
}: {
  scrollUiStore: StageScrollUiStore;
  prompts: OutlinePromptWithIndex[];
  onJumpToSeq: (seq: number) => void;
  visible: boolean;
  offsetPx: number;
  offsetTransition: string;
  isAnimating: boolean;
}) {
  const activePromptSeq = useSyncExternalStore(
    scrollUiStore.subscribe,
    () => scrollUiStore.getSnapshot().activePromptSeq,
    () => scrollUiStore.getSnapshot().activePromptSeq,
  );
  return (
    <ChatOutlineRail
      prompts={prompts}
      activeSeq={activePromptSeq ?? prompts[prompts.length - 1]?.seq ?? null}
      onJumpToSeq={onJumpToSeq}
      visible={visible}
      offsetPx={offsetPx}
      offsetTransition={offsetTransition}
      isAnimating={isAnimating}
    />
  );
});

const StageJumpLatestBanner = memo(function StageJumpLatestBanner({
  scrollUiStore,
  stagePushStyle,
  dockedOverviewLayout,
  onJumpLatest,
}: {
  scrollUiStore: StageScrollUiStore;
  stagePushStyle: CSSProperties;
  dockedOverviewLayout: boolean;
  onJumpLatest: () => void;
}) {
  const snap = useSyncExternalStore(
    scrollUiStore.subscribe,
    scrollUiStore.getSnapshot,
    scrollUiStore.getSnapshot,
  );
  return (
    <div className="pointer-events-none absolute inset-x-0 bottom-full h-7">
      <div
        className={cn(
          "absolute inset-y-0 left-0 transition-opacity duration-150",
          dockedOverviewLayout ? "right-[324px]" : "right-5",
          snap.isOccludingText ? "opacity-100" : "opacity-0",
        )}
        style={{
          background:
            "linear-gradient(to top, var(--surface-canvas) 0%, color-mix(in oklab, var(--surface-canvas) 66%, transparent) 48%, transparent 100%)",
        }}
      />
      <div
        style={stagePushStyle}
        className={cn(
          "pointer-events-none absolute inset-y-0 left-0",
          dockedOverviewLayout ? "right-[324px]" : "right-0",
        )}
      >
        <button
          type="button"
          aria-label="Scroll to latest"
          aria-hidden={!snap.showJumpLatest}
          tabIndex={snap.showJumpLatest ? 0 : -1}
          onClick={onJumpLatest}
          className={cn(
            "border-border bg-elevated text-muted-foreground hover:text-foreground hover:border-foreground/20 absolute bottom-2 left-1/2 inline-flex size-9.5 -translate-x-1/2 cursor-pointer items-center justify-center rounded-full border shadow-[0_2px_8px_rgba(0,0,0,0.08)] transition-[transform,opacity,color,border-color] duration-200 ease-[var(--ease-out-expo)] dark:shadow-[0_2px_12px_rgba(0,0,0,0.36)]",
            snap.showJumpLatest
              ? "pointer-events-auto translate-y-0 scale-100 opacity-100"
              : "pointer-events-none translate-y-1 scale-95 opacity-0",
          )}
        >
          <ArrowDown className="size-[18px] stroke-[1.9]" />
        </button>
      </div>
    </div>
  );
});

const StageDockedComposer = memo(function StageDockedComposer({
  scrollUiStore,
}: {
  scrollUiStore: StageScrollUiStore;
}) {
  const isOccludingText = useSyncExternalStore(
    scrollUiStore.subscribe,
    () => scrollUiStore.getSnapshot().isOccludingText,
    () => scrollUiStore.getSnapshot().isOccludingText,
  );
  return <OmniComposer variant="docked" occluding={isOccludingText} />;
});

const SESSION_TURN_HEIGHT_CACHE_LIMIT = 1000;
const sessionTurnHeightCache = new Map<string, number>();

function getCachedTurnHeight(key: string): number | undefined {
  return sessionTurnHeightCache.get(key);
}

function setCachedTurnHeight(key: string, height: number): void {
  if (height <= 0 || !Number.isFinite(height)) return;
  if (sessionTurnHeightCache.has(key)) {
    sessionTurnHeightCache.delete(key);
  } else if (sessionTurnHeightCache.size >= SESSION_TURN_HEIGHT_CACHE_LIMIT) {
    const oldest = sessionTurnHeightCache.keys().next().value;
    if (oldest !== undefined) sessionTurnHeightCache.delete(oldest);
  }
  sessionTurnHeightCache.set(key, height);
}

const TURN_LIST_GAP_PX = 32;
const IDLE_LOOKAHEAD_UP_PX = 2400;
const IDLE_LOOKAHEAD_DOWN_PX = 1400;
const SCROLL_LEADING_PX = 3600;
const SCROLL_FAST_LEADING_PX = 5200;
const SCROLL_TRAILING_PX = 1200;
const FAST_FLICK_PX_PER_SEC = 2200;
const MAX_OVERSCAN_ITEMS_PER_SIDE = 24;
const WARM_RESTING_CAP = 36;
const WARM_SCROLLING_CAP = 52;
const IDLE_PREWARM_BATCH = 2;

function spanCharLen(spans: InlineSpanView[]): number {
  let len = 0;
  for (const s of spans) {
    len += s.text.length;
  }
  return len;
}

function estimateRichBlockHeight(block: RichBlockView): number {
  switch (block.kind) {
    case "code_block":
      return Math.min(1400, Math.max(84, block.lines.length * 21 + 52));
    case "table":
      return Math.min(1200, Math.max(96, (block.rows.length + 1) * 38 + 24));
    case "heading":
      return 42;
    case "list_item":
      return Math.max(28, Math.ceil(spanCharLen(block.spans) / 68) * 24 + 8);
    case "paragraph":
      return Math.max(34, Math.ceil(spanCharLen(block.spans) / 72) * 25 + 14);
    case "block_quote":
      return Math.max(44, Math.ceil(spanCharLen(block.spans) / 66) * 24 + 16);
    case "horizontal_rule":
      return 28;
  }
}

function estimateTurnSizeFallback(turn: StageTurnView | undefined): number {
  if (!turn) return 180;
  let est = 24;
  if (turn.user_prompt?.item_type === "user_prompt") {
    const promptLen = turn.user_prompt.text?.length ?? 0;
    const badges = turn.user_prompt.badges ?? [];
    const imageCount = badges.filter((b) => b.kind === "image").length;
    const otherBadgeCount = badges.length - imageCount;
    est =
      56 +
      Math.min(124, Math.ceil(promptLen / 56) * 24) +
      (otherBadgeCount > 0 ? 28 : 0) +
      (imageCount > 0 ? Math.min(204, Math.ceil(imageCount / 5) * 102) : 0);
  }
  if (turn.agent_message?.item_type === "agent_message") {
    est += 110;
  }
  for (const seg of turn.live_segments) {
    if (seg.kind === "assistant_message" && seg.item.item_type === "assistant_message") {
      if (seg.item.blocks.length === 0) {
        est += 48;
      } else {
        let blockSum = 16;
        for (const block of seg.item.blocks) {
          blockSum += estimateRichBlockHeight(block);
        }
        est += blockSum;
      }
    } else if (seg.kind === "step_group") {
      est += 34;
    } else if (seg.kind === "wakeup") {
      if (!isHiddenWakeupLabel(seg.label)) {
        est += 28;
      }
    } else if (seg.kind === "compaction") {
      est += 32;
    } else {
      est += 44;
    }
  }
  if (turn.turn_diffs.length > 0) est += 38;
  return Math.max(72, Math.min(5600, est));
}

interface ScrollVelocityState {
  offset: number;
  ts: number;
  dir: "up" | "down" | null;
  fast: boolean;
  wasScrolling: boolean;
}

function findTurnIndexForSeq(
  turns: StageTurnView[],
  targetSeq: number,
): { index: number; exact: boolean } {
  if (turns.length === 0) return { index: -1, exact: false };
  for (let i = 0; i < turns.length; i += 1) {
    const t = turns[i];
    if (
      t.anchor_seq === targetSeq ||
      t.user_prompt?.seq === targetSeq ||
      t.agent_message?.seq === targetSeq ||
      t.terminal_reply?.seq === targetSeq ||
      t.last_assistant_reply?.seq === targetSeq ||
      t.folded_steps.some((item) => item.seq === targetSeq) ||
      t.notices.some((item) => item.seq === targetSeq) ||
      t.compactions.some((item) => item.seq === targetSeq)
    ) {
      return { index: i, exact: true };
    }
    for (const seg of t.live_segments) {
      if (seg.kind === "step_group" && seg.items.some((item) => item.seq === targetSeq)) {
        return { index: i, exact: true };
      }
      if (
        (seg.kind === "assistant_message" || seg.kind === "notice") &&
        seg.item.seq === targetSeq
      ) {
        return { index: i, exact: true };
      }
    }
  }
  // Fallback: last chronological turn whose anchor_seq <= targetSeq
  let best = 0;
  for (let i = 0; i < turns.length; i += 1) {
    if (turns[i].anchor_seq <= targetSeq) {
      best = i;
    } else {
      break;
    }
  }
  return { index: best, exact: false };
}

const VirtualizedTurnList = memo(function VirtualizedTurnList({
  selectedHostId,
  selectedAgentId,
  stageTurns,
  highlightedTurnKey,
  hasPendingJump,
  isLatestTurnActive,
  agentStatusLabel,
  hasLiveBackgroundTasks,
  baselineMaxSeq,
  isNetworkOffline,
  autoFoldCompleted,
  effectiveCwd,
  stageMaxContentWidthPx,
  scrollRef,
  stickToBottomRef,
  holdActiveRef,
  virtualizerRef,
  turnTimingsRef,
  thinkingTimingsRef,
  getThinkingDurationMs,
  holdViewport,
  onRequestRewind,
  onRequestFork,
  canForkWorktree,
  onPreviewImage,
}: {
  selectedHostId: string;
  selectedAgentId: string;
  stageTurns: StageTurnView[];
  highlightedTurnKey: string | null;
  hasPendingJump: boolean;
  isLatestTurnActive: boolean;
  agentStatusLabel: string | null;
  hasLiveBackgroundTasks: boolean;
  baselineMaxSeq: number;
  isNetworkOffline: boolean;
  autoFoldCompleted: boolean;
  effectiveCwd: string;
  stageMaxContentWidthPx: number | null;
  scrollRef: React.RefObject<HTMLDivElement | null>;
  stickToBottomRef: React.RefObject<boolean>;
  holdActiveRef: React.RefObject<boolean>;
  virtualizerRef: React.MutableRefObject<ReactVirtualizer<HTMLDivElement, HTMLDivElement> | null>;
  turnTimingsRef: React.RefObject<Record<string, TurnTimingRecord>>;
  thinkingTimingsRef: React.RefObject<Record<string, TurnTimingRecord>>;
  getThinkingDurationMs: ThinkingDurationLookupFn;
  holdViewport: ViewportAnchorHoldFn;
  onRequestRewind: (seq: number, promptText: string) => void;
  onRequestFork: (upToSeq: number | null, worktree: boolean) => void;
  canForkWorktree: boolean;
  onPreviewImage: (img: PromptImagePreviewTarget) => void;
}) {
  const widthBucket = stageMaxContentWidthPx ? Math.round(stageMaxContentWidthPx / 32) : 24;
  const selectionTurnsRef = useRef<number[]>([]);
  const warmKeysRef = useRef<Set<string>>(new Set());
  const [warmRevision, setWarmRevision] = useState(0);
  const scrollVelocityRef = useRef<ScrollVelocityState>({
    offset: 0,
    ts: 0,
    dir: null,
    fast: false,
    wasScrolling: false,
  });
  const localVirtualizerRef = useRef<ReactVirtualizer<HTMLDivElement, HTMLDivElement> | null>(null);

  const turnsRef = useRef(stageTurns);
  turnsRef.current = stageTurns;
  const activeRef = useRef(isLatestTurnActive);
  activeRef.current = isLatestTurnActive;

  useEffect(() => {
    const onSelectionChange = () => {
      const sel = window.getSelection();
      if (!sel || sel.isCollapsed || sel.rangeCount === 0) {
        selectionTurnsRef.current = [];
        return;
      }
      const getTurnIndex = (node: Node | null): number | null => {
        const el =
          node instanceof Element ? node : (node?.parentElement ?? null);
        const host = el?.closest<HTMLElement>("[data-orbit-turn-index]");
        if (!host) return null;
        const parsed = Number.parseInt(host.dataset.orbitTurnIndex ?? "", 10);
        return Number.isNaN(parsed) ? null : parsed;
      };
      const aIdx = getTurnIndex(sel.anchorNode);
      const fIdx = getTurnIndex(sel.focusNode);
      const pins: number[] = [];
      if (aIdx !== null && fIdx !== null) {
        const lo = Math.min(aIdx, fIdx);
        const hi = Math.max(aIdx, fIdx);
        if (hi - lo <= 8) {
          for (let k = lo; k <= hi; k += 1) pins.push(k);
        } else {
          pins.push(aIdx, fIdx);
        }
      } else {
        if (aIdx !== null) pins.push(aIdx);
        if (fIdx !== null && fIdx !== aIdx) pins.push(fIdx);
      }
      selectionTurnsRef.current = pins;
    };
    document.addEventListener("selectionchange", onSelectionChange);
    return () => document.removeEventListener("selectionchange", onSelectionChange);
  }, []);

  const readScrollVelocity = useCallback(
    (v: Virtualizer<HTMLDivElement, HTMLDivElement> | null): ScrollVelocityState => {
      const prev = scrollVelocityRef.current;
      if (!v) return prev;
      const offset = v.scrollOffset ?? 0;
      const isScrolling = Boolean(v.isScrolling);
      if (offset === prev.offset && isScrolling === prev.wasScrolling) {
        return prev;
      }

      const now = performance.now();
      const dt = now - prev.ts;
      const delta = offset - prev.offset;
      let dir = prev.dir;
      let fast = prev.fast;

      if (!isScrolling) {
        dir = null;
        fast = false;
      } else if (Math.abs(delta) > 0.5) {
        dir = delta < 0 ? "up" : "down";
        if (dt >= 2 && dt <= 250) {
          const speed = (Math.abs(delta) * 1000) / dt;
          fast = speed >= FAST_FLICK_PX_PER_SEC;
        } else if (dt > 250) {
          fast = false;
        }
      }

      const next: ScrollVelocityState = {
        offset,
        ts: now,
        dir,
        fast,
        wasScrolling: isScrolling,
      };
      scrollVelocityRef.current = next;
      return next;
    },
    [],
  );

  const rangeExtractor = useCallback(
    (range: Range): number[] => {
      const turns = turnsRef.current;
      const total = turns.length;
      if (total === 0) {
        warmKeysRef.current.clear();
        return [];
      }

      const v = localVirtualizerRef.current;
      const measurements = v?.measurementsCache;
      const isScrolling = Boolean(v?.isScrolling);
      const vel = readScrollVelocity(v);

      let upBudgetPx = IDLE_LOOKAHEAD_UP_PX;
      let downBudgetPx = IDLE_LOOKAHEAD_DOWN_PX;
      if (vel.dir === "up") {
        upBudgetPx = vel.fast ? SCROLL_FAST_LEADING_PX : SCROLL_LEADING_PX;
        downBudgetPx = SCROLL_TRAILING_PX;
      } else if (vel.dir === "down") {
        upBudgetPx = SCROLL_TRAILING_PX;
        downBudgetPx = vel.fast ? SCROLL_FAST_LEADING_PX : SCROLL_LEADING_PX;
      }

      const clampedStart = Math.max(0, Math.min(range.startIndex, total - 1));
      const clampedEnd = Math.max(clampedStart, Math.min(range.endIndex, total - 1));

      let windowStart = clampedStart;
      let accUp = 0;
      while (
        windowStart > 0 &&
        accUp < upBudgetPx &&
        clampedStart - windowStart < MAX_OVERSCAN_ITEMS_PER_SIDE
      ) {
        const prevIdx = windowStart - 1;
        const size = measurements?.[prevIdx]?.size ?? estimateTurnSizeFallback(turns[prevIdx]);
        accUp += size + TURN_LIST_GAP_PX;
        windowStart = prevIdx;
      }

      let windowEnd = clampedEnd;
      let accDown = 0;
      while (
        windowEnd < total - 1 &&
        accDown < downBudgetPx &&
        windowEnd - clampedEnd < MAX_OVERSCAN_ITEMS_PER_SIDE
      ) {
        const nextIdx = windowEnd + 1;
        const size = measurements?.[nextIdx]?.size ?? estimateTurnSizeFallback(turns[nextIdx]);
        accDown += size + TURN_LIST_GAP_PX;
        windowEnd = nextIdx;
      }

      const activeSet = new Set<number>();
      for (let i = windowStart; i <= windowEnd; i += 1) {
        activeSet.add(i);
      }
      if (activeRef.current) {
        activeSet.add(total - 1);
      }
      for (const idx of selectionTurnsRef.current) {
        if (idx >= 0 && idx < total) {
          activeSet.add(idx);
        }
      }

      const prevWarmKeys = warmKeysRef.current;
      const nextWarmKeys = new Set<string>();
      const merged = new Set<number>(activeSet);
      for (const idx of activeSet) {
        nextWarmKeys.add(turns[idx].key);
      }
      if (prevWarmKeys.size > 0) {
        for (let i = 0; i < total; i += 1) {
          const key = turns[i].key;
          if (prevWarmKeys.has(key)) {
            merged.add(i);
            nextWarmKeys.add(key);
          }
        }
      }

      const cap = isScrolling ? WARM_SCROLLING_CAP : WARM_RESTING_CAP;
      if (merged.size > cap) {
        const center = (clampedStart + clampedEnd) * 0.5;
        const evictable: number[] = [];
        for (const idx of merged) {
          if (!activeSet.has(idx)) {
            evictable.push(idx);
          }
        }
        evictable.sort((a, b) => Math.abs(b - center) - Math.abs(a - center));
        for (const idx of evictable) {
          if (merged.size <= cap) break;
          merged.delete(idx);
          nextWarmKeys.delete(turns[idx].key);
        }
      }

      warmKeysRef.current = nextWarmKeys;
      return Array.from(merged).sort((a, b) => a - b);
    },
    [readScrollVelocity, warmRevision],
  );

  const getItemKey = useCallback(
    (index: number) => turnsRef.current[index]?.key ?? index,
    [],
  );

  const estimateSize = useCallback(
    (index: number) => {
      const turn = turnsRef.current[index];
      if (!turn) return 180;
      const cached = getCachedTurnHeight(`${selectedAgentId}::${turn.key}::${widthBucket}`);
      return cached ?? estimateTurnSizeFallback(turn);
    },
    [selectedAgentId, widthBucket],
  );

  const handleVirtualizerChange = useCallback(
    (instance: Virtualizer<HTMLDivElement, HTMLDivElement>) => {
      const wasScrolling = scrollVelocityRef.current.wasScrolling;
      readScrollVelocity(instance);
      if (wasScrolling && !instance.isScrolling && warmKeysRef.current.size > WARM_RESTING_CAP) {
        setWarmRevision((n) => n + 1);
      }
    },
    [readScrollVelocity],
  );

  const initialRect = useMemo(() => {
    const el = scrollRef.current;
    const width =
      el && el.clientWidth > 0
        ? el.clientWidth
        : typeof window !== "undefined"
          ? Math.max(360, window.innerWidth)
          : 1280;
    const height =
      el && el.clientHeight > 0
        ? el.clientHeight
        : typeof window !== "undefined"
          ? Math.max(400, window.innerHeight - 160)
          : 720;
    return { width, height };
    // Compute initial viewport estimate only when mounting a fresh session list
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAgentId]);

  const hasPendingJumpRef = useRef(hasPendingJump);
  hasPendingJumpRef.current = hasPendingJump;

  const initialBottomOffset = useMemo(() => {
    if (hasPendingJump || stageTurns.length === 0) return 0;
    let total = 0;
    for (let i = 0; i < stageTurns.length; i += 1) {
      const turn = stageTurns[i];
      const cached = getCachedTurnHeight(`${selectedAgentId}::${turn.key}::${widthBucket}`);
      total += (cached ?? estimateTurnSizeFallback(turn)) + TURN_LIST_GAP_PX;
    }
    return Math.max(0, total - initialRect.height);
    // Compute initial bottom anchor only when mounting a fresh session list
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [selectedAgentId, initialRect.height, hasPendingJump]);

  const observeOffsetWithBottomSeed = useCallback(
    (
      instance: Virtualizer<HTMLDivElement, HTMLDivElement>,
      cb: (offset: number, isScrolling: boolean) => void,
    ) => {
      const el = instance.scrollElement;
      if (
        el &&
        stickToBottomRef.current &&
        !hasPendingJumpRef.current &&
        initialBottomOffset > 0 &&
        el.scrollTop === 0
      ) {
        const maxTop = Math.max(0, el.scrollHeight - el.clientHeight);
        if (maxTop > 0) {
          el.scrollTop = maxTop;
        }
      }
      return observeElementOffset(instance, cb);
    },
    [initialBottomOffset, stickToBottomRef],
  );

  const virtualizer = useVirtualizer<HTMLDivElement, HTMLDivElement>({
    count: stageTurns.length,
    getScrollElement: () => scrollRef.current,
    getItemKey,
    estimateSize,
    initialRect,
    initialOffset: initialBottomOffset,
    observeElementOffset: observeOffsetWithBottomSeed,
    rangeExtractor,
    onChange: handleVirtualizerChange,
    gap: TURN_LIST_GAP_PX,
    overscan: 0,
    useScrollendEvent: true,
    directDomUpdates: true,
  });

  virtualizer.shouldAdjustScrollPositionOnItemSizeChange = (item, _delta, instance) => {
    if (holdActiveRef.current) {
      return false;
    }
    const scrollOffset = instance.scrollOffset ?? 0;
    if (stickToBottomRef.current) {
      return item.end <= scrollOffset;
    }
    const key = String(item.key);
    const alreadyMeasured = instance.itemSizeCache.has(key);
    return alreadyMeasured ? item.end <= scrollOffset : item.start < scrollOffset;
  };

  localVirtualizerRef.current = virtualizer;
  virtualizerRef.current = virtualizer;
  useEffect(() => {
    return () => {
      if (localVirtualizerRef.current === virtualizer) {
        localVirtualizerRef.current = null;
      }
      if (virtualizerRef.current === virtualizer) {
        virtualizerRef.current = null;
      }
    };
  }, [virtualizer, virtualizerRef]);

  // Gently pre-warm offscreen preceding turns during idle time (never while streaming or scrolling)
  useEffect(() => {
    if (isLatestTurnActive || stageTurns.length <= 2) return;

    let cancelled = false;
    let initialTimer: number | null = null;
    let fallbackTimer: number | null = null;
    let idleHandle: number | null = null;

    const scheduleStep = () => {
      if (cancelled) return;
      if (typeof window.requestIdleCallback === "function") {
        idleHandle = window.requestIdleCallback(runStep, { timeout: 180 });
      } else {
        fallbackTimer = window.setTimeout(runStep, 48);
      }
    };

    const runStep = () => {
      idleHandle = null;
      fallbackTimer = null;
      if (cancelled) return;

      if (localVirtualizerRef.current?.isScrolling) {
        scheduleStep();
        return;
      }

      const turns = turnsRef.current;
      const warm = warmKeysRef.current;
      if (turns.length === 0 || warm.size >= WARM_RESTING_CAP) {
        return;
      }

      let lowestMounted = turns.length;
      for (let i = 0; i < turns.length; i += 1) {
        if (warm.has(turns[i].key)) {
          lowestMounted = i;
          break;
        }
      }

      let added = 0;
      for (
        let idx = lowestMounted - 1;
        idx >= 0 && added < IDLE_PREWARM_BATCH && warm.size < WARM_RESTING_CAP;
        idx -= 1
      ) {
        const key = turns[idx].key;
        if (!warm.has(key)) {
          warm.add(key);
          added += 1;
        }
      }

      if (added > 0) {
        setWarmRevision((n) => n + 1);
        if (warm.size < WARM_RESTING_CAP) {
          scheduleStep();
        }
      }
    };

    initialTimer = window.setTimeout(scheduleStep, 220);

    return () => {
      cancelled = true;
      if (initialTimer !== null) window.clearTimeout(initialTimer);
      if (fallbackTimer !== null) window.clearTimeout(fallbackTimer);
      if (idleHandle !== null && typeof window.cancelIdleCallback === "function") {
        window.cancelIdleCallback(idleHandle);
      }
    };
  }, [selectedAgentId, stageTurns.length, isLatestTurnActive]);

  const measureTurnNode = useCallback(
    (el: HTMLDivElement | null) => {
      virtualizer.measureElement(el);
      if (el) {
        const idxAttr = el.getAttribute("data-index");
        const idx = idxAttr ? Number.parseInt(idxAttr, 10) : -1;
        const turn = idx >= 0 ? turnsRef.current[idx] : undefined;
        const cachedSize = turn ? virtualizer.itemSizeCache.get(turn.key) : undefined;
        if (turn && cachedSize && cachedSize > 0) {
          setCachedTurnHeight(`${selectedAgentId}::${turn.key}::${widthBucket}`, cachedSize);
        }
      }
    },
    [virtualizer, selectedAgentId, widthBucket],
  );

  const virtualItems = virtualizer.getVirtualItems();

  return (
    <div ref={virtualizer.containerRef} className="relative w-full min-w-0">
      {virtualItems.map((vItem) => {
        const turn = stageTurns[vItem.index];
        if (!turn) return null;
        const isLast = vItem.index === stageTurns.length - 1;
        const turnActive = isLast && isLatestTurnActive;
        const isFreshLivePrompt =
          turn.user_prompt?.item_type === "user_prompt" &&
          turn.user_prompt.seq > baselineMaxSeq;
        const timing = turnTimingsRef.current[`${selectedAgentId}::${turn.key}`];
        const thinkingSignature = buildTurnThinkingSignature(
          turn,
          selectedAgentId,
          thinkingTimingsRef.current,
        );

        const isHighlighted = highlightedTurnKey === turn.key;

        return (
          <div
            key={vItem.key}
            data-index={vItem.index}
            data-orbit-turn-index={vItem.index}
            ref={measureTurnNode}
            className={cn(
              "absolute top-0 left-0 w-full min-w-0 rounded-2xl transition-colors duration-700",
              isHighlighted &&
                "bg-foreground/[0.035] ring-1 ring-inset ring-foreground/[0.12] dark:bg-foreground/[0.055] dark:ring-foreground/[0.18]",
            )}
          >
            <TurnStageSection
              turn={turn}
              isTurnActive={turnActive}
              isLastTurn={isLast}
              agentStatusLabel={turnActive ? agentStatusLabel : null}
              hasLiveBackgroundTasks={turnActive && hasLiveBackgroundTasks}
              isNetworkOffline={isNetworkOffline}
              animatePromptEntrance={isFreshLivePrompt}
              autoFoldCompleted={autoFoldCompleted}
              hostId={selectedHostId}
              cwd={effectiveCwd}
              agentId={selectedAgentId}
              timing={timing}
              thinkingSignature={thinkingSignature}
              getThinkingDurationMs={getThinkingDurationMs}
              holdViewport={holdViewport}
              onRequestRewind={onRequestRewind}
              onRequestFork={onRequestFork}
              canForkWorktree={canForkWorktree}
              onPreviewImage={onPreviewImage}
            />
          </div>
        );
      })}
    </div>
  );
});

export function StreamStage() {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const sessionScrollNonce = useOrbitSelector((s) => s.sessionScrollNonce);
  const pendingJumpTarget = useOrbitSelector((s) => s.pendingJumpTarget);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const [highlightedTurnKey, setHighlightedTurnKey] = useState<string | null>(null);
  const [jumpFallbackNonce, setJumpFallbackNonce] = useState<number | null>(null);
  const highlightTimerRef = useRef<number | null>(null);
  const jumpFallbackTimerRef = useRef<number | null>(null);
  const jumpRafRef = useRef<{ raf1: number | null; raf2: number | null }>({
    raf1: null,
    raf2: null,
  });
  const isNetworkOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const {
    canDockOverview,
    isStageWide,
    dockedOverviewLayout,
    overviewReservePx,
    overviewReserveTransition,
    stageShiftPx,
    stageTransition,
    stageMaxContentWidthPx,
    outlineOffsetPx,
    outlineTransition,
    isAnimating,
    isResizing,
    onStageTransitionEnd,
  } = useWorkspaceMotion();
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const stageView = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? (s.stages[`${s.selectedHostId}::${s.selectedAgentId}`] ?? null)
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
  const virtualizerRef = useRef<ReactVirtualizer<HTMLDivElement, HTMLDivElement> | null>(null);
  const prevComposerHeightRef = useRef<number>(0);
  const lastObservedScrollTopRef = useRef<number>(0);
  const stickToBottomRef = useRef<boolean>(true);
  const sessionEntryLockUntilRef = useRef<number>(0);
  const prevTurnsCountRef = useRef<number>(0);
  const holdActiveRef = useRef<boolean>(false);
  const programmaticScrollRef = useRef<boolean>(false);
  const programmaticScrollTimerRef = useRef<number | null>(null);
  const scrollRafRef = useRef<number | null>(null);
  const bottomSyncRafRef = useRef<number | null>(null);
  const lastOutlineSyncAtRef = useRef<number>(0);
  const scrollUiStoreRef = useRef<StageScrollUiStore | null>(null);
  if (scrollUiStoreRef.current === null) {
    scrollUiStoreRef.current = createStageScrollUiStore();
  }
  const scrollUiStore = scrollUiStoreRef.current;

  const turnTimingsRef = useRef<Record<string, TurnTimingRecord>>({});
  const thinkingTimingsRef = useRef<Record<string, TurnTimingRecord>>({});
  const mountedBaselineRef = useRef<{ agentId: string | null; maxSeq: number }>({
    agentId: null,
    maxSeq: 0,
  });

  const isStageWideRef = useRef(isStageWide);
  isStageWideRef.current = isStageWide;
  const isShellLayoutMovingRef = useRef(isAnimating || isResizing);
  isShellLayoutMovingRef.current = isAnimating || isResizing;
  const [, setTimingTick] = useState(0);
  const [rewindTarget, setRewindTarget] = useState<RewindRequestTarget | null>(null);
  const [rewindSubmitting, setRewindSubmitting] = useState(false);
  const [imagePreviewTarget, setImagePreviewTarget] =
    useState<PromptImagePreviewTarget | null>(null);
  const handleCloseImagePreview = useCallback(() => setImagePreviewTarget(null), []);

  const getThinkingDurationMs = useCallback<ThinkingDurationLookupFn>((thinkKey) => {
    return thinkingTimingsRef.current[thinkKey]?.durationMs ?? null;
  }, []);

  const currentAgent = useMemo(
    () =>
      hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ??
      stageView?.agent ??
      null,
    [hostDetail, selectedAgentId, stageView?.agent],
  );
  const isAgentRunning = isAgentForegroundRunning(currentAgent);
  const fleetHydrated = useOrbitSelector((s) => s.fleetHydrated);
  const bootstrapError = useOrbitSelector((s) => s.bootstrapError);

  const stageTurns = stageView?.turns ?? EMPTY_TURNS;
  const pendingPermissions = useMemo(
    () =>
      (stageView?.pending_permissions ?? EMPTY_TIMELINE_ITEMS).filter(
        (it): it is Extract<TimelineItemView, { item_type: "permission_request" }> =>
          it.item_type === "permission_request",
      ),
    [stageView?.pending_permissions],
  );
  const pendingQuestions = useMemo(
    () =>
      (stageView?.pending_questions ?? EMPTY_TIMELINE_ITEMS).filter(
        (it): it is Extract<TimelineItemView, { item_type: "question_request" }> =>
          it.item_type === "question_request",
      ),
    [stageView?.pending_questions],
  );
  const sessionTurnDiffs = stageView?.session_turn_diffs ?? EMPTY_DIFF_FILES;
  const stageInsights = stageView?.insights ?? EMPTY_INSIGHTS;

  const latestTurn = stageTurns[stageTurns.length - 1] ?? null;
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
      latestTurn.started_at_ms !== null && latestTurn.started_at_ms <= now
        ? latestTurn.started_at_ms
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
      } else if (latestTurn.terminal_reply !== null && existing.workDurationMs === null) {
        turnTimingsRef.current[timingKey] = {
          ...existing,
          workDurationMs: Math.max(0, now - existing.startedAtMs),
        };
        changed = true;
      } else if (latestTurn.terminal_reply === null && existing.workDurationMs !== null) {
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

    const lastSeg = latestTurn.live_segments[latestTurn.live_segments.length - 1] ?? null;
    const lastSegItem =
      lastSeg?.kind === "step_group" ? (lastSeg.items[lastSeg.items.length - 1] ?? null) : null;

    for (const seg of latestTurn.live_segments) {
      if (seg.kind !== "step_group") continue;
      for (const item of seg.items) {
        if (item.item_type !== "assistant_thinking") continue;

        const thinkKey = `${selectedAgentId}::think::${item.item_id || item.seq}`;
        const isCurrentlyThinking =
          isLatestTurnActive &&
          lastSegItem?.item_type === "assistant_thinking" &&
          lastSegItem.seq === item.seq;
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
    }

    if (changed) {
      setTimingTick((n) => n + 1);
    }
  }, [selectedAgentId, latestTurn, isLatestTurnActive]);

  const stagePushStyle = useMemo<CSSProperties>(
    () => ({
      transform: stageShiftPx !== 0 ? `translate3d(${stageShiftPx}px, 0, 0)` : undefined,
      transition: stageTransition,
      willChange: isAnimating ? "transform" : undefined,
    }),
    [stageShiftPx, stageTransition, isAnimating],
  );

  const outlinePrompts = useMemo<OutlinePromptWithIndex[]>(() => {
    const out: OutlinePromptWithIndex[] = [];
    for (let turnIndex = 0; turnIndex < stageTurns.length; turnIndex += 1) {
      const t = stageTurns[turnIndex];
      if (t.user_prompt?.item_type === "user_prompt" && !t.is_compaction_command) {
        const promptItem = t.user_prompt as Extract<TimelineItemView, { item_type: "user_prompt" }>;
        const pres = extractUserPromptPresentation(promptItem);
        const summary =
          pres.cleanText || pres.badges.map((b) => b.label).join(" ") || "Prompt";
        out.push({
          seq: t.anchor_seq,
          turnIndex,
          preview: summary.replace(/\s+/g, " ").trim().slice(0, 110),
        });
      }
    }
    return out;
  }, [stageTurns]);

  // Refs for `syncScrollMetrics` so its callback identity (and `holdViewport`) stays 100% stable forever
  const outlinePromptsRef = useRef(outlinePrompts);
  outlinePromptsRef.current = outlinePrompts;
  const stageTurnsRef = useRef(stageTurns);
  stageTurnsRef.current = stageTurns;
  const turnsCountRef = useRef(stageTurns.length);
  turnsCountRef.current = stageTurns.length;

  const effectiveCwdRef = useRef("");
  effectiveCwdRef.current = selectedCwd || currentAgent?.cwd || "";

  const handleRequestRewind = useCallback((seq: number, promptText: string) => {
    const activeCwd = effectiveCwdRef.current;
    const affected = stageTurnsRef.current.filter((t) => t.anchor_seq >= seq);
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
      for (const diff of turn.turn_diffs) {
        if (diff.path) recordFile(diff.path, diff.additions, diff.deletions);
      }
      for (const seg of turn.live_segments) {
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

  const handleRequestFork = useCallback(
    (upToSeq: number | null, worktree: boolean) => {
      if (!selectedHostId || !selectedAgentId) return;
      void orbitActions.forkAgent({
        hostId: selectedHostId,
        sourceAgentId: selectedAgentId,
        upToSeq,
        worktree,
      });
    },
    [selectedHostId, selectedAgentId],
  );

  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const canForkWorktree = useMemo(
    () => isAgentInGitRepo(selectedHostId, currentAgent, hostDetail, sidebarView),
    [selectedHostId, currentAgent, hostDetail, sidebarView],
  );

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

  useEffect(() => {
    return () => {
      if (programmaticScrollTimerRef.current !== null) {
        window.clearTimeout(programmaticScrollTimerRef.current);
      }
      if (scrollRafRef.current !== null) {
        window.cancelAnimationFrame(scrollRafRef.current);
      }
      if (bottomSyncRafRef.current !== null) {
        window.cancelAnimationFrame(bottomSyncRafRef.current);
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

    let nextOccluding = false;
    let nextJump = false;

    if (programmaticScrollRef.current) {
      if (distanceFromBottom < 8) {
        programmaticScrollRef.current = false;
        if (programmaticScrollTimerRef.current !== null) {
          window.clearTimeout(programmaticScrollTimerRef.current);
          programmaticScrollTimerRef.current = null;
        }
      }
      stickToBottomRef.current = true;
    } else {
      const nearBottom = distanceFromBottom < 36;
      const scrolledUp = currentTop < prevTop - 1.5;
      const inEntrySettle = performance.now() < sessionEntryLockUntilRef.current;
      if (inEntrySettle && stickToBottomRef.current) {
        // Protect bottom-pinning from browser/virtualizer scrollTop clamping during session switch
        // or return from New Conversation; explicit user wheel/scrollbar gestures clear this lock immediately.
        if (distanceFromBottom >= 2) {
          el.scrollTop = el.scrollHeight;
          lastObservedScrollTopRef.current = el.scrollTop;
        }
      } else if (nearBottom) {
        stickToBottomRef.current = true;
      } else if (scrolledUp && !isShellLayoutMovingRef.current) {
        // Ignore browser/virtualizer scrollTop clamping during sidebar open/close/resize animations
        // so bottom-pinned conversations never lose their pin mid-transition; explicit user wheel-up
        // still unpins immediately via `onPassiveWheel`.
        stickToBottomRef.current = false;
      }
      const pinned = stickToBottomRef.current;
      nextOccluding = !pinned && distanceFromBottom > 4 && hasTurns;
      nextJump = !pinned && !nearBottom && hasTurns;
    }

    // O(log N) binary search over virtualizer offset cache — zero DOM layout reads during scroll
    let nextActiveSeq = scrollUiStore.getSnapshot().activePromptSeq;
    const prompts = outlinePromptsRef.current;
    if (isStageWideRef.current && prompts.length >= 2) {
      const now = performance.now();
      if (now - lastOutlineSyncAtRef.current >= 80) {
        lastOutlineSyncAtRef.current = now;
        const measurements = virtualizerRef.current?.measurementsCache;
        const anchorOffset = Math.max(0, currentTop + 96);
        if (measurements && measurements.length > 0) {
          let lo = 0;
          let hi = prompts.length - 1;
          let best = prompts[0].seq;
          while (lo <= hi) {
            const mid = (lo + hi) >> 1;
            const m = measurements[prompts[mid].turnIndex];
            if (!m || m.start <= anchorOffset) {
              best = prompts[mid].seq;
              lo = mid + 1;
            } else {
              hi = mid - 1;
            }
          }
          nextActiveSeq = best;
        }
      }
    }

    scrollUiStore.update({
      isOccludingText: nextOccluding,
      showJumpLatest: nextJump,
      activePromptSeq: nextActiveSeq,
    });
  }, [scrollUiStore]);

  const snapScrollerToBottom = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
    lastObservedScrollTopRef.current = el.scrollTop;
  }, []);

  const scheduleBottomSync = useCallback(() => {
    if (bottomSyncRafRef.current !== null) return;
    bottomSyncRafRef.current = window.requestAnimationFrame(() => {
      bottomSyncRafRef.current = null;
      if (stickToBottomRef.current && !programmaticScrollRef.current) {
        snapScrollerToBottom();
      }
      syncScrollMetrics();
    });
  }, [snapScrollerToBottom, syncScrollMetrics]);

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
        sessionEntryLockUntilRef.current = 0;
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
    const onPointerDown = (e: PointerEvent) => {
      // Grabbing the native right-edge scrollbar immediately yields control to the user
      const rect = el.getBoundingClientRect();
      if (e.clientX >= rect.right - 20) {
        sessionEntryLockUntilRef.current = 0;
      }
    };
    el.addEventListener("wheel", onPassiveWheel, { passive: true });
    el.addEventListener("pointerdown", onPointerDown, { passive: true });
    return () => {
      el.removeEventListener("wheel", onPassiveWheel);
      el.removeEventListener("pointerdown", onPointerDown);
    };
  }, [isDraft, selectedHostId, selectedAgentId]);

  // Keep conversation bottom anchored cleanly when virtualized content or viewport height changes.
  // Attached in useLayoutEffect so it catches the very first virtualizer container height expansion on mount.
  useLayoutEffect(() => {
    const contentEl = contentRef.current;
    const scrollerEl = scrollRef.current;
    if (!contentEl || !scrollerEl || typeof ResizeObserver === "undefined") return;

    let lastScrollHeight = scrollerEl.scrollHeight;
    let lastClientHeight = scrollerEl.clientHeight;

    const ro = new ResizeObserver(() => {
      const sh = scrollerEl.scrollHeight;
      const ch = scrollerEl.clientHeight;
      const heightChanged = sh !== lastScrollHeight || ch !== lastClientHeight;
      lastScrollHeight = sh;
      lastClientHeight = ch;

      if (heightChanged) {
        // Snap synchronously before paint so continuous width transitions stay pixel-locked
        // to the bottom with zero 1-frame lag, then follow up with metrics sync.
        if (stickToBottomRef.current && !programmaticScrollRef.current) {
          snapScrollerToBottom();
        }
        scheduleBottomSync();
      } else {
        handleScroll();
      }
    });
    ro.observe(contentEl);
    ro.observe(scrollerEl);
    return () => ro.disconnect();
  }, [
    isDraft,
    selectedHostId,
    selectedAgentId,
    snapScrollerToBottom,
    scheduleBottomSync,
    handleScroll,
  ]);

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
    scrollUiStore.update({ isOccludingText: false, showJumpLatest: false });

    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
    }
    programmaticScrollTimerRef.current = window.setTimeout(() => {
      programmaticScrollRef.current = false;
      programmaticScrollTimerRef.current = null;
      syncScrollMetrics();
    }, 800);

    el.scrollTo({ top: el.scrollHeight, behavior: "smooth" });
  }, [scrollUiStore, syncScrollMetrics]);

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
      holdActiveRef.current = true;

      apply();

      requestAnimationFrame(() => {
        const curScroller = scrollRef.current;
        if (!curScroller || !triggerEl.isConnected) {
          holdActiveRef.current = false;
          return;
        }
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
        requestAnimationFrame(() => {
          holdActiveRef.current = false;
        });
      });
    },
    [syncScrollMetrics],
  );

  const jumpToPromptSeq = useCallback(
    (seq: number) => {
      const entry = outlinePromptsRef.current.find((p) => p.seq === seq);
      const v = virtualizerRef.current;
      if (!entry || !v) return;
      stickToBottomRef.current = false;
      scrollUiStore.update({ activePromptSeq: seq });
      v.scrollToIndex(entry.turnIndex, { align: "start", behavior: "smooth" });
    },
    [scrollUiStore],
  );

  const activeSessionScope =
    !isDraft && selectedHostId && selectedAgentId
      ? `${selectedHostId}::${selectedAgentId}`
      : null;

  const hasPendingJumpForActive = Boolean(
    pendingJumpTarget &&
      selectedHostId &&
      selectedAgentId &&
      pendingJumpTarget.hostId === selectedHostId &&
      pendingJumpTarget.agentId === selectedAgentId,
  );
  const hasPendingJumpForActiveRef = useRef(hasPendingJumpForActive);
  hasPendingJumpForActiveRef.current = hasPendingJumpForActive;

  // Always reset bottom-pinning cleanly before paint whenever switching into a session
  useLayoutEffect(() => {
    if (!activeSessionScope) {
      prevTurnsCountRef.current = 0;
      return;
    }
    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
      programmaticScrollTimerRef.current = null;
    }
    if (jumpRafRef.current.raf1 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf1);
      jumpRafRef.current.raf1 = null;
    }
    if (jumpRafRef.current.raf2 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf2);
      jumpRafRef.current.raf2 = null;
    }
    if (jumpFallbackTimerRef.current !== null) {
      window.clearTimeout(jumpFallbackTimerRef.current);
      jumpFallbackTimerRef.current = null;
    }
    setJumpFallbackNonce(null);
    programmaticScrollRef.current = false;
    holdActiveRef.current = false;
    const isTargetedJump = hasPendingJumpForActiveRef.current;
    if (!isTargetedJump) {
      if (highlightTimerRef.current !== null) {
        window.clearTimeout(highlightTimerRef.current);
        highlightTimerRef.current = null;
      }
      setHighlightedTurnKey(null);
    }
    stickToBottomRef.current = !isTargetedJump;
    sessionEntryLockUntilRef.current = isTargetedJump ? 0 : performance.now() + 600;
    lastObservedScrollTopRef.current = 0;
    prevTurnsCountRef.current = turnsCountRef.current;
    scrollUiStore.update({
      isOccludingText: false,
      showJumpLatest: false,
      activePromptSeq: null,
    });
    setRewindTarget(null);

    if (isTargetedJump) {
      return;
    }

    const anchorToBottom = () => {
      if (!stickToBottomRef.current || programmaticScrollRef.current) return;
      if (turnsCountRef.current > 0) {
        virtualizerRef.current?.scrollToIndex(turnsCountRef.current - 1, { align: "end" });
      }
      snapScrollerToBottom();
    };

    anchorToBottom();
    scheduleBottomSync();

    // Re-anchor across the next two frames once TanStack Virtual measures scrollRect & bottom turn heights
    let raf2: number | null = null;
    const raf1 = window.requestAnimationFrame(() => {
      anchorToBottom();
      raf2 = window.requestAnimationFrame(() => {
        anchorToBottom();
        syncScrollMetrics();
      });
    });
    return () => {
      window.cancelAnimationFrame(raf1);
      if (raf2 !== null) window.cancelAnimationFrame(raf2);
    };
  }, [
    activeSessionScope,
    sessionScrollNonce,
    scrollUiStore,
    snapScrollerToBottom,
    scheduleBottomSync,
    syncScrollMetrics,
  ]);

  // Execute search-result jump-to-seq scroll + subtle pulse highlight once turns are hydrated
  useEffect(() => {
    if (!activeSessionScope || !pendingJumpTarget) return;
    if (
      pendingJumpTarget.hostId !== selectedHostId ||
      pendingJumpTarget.agentId !== selectedAgentId
    ) {
      return;
    }
    if (stageTurns.length === 0) return;

    const match = findTurnIndexForSeq(stageTurns, pendingJumpTarget.seq);
    if (match.index < 0) return;
    const jumpNonce = pendingJumpTarget.nonce;
    const stageLatestSeq = stageView?.latest_seq ?? 0;

    // If cached turns haven't caught up to the searched seq yet, wait for pullStageDelta
    // hydration before consuming the jump target (with a 1500ms fallback if rewound/pruned).
    if (!match.exact && stageLatestSeq < pendingJumpTarget.seq && jumpFallbackNonce !== jumpNonce) {
      stickToBottomRef.current = false;
      sessionEntryLockUntilRef.current = 0;
      if (jumpFallbackTimerRef.current === null) {
        jumpFallbackTimerRef.current = window.setTimeout(() => {
          jumpFallbackTimerRef.current = null;
          setJumpFallbackNonce(jumpNonce);
        }, 1500);
      }
      return;
    }

    if (jumpFallbackTimerRef.current !== null) {
      window.clearTimeout(jumpFallbackTimerRef.current);
      jumpFallbackTimerRef.current = null;
    }

    const turnIdx = match.index;
    const targetTurn = stageTurns[turnIdx];

    stickToBottomRef.current = false;
    sessionEntryLockUntilRef.current = 0;
    programmaticScrollRef.current = true;
    if (programmaticScrollTimerRef.current !== null) {
      window.clearTimeout(programmaticScrollTimerRef.current);
    }
    programmaticScrollTimerRef.current = window.setTimeout(() => {
      programmaticScrollRef.current = false;
      programmaticScrollTimerRef.current = null;
      syncScrollMetrics();
    }, 250);

    const performJump = () => {
      virtualizerRef.current?.scrollToIndex(turnIdx, {
        align: "center",
        behavior: "auto",
      });
    };

    if (jumpRafRef.current.raf1 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf1);
      jumpRafRef.current.raf1 = null;
    }
    if (jumpRafRef.current.raf2 !== null) {
      window.cancelAnimationFrame(jumpRafRef.current.raf2);
      jumpRafRef.current.raf2 = null;
    }

    performJump();
    jumpRafRef.current.raf1 = window.requestAnimationFrame(() => {
      jumpRafRef.current.raf1 = null;
      performJump();
      jumpRafRef.current.raf2 = window.requestAnimationFrame(() => {
        jumpRafRef.current.raf2 = null;
        performJump();
        syncScrollMetrics();
      });
    });

    setHighlightedTurnKey(targetTurn.key);
    if (highlightTimerRef.current !== null) {
      window.clearTimeout(highlightTimerRef.current);
    }
    highlightTimerRef.current = window.setTimeout(() => {
      setHighlightedTurnKey((cur) => (cur === targetTurn.key ? null : cur));
      highlightTimerRef.current = null;
    }, 1800);

    orbitActions.clearPendingJumpTarget(jumpNonce);
  }, [
    activeSessionScope,
    selectedHostId,
    selectedAgentId,
    pendingJumpTarget,
    stageTurns,
    stageView?.latest_seq,
    jumpFallbackNonce,
    syncScrollMetrics,
  ]);

  useEffect(() => {
    return () => {
      if (highlightTimerRef.current !== null) {
        window.clearTimeout(highlightTimerRef.current);
      }
      if (jumpFallbackTimerRef.current !== null) {
        window.clearTimeout(jumpFallbackTimerRef.current);
      }
      if (jumpRafRef.current.raf1 !== null) {
        window.cancelAnimationFrame(jumpRafRef.current.raf1);
      }
      if (jumpRafRef.current.raf2 !== null) {
        window.cancelAnimationFrame(jumpRafRef.current.raf2);
      }
    };
  }, []);

  // Synchronously keep pinned to bottom before paint as timeline items arrive, turns append,
  // or sidebar layout motion starts/settles.
  useLayoutEffect(() => {
    if (!activeSessionScope) return;
    const prevCount = prevTurnsCountRef.current;
    prevTurnsCountRef.current = stageTurns.length;

    // Async stage hydration (0 -> N turns) after switching into an uncached session
    if (prevCount === 0 && stageTurns.length > 0 && stickToBottomRef.current) {
      sessionEntryLockUntilRef.current = performance.now() + 600;
      virtualizerRef.current?.scrollToIndex(stageTurns.length - 1, { align: "end" });
      snapScrollerToBottom();
      const raf = window.requestAnimationFrame(() => {
        if (stickToBottomRef.current && !programmaticScrollRef.current && turnsCountRef.current > 0) {
          virtualizerRef.current?.scrollToIndex(turnsCountRef.current - 1, { align: "end" });
          snapScrollerToBottom();
          syncScrollMetrics();
        }
      });
      return () => window.cancelAnimationFrame(raf);
    }

    if (stickToBottomRef.current && !programmaticScrollRef.current) {
      snapScrollerToBottom();
    }
    scheduleBottomSync();
  }, [
    activeSessionScope,
    stageView?.latest_seq,
    stageTurns.length,
    stageMaxContentWidthPx,
    isAnimating,
    snapScrollerToBottom,
    scheduleBottomSync,
    syncScrollMetrics,
  ]);

  if (!selectedHostId) {
    if (!fleetHydrated) {
      return <StageHydratingFallback bootstrapError={bootstrapError} />;
    }
    return (
      <div className="text-muted-foreground flex flex-1 items-center justify-center text-[13.5px]">
        Pair or select an Orbit host to begin.
      </div>
    );
  }

  if (isDraft || !selectedAgentId) {
    mountedBaselineRef.current = { agentId: null, maxSeq: 0 };
    return (
      <ScrollArea
        className="min-h-0 flex-1"
        viewportClassName="[&>div]:h-full"
        style={
          stageMaxContentWidthPx
            ? ({ "--orbit-stage-max-w": `${stageMaxContentWidthPx}px` } as CSSProperties)
            : undefined
        }
      >
        <div className="h-full w-full" style={stagePushStyle}>
          <NewSessionLaunchpad />
        </div>
      </ScrollArea>
    );
  }

  if (mountedBaselineRef.current.agentId !== selectedAgentId) {
    const isFreshFirstTurn = stageTurns.length === 1 && isLatestTurnActive;
    mountedBaselineRef.current = {
      agentId: selectedAgentId,
      maxSeq: isFreshFirstTurn
        ? 0
        : (stageView?.latest_seq ?? Number.MAX_SAFE_INTEGER),
    };
  } else if (
    stageView &&
    stageView.latest_seq < mountedBaselineRef.current.maxSeq
  ) {
    mountedBaselineRef.current = {
      agentId: selectedAgentId,
      maxSeq: stageView.latest_seq,
    };
  }
  const baselineMaxSeq = mountedBaselineRef.current.maxSeq;

  const effectiveCwd = selectedCwd || currentAgent?.cwd || "";

  return (
    <div
      ref={stageContainerRef}
      style={
        stageMaxContentWidthPx
          ? ({ "--orbit-stage-max-w": `${stageMaxContentWidthPx}px` } as CSSProperties)
          : undefined
      }
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
    >
      {/* Peripheral Megathread Outline Rail (clean left gutter >= 800px / < 776px) */}
      {outlinePrompts.length >= 2 && (
        <StageOutlineRailBridge
          visible={isStageWide}
          offsetPx={outlineOffsetPx}
          offsetTransition={outlineTransition}
          isAnimating={isAnimating}
          scrollUiStore={scrollUiStore}
          prompts={outlinePrompts}
          onJumpToSeq={jumpToPromptSeq}
        />
      )}

      {/* Parent Back Banner when inspecting a managed subagent */}
      {selectedHostId && currentAgent?.parent_agent_id && (
        <div
          style={{
            paddingRight: `${24 + overviewReservePx}px`,
            transition: overviewReserveTransition,
          }}
          className="shrink-0 pl-[24px]"
        >
          <div style={stagePushStyle}>
            <ParentSubagentBanner hostId={selectedHostId} agent={currentAgent} />
          </div>
        </div>
      )}

      {/* Subtle 1-line provenance chip when viewing a forked conversation */}
      {selectedHostId && currentAgent?.forked_from_agent_id && (
        <div
          style={{
            paddingRight: `${24 + overviewReservePx}px`,
            transition: overviewReserveTransition,
          }}
          className="shrink-0 pl-[24px]"
        >
          <div style={stagePushStyle}>
            <ForkedFromBanner hostId={selectedHostId} agent={currentAgent} />
          </div>
        </div>
      )}

      {/* 1. Unified Scrollable Conversation Viewport — scrollbar stays on far right edge */}
      <ScrollArea
        className="min-h-0 flex-1"
        viewportRef={scrollRef}
        onViewportScroll={handleScroll}
        viewportStyle={{
          paddingRight: `${24 + overviewReservePx}px`,
          transition: overviewReserveTransition,
        }}
        onViewportTransitionEnd={onStageTransitionEnd}
        viewportClassName="pt-6 pb-11 pl-[24px]"
      >
        <div
          key={selectedAgentId}
          ref={contentRef}
          onTransitionEnd={onStageTransitionEnd}
          style={stagePushStyle}
          className="mx-auto w-full min-w-0 max-w-[var(--orbit-stage-max-w,768px)]"
        >
          {stageTurns.length === 0 &&
            wasSessionRewound &&
            stageView !== null &&
            (stageView.agent?.latest_seq ?? 0) === 0 &&
            !isAgentRunning && (
              <div className="text-muted-foreground flex flex-col items-center justify-center py-20 text-center">
                <RotateCcw className="mb-2.5 size-5 opacity-45" />
                <p className="text-[13.5px]">
                  Rewound to the start of this session. Edit your prompt below to continue.
                </p>
              </div>
            )}

          {stageTurns.length > 0 && (
            <VirtualizedTurnList
              key={activeSessionScope ?? selectedAgentId}
              selectedHostId={selectedHostId}
              selectedAgentId={selectedAgentId}
              stageTurns={stageTurns}
              highlightedTurnKey={highlightedTurnKey}
              hasPendingJump={hasPendingJumpForActive}
              isLatestTurnActive={isLatestTurnActive}
              agentStatusLabel={isLatestTurnActive ? (currentAgent?.status_label ?? null) : null}
              hasLiveBackgroundTasks={stageInsights.background_tasks.length > 0}
              baselineMaxSeq={baselineMaxSeq}
              isNetworkOffline={isNetworkOffline}
              autoFoldCompleted={coalesceExploration}
              effectiveCwd={effectiveCwd}
              stageMaxContentWidthPx={stageMaxContentWidthPx}
              scrollRef={scrollRef}
              stickToBottomRef={stickToBottomRef}
              holdActiveRef={holdActiveRef}
              virtualizerRef={virtualizerRef}
              turnTimingsRef={turnTimingsRef}
              thinkingTimingsRef={thinkingTimingsRef}
              getThinkingDurationMs={getThinkingDurationMs}
              holdViewport={holdViewport}
              onRequestRewind={handleRequestRewind}
              onRequestFork={handleRequestFork}
              canForkWorktree={canForkWorktree}
              onPreviewImage={setImagePreviewTarget}
            />
          )}
        </div>
      </ScrollArea>

      {/* 2. Docked Composer Shelf — normal flex sibling below scrollRef (scrollbar never enters here) */}
      <div ref={composerShelfRef} className="bg-canvas relative z-20 shrink-0">
        {/* Upward Occlusion Fade & `38×38` [↓] Button */}
        <StageJumpLatestBanner
          scrollUiStore={scrollUiStore}
          stagePushStyle={stagePushStyle}
          dockedOverviewLayout={dockedOverviewLayout}
          onJumpLatest={scrollToLatest}
        />

        <div
          style={{
            paddingRight: `${overviewReservePx}px`,
            transition: overviewReserveTransition,
          }}
        >
          <div style={stagePushStyle}>
            <SubagentApprovalBanner hostId={selectedHostId} agentId={selectedAgentId} />
            <StickyApprovalDock
              hostId={selectedHostId}
              agentId={selectedAgentId}
              cwd={effectiveCwd}
              permissions={pendingPermissions}
              questions={pendingQuestions}
            />
            <StageDockedComposer scrollUiStore={scrollUiStore} />
          </div>
        </div>
      </div>

      {/* 3. Top-Right Viewport-Anchored Overview Card Overlay (to the left of the far-right scrollbar) */}
      <StageOverviewPanel
        canDock={canDockOverview}
        hostId={selectedHostId}
        agentId={selectedAgentId}
        cwd={effectiveCwd}
        insights={stageInsights}
        sessionTurnDiffs={sessionTurnDiffs}
        currentAgent={currentAgent}
        stageContainerRef={stageContainerRef}
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

      <PromptImageLightbox
        target={imagePreviewTarget}
        onClose={handleCloseImagePreview}
      />
    </div>
  );
}
