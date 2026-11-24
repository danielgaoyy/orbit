import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUpRight,
  Bot,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  Copy,
  FilePenLine,
  FileText,
  MessageCircleQuestion,
  PenLine,
  Search,
  Shield,
  ShieldAlert,
  Terminal,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  type GuardianAssessment,
  type QuestionItem,
  type TimelineItemView,
  type ToolCallDetail,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isImeKeyEvent, useImeCompositionGuard } from "@/lib/ime";
import { type MessageKey, useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import {
  isAlwaysDecision,
  isDenyDecision,
  parseGenericToolName,
  setDecisionQueueStepper,
  setFocusedDecisionTarget,
  stripWorkspacePrefix,
} from "@/lib/timeline-coalesce";
import { Button } from "@/components/ui/button";
import { DiffStat } from "@/components/ui/diff-stat";
import { ShortcutKbd } from "@/components/ui/kbd";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import type {
  SessionSubagentWithAvatar,
  UnifiedPendingSubagentPermission,
} from "@/state/selectors/session-selectors";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type PendingPermissionItem = Extract<TimelineItemView, { item_type: "permission_request" }>;
type PendingQuestionItem = Extract<TimelineItemView, { item_type: "question_request" }>;

interface QuestionDraftState {
  selections: Record<number, string[]>;
  customAnswers: Record<number, string>;
}

const EMPTY_QUESTION_DRAFT: QuestionDraftState = {
  selections: {},
  customAnswers: {},
};

type QueuedDecision =
  | {
      kind: "permission";
      queueKey: string;
      sortGroup: 0 | 1;
      orderKey: number;
      targetHostId: string;
      targetAgentId: string;
      requestId: string;
      title: string;
      detail: ToolCallDetail;
      allowedDecisions: string[];
      guardianAssessment?: GuardianAssessment | null;
      subagent?: SessionSubagentWithAvatar;
    }
  | {
      kind: "question";
      queueKey: string;
      sortGroup: 0;
      orderKey: number;
      targetHostId: string;
      targetAgentId: string;
      item: PendingQuestionItem;
    };

interface PermissionTargetPresentation {
  kindLabel: string;
  icon: LucideIcon;
  preview: string;
  secondaryMeta: string | null;
  diffStats: { additions: number; deletions: number } | null;
  copyText: string | null;
  openFilePath: string | null;
  isShell: boolean;
  expandableContent: string | null;
  expandLabel: string;
  isDiff: boolean;
}

const GENERIC_SUMMARY_KEYS = [
  "command",
  "CommandLine",
  "path",
  "file",
  "TargetFile",
  "url",
  "query",
  "pattern",
  "repo",
  "title",
  "host_id",
];

function countUnifiedDiffChanges(unifiedDiff: string): { additions: number; deletions: number } {
  let additions = 0;
  let deletions = 0;
  for (const line of unifiedDiff.split(/\r?\n/)) {
    if (line.startsWith("+") && !line.startsWith("+++")) additions += 1;
    else if (line.startsWith("-") && !line.startsWith("---")) deletions += 1;
  }
  return { additions, deletions };
}

function normalizeDirPath(raw: string | null | undefined): string {
  return (raw ?? "").trim().replace(/\\/g, "/").replace(/\/+$/, "");
}

function formatSignalLabel(raw: string): string {
  const clean = raw.trim().replace(/[._-]+/g, " ").replace(/\s+/g, " ");
  return clean;
}

function resolveDisplayHeaderTitle(
  rawTitle: string,
  detail: ToolCallDetail,
  target: PermissionTargetPresentation,
): string {
  const trimmed = (rawTitle ?? "").trim();
  if (detail.kind === "shell") {
    const cmd = (detail.command ?? "").trim();
    if (cmd && trimmed.toLowerCase().includes(cmd.toLowerCase())) {
      const stripped = trimmed
        .replace(/:\s*[\s\S]*$/, "")
        .trim();
      if (stripped && stripped.length < trimmed.length) {
        return stripped;
      }
    }
  }
  if (detail.kind === "generic") {
    const toolName = (detail.tool_name ?? "").trim();
    const parsed = parseGenericToolName(toolName);
    if (
      !trimmed ||
      (toolName && trimmed.toLowerCase() === toolName.toLowerCase())
    ) {
      return parsed.displayLabel;
    }
  }
  return trimmed || target.kindLabel;
}

function formatDecisionLabel(
  dec: string,
  t: (key: MessageKey) => string,
): string {
  const norm = dec.trim().toLowerCase();
  if (norm === "allow" || norm === "allow_once" || norm === "accept") return t("approval.allow");
  if (norm === "allow_always" || norm === "always") return t("approval.alwaysAllow");
  if (norm === "deny" || norm === "reject" || norm === "reject_once") return t("approval.deny");
  if (norm === "decline") return t("approval.decline");
  const words = dec.replace(/_+/g, " ").trim();
  return words ? words.charAt(0).toUpperCase() + words.slice(1) : dec;
}

function orderDecisions(raw: readonly string[]): string[] {
  const source = raw.length > 0 ? raw : ["allow", "allow_always", "deny"];
  const deduped = Array.from(new Set(source));
  const rank = (d: string) => (isDenyDecision(d) ? 0 : isAlwaysDecision(d) ? 1 : 2);
  return deduped
    .map((value, idx) => ({ value, idx, rank: rank(value) }))
    .sort((a, b) => a.rank - b.rank || a.idx - b.idx)
    .map((entry) => entry.value);
}

function formatOptionDisplay(raw: string): { label: string; recommended: boolean } {
  const match = raw.match(/^\(recommended\)\s*/i);
  if (!match) return { label: raw, recommended: false };
  return { label: raw.slice(match[0].length).trimStart(), recommended: true };
}

function describePermissionTarget(
  detail: ToolCallDetail,
  cwd: string,
  t: (key: MessageKey, params?: Record<string, string | number>) => string,
): PermissionTargetPresentation {
  switch (detail.kind) {
    case "shell": {
      const cmd = (detail.command ?? "").trim();
      const needsExpand = cmd.includes("\n") || cmd.length > 72;
      const cmdCwd = normalizeDirPath(detail.cwd);
      const sessionCwd = normalizeDirPath(cwd);
      let cwdHint: string | null = null;
      if (cmdCwd && cmdCwd !== sessionCwd) {
        const rel = stripWorkspacePrefix(cmdCwd, sessionCwd) || cmdCwd;
        cwdHint = t("approval.shellCwdLabel", { cwd: rel });
      }
      return {
        kindLabel: t("approval.shell"),
        icon: Terminal,
        preview: cmd || t("approval.emptyCommand"),
        secondaryMeta: cwdHint,
        diffStats: null,
        copyText: cmd || null,
        openFilePath: null,
        isShell: true,
        expandableContent: needsExpand ? cmd : null,
        expandLabel: t("approval.showFullCommand"),
        isDiff: false,
      };
    }
    case "file_edit": {
      const rel = stripWorkspacePrefix(detail.path, cwd) || detail.path;
      const diff = detail.unified_diff?.trim() || null;
      const isCreate = Boolean(diff && /---\s+\/dev\/null/.test(diff));
      const stats = diff ? countUnifiedDiffChanges(diff) : null;
      const hasStats = Boolean(stats && (stats.additions > 0 || stats.deletions > 0));
      return {
        kindLabel: isCreate ? t("approval.create") : t("approval.edit"),
        icon: FilePenLine,
        preview: rel,
        secondaryMeta: null,
        diffStats: hasStats ? stats : null,
        copyText: detail.path || null,
        openFilePath: detail.path || null,
        isShell: false,
        expandableContent: diff,
        expandLabel: t("approval.viewDiff"),
        isDiff: true,
      };
    }
    case "file_read": {
      const rel = stripWorkspacePrefix(detail.path, cwd) || detail.path;
      const lineRange =
        detail.start_line && detail.end_line
          ? `L${detail.start_line}–L${detail.end_line}`
          : detail.start_line
            ? `L${detail.start_line}+`
            : null;
      return {
        kindLabel: t("approval.read"),
        icon: FileText,
        preview: rel,
        secondaryMeta: lineRange,
        diffStats: null,
        copyText: detail.path || null,
        openFilePath: detail.path || null,
        isShell: false,
        expandableContent: null,
        expandLabel: t("approval.showDetails"),
        isDiff: false,
      };
    }
    case "search": {
      const scopeRel = detail.scope ? stripWorkspacePrefix(detail.scope, cwd) || detail.scope : "";
      const preview = scopeRel
        ? t("approval.searchInScope", { query: detail.query, scope: scopeRel })
        : detail.query;
      return {
        kindLabel: t("approval.search"),
        icon: Search,
        preview,
        secondaryMeta: null,
        diffStats: null,
        copyText: detail.query || null,
        openFilePath: null,
        isShell: false,
        expandableContent: null,
        expandLabel: t("approval.showDetails"),
        isDiff: false,
      };
    }
    case "subagent": {
      const cleanPrompt = detail.prompt_preview?.trim() || "";
      const firstLine = cleanPrompt.split(/\r?\n/).find((l) => l.trim().length > 0)?.trim() ?? "";
      const roleLabel = detail.role?.trim() || t("approval.subagent");
      return {
        kindLabel: t("approval.subagent"),
        icon: Bot,
        preview: firstLine ? `${roleLabel} — ${firstLine}` : roleLabel,
        secondaryMeta: null,
        diffStats: null,
        copyText: cleanPrompt || roleLabel,
        openFilePath: null,
        isShell: false,
        expandableContent: cleanPrompt || null,
        expandLabel: t("approval.showPrompt"),
        isDiff: false,
      };
    }
    case "generic": {
      const parsedTool = parseGenericToolName(detail.tool_name ?? "");
      const rawArgs = (detail.arguments_json ?? "").trim();
      let summary = "";
      let expandableContent: string | null = null;

      if (rawArgs && rawArgs !== "{}") {
        try {
          const parsed: unknown = JSON.parse(rawArgs);
          if (parsed && typeof parsed === "object" && !Array.isArray(parsed)) {
            const obj = parsed as Record<string, unknown>;
            const entries = Object.entries(obj);
            if (entries.length > 0) {
              expandableContent = JSON.stringify(obj, null, 2);
              const preferred = GENERIC_SUMMARY_KEYS.filter(
                (k) => k in obj && obj[k] !== null && obj[k] !== undefined && obj[k] !== "",
              ).slice(0, 2);
              const pickedEntries: Array<[string, unknown]> =
                preferred.length > 0
                  ? preferred.map((k) => [k, obj[k]])
                  : entries
                      .filter(([, v]) => typeof v === "string" || typeof v === "number")
                      .slice(0, 2);
              if (pickedEntries.length > 0) {
                summary = pickedEntries
                  .map(([k, v]) => {
                    const valStr = typeof v === "string" ? v : JSON.stringify(v);
                    const clipped = valStr.length > 60 ? `${valStr.slice(0, 60)}…` : valStr;
                    return `${k}: ${clipped}`;
                  })
                  .join(" · ");
              } else {
                summary = entries.map(([k]) => k).join(", ");
              }
            }
          } else if (parsed !== null && parsed !== undefined) {
            expandableContent = JSON.stringify(parsed, null, 2);
            summary = typeof parsed === "string" ? parsed : rawArgs;
          }
        } catch {
          expandableContent = rawArgs;
          summary = rawArgs.slice(0, 80);
        }
      }

      return {
        kindLabel: parsedTool.serverName ?? t("approval.tool"),
        icon: Shield,
        preview: summary ? `${parsedTool.displayLabel} · ${summary}` : parsedTool.displayLabel,
        secondaryMeta: null,
        diffStats: null,
        copyText: expandableContent || detail.tool_name,
        openFilePath: null,
        isShell: false,
        expandableContent,
        expandLabel: t("approval.showArguments"),
        isDiff: false,
      };
    }
  }
}

function DecisionQueueStepper({
  activeIndex,
  total,
  onPrev,
  onNext,
}: {
  activeIndex: number;
  total: number;
  onPrev: () => void;
  onNext: () => void;
}) {
  const { t } = useI18n();
  const mod = modSymbol();
  if (total <= 1) return null;

  return (
    <div className="bg-surface-recessed border-border-subtle inline-flex items-center rounded-full border p-0.5">
      <SimpleTooltip label={t("approval.previousRequest")} shortcut={`${mod}[`} side="top">
        <button
          type="button"
          onClick={onPrev}
          aria-label={t("approval.previousRequest")}
          className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex size-5.5 cursor-pointer items-center justify-center rounded-full transition-colors"
        >
          <ChevronLeft className="size-3.5 stroke-[1.9]" />
        </button>
      </SimpleTooltip>
      <span className="text-ink-muted tabular-nums px-1.5 font-mono text-[11px] font-medium">
        {activeIndex + 1}/{total}
      </span>
      <SimpleTooltip label={t("approval.nextRequest")} shortcut={`${mod}]`} side="top">
        <button
          type="button"
          onClick={onNext}
          aria-label={t("approval.nextRequest")}
          className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex size-5.5 cursor-pointer items-center justify-center rounded-full transition-colors"
        >
          <ChevronRight className="size-3.5 stroke-[1.9]" />
        </button>
      </SimpleTooltip>
    </div>
  );
}

function PermissionDetailExpanded({ content, isDiff }: { content: string; isDiff: boolean }) {
  const lines = content.split(/\r?\n/);

  return (
    <ScrollArea
      orientation="both"
      className="border-border-subtle bg-surface-recessed mt-2 rounded-xl border"
      viewportClassName="selectable-text max-h-44 overscroll-contain px-3 py-2 font-mono text-[12px] leading-[1.5]"
    >
      {isDiff ? (
        lines.map((line, i) => {
          const isAdd = line.startsWith("+") && !line.startsWith("+++");
          const isDel = line.startsWith("-") && !line.startsWith("---");
          const isMeta =
            line.startsWith("@@") || line.startsWith("---") || line.startsWith("+++");
          return (
            <div
              key={i}
              className={cn(
                "min-w-fit rounded-[2px] px-1 whitespace-pre",
                isAdd && "bg-signal-success-bg text-signal-success",
                isDel && "bg-signal-danger-bg text-signal-danger",
                isMeta && "text-ink-muted",
                !isAdd && !isDel && !isMeta && "text-ink-secondary",
              )}
            >
              {line || " "}
            </div>
          );
        })
      ) : (
        <pre className="text-ink-primary m-0 font-mono text-[12px] leading-[1.5] whitespace-pre-wrap break-words">
          {content}
        </pre>
      )}
    </ScrollArea>
  );
}

function PermissionApprovalCard({
  queueKey,
  hostId,
  agentId,
  parentAgentId,
  cwd,
  requestId,
  title,
  detail,
  allowedDecisions,
  guardianAssessment,
  subagent,
  queueIndex,
  queueTotal,
  onStepPrev,
  onStepNext,
}: {
  queueKey: string;
  hostId: string;
  agentId: string;
  parentAgentId: string;
  cwd: string;
  requestId: string;
  title: string;
  detail: ToolCallDetail;
  allowedDecisions: string[];
  guardianAssessment?: GuardianAssessment | null;
  subagent?: SessionSubagentWithAvatar;
  queueIndex: number;
  queueTotal: number;
  onStepPrev: () => void;
  onStepNext: () => void;
}) {
  const { t } = useI18n();
  const mod = modSymbol();
  const [expanded, setExpanded] = useState(false);
  const [copied, setCopied] = useState(false);
  const [submittingDecision, setSubmittingDecision] = useState<string | null>(null);
  const submittingRef = useRef<string | null>(null);
  submittingRef.current = submittingDecision;

  useEffect(() => {
    if (!submittingDecision) return;
    const id = window.setTimeout(() => setSubmittingDecision(null), 3500);
    return () => window.clearTimeout(id);
  }, [submittingDecision]);

  const decisions = useMemo(() => orderDecisions(allowedDecisions), [allowedDecisions]);
  const primaryAllowChoice = decisions.find((d) => !isDenyDecision(d) && !isAlwaysDecision(d)) ?? null;
  const primaryAlwaysChoice = decisions.find((d) => isAlwaysDecision(d)) ?? null;
  const primaryDenyChoice = decisions.find((d) => isDenyDecision(d)) ?? null;

  const target = useMemo(() => describePermissionTarget(detail, cwd, t), [detail, cwd, t]);
  const KindIcon = target.icon;

  const handleResolve = useCallback(
    async (dec: string) => {
      if (submittingRef.current) return;
      submittingRef.current = dec;
      setSubmittingDecision(dec);
      try {
        await orbitIpc.resolvePermission(hostId, agentId, requestId, dec);
      } catch (err) {
        submittingRef.current = null;
        setSubmittingDecision(null);
        orbitActions.showToast(t("approval.permissionFailed", { error: String(err) }));
      }
    },
    [hostId, agentId, requestId, t],
  );

  // Register exact target (parent OR child subagent) and live resolve handler for `⌘Y / ⌘⇧Y / ⌘⌫`
  useEffect(() => {
    setFocusedDecisionTarget({
      kind: "permission",
      queueKey,
      hostId,
      agentId,
      requestId,
      allowedDecisions: decisions,
      onResolve: (choice) => {
        void handleResolve(choice);
      },
    });
    return () => {
      setFocusedDecisionTarget(null);
    };
  }, [queueKey, hostId, agentId, requestId, decisions, handleResolve]);

  const handleCopyPreview = () => {
    if (!target.copyText) return;
    void navigator.clipboard.writeText(target.copyText).then(() => {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1400);
    });
  };

  const riskTone =
    guardianAssessment?.risk_level === "critical" || guardianAssessment?.risk_level === "high"
      ? "danger"
      : guardianAssessment?.risk_level === "medium"
        ? "warning"
        : undefined;
  const isLowRisk = guardianAssessment?.risk_level === "low";
  const displayHeaderTitle = useMemo(
    () => resolveDisplayHeaderTitle(title, detail, target),
    [title, detail, target],
  );
  const guardianTierLabel =
    guardianAssessment?.classifier_tier === "ai_rubric"
      ? t("approval.guardianAiClassifier")
      : guardianAssessment?.classifier_tier === "deterministic"
        ? t("approval.guardianRulePolicy")
        : null;
  const guardianSignals = useMemo(
    () => (guardianAssessment?.signals ?? []).filter(Boolean).slice(0, 3),
    [guardianAssessment?.signals],
  );
  const guardianReasonText =
    guardianAssessment?.reason?.trim() ||
    (guardianAssessment?.classifier_tier === "fail_closed"
      ? t("approval.guardianFailClosed")
      : "");

  return (
    <div
      data-risk={riskTone}
      className="approval-dock-surface orbit-dock-enter overflow-hidden rounded-2xl"
    >
      <div className="border-border-subtle flex items-center justify-between gap-3 border-b px-4 py-2.5">
        <div className="flex min-w-0 items-center gap-2">
          {subagent ? (
            <SubagentAvatar
              character={subagent.character}
              status="awaiting_permission"
              size="sm"
            />
          ) : (
            <span
              className={cn(
                "size-1.5 shrink-0 rounded-full",
                riskTone === "danger" ? "bg-signal-danger" : "bg-signal-warning",
              )}
            />
          )}
          <KindIcon className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
          <span className="text-ink-primary truncate text-[12.5px] font-medium">
            {subagent ? `${subagent.title} · ${displayHeaderTitle}` : displayHeaderTitle}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {target.diffStats && (
            <DiffStat
              additions={target.diffStats.additions}
              deletions={target.diffStats.deletions}
              className="text-[11px]"
            />
          )}
          {subagent && (
            <button
              type="button"
              onClick={() =>
                orbitActions.openSubagentTab({
                  callId: subagent.callId,
                  role: subagent.title,
                  promptPreview: subagent.promptPreview,
                  childAgentId: subagent.childAgentId,
                  childHostId: subagent.childHostId || undefined,
                  providerSubagentId: subagent.providerSubagentId,
                  parentAgentId,
                })
              }
              className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex h-[22px] cursor-pointer items-center gap-0.5 rounded-[6px] px-1.5 text-[11px] font-medium transition-colors"
            >
              <span>{t("approval.inspectSubagent")}</span>
              <ArrowUpRight className="size-3 shrink-0" />
            </button>
          )}
          <span className="text-ink-muted text-[11px] font-medium">
            {target.kindLabel}
          </span>
          <DecisionQueueStepper
            activeIndex={queueIndex}
            total={queueTotal}
            onPrev={onStepPrev}
            onNext={onStepNext}
          />
        </div>
      </div>

      <div className="px-4 py-3">
        {guardianAssessment && !isLowRisk && (
          <div
            className={cn(
              "mb-2 flex items-start gap-2 rounded-lg border px-2.5 py-1.5 text-[11.5px] leading-[1.45]",
              riskTone === "danger"
                ? "bg-signal-danger-bg border-signal-danger-border text-signal-danger"
                : "bg-signal-warning-bg border-signal-warning-border text-signal-warning",
            )}
          >
            <ShieldAlert className="mt-[2px] size-3.5 shrink-0 stroke-[1.8]" />
            <div className="flex min-w-0 flex-1 flex-col gap-1">
              <div className="line-clamp-3 min-w-0 break-words">
                <span className="font-medium">
                  {guardianAssessment.risk_level === "critical"
                    ? t("approval.criticalRisk")
                    : guardianAssessment.risk_level === "high"
                      ? t("approval.highRisk")
                      : t("approval.mediumRisk")}
                </span>
                {guardianTierLabel && (
                  <span className="ml-1.5 font-normal opacity-75">({guardianTierLabel})</span>
                )}
                {guardianReasonText && (
                  <>
                    <span className="mx-1.5 opacity-45">·</span>
                    <span className="opacity-95">{guardianReasonText}</span>
                  </>
                )}
              </div>
              {guardianSignals.length > 0 && (
                <div className="flex flex-wrap items-center gap-1">
                  {guardianSignals.map((sig) => (
                    <span
                      key={sig}
                      className="rounded border border-current/15 bg-current/8 px-1.5 py-0.2 font-mono text-[10px] leading-[15px] opacity-90"
                    >
                      {formatSignalLabel(sig)}
                    </span>
                  ))}
                </div>
              )}
            </div>
          </div>
        )}

        <div className="group/preview bg-surface-recessed border-border-subtle relative flex items-start justify-between gap-2 rounded-xl border px-3 py-1.5">
          <div className="min-w-0 flex-1 py-[3px]">
            <div className="selectable-text text-ink-primary line-clamp-2 font-mono text-[12px] leading-[1.5] break-words [overflow-wrap:anywhere]">
              {target.isShell && (
                <span className="text-ink-muted mr-1.5 select-none">$</span>
              )}
              <span>{target.preview}</span>
              {target.secondaryMeta && !target.isShell && (
                <span className="text-ink-muted ml-1.5 text-[11px]">{target.secondaryMeta}</span>
              )}
            </div>
            {target.secondaryMeta && target.isShell && (
              <div className="text-ink-muted mt-0.5 truncate font-mono text-[10.5px]">
                {target.secondaryMeta}
              </div>
            )}
          </div>

          {(target.openFilePath || target.copyText) && (
            <div className="flex shrink-0 items-center gap-0.5">
              {target.openFilePath && (
                <SimpleTooltip label={t("approval.openFile")} side="top">
                  <button
                    type="button"
                    aria-label={t("approval.openFile")}
                    onClick={() =>
                      orbitActions.openFileInSidePanel(target.openFilePath!, { hostId, cwd })
                    }
                    className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-6 cursor-pointer items-center justify-center rounded-[6px] transition-colors"
                  >
                    <ArrowUpRight className="size-3.5 stroke-[1.75]" />
                  </button>
                </SimpleTooltip>
              )}
              {target.copyText && (
                <SimpleTooltip
                  label={copied ? t("approval.copiedCommand") : t("approval.copyCommand")}
                  side="top"
                >
                  <button
                    type="button"
                    aria-label={t("approval.copyCommand")}
                    onClick={handleCopyPreview}
                    className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-6 cursor-pointer items-center justify-center rounded-[6px] transition-colors"
                  >
                    {copied ? (
                      <Check className="text-signal-success size-3.5 stroke-[2]" />
                    ) : (
                      <Copy className="size-3.5 stroke-[1.75]" />
                    )}
                  </button>
                </SimpleTooltip>
              )}
            </div>
          )}
        </div>

        {target.expandableContent && (
          <div className="mt-2">
            <button
              type="button"
              aria-expanded={expanded}
              onClick={() => setExpanded((v) => !v)}
              className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-[6px] px-1.5 py-0.5 text-[11.5px] font-medium transition-colors"
            >
              <ChevronDown
                className={cn(
                  "size-3.5 stroke-[1.75] transition-transform duration-150",
                  expanded && "rotate-180",
                )}
              />
              <span>{expanded ? t("common.hide") : target.expandLabel}</span>
            </button>
            {expanded && (
              <PermissionDetailExpanded
                content={target.expandableContent}
                isDiff={target.isDiff}
              />
            )}
          </div>
        )}
      </div>

      <div className="border-border-subtle flex flex-wrap items-center justify-end gap-1.5 border-t px-4 py-2.5">
        {decisions.map((dec) => {
          const isDeny = isDenyDecision(dec);
          const isAlways = isAlwaysDecision(dec);
          const isPrimaryAllow = dec === primaryAllowChoice;
          const isMatchedAlways = dec === primaryAlwaysChoice;
          const isMatchedDeny = dec === primaryDenyChoice;
          const label = formatDecisionLabel(dec, t);
          const isThisSubmitting = submittingDecision === dec;
          const disabled = submittingDecision !== null;

          const buttonEl = (
            <button
              key={dec}
              type="button"
              disabled={disabled}
              onClick={() => void handleResolve(dec)}
              className={cn(
                "orbit-press inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-full text-[12px] font-medium disabled:cursor-not-allowed disabled:opacity-50",
                isDeny
                  ? "text-ink-tertiary hover:bg-signal-danger-bg hover:text-signal-danger px-3"
                  : isAlways
                    ? "text-ink-secondary hover:text-ink-primary hover:bg-accent px-3"
                    : "bg-action-primary-bg text-action-primary-fg hover:bg-action-primary-hover px-3.5",
              )}
            >
              {isThisSubmitting && <OrbitSpinner size={12} className="shrink-0" />}
              <span>{isThisSubmitting ? t("approval.submittingDecision") : label}</span>
              {isPrimaryAllow && !isThisSubmitting && (
                <ShortcutKbd keys={`${mod}Y`} size="xs" variant="inverted" className="ml-0.5" />
              )}
            </button>
          );

          if (isMatchedAlways) {
            return (
              <SimpleTooltip
                key={dec}
                label={t("approval.alwaysAllowSessionTooltip")}
                shortcut={`${mod}⇧Y`}
                side="top"
              >
                {buttonEl}
              </SimpleTooltip>
            );
          }

          if (isMatchedDeny) {
            return (
              <SimpleTooltip key={dec} label={label} shortcut={`${mod}⌫`} side="top">
                {buttonEl}
              </SimpleTooltip>
            );
          }

          return buttonEl;
        })}
      </div>
    </div>
  );
}

