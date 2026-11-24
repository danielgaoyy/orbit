import { memo, useRef, useState, type PointerEvent, type ReactNode } from "react";
import {
  AlertTriangle,
  ArrowUpRight,
  Check,
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
import { DiffStat } from "@/components/ui/diff-stat";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  TraceCollapsible,
  TraceCollapsibleContent,
  TraceCollapsibleTrigger,
  TraceDescription,
  TraceIcon,
  TraceLine,
  TraceTitle,
} from "@/components/ui/trace";
import { useLiveSecondClock } from "@/lib/live-clock";
import {
  cancelHoverPrefetch,
  scheduleHoverPrefetch,
  shouldPrefetchOnPointerDown,
} from "@/lib/file-prefetch";
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
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";
import {
  EMPTY_SUBAGENTS,
  type ViewportAnchorHoldFn,
  formatJsonPreview,
  parseUnifiedDiffFallback,
} from "./stage-helpers";
import { StageCopyButton } from "./stage-prompt-controls";

const quickActionClass =
  "text-ink-muted hover:text-ink-primary inline-flex shrink-0 cursor-pointer rounded p-0.5 opacity-0 transition-opacity duration-150 group-hover/tool:opacity-100 group-focus-within/tool:opacity-100 [@media(hover:none)]:opacity-100 motion-reduce:transition-none";

function StepOutput({
  text,
  children,
  className = "rounded-lg border bg-surface-recessed",
  viewportClassName = "max-h-44 py-2 pl-3",
}: {
  text: string;
  children?: ReactNode;
  className?: string;
  viewportClassName?: string;
}) {
  return (
    <div className={cn("border-border-subtle flex min-w-0 items-start", className)}>
      <ScrollArea
        className="min-w-0 flex-1"
        viewportClassName={cn("overscroll-contain [overflow-wrap:anywhere]", viewportClassName)}
      >
        {children ?? (
          <pre className="selectable-text text-ink-secondary font-mono text-[13px] leading-[1.5] whitespace-pre-wrap">
            {text}
          </pre>
        )}
      </ScrollArea>
      <StageCopyButton text={text} className="m-1 shrink-0" />
    </div>
  );
}

function thinkingInline(text: string): ReactNode {
  const parts = text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g);
  return parts.map((part, i) => {
    if (i % 2 === 0) return part;
    if (part.startsWith("**")) return <strong key={i}>{thinkingInline(part.slice(2, -2))}</strong>;
    return (
      <code key={i} className="bg-surface-recessed rounded px-1 font-mono text-[0.88em]">
        {part.slice(1, -1)}
      </code>
    );
  });
}

/** Thinking items have text only. Keep code literal and format prose without interpreting HTML. */
function ThinkingMarkdown({ text }: { text: string }) {
  const lines = text.split(/\r?\n/);
  const blocks: ReactNode[] = [];
  const listMarker = /^\s*(?:([-*+])|(\d+)\.)\s+(.+)$/;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    if (!line.trim()) continue;
    if (/^\s*```/.test(line)) {
      const start = i + 1;
      i = start;
      while (i < lines.length && !/^\s*```\s*$/.test(lines[i])) i++;
      blocks.push(
        <pre key={start - 1} className="bg-surface-recessed rounded p-2 font-mono whitespace-pre-wrap">
          {lines.slice(start, i).join("\n")}
        </pre>,
      );
      continue;
    }
    const first = line.match(listMarker);
    if (first) {
      const start = i;
      const items = [<li key={i}>{thinkingInline(first[3])}</li>];
      let next = lines[i + 1]?.match(listMarker);
      while (next && Boolean(next[2]) === Boolean(first[2])) {
        i++;
        items.push(
          <li key={i} value={next[2] ? Number(next[2]) : undefined}>
            {thinkingInline(next[3])}
          </li>,
        );
        next = lines[i + 1]?.match(listMarker);
      }
      blocks.push(first[2] ? (
        <ol key={start} start={Number(first[2])} className="list-decimal pl-5">{items}</ol>
      ) : (
        <ul key={start} className="list-disc pl-5">{items}</ul>
      ));
      continue;
    }
    const heading = line.match(/^\s*(#{1,6})\s+(.+)$/);
    blocks.push(heading ? (
      <div key={i} role="heading" aria-level={heading[1].length} className="text-ink-primary font-semibold">
        {thinkingInline(heading[2])}
      </div>
    ) : <p key={i}>{thinkingInline(line)}</p>);
  }
  return (
    <div className="selectable-text text-ink-secondary flex flex-col gap-1 text-[13px] leading-[1.55] whitespace-pre-wrap">
      {blocks}
    </div>
  );
}

