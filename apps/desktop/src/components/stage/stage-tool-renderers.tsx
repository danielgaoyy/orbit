import { memo, useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowUpRight,
  Check,
  ChevronRight,
  FileCode2,
  FilePlus2,
  FileSearch,
  FileSymlink,
  FileText,
  Globe,
  Layers,
  MessageSquareText,
  Search,
  Sparkles,
  Terminal as TerminalIcon,
  Wrench,
  type LucideIcon,
} from "lucide-react";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { DiffLineView, TimelineItemView } from "@/lib/orbit-ipc";
import {
  type ResolvedSubagentItem,
  extractSubagentMessageCall,
  resolveToolCallSubagent,
} from "@/lib/subagents";
import {
  classifySearchTool,
  describeToolStep,
  extractThinkingHeadline,
  formatDurationCompact,
  formatThoughtSummaryLabel,
} from "@/lib/timeline-coalesce";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";
import {
  EMPTY_SUBAGENTS,
  type ViewportAnchorHoldFn,
  formatJsonPreview,
  parseUnifiedDiffFallback,
} from "./stage-helpers";

/** Inline compact diff preview used when expanding an `Edited` / `Created` step inside a turn. */
export const InlineStepDiffPreview = memo(function InlineStepDiffPreview({
  lines,
}: {
  lines: DiffLineView[];
}) {
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
              ln.kind === "hunk_header" &&
                "text-muted-foreground/75 py-0.5 text-[11px] select-none",
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
});

export function resolveToolStepIcon(
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

/** Isolated 1-second live elapsed ticker with offline & silent-stall hints. */
export const InlineLiveElapsed = memo(function InlineLiveElapsed({
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

/**
 * Borderless step row for reasoning, intermediate notes, permissions, and tool calls:
 * - Zero box/border clutter at rest; tight vertical grouping.
 * - Pure-CSS `group-hover` swap between semantic icon and `ChevronRight` (0 React hover re-renders during scroll).
 * - Active thinking/tool step sweeps a calm linear-gradient text shimmer.
 */
export const StepItemRow = memo(function StepItemRow({
  item,
  hostId,
  agentId,
  cwd,
  holdViewport,
  isActivelyThinking = false,
  thoughtDurationMs = null,
  autoExpandError = false,
  sessionSubagents,
  onOpenSubagent,
}: {
  item: TimelineItemView;
  hostId?: string;
  agentId?: string;
  cwd: string;
  holdViewport: ViewportAnchorHoldFn;
  isActivelyThinking?: boolean;
  thoughtDurationMs?: number | null;
  autoExpandError?: boolean;
  sessionSubagents?: readonly ResolvedSubagentItem[];
  onOpenSubagent?: (subagent: ResolvedSubagentItem) => void;
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
  const resolvedSubagentMatch = resolveToolCallSubagent(
    item,
    sessionSubagents ?? EMPTY_SUBAGENTS,
    hostId,
  );
  const isSubagentStep = item.detail.kind === "subagent" || resolvedSubagentMatch !== null;
  const handleOpenResolvedSubagent = () => {
    if (!resolvedSubagentMatch) return;
    if (onOpenSubagent) {
      onOpenSubagent(resolvedSubagentMatch.subagent);
      return;
    }
    const sa = resolvedSubagentMatch.subagent;
    orbitActions.openSubagentTab({
      callId: sa.callId,
      role: sa.title,
      promptPreview: sa.promptPreview,
      childAgentId: sa.childAgentId,
      childHostId: sa.childHostId || undefined,
      providerSubagentId: sa.providerSubagentId,
      parentAgentId: agentId || undefined,
    });
  };
  const displayTarget =
    desc.target || (resolvedSubagentMatch ? resolvedSubagentMatch.subagent.title : "");
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

          {displayTarget &&
            (resolvedSubagentMatch ? (
              <button
                type="button"
                title="Open subagent details"
                onClick={(e) => {
                  e.stopPropagation();
                  handleOpenResolvedSubagent();
                }}
                className={cn(
                  "min-w-0 cursor-pointer truncate text-left text-[12.5px] font-medium hover:underline underline-offset-2",
                  desc.isRunning
                    ? "agent-text-shimmer"
                    : "text-foreground/82 group-hover/tool:text-foreground",
                )}
              >
                {displayTarget}
              </button>
            ) : canOpenTargetDirectly ? (
              <button
                type="button"
                title={
                  canOpenFileInDiff
                    ? `Open ${displayTarget} diff`
                    : `Open ${displayTarget} in Side Panel`
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
                {displayTarget}
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
                {displayTarget}
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

          {resolvedSubagentMatch && (
            <button
              type="button"
              title="Open subagent details"
              onClick={(e) => {
                e.stopPropagation();
                handleOpenResolvedSubagent();
              }}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded p-0.5 opacity-0 transition-opacity group-hover/tool:opacity-100"
            >
              <ArrowUpRight className="size-3.5" />
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

          {item.detail.kind === "generic" && item.detail.arguments_json && (() => {
            const msgCall = extractSubagentMessageCall(item);
            if (msgCall && msgCall.message) {
              return (
                <ScrollArea
                  className="border-border bg-muted/45 dark:bg-muted/25 rounded-lg border"
                  viewportClassName="max-h-60 overscroll-contain px-3 py-2"
                >
                  <pre className="selectable-text text-foreground/85 font-sans text-[12.5px] leading-relaxed whitespace-pre-wrap break-words">
                    {msgCall.message}
                  </pre>
                </ScrollArea>
              );
            }
            return (
              <ScrollArea
                className="border-border bg-muted/45 dark:bg-muted/25 rounded-lg border"
                viewportClassName="max-h-44 overscroll-contain px-3 py-2"
              >
                <pre className="selectable-text text-muted-foreground font-mono text-[12px] whitespace-pre-wrap">
                  {formatJsonPreview(item.detail.arguments_json)}
                </pre>
              </ScrollArea>
            );
          })()}
        </div>
      )}
    </div>
  );
});