function StickyQuestionDockCard({
  queueKey,
  hostId,
  agentId,
  requestId,
  questions,
  draft,
  onDraftChange,
  onSubmitted,
  queueIndex,
  queueTotal,
  onStepPrev,
  onStepNext,
}: {
  queueKey: string;
  hostId: string;
  agentId: string;
  requestId: string;
  questions: QuestionItem[];
  draft: QuestionDraftState;
  onDraftChange: (updater: (prev: QuestionDraftState) => QuestionDraftState) => void;
  onSubmitted: () => void;
  queueIndex: number;
  queueTotal: number;
  onStepPrev: () => void;
  onStepNext: () => void;
}) {
  const { t } = useI18n();
  const mod = modSymbol();
  const [submitting, setSubmitting] = useState(false);
  const inputRefs = useRef<Array<HTMLInputElement | null>>([]);
  const ime = useImeCompositionGuard();

  const { selections, customAnswers } = draft;

  useEffect(() => {
    setFocusedDecisionTarget({
      kind: "question",
      queueKey,
      hostId,
      agentId,
      requestId,
    });
    return () => {
      setFocusedDecisionTarget(null);
    };
  }, [queueKey, hostId, agentId, requestId]);

  useEffect(() => {
    if (!submitting) return;
    const id = window.setTimeout(() => setSubmitting(false), 3500);
    return () => window.clearTimeout(id);
  }, [submitting]);

  const toggleOption = useCallback(
    (qIdx: number, opt: string, multi: boolean) => {
      if (submitting) return;
      onDraftChange((prev) => {
        const cur = prev.selections[qIdx] ?? [];
        const nextSelections = !multi
          ? { ...prev.selections, [qIdx]: [opt] }
          : {
              ...prev.selections,
              [qIdx]: cur.includes(opt) ? cur.filter((x) => x !== opt) : [...cur, opt],
            };
        const nextCustom =
          !multi && prev.customAnswers[qIdx]
            ? { ...prev.customAnswers, [qIdx]: "" }
            : prev.customAnswers;
        return { selections: nextSelections, customAnswers: nextCustom };
      });
    },
    [submitting, onDraftChange],
  );

  const handleCustomChange = useCallback(
    (qIdx: number, value: string, multi: boolean) => {
      if (submitting) return;
      onDraftChange((prev) => {
        const nextCustom = { ...prev.customAnswers, [qIdx]: value };
        const nextSelections =
          !multi && value.trim().length > 0 && prev.selections[qIdx]?.length
            ? { ...prev.selections, [qIdx]: [] }
            : prev.selections;
        return { selections: nextSelections, customAnswers: nextCustom };
      });
    },
    [submitting, onDraftChange],
  );

  // Number-key quick selection (`1`–`9`) when a single question is shown and no input is focused
  const singleQuestion = questions.length === 1 ? questions[0] : null;
  const singleOptionCount = singleQuestion ? Math.min(9, singleQuestion.options.length) : 0;

  useEffect(() => {
    if (!singleQuestion || singleOptionCount === 0 || submitting) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.metaKey || e.ctrlKey || e.altKey || e.shiftKey || isImeKeyEvent(e)) return;
      if (!/^[1-9]$/.test(e.key)) return;
      const activeEl = document.activeElement as HTMLElement | null;
      const activeTag = activeEl?.tagName;
      if (
        activeTag === "INPUT" ||
        activeTag === "TEXTAREA" ||
        activeTag === "SELECT" ||
        activeEl?.isContentEditable
      ) {
        return;
      }
      const idx = Number.parseInt(e.key, 10) - 1;
      if (idx >= 0 && idx < singleQuestion.options.length) {
        e.preventDefault();
        toggleOption(0, singleQuestion.options[idx], singleQuestion.is_multi_select);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [singleQuestion, singleOptionCount, submitting, toggleOption]);

  const orderedAnswers = questions.map((q, idx) => {
    const custom = (customAnswers[idx] ?? "").trim();
    const chosen = selections[idx] ?? [];
    if (!q.is_multi_select) {
      return custom ? [custom] : chosen;
    }
    return custom && !chosen.includes(custom) ? [...chosen, custom] : chosen;
  });

  const answeredCount = orderedAnswers.filter((vals) => vals.length > 0).length;
  const canSubmit = questions.length > 0 && answeredCount === questions.length;

  const handleSubmit = async () => {
    if (!canSubmit || submitting) return;
    setSubmitting(true);
    try {
      await orbitIpc.answerQuestion(hostId, agentId, requestId, orderedAnswers);
      onSubmitted();
    } catch (err) {
      setSubmitting(false);
      orbitActions.showToast(t("approval.submitFailed", { error: String(err) }));
    }
  };

  const focusNextUnansweredInput = (currentIdx: number) => {
    for (let offset = 1; offset < questions.length; offset += 1) {
      const candidateIdx = (currentIdx + offset) % questions.length;
      if ((orderedAnswers[candidateIdx]?.length ?? 0) === 0) {
        inputRefs.current[candidateIdx]?.focus();
        return;
      }
    }
    if (currentIdx + 1 < questions.length) {
      inputRefs.current[currentIdx + 1]?.focus();
    }
  };

  return (
    <div
      onKeyDown={(e) => {
        if (isImeKeyEvent(e)) return;
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canSubmit && !submitting) {
          e.preventDefault();
          void handleSubmit();
        }
      }}
      className="approval-dock-surface orbit-dock-enter overflow-hidden rounded-2xl"
    >
      <div className="border-border-subtle flex items-center justify-between gap-3 border-b px-4 py-2.5">
        <div className="flex min-w-0 items-center gap-2">
          <span className="bg-signal-warning size-1.5 shrink-0 rounded-full" />
          <MessageCircleQuestion className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
          <span className="text-ink-primary truncate text-[12.5px] font-medium">
            {t("approval.clarificationNeeded")}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {questions.length > 1 && (
            <span className="text-ink-muted tabular-nums font-mono text-[11px] font-medium">
              {answeredCount}/{questions.length}
            </span>
          )}
          <DecisionQueueStepper
            activeIndex={queueIndex}
            total={queueTotal}
            onPrev={onStepPrev}
            onNext={onStepNext}
          />
        </div>
      </div>

      <ScrollArea viewportClassName="max-h-[min(42vh,340px)] overscroll-contain px-4 py-3">
        <div className="flex flex-col gap-4">
          {questions.map((q, qIdx) => {
            const cleanHeader = q.header?.trim();
            const showHeader = Boolean(cleanHeader && cleanHeader !== q.question.trim());
            const customVal = customAnswers[qIdx] ?? "";
            const hasCustomText = customVal.trim().length > 0;

            return (
              <div
                key={q.id || qIdx}
                className={cn(
                  "flex flex-col gap-2",
                  qIdx > 0 && "border-border-subtle border-t pt-3.5",
                )}
              >
                <div className="flex flex-col gap-0.5">
                  {showHeader && (
                    <div className="text-ink-muted text-[11px] font-medium tracking-wide uppercase">
                      {cleanHeader}
                    </div>
                  )}
                  <div className="selectable-text text-ink-primary text-[13.5px] leading-[1.45] font-medium">
                    {questions.length > 1 && (
                      <span className="text-ink-muted tabular-nums mr-1.5 font-mono text-[12.5px]">
                        {qIdx + 1}.
                      </span>
                    )}
                    <span>{q.question}</span>
                  </div>
                  {q.is_multi_select && (
                    <div className="text-ink-tertiary text-[11.5px]">
                      {t("approval.selectAllThatApply")}
                    </div>
                  )}
                </div>

                <div className="divide-border-subtle border-border-subtle divide-y overflow-hidden rounded-xl border">
                  {q.options.map((opt, optIdx) => {
                    const chosen = selections[qIdx]?.includes(opt) ?? false;
                    const { label, recommended } = formatOptionDisplay(opt);

                    return (
                      <button
                        key={`${q.id || qIdx}-${optIdx}`}
                        type="button"
                        disabled={submitting}
                        onClick={() => toggleOption(qIdx, opt, q.is_multi_select)}
                        className={cn(
                          "flex w-full cursor-pointer items-start gap-2.5 px-3 py-2 text-left text-[13px] leading-[1.45] transition-colors disabled:cursor-not-allowed disabled:opacity-60",
                          chosen
                            ? "bg-accent text-ink-primary font-medium"
                            : "text-ink-secondary hover:bg-accent/60 hover:text-ink-primary bg-transparent",
                        )}
                      >
                        <span
                          className={cn(
                            "mt-[2.5px] flex size-[15px] shrink-0 items-center justify-center border transition-colors",
                            q.is_multi_select ? "rounded-[4px]" : "rounded-full",
                            chosen
                              ? "border-action-primary-bg bg-action-primary-bg"
                              : "border-border-strong",
                          )}
                        >
                          {chosen &&
                            (q.is_multi_select ? (
                              <Check className="text-action-primary-fg size-2.5 stroke-[2.6]" />
                            ) : (
                              <span className="bg-action-primary-fg size-[5px] rounded-full" />
                            ))}
                        </span>
                        <span className="min-w-0 flex-1 break-words">
                          <span>{label}</span>
                          {recommended && (
                            <span className="bg-surface-recessed text-ink-tertiary border-border-subtle ml-1.5 inline-flex items-center rounded-full border px-1.5 py-0.5 align-middle text-[10.5px] leading-none font-medium">
                              {t("common.recommended")}
                            </span>
                          )}
                        </span>
                        {singleQuestion && optIdx < 9 && (
                          <span className="text-ink-faint tabular-nums mt-0.5 shrink-0 font-mono text-[10.5px]">
                            {optIdx + 1}
                          </span>
                        )}
                      </button>
                    );
                  })}

                  <div
                    className={cn(
                      "flex items-center gap-2.5 px-3 py-2 transition-colors",
                      hasCustomText && "bg-accent",
                    )}
                  >
                    <PenLine
                      className={cn(
                        "size-3.5 shrink-0 stroke-[1.75]",
                        hasCustomText ? "text-ink-primary" : "text-ink-faint",
                      )}
                    />
                    <input
                      ref={(el) => {
                        inputRefs.current[qIdx] = el;
                      }}
                      type="text"
                      disabled={submitting}
                      value={customVal}
                      onChange={(e) => handleCustomChange(qIdx, e.target.value, q.is_multi_select)}
                      {...ime.compositionProps}
                      onKeyDown={(e) => {
                        if (ime.shouldIgnoreKeyDown(e)) return;
                        if (e.key === "Enter" && !e.shiftKey && !(e.metaKey || e.ctrlKey)) {
                          e.preventDefault();
                          if (canSubmit) {
                            void handleSubmit();
                          } else {
                            focusNextUnansweredInput(qIdx);
                          }
                        }
                      }}
                      placeholder={
                        q.options.length > 0
                          ? t("approval.otherPlaceholder")
                          : t("approval.typeAnswerPlaceholder")
                      }
                      className="text-ink-primary placeholder:text-ink-faint w-full bg-transparent text-[13px] leading-[1.45] outline-none disabled:cursor-not-allowed disabled:opacity-60"
                    />
                    {customVal.length > 0 && !submitting && (
                      <button
                        type="button"
                        aria-label={t("approval.clearCustomAnswer")}
                        onClick={() => handleCustomChange(qIdx, "", q.is_multi_select)}
                        className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-full transition-colors"
                      >
                        <X className="size-3" />
                      </button>
                    )}
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      </ScrollArea>

      <div className="border-border-subtle flex items-center justify-between gap-2 border-t px-4 py-2.5">
        <span className="text-ink-muted inline-flex items-center gap-1.5 text-[11.5px]">
          {canSubmit ? (
            <>
              <ShortcutKbd keys={`${mod}↵`} size="xs" variant="subtle" />
              <span>{t("common.submit")}</span>
            </>
          ) : singleOptionCount > 0 ? (
            t("approval.digitShortcutHint", { count: singleOptionCount })
          ) : (
            t("approval.chooseOptionOrWrite")
          )}
        </span>
        <Button
          type="button"
          size="sm"
          disabled={!canSubmit || submitting}
          onClick={() => void handleSubmit()}
          className="rounded-full px-3.5"
        >
          {submitting && <OrbitSpinner size={12} className="shrink-0" />}
          <span>{submitting ? t("common.submitting") : t("common.submit")}</span>
        </Button>
      </div>
    </div>
  );
}

const EMPTY_SUBAGENT_PERMS: readonly UnifiedPendingSubagentPermission[] = [];

function sameGuardian(
  a: GuardianAssessment | null | undefined,
  b: GuardianAssessment | null | undefined,
): boolean {
  if (a === b) return true;
  if (!a || !b) return false;
  return (
    a.risk_level === b.risk_level &&
    a.reason === b.reason &&
    a.classifier_tier === b.classifier_tier &&
    a.signals.length === b.signals.length
  );
}

export const DecisionDock = memo(
  function DecisionDock({
    hostId,
    agentId,
    cwd,
    permissions,
    questions,
    subagentPermissions = EMPTY_SUBAGENT_PERMS,
  }: {
    hostId: string;
    agentId: string;
    cwd: string;
    permissions: readonly PendingPermissionItem[];
    questions: readonly PendingQuestionItem[];
    subagentPermissions?: readonly UnifiedPendingSubagentPermission[];
  }) {
    const [focusedKey, setFocusedKey] = useState<string | null>(null);
    const [fallbackSlotIndex, setFallbackSlotIndex] = useState<number>(0);
    const [questionDrafts, setQuestionDrafts] = useState<Record<string, QuestionDraftState>>({});

    const queue = useMemo<QueuedDecision[]>(() => {
      const seen = new Set<string>();
      const merged: QueuedDecision[] = [];

      for (const item of permissions) {
        const key = `permission:${hostId}:${agentId}:${item.request_id}`;
        if (!seen.has(key)) {
          seen.add(key);
          merged.push({
            kind: "permission",
            queueKey: key,
            sortGroup: 0,
            orderKey: item.seq,
            targetHostId: hostId,
            targetAgentId: agentId,
            requestId: item.request_id,
            title: item.title,
            detail: item.detail,
            allowedDecisions: item.allowed_decisions,
            guardianAssessment: item.guardian_assessment,
          });
        }
      }
      for (const item of questions) {
        const key = `question:${hostId}:${agentId}:${item.request_id}`;
        if (!seen.has(key)) {
          seen.add(key);
          merged.push({
            kind: "question",
            queueKey: key,
            sortGroup: 0,
            orderKey: item.seq,
            targetHostId: hostId,
            targetAgentId: agentId,
            item,
          });
        }
      }
      for (const subPerm of subagentPermissions) {
        const key = `subperm:${subPerm.key}`;
        if (!seen.has(key)) {
          seen.add(key);
          const ts = Date.parse(subPerm.permission.created_at);
          merged.push({
            kind: "permission",
            queueKey: key,
            sortGroup: 1,
            orderKey: Number.isFinite(ts) ? ts : Number.MAX_SAFE_INTEGER,
            targetHostId: subPerm.hostId,
            targetAgentId: subPerm.agentId,
            requestId: subPerm.permission.request_id,
            title: subPerm.permission.title,
            detail: subPerm.permission.detail,
            allowedDecisions: subPerm.permission.allowed_decisions,
            guardianAssessment: subPerm.permission.guardian_assessment,
            subagent: subPerm.subagent,
          });
        }
      }

      merged.sort((a, b) => a.sortGroup - b.sortGroup || a.orderKey - b.orderKey);
      return merged;
    }, [hostId, agentId, permissions, questions, subagentPermissions]);

    // Prune stale question drafts once those question requests leave the active queue
    useEffect(() => {
      const activeQuestionKeys = new Set(
        queue.filter((entry) => entry.kind === "question").map((entry) => entry.queueKey),
      );
      setQuestionDrafts((prev) => {
        const keys = Object.keys(prev);
        if (keys.every((k) => activeQuestionKeys.has(k))) return prev;
        const next: Record<string, QuestionDraftState> = {};
        for (const k of keys) {
          if (activeQuestionKeys.has(k)) next[k] = prev[k];
        }
        return next;
      });
    }, [queue]);

    const activeIndex = useMemo(() => {
      if (queue.length === 0) return 0;
      if (focusedKey) {
        const idx = queue.findIndex((entry) => entry.queueKey === focusedKey);
        if (idx >= 0) return idx;
      }
      return Math.min(Math.max(0, fallbackSlotIndex), queue.length - 1);
    }, [queue, focusedKey, fallbackSlotIndex]);

    const activeEntry = queue[activeIndex] ?? null;

    const handleStep = useCallback(
      (delta: -1 | 1) => {
        if (queue.length <= 1) return;
        const nextIdx = (activeIndex + delta + queue.length) % queue.length;
        setFallbackSlotIndex(nextIdx);
        setFocusedKey(queue[nextIdx].queueKey);
      },
      [queue, activeIndex],
    );

    useEffect(() => {
      if (queue.length <= 1) {
        setDecisionQueueStepper(null);
        return;
      }
      setDecisionQueueStepper(handleStep);
      return () => setDecisionQueueStepper(null);
    }, [queue.length, handleStep]);

    const updateQuestionDraft = useCallback(
      (queueKey: string, updater: (prev: QuestionDraftState) => QuestionDraftState) => {
        setQuestionDrafts((prev) => ({
          ...prev,
          [queueKey]: updater(prev[queueKey] ?? EMPTY_QUESTION_DRAFT),
        }));
      },
      [],
    );

    const clearQuestionDraft = useCallback((queueKey: string) => {
      setQuestionDrafts((prev) => {
        if (!(queueKey in prev)) return prev;
        const next = { ...prev };
        delete next[queueKey];
        return next;
      });
    }, []);

    if (!activeEntry) return null;

    return (
      <div className="w-full px-[24px] pb-2">
        <div className="mx-auto w-full max-w-[var(--orbit-stage-max-w,740px)]">
          {activeEntry.kind === "permission" ? (
            <PermissionApprovalCard
              key={activeEntry.queueKey}
              queueKey={activeEntry.queueKey}
              hostId={activeEntry.targetHostId}
              agentId={activeEntry.targetAgentId}
              parentAgentId={agentId}
              cwd={cwd}
              requestId={activeEntry.requestId}
              title={activeEntry.title}
              detail={activeEntry.detail}
              allowedDecisions={activeEntry.allowedDecisions}
              guardianAssessment={activeEntry.guardianAssessment}
              subagent={activeEntry.subagent}
              queueIndex={activeIndex}
              queueTotal={queue.length}
              onStepPrev={() => handleStep(-1)}
              onStepNext={() => handleStep(1)}
            />
          ) : (
            <StickyQuestionDockCard
              key={activeEntry.queueKey}
              queueKey={activeEntry.queueKey}
              hostId={activeEntry.targetHostId}
              agentId={activeEntry.targetAgentId}
              requestId={activeEntry.item.request_id}
              questions={activeEntry.item.questions}
              draft={questionDrafts[activeEntry.queueKey] ?? EMPTY_QUESTION_DRAFT}
              onDraftChange={(updater) => updateQuestionDraft(activeEntry.queueKey, updater)}
              onSubmitted={() => clearQuestionDraft(activeEntry.queueKey)}
              queueIndex={activeIndex}
              queueTotal={queue.length}
              onStepPrev={() => handleStep(-1)}
              onStepNext={() => handleStep(1)}
            />
          )}
        </div>
      </div>
    );
  },
  (prev, next) =>
    prev.hostId === next.hostId &&
    prev.agentId === next.agentId &&
    prev.cwd === next.cwd &&
    prev.permissions.length === next.permissions.length &&
    prev.questions.length === next.questions.length &&
    (prev.subagentPermissions?.length ?? 0) === (next.subagentPermissions?.length ?? 0) &&
    prev.permissions.every((p, i) => {
      const np = next.permissions[i];
      return (
        np !== undefined &&
        p.request_id === np.request_id &&
        p.allowed_decisions.length === np.allowed_decisions.length &&
        sameGuardian(p.guardian_assessment, np.guardian_assessment)
      );
    }) &&
    prev.questions.every((q, i) => q.request_id === next.questions[i]?.request_id) &&
    (prev.subagentPermissions ?? EMPTY_SUBAGENT_PERMS).every((sp, i) => {
      const nsp = next.subagentPermissions?.[i];
      return (
        nsp !== undefined &&
        sp.key === nsp.key &&
        sp.subagent.title === nsp.subagent.title &&
        sp.subagent.character === nsp.subagent.character &&
        sameGuardian(sp.permission.guardian_assessment, nsp.permission.guardian_assessment)
      );
    }),
);

export const StickyApprovalDock = DecisionDock;
