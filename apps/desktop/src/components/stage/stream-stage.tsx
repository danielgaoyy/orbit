import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
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
  HelpCircle,
  Layers,
  ListTodo,
  MessageSquareText,
  Quote,
  Scissors,
  Search,
  ShieldAlert,
  Sparkles,
  SquareTerminal,
  Terminal as TerminalIcon,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import {
  type DiffLineView,
  type PromptBadge,
  type QuestionItem,
  type TimelineItemView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { modSymbol } from "@/lib/keybindings";
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
import { ShortcutKbd } from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const USER_PROMPT_PREVIEW_MAX_PX = 240;
const LIVE_STEP_AUTO_COLLAPSE_THRESHOLD = 5;
const LIVE_STEP_VISIBLE_TAIL_COUNT = 3;

interface TurnTimingRecord {
  startedAtMs: number;
  completedAtMs: number | null;
  durationMs: number | null;
}

type ViewportAnchorHoldFn = (
  triggerEl: HTMLElement | null,
  apply: () => void,
  edge?: "top" | "bottom",
) => void;

function formatClockTime(ms: number): string {
  try {
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" });
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
    <div className="selectable-text border-border bg-muted/45 dark:bg-muted/25 max-h-56 overflow-auto overscroll-contain rounded-lg border py-1.5 font-mono text-[12px] leading-[1.55]">
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
              ln.kind === "hunk_header" && "text-muted-foreground/75 py-0.5 text-[11px]",
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
        <div className="text-muted-foreground px-3 pt-1 text-[11px]">
          … {lines.length - 80} more diff lines (open in Diff Review for full file)
        </div>
      )}
    </div>
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
    const headline = extractThinkingHeadline(item.text, isActivelyThinking);
    const canExpand = item.text.trim().length > 0;
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
          <pre className="selectable-text text-muted-foreground border-border my-1 ml-4 max-h-56 overflow-y-auto overscroll-contain border-l pl-3 font-mono text-[12px] leading-relaxed whitespace-pre-wrap">
            {item.text}
          </pre>
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
          <span className="selectable-text text-foreground/78">{trimmed}</span>
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
    const denied = /deny|reject|decline/i.test(item.resolved_decision ?? "");
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

  return (
    <div>
      <div
        onMouseEnter={() => setHovered(true)}
        onMouseLeave={() => setHovered(false)}
        className={cn(
          "group/tool -ml-1.5 flex items-center justify-between gap-2 rounded-md px-1.5 py-1 text-[13px] transition-colors",
          (hasExpandableDetail || canOpenFileInDiff) && "hover:bg-accent/55",
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
                  : undefined
              }
              className={cn(
                "truncate font-mono text-[12px]",
                desc.isRunning
                  ? "agent-text-shimmer"
                  : "text-muted-foreground group-hover/tool:text-foreground/70",
                canOpenFileInDiff && !hasExpandableDetail && "cursor-pointer hover:underline",
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
        </div>
      </div>

      {expanded && hasExpandableDetail && (
        <div className="my-1.5 ml-4 pl-3">
          {item.detail.kind === "shell" && (
            <pre
              className={cn(
                "selectable-text max-h-56 overflow-auto overscroll-contain rounded-lg border px-3 py-2 font-mono text-[12px] leading-relaxed whitespace-pre-wrap",
                isError
                  ? "border-rose-500/30 bg-rose-500/[0.04] text-foreground/90"
                  : "border-border bg-muted/45 dark:bg-muted/25 text-foreground/88",
              )}
            >
              <span className="text-muted-foreground select-none">$ </span>
              {item.detail.command}
              {item.detail.output_preview
                ? `\n\n${item.detail.output_preview}`
                : !desc.isRunning
                  ? "\n\n(no output)"
                  : ""}
            </pre>
          )}

          {item.detail.kind === "file_edit" && diffLines.length > 0 && (
            <InlineStepDiffPreview lines={diffLines} />
          )}

          {item.detail.kind === "subagent" && item.detail.prompt_preview && (
            <pre className="selectable-text text-muted-foreground border-border bg-muted/45 dark:bg-muted/25 max-h-44 overflow-auto overscroll-contain rounded-lg border px-3 py-2 font-mono text-[12px] whitespace-pre-wrap">
              {item.detail.prompt_preview}
            </pre>
          )}

          {item.detail.kind === "generic" && item.detail.arguments_json && (
            <pre className="selectable-text text-muted-foreground border-border bg-muted/45 dark:bg-muted/25 max-h-44 overflow-auto overscroll-contain rounded-lg border px-3 py-2 font-mono text-[12px] whitespace-pre-wrap">
              {formatJsonPreview(item.detail.arguments_json)}
            </pre>
          )}
        </div>
      )}
    </div>
  );
});

/** Vertical step connector rail for expanded or actively streaming steps. */
const StepTimelineRail = memo(function StepTimelineRail({
  items,
  cwd,
  holdViewport,
  indented = false,
  isLiveActive = false,
  autoCollapseEarlierLiveSteps = false,
  autoExpandErrors = false,
  agentId,
  thinkingTimings,
}: {
  items: TimelineItemView[];
  cwd: string;
  holdViewport: ViewportAnchorHoldFn;
  indented?: boolean;
  isLiveActive?: boolean;
  autoCollapseEarlierLiveSteps?: boolean;
  autoExpandErrors?: boolean;
  agentId: string;
  thinkingTimings: Record<string, TurnTimingRecord>;
}) {
  const [showAllLiveSteps, setShowAllLiveSteps] = useState(false);
  const collapseBtnRef = useRef<HTMLButtonElement | null>(null);

  if (items.length === 0) return null;

  const canRollUpEarlier =
    isLiveActive &&
    autoCollapseEarlierLiveSteps &&
    items.length >= LIVE_STEP_AUTO_COLLAPSE_THRESHOLD;
  const hiddenCount = canRollUpEarlier
    ? Math.max(0, items.length - LIVE_STEP_VISIBLE_TAIL_COUNT)
    : 0;
  const visibleItems =
    canRollUpEarlier && !showAllLiveSteps
      ? items.slice(items.length - LIVE_STEP_VISIBLE_TAIL_COUNT)
      : items;

  const lastSeq = items[items.length - 1]?.seq;

  return (
    <div
      className={cn(
        "space-y-0.5",
        indented && "border-border mt-1 ml-[7px] border-l pl-3.5",
      )}
    >
      {canRollUpEarlier && hiddenCount > 0 && (
        <button
          ref={collapseBtnRef}
          type="button"
          aria-expanded={showAllLiveSteps}
          onClick={() =>
            holdViewport(collapseBtnRef.current, () => setShowAllLiveSteps((v) => !v))
          }
          className="text-muted-foreground hover:text-foreground hover:bg-accent/55 -ml-1.5 inline-flex cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-1 text-[12.5px] transition-colors"
        >
          <ChevronRight
            className={cn(
              "size-3.5 stroke-[1.85] transition-transform duration-150",
              showAllLiveSteps && "rotate-90",
            )}
          />
          <span>
            {showAllLiveSteps
              ? "Hide steps"
              : `${hiddenCount} ${hiddenCount === 1 ? "step" : "steps"}`}
          </span>
        </button>
      )}

      {visibleItems.map((sub, i) => {
        const isLastInLiveGroup = isLiveActive && sub.seq === lastSeq;
        const thinkingKey =
          sub.item_type === "assistant_thinking"
            ? `${agentId}::think::${sub.item_id || sub.seq}`
            : null;
        const thoughtDurationMs = thinkingKey
          ? (thinkingTimings[thinkingKey]?.durationMs ?? null)
          : null;

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
 * Collapsible Step Group Disclosure:
 * Renders settled work batches as a single concise 1-line summary button
 * (`Edited 2 files, ran a command`, `Read omni-composer.tsx`, `Thought for 6s`),
 * expanding on click to reveal the indented vertical step timeline.
 */
const CollapsibleStepGroup = memo(function CollapsibleStepGroup({
  items,
  summaryLabel,
  summaryCounts,
  labelIncludesFailure,
  shouldFold,
  isLiveActive,
  autoCollapseLiveOverflow,
  showInlineDiffStat = false,
  cwd,
  agentId,
  thinkingTimings,
  holdViewport,
}: {
  items: TimelineItemView[];
  summaryLabel: string;
  summaryCounts: WorkSummaryResult["counts"];
  labelIncludesFailure: boolean;
  shouldFold: boolean;
  isLiveActive: boolean;
  autoCollapseLiveOverflow: boolean;
  showInlineDiffStat?: boolean;
  cwd: string;
  agentId: string;
  thinkingTimings: Record<string, TurnTimingRecord>;
  holdViewport: ViewportAnchorHoldFn;
}) {
  const [manualExpanded, setManualExpanded] = useState(false);
  const foldBtnRef = useRef<HTMLButtonElement | null>(null);

  if (items.length === 0) return null;

  if (!shouldFold) {
    return (
      <StepTimelineRail
        items={items}
        cwd={cwd}
        holdViewport={holdViewport}
        indented={items.length > 1}
        isLiveActive={isLiveActive}
        autoCollapseEarlierLiveSteps={autoCollapseLiveOverflow}
        autoExpandErrors={!isLiveActive && summaryCounts.unrecoveredFailureCount > 0}
        agentId={agentId}
        thinkingTimings={thinkingTimings}
      />
    );
  }

  const showFailureBadge =
    summaryCounts.unrecoveredFailureCount > 0 && !labelIncludesFailure;
  const showDiffNumbers =
    showInlineDiffStat && (summaryCounts.additions > 0 || summaryCounts.deletions > 0);

  return (
    <div>
      <button
        ref={foldBtnRef}
        type="button"
        aria-expanded={manualExpanded}
        onClick={() =>
          holdViewport(foldBtnRef.current, () => setManualExpanded((v) => !v))
        }
        className="group/fold text-muted-foreground hover:text-foreground hover:bg-accent/55 -ml-1.5 inline-flex max-w-full cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-0.5 text-[13.5px] font-normal transition-colors"
      >
        <span
          className={cn(
            "truncate",
            labelIncludesFailure && "text-rose-600 dark:text-rose-400",
          )}
        >
          {summaryLabel}
        </span>

        {showFailureBadge && (
          <span className="shrink-0 text-[12.5px] text-rose-600 dark:text-rose-400">
            · {summaryCounts.unrecoveredFailureCount} failed
          </span>
        )}

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

        <ChevronRight
          className={cn(
            "size-3.5 shrink-0 opacity-70 transition-transform duration-150 group-hover/fold:opacity-100",
            manualExpanded && "rotate-90",
          )}
        />
      </button>

      {manualExpanded && (
        <StepTimelineRail
          items={items}
          cwd={cwd}
          holdViewport={holdViewport}
          indented
          isLiveActive={false}
          autoExpandErrors={summaryCounts.failedCount > 0 || summaryCounts.deniedCount > 0}
          agentId={agentId}
          thinkingTimings={thinkingTimings}
        />
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

/**
 * Extracts clean user-visible text and semantic chips from `user_prompt`,
 * including backwards-compatible cleanup for older turns stored before structured `badges`.
 */
function extractUserPromptPresentation(
  item: Extract<TimelineItemView, { item_type: "user_prompt" }>,
): {
  cleanText: string;
  badges: PromptBadge[];
  sanitizedLegacy: boolean;
} {
  if (item.badges && item.badges.length > 0) {
    return {
      cleanText: item.text.trim(),
      badges: item.badges,
      sanitizedLegacy: false,
    };
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

  return {
    cleanText: working,
    badges: inferredBadges,
    sanitizedLegacy: sanitized,
  };
}

/**
 * Orbit Native User Prompt Bubble:
 * - Crisp cool-neutral Orbit surface (`bg-secondary` + `border-border`) aligned flush right.
 * - Renders semantic context & slash workflow tokens (`/plan`, `/review`, `@file`, `Diff`)
 *   without exposing hidden wire scaffolding.
 * - Capped at `240px` height with smooth bottom EdgeFade + `Show more / Show less`.
 * - Quiet hover `[Copy]` action without layout shift.
 */
const UserPromptBubble = memo(function UserPromptBubble({
  item,
  holdViewport,
}: {
  item: Extract<TimelineItemView, { item_type: "user_prompt" }>;
  holdViewport: ViewportAnchorHoldFn;
}) {
  const [expanded, setExpanded] = useState(false);
  const [overflowing, setOverflowing] = useState(false);
  const [copied, setCopied] = useState(false);
  const bodyRef = useRef<HTMLDivElement | null>(null);
  const toggleBtnRef = useRef<HTMLButtonElement | null>(null);

  const { cleanText, badges, sanitizedLegacy } = useMemo(
    () => extractUserPromptPresentation(item),
    [item],
  );

  useLayoutEffect(() => {
    const el = bodyRef.current;
    if (!el) return;
    setOverflowing(el.scrollHeight > USER_PROMPT_PREVIEW_MAX_PX + 12);
  }, [cleanText, badges.length]);

  const copyPayload = cleanText || badges.map((b) => b.label).join(" ");

  const handleCopy = async () => {
    await navigator.clipboard.writeText(copyPayload);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1400);
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

  return (
    <div className="group/user flex flex-col items-end pt-2">
      <div className="prompt-bubble-surface text-foreground relative max-w-[82%] rounded-[22px] px-5 py-3">
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
            <div className="flex flex-wrap items-center gap-1.5">
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
            <p className="selectable-text text-foreground text-[14.5px] leading-[1.68] whitespace-pre-wrap">
              <span className="mr-1.5 inline-flex items-center gap-1.5 align-baseline">
                <SlashIcon className="text-muted-foreground relative top-[2px] size-[14px] shrink-0 stroke-[1.75]" />
                <span className="text-foreground font-medium">{slashLabel}</span>
              </span>
              {cleanText}
            </p>
          ) : (
            cleanText.length > 0 &&
            (sanitizedLegacy ? (
              <p className="selectable-text text-foreground text-[14.5px] leading-[1.68] whitespace-pre-wrap">
                {cleanText}
              </p>
            ) : (
              <MarkdownBlocksView blocks={item.blocks} />
            ))
          )}

          {!expanded && overflowing && (
            <div
              aria-hidden="true"
              className="pointer-events-none absolute inset-x-0 bottom-0 h-12"
              style={{
                background:
                  "linear-gradient(to top, var(--surface-elevated) 0%, transparent 100%)",
              }}
            />
          )}
        </div>

        {overflowing && (
          <button
            ref={toggleBtnRef}
            type="button"
            onClick={() =>
              holdViewport(toggleBtnRef.current, () => setExpanded((v) => !v), "bottom")
            }
            className="text-muted-foreground hover:text-foreground mt-1 inline-flex cursor-pointer items-center text-[12px] font-medium transition-colors"
          >
            {expanded ? "Show less" : "Show more"}
          </button>
        )}
      </div>

      <div className="mt-1 flex h-4 items-center pr-1 opacity-0 transition-opacity duration-150 group-hover/user:opacity-100">
        <button
          type="button"
          title="Copy prompt"
          onClick={() => void handleCopy()}
          className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 rounded px-1 py-0.5 text-[11.5px] transition-colors"
        >
          {copied ? (
            <Check className="size-3 text-emerald-600 dark:text-emerald-400" />
          ) : (
            <Copy className="size-3" />
          )}
        </button>
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

/** Isolated 1-second live elapsed ticker so parent rows never re-render on ticks. */
const LiveTurnElapsed = memo(function LiveTurnElapsed({ startedAtMs }: { startedAtMs: number }) {
  const [nowMs, setNowMs] = useState(() => Date.now());

  useEffect(() => {
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [startedAtMs]);

  const elapsedSec = Math.max(0, Math.floor((nowMs - startedAtMs) / 1000));
  const mins = Math.floor(elapsedSec / 60);
  const secs = elapsedSec % 60;
  const formatted = `${mins}:${String(secs).padStart(2, "0")}`;

  return (
    <span className="text-muted-foreground tabular-nums flex items-center gap-2 text-[12px]">
      <span className="bg-muted-foreground/70 size-1.5 animate-pulse rounded-full" />
      <span>{formatted}</span>
    </span>
  );
});

/**
 * Constant-height (`24px`) Turn Footer:
 * Swaps cleanly between running elapsed timer (`0:14`) and completed `[Copy · duration]`
 * with `0px` vertical layout shift.
 */
const TurnFooter = memo(function TurnFooter({
  isTurnActive,
  timing,
  replyText,
}: {
  isTurnActive: boolean;
  timing?: TurnTimingRecord;
  replyText: string | null;
}) {
  const [copied, setCopied] = useState(false);
  const [hoverTime, setHoverTime] = useState(false);

  if (!isTurnActive && !replyText && !timing?.durationMs) {
    return null;
  }

  const handleCopy = async () => {
    if (!replyText) return;
    await navigator.clipboard.writeText(replyText);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  };

  const durationText =
    timing?.durationMs && timing.durationMs >= 1000
      ? formatDurationCompact(timing.durationMs)
      : null;
  const completedClock = timing?.completedAtMs ? formatClockTime(timing.completedAtMs) : null;

  return (
    <div className="mt-1.5 flex h-6 items-center gap-2">
      {isTurnActive ? (
        <LiveTurnElapsed startedAtMs={timing?.startedAtMs ?? Date.now()} />
      ) : (
        <>
          {replyText && (
            <button
              type="button"
              title="Copy response"
              onClick={() => void handleCopy()}
              className="text-muted-foreground hover:text-foreground hover:bg-accent/65 -ml-1 inline-flex size-6 cursor-pointer items-center justify-center rounded-md transition-colors"
            >
              {copied ? (
                <Check className="size-3.5 text-emerald-600 dark:text-emerald-400" />
              ) : (
                <Copy className="size-3.5" />
              )}
            </button>
          )}

          {durationText && (
            <span
              onMouseEnter={() => setHoverTime(true)}
              onMouseLeave={() => setHoverTime(false)}
              className="text-muted-foreground tabular-nums text-[12px] select-none"
            >
              {hoverTime && completedClock ? completedClock : durationText}
            </span>
          )}
        </>
      )}
    </div>
  );
});

const CompactionDivider = memo(function CompactionDivider({ label }: { label?: string }) {
  return (
    <div className="text-muted-foreground flex items-center gap-3 px-[10px] py-3 text-[12px]">
      <div className="border-border flex-1 border-t" />
      <span className="inline-flex items-center gap-1.5">
        <Scissors className="size-3 opacity-75" />
        <span>{label || "Context compacted"}</span>
      </span>
      <div className="border-border flex-1 border-t" />
    </div>
  );
});

function computeTotalThinkingDurationMs(
  items: TimelineItemView[],
  agentId: string,
  thinkingTimings: Record<string, TurnTimingRecord>,
): number | null {
  let total = 0;
  let foundAny = false;
  for (const it of items) {
    if (it.item_type !== "assistant_thinking") continue;
    const rec = thinkingTimings[`${agentId}::think::${it.item_id || it.seq}`];
    if (rec?.durationMs !== null && rec?.durationMs !== undefined) {
      total += rec.durationMs;
      foundAny = true;
    }
  }
  return foundAny ? total : null;
}

/**
 * Renders a complete Conversation Turn:
 * - Pre-reply steps fold automatically into a 1-line summary header (`Edited 2 files, ran a command ›`)
 *   the moment `terminalReply` begins, preventing any layout jump when streaming completes.
 * - During multi-phase runs (`[Steps 1] -> [Note] -> [Steps 2]`), settled earlier `step_group` batches
 *   auto-fold immediately while the trailing active `step_group` streams live at the bottom.
 * - Unrecovered tool failures without a final reply stay expanded so error output is immediately visible.
 */
const TurnStageSection = memo(function TurnStageSection({
  turn,
  isTurnActive,
  autoFoldCompleted,
  cwd,
  agentId,
  timing,
  thinkingTimings,
  holdViewport,
  registerTurnEl,
}: {
  turn: StageTurn;
  isTurnActive: boolean;
  autoFoldCompleted: boolean;
  cwd: string;
  agentId: string;
  timing?: TurnTimingRecord;
  thinkingTimings: Record<string, TurnTimingRecord>;
  holdViewport: ViewportAnchorHoldFn;
  registerTurnEl: (seq: number, el: HTMLDivElement | null) => void;
}) {
  const setRef = useCallback(
    (el: HTMLDivElement | null) => {
      registerTurnEl(turn.anchorSeq, el);
    },
    [registerTurnEl, turn.anchorSeq],
  );

  if (turn.isCompactionCommand && turn.liveSegments.length === 0) {
    return (
      <div ref={setRef}>
        <CompactionDivider label="Context compacted" />
      </div>
    );
  }

  // Fold pre-reply work as soon as `terminalReply` starts (all prior steps are already done)
  const canFoldPreReplyWork =
    autoFoldCompleted && turn.terminalReply !== null && turn.foldedSteps.length > 0;

  const preReplySummaryLabel = useMemo(() => {
    if (turn.workCounts.actionSteps === 0 && turn.workCounts.thoughtCount > 0) {
      const thoughtMs = computeTotalThinkingDurationMs(
        turn.foldedSteps,
        agentId,
        thinkingTimings,
      );
      return formatThoughtSummaryLabel(thoughtMs, turn.workCounts.thoughtCount);
    }
    return turn.workSummaryLabel;
  }, [
    agentId,
    thinkingTimings,
    turn.foldedSteps,
    turn.workCounts.actionSteps,
    turn.workCounts.thoughtCount,
    turn.workSummaryLabel,
  ]);

  return (
    <div ref={setRef} className="space-y-3">
      {turn.userPrompt && (
        <UserPromptBubble item={turn.userPrompt} holdViewport={holdViewport} />
      )}

      {(turn.liveSegments.length > 0 || isTurnActive) && (
        <div className="px-[10px]">
          {canFoldPreReplyWork ? (
            <div className="space-y-2">
              <CollapsibleStepGroup
                items={turn.foldedSteps}
                summaryLabel={preReplySummaryLabel}
                summaryCounts={turn.workCounts}
                labelIncludesFailure={turn.workSummaryIncludesFailure}
                shouldFold
                isLiveActive={false}
                autoCollapseLiveOverflow={autoFoldCompleted}
                cwd={cwd}
                agentId={agentId}
                thinkingTimings={thinkingTimings}
                holdViewport={holdViewport}
              />

              {turn.notices.map((n) => (
                <div
                  key={`notice-${n.seq}`}
                  className="text-muted-foreground border-border bg-muted/50 flex items-center gap-2 rounded-xl border px-3.5 py-2 text-[13px]"
                >
                  <AlertTriangle className="size-4 shrink-0 text-amber-600 dark:text-amber-400" />
                  <span>{n.message}</span>
                </div>
              ))}

              {turn.terminalReply && (
                <div className="pt-0.5">
                  <MarkdownBlocksView
                    blocks={turn.terminalReply.blocks}
                    streaming={turn.terminalReply.streaming}
                  />
                </div>
              )}

              {!isTurnActive && <TurnChangesBar diffs={turn.turnDiffs} cwd={cwd} />}

              <TurnFooter
                isTurnActive={isTurnActive}
                timing={timing}
                replyText={!isTurnActive ? (turn.terminalReply?.text ?? null) : null}
              />
            </div>
          ) : (
            <div className="space-y-2">
              {turn.liveSegments.map((seg, segIdx) => {
                switch (seg.kind) {
                  case "step_group": {
                    const isTrailingSeg = segIdx === turn.liveSegments.length - 1;
                    const isSegLiveActive =
                      isTurnActive && isTrailingSeg && !seg.hasFollowingContent;
                    const segSettled = !isSegLiveActive && !seg.hasRunningStep;
                    // Keep an unrecovered trailing error open when there is no final reply after it
                    const keepOpenForTrailingFailure =
                      !seg.hasFollowingContent &&
                      seg.summary.counts.unrecoveredFailureCount > 0;
                    const shouldFoldSeg =
                      autoFoldCompleted && segSettled && !keepOpenForTrailingFailure;

                    const segLabel =
                      seg.summary.counts.actionSteps === 0 &&
                      seg.summary.counts.thoughtCount > 0
                        ? formatThoughtSummaryLabel(
                            computeTotalThinkingDurationMs(
                              seg.items,
                              agentId,
                              thinkingTimings,
                            ),
                            seg.summary.counts.thoughtCount,
                          )
                        : seg.summary.label;

                    return (
                      <CollapsibleStepGroup
                        key={seg.key}
                        items={seg.items}
                        summaryLabel={segLabel}
                        summaryCounts={seg.summary.counts}
                        labelIncludesFailure={seg.summary.labelIncludesFailure}
                        shouldFold={shouldFoldSeg}
                        isLiveActive={isSegLiveActive}
                        autoCollapseLiveOverflow={autoFoldCompleted}
                        showInlineDiffStat={isTurnActive}
                        cwd={cwd}
                        agentId={agentId}
                        thinkingTimings={thinkingTimings}
                        holdViewport={holdViewport}
                      />
                    );
                  }
                  case "assistant_message":
                    return (
                      <div key={seg.key} className="py-0.5">
                        <MarkdownBlocksView
                          blocks={seg.item.blocks}
                          streaming={seg.item.streaming}
                        />
                      </div>
                    );
                  case "compaction":
                    return <CompactionDivider key={seg.key} label={seg.label} />;
                  case "notice":
                    return (
                      <div
                        key={seg.key}
                        className="text-muted-foreground border-border bg-muted/50 flex items-center gap-2 rounded-xl border px-3.5 py-2 text-[13px]"
                      >
                        <AlertTriangle className="size-4 shrink-0 text-amber-600 dark:text-amber-400" />
                        <span>{seg.item.message}</span>
                      </div>
                    );
                }
              })}

              {!isTurnActive && <TurnChangesBar diffs={turn.turnDiffs} cwd={cwd} />}

              <TurnFooter
                isTurnActive={isTurnActive}
                timing={timing}
                replyText={!isTurnActive ? (turn.terminalReply?.text ?? null) : null}
              />
            </div>
          )}
        </div>
      )}
    </div>
  );
});

/**
 * Peripheral left-margin Turn Outline Rail:
 * Appears only on wide viewports (`>= 940px` stage width) when `>= 2` user prompts exist,
 * allowing zero-clutter jump-to-turn navigation in long sessions.
 */
const ChatOutlineRail = memo(function ChatOutlineRail({
  prompts,
  activeSeq,
  onJumpToSeq,
}: {
  prompts: { seq: number; preview: string }[];
  activeSeq: number | null;
  onJumpToSeq: (seq: number) => void;
}) {
  const [hoveredIdx, setHoveredIdx] = useState<number | null>(null);

  if (prompts.length < 2) return null;

  return (
    <nav
      aria-label="Conversation outline"
      onMouseLeave={() => setHoveredIdx(null)}
      className="pointer-events-auto absolute top-[14%] bottom-[16%] left-2 z-20 flex w-7 flex-col items-start justify-center"
    >
      {prompts.map((p, idx) => {
        const isActive = p.seq === activeSeq;
        const dist = hoveredIdx === null ? 99 : Math.abs(idx - hoveredIdx);
        const mag = dist === 0 ? 1 : dist === 1 ? 0.48 : dist === 2 ? 0.18 : 0;
        const baseW = isActive ? 14 : 8;
        const pillW = Math.round(baseW + mag * (22 - baseW));
        const pillH = dist === 0 ? 3 : 2;

        return (
          <div
            key={p.seq}
            onMouseEnter={() => setHoveredIdx(idx)}
            className="relative flex max-h-3.5 min-h-1.5 w-full flex-1 items-center pl-1"
          >
            <button
              type="button"
              aria-label={`Jump to prompt ${idx + 1}: ${p.preview}`}
              onClick={() => onJumpToSeq(p.seq)}
              className="flex h-full w-full cursor-pointer items-center justify-start outline-none"
            >
              <span
                style={{ width: `${pillW}px`, height: `${pillH}px` }}
                className={cn(
                  "rounded-full transition-all duration-150 ease-out",
                  dist === 0
                    ? "bg-foreground"
                    : isActive
                      ? "bg-foreground/45"
                      : "bg-foreground/18",
                )}
              />
            </button>

            {hoveredIdx === idx && (
              <div className="bg-popover text-popover-foreground border-border floating-surface pointer-events-none absolute top-1/2 left-8 z-30 w-56 -translate-y-1/2 rounded-xl border px-3 py-2 text-[12px] leading-snug">
                <div className="line-clamp-2 break-words">{p.preview}</div>
              </div>
            )}
          </div>
        );
      })}
    </nav>
  );
});

function StickyQuestionDockCard({
  hostId,
  agentId,
  requestId,
  questions,
}: {
  hostId: string;
  agentId: string;
  requestId: string;
  questions: QuestionItem[];
}) {
  const [selections, setSelections] = useState<Record<number, string[]>>(() => {
    const init: Record<number, string[]> = {};
    questions.forEach((q, idx) => {
      if (q.options.length > 0) init[idx] = [q.options[0]];
    });
    return init;
  });

  const toggleOption = (qIdx: number, opt: string, multi: boolean) => {
    setSelections((prev) => {
      const cur = prev[qIdx] ?? [];
      if (!multi) return { ...prev, [qIdx]: [opt] };
      return {
        ...prev,
        [qIdx]: cur.includes(opt) ? cur.filter((x) => x !== opt) : [...cur, opt],
      };
    });
  };

  const handleSubmit = () => {
    const ordered = questions.map((_, idx) => selections[idx] ?? []);
    void orbitIpc.answerQuestion(hostId, agentId, requestId, ordered);
  };

  return (
    <div className="bg-elevated rounded-2xl border border-amber-500/35 p-4 shadow-sm">
      <div className="mb-2 flex items-center gap-1.5 text-[13px] font-medium text-amber-700 dark:text-amber-300">
        <HelpCircle className="size-4" />
        <span>Agent needs clarification</span>
      </div>
      <div className="space-y-3">
        {questions.map((q, qIdx) => (
          <div key={q.id || qIdx} className="space-y-1.5">
            <div className="text-foreground text-[13.5px] font-medium">{q.question}</div>
            <div className="flex flex-wrap gap-1.5">
              {q.options.map((opt, optIdx) => {
                const chosen = selections[qIdx]?.includes(opt);
                return (
                  <button
                    key={opt}
                    type="button"
                    onClick={() => toggleOption(qIdx, opt, q.is_multi_select)}
                    className={cn(
                      "inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-[13px] transition-colors",
                      chosen
                        ? "border-amber-500/50 bg-amber-500/15 text-foreground font-medium"
                        : "border-border bg-background/70 text-muted-foreground hover:text-foreground",
                    )}
                  >
                    <span className="font-mono text-[11px] opacity-60">{optIdx + 1}</span>
                    <span>{opt}</span>
                  </button>
                );
              })}
            </div>
          </div>
        ))}
        <div className="flex justify-end">
          <button
            type="button"
            onClick={handleSubmit}
            className="bg-foreground text-background inline-flex h-8 cursor-pointer items-center rounded-lg px-3.5 text-[13px] font-medium transition-opacity hover:opacity-90"
          >
            Submit Response
          </button>
        </div>
      </div>
    </div>
  );
}

function StickyApprovalDock({
  hostId,
  agentId,
  permissions,
  questions,
}: {
  hostId: string;
  agentId: string;
  permissions: Extract<TimelineItemView, { item_type: "permission_request" }>[];
  questions: Extract<TimelineItemView, { item_type: "question_request" }>[];
}) {
  const mod = modSymbol();
  if (permissions.length === 0 && questions.length === 0) return null;

  return (
    <div className="w-full px-[24px] pb-2.5">
      <div className="mx-auto w-full max-w-[768px] space-y-2">
        {permissions.map((perm, idx) => {
          const decisions =
            perm.allowed_decisions.length > 0
              ? perm.allowed_decisions
              : ["allow", "allow_always", "deny"];
          const targetPreview =
            perm.detail.kind === "shell"
              ? `$ ${perm.detail.command}`
              : perm.detail.kind === "file_edit" || perm.detail.kind === "file_read"
                ? perm.detail.path
                : JSON.stringify(perm.detail);

          return (
            <div
              key={perm.request_id}
              className="bg-elevated flex flex-col gap-2.5 rounded-2xl border border-amber-500/35 px-4 py-3 shadow-xs sm:flex-row sm:items-center sm:justify-between"
            >
              <div className="min-w-0 space-y-0.5">
                <div className="flex items-center gap-1.5 text-[13px] font-medium text-amber-700 dark:text-amber-300">
                  <ShieldAlert className="size-4 shrink-0" />
                  <span className="truncate">{perm.title}</span>
                </div>
                <div className="text-foreground/90 truncate font-mono text-[12px]">
                  {targetPreview}
                </div>
              </div>

              <div className="flex shrink-0 items-center gap-1.5">
                {decisions.map((dec) => {
                  const isDeny = dec.toLowerCase().includes("deny");
                  const isAlways = dec.toLowerCase().includes("always");
                  return (
                    <button
                      key={dec}
                      type="button"
                      onClick={() =>
                        void orbitIpc.resolvePermission(hostId, agentId, perm.request_id, dec)
                      }
                      className={cn(
                        "inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12.5px] font-medium transition-colors",
                        isDeny
                          ? "border-rose-500/30 bg-rose-500/10 text-rose-600 hover:bg-rose-500/20 dark:text-rose-300"
                          : isAlways
                            ? "border-border bg-secondary text-secondary-foreground hover:bg-accent"
                            : "bg-foreground text-background border-transparent hover:opacity-90",
                      )}
                    >
                      <span>{dec.replace("_", " ")}</span>
                      {idx === 0 && !isDeny && !isAlways && (
                        <ShortcutKbd
                          keys={`${mod}Y`}
                          kbdClassName="bg-background/20 text-background dark:bg-background/20"
                        />
                      )}
                      {idx === 0 && isAlways && <ShortcutKbd keys={`${mod}⇧Y`} />}
                      {idx === 0 && isDeny && (
                        <ShortcutKbd
                          keys={`${mod}⌫`}
                          kbdClassName="bg-rose-500/15 text-rose-600 dark:bg-rose-500/20 dark:text-rose-200"
                        />
                      )}
                    </button>
                  );
                })}
              </div>
            </div>
          );
        })}

        {questions.map((q) => (
          <StickyQuestionDockCard
            key={q.request_id}
            hostId={hostId}
            agentId={agentId}
            requestId={q.request_id}
            questions={q.questions}
          />
        ))}
      </div>
    </div>
  );
}

function NewSessionLaunchpad() {
  return (
    <div className="flex h-full w-full flex-col justify-center py-12 pr-[18px] pb-[10vh] pl-[24px]">
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
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const cachedTimeline = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? (s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`] ?? null)
      : null,
  );

  const stageContainerRef = useRef<HTMLDivElement | null>(null);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const stickToBottomRef = useRef<boolean>(true);
  const programmaticScrollRef = useRef<boolean>(false);
  const programmaticScrollTimerRef = useRef<number | null>(null);
  const turnElementMapRef = useRef<Map<number, HTMLDivElement>>(new Map());
  const turnTimingsRef = useRef<Record<string, TurnTimingRecord>>({});
  const thinkingTimingsRef = useRef<Record<string, TurnTimingRecord>>({});

  const [isOccludingText, setIsOccludingText] = useState(false);
  const [showJumpLatest, setShowJumpLatest] = useState(false);
  const [stageWidth, setStageWidth] = useState(0);
  const [activePromptSeq, setActivePromptSeq] = useState<number | null>(null);
  const [, setTimingTick] = useState(0);

  const currentAgent = useMemo(
    () =>
      hostDetail?.agents.find((a) => a.agent_id === selectedAgentId) ??
      cachedTimeline?.agent ??
      null,
    [hostDetail, selectedAgentId, cachedTimeline?.agent],
  );
  const isAgentRunning = Boolean(currentAgent?.is_running);

  const stage = useMemo(
    () => processTimelineForStage(cachedTimeline?.items ?? [], coalesceExploration),
    [cachedTimeline?.items, coalesceExploration],
  );

  const latestTurn = stage.turns[stage.turns.length - 1] ?? null;
  const isLatestTurnActive = Boolean(
    latestTurn && (isAgentRunning || latestTurn.hasActiveItem),
  );

  // Record live turn & per-thinking-block start/completion durations for the active session
  useEffect(() => {
    if (!selectedAgentId || !latestTurn) return;
    let changed = false;
    const now = Date.now();

    const timingKey = `${selectedAgentId}::${latestTurn.key}`;
    const existing = turnTimingsRef.current[timingKey];

    if (isLatestTurnActive) {
      if (!existing || existing.completedAtMs !== null) {
        turnTimingsRef.current[timingKey] = {
          startedAtMs: now,
          completedAtMs: null,
          durationMs: null,
        };
        changed = true;
      }
    } else if (existing && existing.completedAtMs === null) {
      turnTimingsRef.current[timingKey] = {
        ...existing,
        completedAtMs: now,
        durationMs: Math.max(0, now - existing.startedAtMs),
      };
      changed = true;
    }

    const rawItems = cachedTimeline?.items ?? [];
    const lastRawItem = rawItems[rawItems.length - 1] ?? null;

    for (const item of rawItems) {
      if (item.item_type !== "assistant_thinking") continue;
      if (item.seq < latestTurn.anchorSeq) continue;

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
          };
          changed = true;
        }
      } else if (prevThink && prevThink.completedAtMs === null) {
        thinkingTimingsRef.current[thinkKey] = {
          ...prevThink,
          completedAtMs: now,
          durationMs: Math.max(0, now - prevThink.startedAtMs),
        };
        changed = true;
      }
    }

    if (changed) {
      setTimingTick((n) => n + 1);
    }
  }, [selectedAgentId, latestTurn, isLatestTurnActive, cachedTimeline?.items]);

  // Measure stage width so peripheral ChatOutlineRail hides cleanly on narrow windows
  useEffect(() => {
    const el = stageContainerRef.current;
    if (!el || typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0;
      setStageWidth(Math.round(w));
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, [isDraft, selectedAgentId]);

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

  useEffect(() => {
    return () => {
      if (programmaticScrollTimerRef.current !== null) {
        window.clearTimeout(programmaticScrollTimerRef.current);
      }
    };
  }, []);

  const syncScrollMetrics = useCallback(() => {
    const el = scrollRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;

    if (programmaticScrollRef.current) {
      if (distanceFromBottom < 8) {
        programmaticScrollRef.current = false;
        if (programmaticScrollTimerRef.current !== null) {
          window.clearTimeout(programmaticScrollTimerRef.current);
          programmaticScrollTimerRef.current = null;
        }
      }
      stickToBottomRef.current = true;
      setIsOccludingText(false);
      setShowJumpLatest(false);
    } else {
      const nearBottom = distanceFromBottom < 48;
      stickToBottomRef.current = nearBottom;
      setIsOccludingText(distanceFromBottom > 4 && stage.turns.length > 0);
      setShowJumpLatest(!nearBottom && stage.turns.length > 0);
    }

    // Update active turn in peripheral outline rail
    if (outlinePrompts.length >= 2) {
      const viewportAnchorY = el.getBoundingClientRect().top + 120;
      let currentSeq = outlinePrompts[0].seq;
      for (const p of outlinePrompts) {
        const node = turnElementMapRef.current.get(p.seq);
        if (node && node.getBoundingClientRect().top <= viewportAnchorY) {
          currentSeq = p.seq;
        }
      }
      setActivePromptSeq(currentSeq);
    }
  }, [outlinePrompts, stage.turns.length]);

  const scrollToLatest = () => {
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
  };

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

  useEffect(() => {
    const el = scrollRef.current;
    if (!el) return;
    if (stickToBottomRef.current) {
      el.scrollTop = el.scrollHeight;
    }
    syncScrollMetrics();
  }, [cachedTimeline?.latestSeq, stage.turns.length, selectedAgentId, syncScrollMetrics]);

  if (!selectedHostId) {
    return (
      <div className="text-muted-foreground flex flex-1 items-center justify-center text-[13.5px]">
        Pair or select an Orbit host to begin.
      </div>
    );
  }

  if (isDraft || !selectedAgentId) {
    return (
      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto [scrollbar-gutter:stable]">
        <NewSessionLaunchpad />
      </div>
    );
  }

  return (
    <div
      ref={stageContainerRef}
      className="relative flex min-h-0 flex-1 flex-col overflow-hidden"
    >
      {/* Peripheral Megathread Outline Rail (wide screens >= 940px only) */}
      {stageWidth >= 940 && outlinePrompts.length >= 2 && (
        <ChatOutlineRail
          prompts={outlinePrompts}
          activeSeq={activePromptSeq ?? outlinePrompts[outlinePrompts.length - 1]?.seq ?? null}
          onJumpToSeq={jumpToPromptSeq}
        />
      )}

      {/* 1. Scrollable Conversation Viewport — ends strictly ABOVE the Composer shelf */}
      <div
        ref={scrollRef}
        onScroll={syncScrollMetrics}
        onWheel={(e) => {
          if (e.deltaY < 0 && programmaticScrollRef.current) {
            programmaticScrollRef.current = false;
            if (programmaticScrollTimerRef.current !== null) {
              window.clearTimeout(programmaticScrollTimerRef.current);
              programmaticScrollTimerRef.current = null;
            }
          }
        }}
        className="min-h-0 flex-1 overflow-y-auto [scrollbar-gutter:stable] pt-5 pr-[18px] pb-3 pl-[24px]"
      >
        <div className="mx-auto w-full max-w-[768px] space-y-6">
          {stage.turns.map((turn, idx) => {
            const isLast = idx === stage.turns.length - 1;
            const turnActive = isLast && isLatestTurnActive;
            const timing = turnTimingsRef.current[`${selectedAgentId}::${turn.key}`];
            return (
              <TurnStageSection
                key={turn.key}
                turn={turn}
                isTurnActive={turnActive}
                autoFoldCompleted={coalesceExploration}
                cwd={selectedCwd || currentAgent?.cwd || ""}
                agentId={selectedAgentId}
                timing={timing}
                thinkingTimings={thinkingTimingsRef.current}
                holdViewport={holdViewport}
                registerTurnEl={registerTurnEl}
              />
            );
          })}
        </div>
      </div>

      {/* 2. Docked Composer Shelf — normal flex sibling below scrollRef (scrollbar never enters here) */}
      <div className="bg-canvas relative z-20 shrink-0">
        {/* Upward Occlusion Fade & `34×34` [↓] Button — `right-[12px]` keeps the right scrollbar 100% clear */}
        <div className="pointer-events-none absolute right-[12px] bottom-full left-0 h-7">
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
          <button
            type="button"
            aria-label="Scroll to latest"
            aria-hidden={!showJumpLatest}
            tabIndex={showJumpLatest ? 0 : -1}
            onClick={scrollToLatest}
            className={cn(
              "border-border bg-elevated text-muted-foreground hover:text-foreground hover:border-foreground/20 absolute bottom-2 left-[calc(50%+6px)] inline-flex size-8.5 -translate-x-1/2 cursor-pointer items-center justify-center rounded-full border shadow-[0_2px_8px_rgba(0,0,0,0.08)] transition-all duration-150 ease-out dark:shadow-[0_2px_12px_rgba(0,0,0,0.36)]",
              showJumpLatest
                ? "pointer-events-auto translate-y-0 scale-100 opacity-100"
                : "pointer-events-none translate-y-1 scale-95 opacity-0",
            )}
          >
            <ArrowDown className="size-4 stroke-[1.9]" />
          </button>
        </div>

        <StickyApprovalDock
          hostId={selectedHostId}
          agentId={selectedAgentId}
          permissions={stage.pendingPermissions}
          questions={stage.pendingQuestions}
        />
        <OmniComposer variant="docked" occluding={isOccludingText} />
      </div>
    </div>
  );
}