/** Inline compact diff preview used when expanding an `Edited` / `Created` step inside a turn. */
export const InlineStepDiffPreview = memo(function InlineStepDiffPreview({
  lines,
  onOpenDiffReview,
}: {
  lines: DiffLineView[];
  onOpenDiffReview?: () => void;
}) {
  const { t } = useI18n();
  const capped = lines.slice(0, 80);
  const remaining = lines.length - capped.length;
  const remainingLabel = t(remaining === 1 ? "stage.moreDiffLine" : "stage.moreDiffLines", { count: remaining });
  return (
    <ScrollArea
      orientation="both"
      className="border-border-subtle bg-surface-recessed rounded-lg border"
      viewportClassName="selectable-text max-h-56 overscroll-contain py-1.5 font-mono text-[13px] leading-[1.5]"
    >
      {capped.map((ln, idx) => {
        if (ln.kind === "meta") return null;
        return (
          <div
            key={idx}
            className={cn(
              "flex min-w-fit px-3 whitespace-pre",
              ln.kind === "added" &&
                "bg-signal-success-bg text-signal-success",
              ln.kind === "removed" &&
                "bg-signal-danger-bg text-signal-danger",
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
      {remaining > 0 && (
        <div className="text-ink-muted px-3 py-1 font-sans text-[11px]">
          {onOpenDiffReview ? (
            <button
              type="button"
              onClick={onOpenDiffReview}
              className="hover:text-ink-primary cursor-pointer rounded-sm hover:underline"
            >
              {remainingLabel} · {t("stage.openDiffReview")}
            </button>
          ) : (
            remainingLabel
          )}
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

const SILENT_STALL_THRESHOLD_MS = 45_000;
const THINKING_STALL_THRESHOLD_MS = 60_000;
const QUEUED_NO_PROGRESS_THRESHOLD_MS = 30_000;

export function computeLiveStallState({
  nowMs,
  startedAtMs,
  lastActivityMs,
  lastVisibleProgressMs,
  isNetworkOffline = false,
  stallLabel,
  queuedNotificationCount = 0,
}: {
  nowMs: number;
  startedAtMs: number;
  lastActivityMs?: number | null;
  lastVisibleProgressMs?: number | null;
  isNetworkOffline?: boolean;
  stallLabel?: string | null;
  queuedNotificationCount?: number;
}) {
  const elapsedMs = Math.max(0, nowMs - startedAtMs);
  const effectiveActivityMs = lastActivityMs ?? startedAtMs;
  const effectiveVisibleProgressMs = lastVisibleProgressMs ?? effectiveActivityMs;
  const silentMs = Math.max(0, nowMs - effectiveActivityMs);
  const noProgressMs = Math.max(0, nowMs - effectiveVisibleProgressMs);

  const isSilentStall =
    !isNetworkOffline && stallLabel !== null && silentMs >= SILENT_STALL_THRESHOLD_MS;
  const isThinkingStall =
    !isNetworkOffline &&
    stallLabel !== null &&
    !isSilentStall &&
    typeof lastVisibleProgressMs === "number" &&
    effectiveActivityMs > lastVisibleProgressMs &&
    noProgressMs >= THINKING_STALL_THRESHOLD_MS;
  const showInterruptContinue =
    !isNetworkOffline &&
    (isSilentStall ||
      isThinkingStall ||
      (queuedNotificationCount > 0 && noProgressMs >= QUEUED_NO_PROGRESS_THRESHOLD_MS));

  return {
    elapsedMs,
    isSilentStall,
    isThinkingStall,
    showInterruptContinue,
  };
}

/** Isolated 1-second live elapsed ticker with offline, thinking-stall, and queue hints. */
export const InlineLiveElapsed = memo(function InlineLiveElapsed({
  startedAtMs,
  lastActivityMs,
  lastVisibleProgressMs,
  isNetworkOffline = false,
  stallLabel,
  queuedNotificationCount = 0,
}: {
  startedAtMs: number;
  lastActivityMs?: number | null;
  lastVisibleProgressMs?: number | null;
  isNetworkOffline?: boolean;
  stallLabel?: string | null;
  queuedNotificationCount?: number;
}) {
  const { t, locale } = useI18n();
  const nowMs = useLiveSecondClock(true);
  const { elapsedMs, isSilentStall, isThinkingStall } = computeLiveStallState({
    nowMs,
    startedAtMs,
    lastActivityMs,
    lastVisibleProgressMs,
    isNetworkOffline,
    stallLabel,
    queuedNotificationCount,
  });

  if (elapsedMs < 1000 && !isNetworkOffline && queuedNotificationCount === 0) return null;

  return (
    <>
      {elapsedMs >= 1000 && (
        <TraceDescription>
          · {formatDurationCompact(elapsedMs, locale)}
        </TraceDescription>
      )}
      {isNetworkOffline ? (
        <TraceDescription className="font-sans text-signal-warning">
          · {t("stage.offline")}
        </TraceDescription>
      ) : isSilentStall ? (
        <TraceDescription className="min-w-0 shrink truncate font-sans">
          · {stallLabel ?? t("stage.waitingForResponse")}
        </TraceDescription>
      ) : isThinkingStall ? (
        <TraceDescription className="font-sans text-signal-warning">
          · {t("stage.thinkingNoProgress")}
        </TraceDescription>
      ) : null}
      {queuedNotificationCount > 0 && (
        <TraceDescription className="text-signal-warning">
          · {t("stage.queuedCount", { count: queuedNotificationCount })}
        </TraceDescription>
      )}
    </>
  );
});

/** Explicit sibling button shown only when a turn is stalled or blocking queued notifications. */
export const LiveStallInterruptButton = memo(function LiveStallInterruptButton({
  startedAtMs,
  lastActivityMs,
  lastVisibleProgressMs,
  isNetworkOffline = false,
  stallLabel,
  queuedNotificationCount = 0,
  onInterruptAndContinue,
}: {
  startedAtMs: number;
  lastActivityMs?: number | null;
  lastVisibleProgressMs?: number | null;
  isNetworkOffline?: boolean;
  stallLabel?: string | null;
  queuedNotificationCount?: number;
  onInterruptAndContinue?: (() => void) | null;
}) {
  const { t } = useI18n();
  const nowMs = useLiveSecondClock(Boolean(onInterruptAndContinue));

  if (!onInterruptAndContinue) return null;

  const { showInterruptContinue } = computeLiveStallState({
    nowMs,
    startedAtMs,
    lastActivityMs,
    lastVisibleProgressMs,
    isNetworkOffline,
    stallLabel,
    queuedNotificationCount,
  });

  if (!showInterruptContinue) return null;

  return (
    <button
      type="button"
      onClick={(e) => {
        e.stopPropagation();
        onInterruptAndContinue();
      }}
      className="orbit-press border-signal-warning-border bg-signal-warning-bg text-signal-warning hover:bg-signal-warning-bg/80 inline-flex h-[22px] shrink-0 cursor-pointer items-center rounded-[6px] border px-2 text-[11px] font-medium"
    >
      {t("stage.interruptAndContinue")}
    </button>
  );
});

function ToolStepDetail({
  item,
  diffLines,
  onOpenDiffReview,
}: {
  item: Extract<TimelineItemView, { item_type: "tool_call" }>;
  diffLines: DiffLineView[];
  onOpenDiffReview?: () => void;
}) {
  const { t } = useI18n();
  const { detail } = item;
  switch (detail.kind) {
    case "shell":
      return (
        <StepOutput
          text={detail.output_preview || detail.command}
          className={cn(
            "rounded-lg border",
            item.status === "failed" || item.status === "denied"
              ? "border-signal-danger-border bg-signal-danger-bg"
              : "bg-surface-recessed",
          )}
          viewportClassName="max-h-56 py-2 pl-3"
        >
          <pre className="selectable-text text-ink-primary font-mono text-[13px] leading-[1.5] whitespace-pre-wrap">
            <span className="text-ink-muted select-none">$ </span>
            {detail.command}
            {detail.output_preview ? (
              `\n\n${detail.output_preview}`
            ) : item.status !== "running" && (
              <span className="text-ink-muted select-none">{"\n\n"}{t("stage.noOutput")}</span>
            )}
          </pre>
        </StepOutput>
      );
    case "file_edit":
      return <InlineStepDiffPreview lines={diffLines} onOpenDiffReview={onOpenDiffReview} />;
    case "subagent":
      return <StepOutput text={detail.prompt_preview} />;
    case "generic": {
      const message = extractSubagentMessageCall(item)?.message;
      if (!message) return <StepOutput text={formatJsonPreview(detail.arguments_json)} />;
      return (
        <StepOutput text={message} viewportClassName="max-h-60 py-2 pl-3">
          <p className="selectable-text text-ink-secondary text-[13px] leading-[1.5] whitespace-pre-wrap">{message}</p>
        </StepOutput>
      );
    }
    default:
      return null;
  }
}
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
  const { t, locale } = useI18n();
  const defaultOpen =
    autoExpandError &&
    item.item_type === "tool_call" &&
    (item.status === "failed" || item.status === "denied");
  const [expanded, setExpanded] = useState(defaultOpen);
  const triggerRef = useRef<HTMLButtonElement | null>(null);

  function renderStep(line: ReactNode, detail?: ReactNode, actions?: ReactNode, triggerLabel?: string) {
    const summary = detail ? (
      <TraceCollapsibleTrigger ref={triggerRef} aria-label={triggerLabel}>
        {line}
      </TraceCollapsibleTrigger>
    ) : line;
    const header = actions ? (
      <div className="group/tool flex min-w-0 items-center gap-2">
        {summary}
        {actions}
      </div>
    ) : summary;

    if (!detail) return header;
    return (
      <TraceCollapsible
        open={expanded}
        onOpenChange={(open) => holdViewport(triggerRef.current, () => setExpanded(open), "top", {
          intent: open ? "expand" : "collapse",
        })}
      >
        {header}
        <TraceCollapsibleContent>{detail}</TraceCollapsibleContent>
      </TraceCollapsible>
    );
  }

  if (item.item_type === "assistant_thinking") {
    const trimmed = item.text.trim();
    const headline = extractThinkingHeadline(trimmed, isActivelyThinking);
    const label = isActivelyThinking
      ? t("stage.thinking")
      : formatThoughtSummaryLabel(thoughtDurationMs, 1, locale);
    return renderStep(
      <TraceLine status={isActivelyThinking ? "running" : undefined}>
        <TraceIcon><Sparkles /></TraceIcon>
        <TraceTitle>{label}</TraceTitle>
        {!expanded && headline && (
          <TraceDescription className="min-w-0 shrink truncate font-sans">{headline}</TraceDescription>
        )}
      </TraceLine>,
      trimmed && (
        <StepOutput text={item.text} className="bg-transparent" viewportClassName="max-h-56">
          <ThinkingMarkdown text={trimmed} />
        </StepOutput>
      ),
    );
  }

  if (item.item_type === "assistant_message") {
    const trimmed = item.text.trim();
    if (!trimmed) return null;
    const preview = trimmed.replace(/\s+/g, " ");
    const compact = !trimmed.includes("\n") && preview.length <= 120;
    if (compact && item.blocks.length > 0) {
      return (
        <TraceLine render={<div />} className="items-start">
          <TraceIcon><MessageSquareText /></TraceIcon>
          <TraceTitle render={<div />} className="overflow-visible whitespace-normal">
            <MarkdownBlocksView blocks={item.blocks} streaming={item.streaming} compact />
          </TraceTitle>
        </TraceLine>
      );
    }
    return renderStep(
      <TraceLine>
        <TraceIcon><MessageSquareText /></TraceIcon>
        <TraceTitle className={compact ? "selectable-text whitespace-normal" : undefined}>{preview}</TraceTitle>
      </TraceLine>,
      !compact && (
        <StepOutput text={item.text} className="bg-transparent" viewportClassName="pl-0">
          {item.blocks.length ? (
            <MarkdownBlocksView blocks={item.blocks} streaming={item.streaming} />
          ) : (
            <p className="selectable-text whitespace-pre-wrap">{trimmed}</p>
          )}
        </StepOutput>
      ),
    );
  }

  if (item.item_type === "permission_request") {
    const decision = item.resolved_decision;
    if (decision == null) return null;
    const denied = /deny|denied|reject|decline|cancel/i.test(decision);
    let decisionLabel = decision.replace(/_/g, " ") || t("stage.resolved");
    if (denied) decisionLabel = t("stage.permissionDenied");
    else if (decision === "allow_always" || decision === "always") decisionLabel = t("stage.permissionAlwaysAllowed");
    else if (decision === "allow" || decision === "allow_once" || decision === "accept") decisionLabel = t("stage.permissionAllowed");
    return (
      <TraceLine status={denied ? "error" : undefined}>
        <TraceIcon>{denied ? <AlertTriangle /> : <Check className="text-signal-success" />}</TraceIcon>
        <TraceTitle>{item.title} · {decisionLabel}</TraceTitle>
      </TraceLine>
    );
  }

  if (item.item_type === "question_request") {
    if (!item.resolved_answers) return null;
    return (
      <TraceLine>
        <TraceIcon><Check className="text-signal-info" /></TraceIcon>
        <TraceTitle>
          {t(item.questions.length === 1 ? "stage.answeredQuestion" : "stage.answeredQuestions", { count: item.questions.length })}
        </TraceTitle>
      </TraceLine>
    );
  }

  if (item.item_type !== "tool_call") {
    return null;
  }

  const desc = describeToolStep(item, cwd, locale);
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
        hostId,
        cwd,
        focusLine: readStartLine,
        focusEndLine: readEndLine,
      });
    }
  };

  const openTarget = resolvedSubagentMatch
    ? handleOpenResolvedSubagent
    : canOpenTargetDirectly ? handleOpenTargetFile : undefined;
  const targetTitle = resolvedSubagentMatch
    ? t("stage.openSubagentDetails")
    : t(canOpenFileInDiff ? "stage.openTargetDiff" : "stage.openTargetInPanel", { target: displayTarget });
  const line = (
    <TraceLine status={isError ? "error" : desc.isRunning ? "running" : undefined}>
      <TraceIcon>{isError ? <AlertTriangle /> : <IconComp />}</TraceIcon>
      <TraceTitle>
        {desc.verb}
        {displayTarget && !openTarget && <> <span className="font-mono">{displayTarget}</span></>}
      </TraceTitle>
      {desc.secondaryMeta && !canOpenFileDirect && (
        <TraceDescription className="min-w-0 shrink truncate">{desc.secondaryMeta}</TraceDescription>
      )}
      {(desc.additions > 0 || desc.deletions > 0) && (
        <TraceDescription><DiffStat size="xs" additions={desc.additions} deletions={desc.deletions} /></TraceDescription>
      )}
      {desc.kind === "shell" && !desc.isRunning && desc.exitCode !== null && desc.exitCode !== 0 && (
        <TraceDescription className="text-destructive">{t("stage.exitCode", { code: desc.exitCode })}</TraceDescription>
      )}
    </TraceLine>
  );

  const handlePrefetchTargetHover =
    canOpenFileDirect && !canOpenFileInDiff && openFilePath
      ? () =>
          scheduleHoverPrefetch(() =>
            orbitActions.prefetchWorkspaceFile(openFilePath, { hostId, cwd, intent: "hover" }),
          )
      : undefined;
  const handlePrefetchTargetLeave =
    canOpenFileDirect && !canOpenFileInDiff && openFilePath ? cancelHoverPrefetch : undefined;
  const handlePrefetchTargetPointerDown =
    canOpenFileDirect && !canOpenFileInDiff && openFilePath
      ? (e: PointerEvent) => {
          if (shouldPrefetchOnPointerDown(e)) {
            cancelHoverPrefetch();
            orbitActions.prefetchWorkspaceFile(openFilePath, { hostId, cwd, intent: "press" });
          }
        }
      : undefined;

  const actions = (openTarget || canOpenFileInDiff || canOpenFileDirect) && (
    <>
      {displayTarget && openTarget && (
        <button
          type="button"
          title={targetTitle}
          onMouseEnter={!resolvedSubagentMatch ? handlePrefetchTargetHover : undefined}
          onMouseLeave={!resolvedSubagentMatch ? handlePrefetchTargetLeave : undefined}
          onPointerDown={!resolvedSubagentMatch ? handlePrefetchTargetPointerDown : undefined}
          onClick={openTarget}
          className={cn(
            "min-w-0 cursor-pointer truncate rounded-sm text-left text-[13px] leading-[18px] hover:underline",
            resolvedSubagentMatch ? "font-medium" : "font-mono",
            desc.isRunning ? "shimmer" : "text-ink-secondary hover:text-ink-primary",
          )}
        >
          {displayTarget}
        </button>
      )}
      {canOpenFileDirect && desc.secondaryMeta && (
        <button
          type="button"
          title={t("stage.jumpToFileLine", { target: desc.target || openFilePath || "", range: desc.secondaryMeta })}
          onMouseEnter={handlePrefetchTargetHover}
          onMouseLeave={handlePrefetchTargetLeave}
          onPointerDown={handlePrefetchTargetPointerDown}
          onClick={handleOpenTargetFile}
          className="text-ink-muted hover:text-ink-primary shrink-0 cursor-pointer font-mono text-[11px] hover:underline"
        >
          {desc.secondaryMeta}
        </button>
      )}
      {(canOpenFileInDiff || canOpenFileDirect) && (
        <button
          type="button"
          aria-label={t(canOpenFileInDiff ? "stage.openDiffReview" : "stage.openFileInPanel")}
          title={t(canOpenFileInDiff ? "stage.openDiffReview" : "stage.openFileInPanel")}
          onMouseEnter={handlePrefetchTargetHover}
          onMouseLeave={handlePrefetchTargetLeave}
          onPointerDown={handlePrefetchTargetPointerDown}
          onClick={handleOpenTargetFile}
          className={quickActionClass}
        >
          <FileSymlink className="size-3.5" />
        </button>
      )}
      {resolvedSubagentMatch && (
        <button
          type="button"
          aria-label={t("stage.openSubagentDetails")}
          title={t("stage.openSubagentDetails")}
          onClick={handleOpenResolvedSubagent}
          className={quickActionClass}
        >
          <ArrowUpRight className="size-3.5" />
        </button>
      )}
    </>
  );

  return renderStep(
    line,
    hasExpandableDetail && (
      <ToolStepDetail item={item} diffLines={diffLines} onOpenDiffReview={canOpenFileInDiff ? handleOpenTargetFile : undefined} />
    ),
    actions,
    openTarget && displayTarget ? `${desc.verb} ${displayTarget}` : undefined,
  );
});
