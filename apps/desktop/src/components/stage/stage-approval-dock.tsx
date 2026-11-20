import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  Bot,
  Check,
  ChevronDown,
  ChevronLeft,
  ChevronRight,
  FilePenLine,
  FileText,
  MessageCircleQuestion,
  PenLine,
  Search,
  Shield,
  Terminal,
  X,
  type LucideIcon,
} from "lucide-react";
import {
  type QuestionItem,
  type TimelineItemView,
  type ToolCallDetail,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { modSymbol } from "@/lib/keybindings";
import {
  isAlwaysDecision,
  isDenyDecision,
  setFocusedDecisionTarget,
  stripWorkspacePrefix,
} from "@/lib/timeline-coalesce";
import { SimpleTooltip } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type PendingPermissionItem = Extract<TimelineItemView, { item_type: "permission_request" }>;
type PendingQuestionItem = Extract<TimelineItemView, { item_type: "question_request" }>;

type QueuedDecision =
  | { kind: "permission"; seq: number; item: PendingPermissionItem }
  | { kind: "question"; seq: number; item: PendingQuestionItem };

interface PermissionTargetPresentation {
  kindLabel: string;
  icon: LucideIcon;
  preview: string;
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
  "title",
];

function formatDecisionLabel(dec: string): string {
  const norm = dec.trim().toLowerCase();
  if (norm === "allow" || norm === "allow_once" || norm === "accept") return "Allow";
  if (norm === "allow_always" || norm === "always") return "Always allow";
  if (norm === "deny" || norm === "reject" || norm === "reject_once") return "Deny";
  if (norm === "decline") return "Decline";
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
): PermissionTargetPresentation {
  switch (detail.kind) {
    case "shell": {
      const cmd = detail.command.trim();
      const needsExpand = cmd.includes("\n") || cmd.length > 110;
      return {
        kindLabel: "Shell",
        icon: Terminal,
        preview: cmd || "(empty command)",
        isShell: true,
        expandableContent: needsExpand ? cmd : null,
        expandLabel: "Show full command",
        isDiff: false,
      };
    }
    case "file_edit": {
      const rel = stripWorkspacePrefix(detail.path, cwd) || detail.path;
      const diff = detail.unified_diff?.trim() || null;
      return {
        kindLabel: "Edit",
        icon: FilePenLine,
        preview: rel,
        isShell: false,
        expandableContent: diff,
        expandLabel: "View diff",
        isDiff: true,
      };
    }
    case "file_read": {
      const rel = stripWorkspacePrefix(detail.path, cwd) || detail.path;
      return {
        kindLabel: "Read",
        icon: FileText,
        preview: rel,
        isShell: false,
        expandableContent: null,
        expandLabel: "Show details",
        isDiff: false,
      };
    }
    case "search": {
      const scopeRel = detail.scope ? stripWorkspacePrefix(detail.scope, cwd) || detail.scope : "";
      return {
        kindLabel: "Search",
        icon: Search,
        preview: scopeRel ? `${detail.query} in ${scopeRel}` : detail.query,
        isShell: false,
        expandableContent: null,
        expandLabel: "Show details",
        isDiff: false,
      };
    }
    case "subagent": {
      return {
        kindLabel: "Subagent",
        icon: Bot,
        preview: detail.role || "Subagent",
        isShell: false,
        expandableContent: detail.prompt_preview?.trim() || null,
        expandLabel: "Show prompt",
        isDiff: false,
      };
    }
    case "generic": {
      const rawArgs = detail.arguments_json.trim();
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
              const preferredKey = GENERIC_SUMMARY_KEYS.find(
                (k) => k in obj && obj[k] !== null && obj[k] !== undefined && obj[k] !== "",
              );
              const picked = preferredKey
                ? [preferredKey, obj[preferredKey]]
                : entries.find(([, v]) => typeof v === "string" || typeof v === "number");
              if (picked) {
                const valStr =
                  typeof picked[1] === "string" ? picked[1] : JSON.stringify(picked[1]);
                const clipped = valStr.length > 72 ? `${valStr.slice(0, 72)}…` : valStr;
                summary = `${picked[0]}: ${clipped}`;
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
        kindLabel: "Tool",
        icon: Shield,
        preview: summary ? `${detail.tool_name} · ${summary}` : detail.tool_name,
        isShell: false,
        expandableContent,
        expandLabel: "Show arguments",
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
  if (total <= 1) return null;

  return (
    <div className="bg-black/[0.04] dark:bg-white/[0.06] inline-flex items-center rounded-full p-0.5">
      <button
        type="button"
        onClick={onPrev}
        aria-label="Previous pending request"
        className="text-muted-foreground hover:text-foreground inline-flex size-5.5 cursor-pointer items-center justify-center rounded-full transition-colors hover:bg-black/[0.05] dark:hover:bg-white/[0.08]"
      >
        <ChevronLeft className="size-3.5 stroke-[1.9]" />
      </button>
      <span className="text-muted-foreground tabular-nums px-1.5 text-[11px] font-medium">
        {activeIndex + 1} / {total}
      </span>
      <button
        type="button"
        onClick={onNext}
        aria-label="Next pending request"
        className="text-muted-foreground hover:text-foreground inline-flex size-5.5 cursor-pointer items-center justify-center rounded-full transition-colors hover:bg-black/[0.05] dark:hover:bg-white/[0.08]"
      >
        <ChevronRight className="size-3.5 stroke-[1.9]" />
      </button>
    </div>
  );
}

function PermissionDetailExpanded({ content, isDiff }: { content: string; isDiff: boolean }) {
  const lines = content.split(/\r?\n/);

  return (
    <ScrollArea
      orientation="both"
      className="mt-2 rounded-[10px] border border-black/[0.06] bg-black/[0.022] dark:border-white/[0.07] dark:bg-white/[0.025]"
      viewportClassName="selectable-text max-h-40 overscroll-contain px-3 py-2 font-mono text-[12px] leading-[1.5]"
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
                "min-w-fit px-1 whitespace-pre",
                isAdd &&
                  "bg-emerald-500/[0.10] text-emerald-900 dark:bg-emerald-500/[0.14] dark:text-emerald-200",
                isDel &&
                  "bg-rose-500/[0.10] text-rose-900 dark:bg-rose-500/[0.14] dark:text-rose-200",
                isMeta && "text-muted-foreground/75",
                !isAdd && !isDel && !isMeta && "text-foreground/82",
              )}
            >
              {line || " "}
            </div>
          );
        })
      ) : (
        <pre className="text-foreground/85 m-0 font-mono text-[12px] leading-[1.5] whitespace-pre-wrap break-words">
          {content}
        </pre>
      )}
    </ScrollArea>
  );
}

function PermissionApprovalCard({
  hostId,
  agentId,
  cwd,
  perm,
  queueIndex,
  queueTotal,
  onStepPrev,
  onStepNext,
}: {
  hostId: string;
  agentId: string;
  cwd: string;
  perm: PendingPermissionItem;
  queueIndex: number;
  queueTotal: number;
  onStepPrev: () => void;
  onStepNext: () => void;
}) {
  const mod = modSymbol();
  const [expanded, setExpanded] = useState(false);
  const [submittingDecision, setSubmittingDecision] = useState<string | null>(null);

  useEffect(() => {
    if (!submittingDecision) return;
    const id = window.setTimeout(() => setSubmittingDecision(null), 2500);
    return () => window.clearTimeout(id);
  }, [submittingDecision]);

  const decisions = useMemo(
    () => orderDecisions(perm.allowed_decisions),
    [perm.allowed_decisions],
  );
  const primaryAllowChoice = decisions.find((d) => !isDenyDecision(d) && !isAlwaysDecision(d)) ?? null;
  const primaryAlwaysChoice = decisions.find((d) => isAlwaysDecision(d)) ?? null;
  const primaryDenyChoice = decisions.find((d) => isDenyDecision(d)) ?? null;

  const target = useMemo(
    () => describePermissionTarget(perm.detail, cwd),
    [perm.detail, cwd],
  );
  const KindIcon = target.icon;

  const handleResolve = async (dec: string) => {
    if (submittingDecision) return;
    setSubmittingDecision(dec);
    try {
      await orbitIpc.resolvePermission(hostId, agentId, perm.request_id, dec);
    } catch (err) {
      setSubmittingDecision(null);
      orbitActions.showToast(`Permission action failed: ${String(err)}`);
    }
  };

  return (
    <div className="approval-dock-surface orbit-dock-enter overflow-hidden rounded-[18px]">
      <div className="flex items-center justify-between gap-3 border-b border-black/[0.055] px-4 py-2.5 dark:border-white/[0.06]">
        <div className="flex min-w-0 items-center gap-2">
          <span className="size-1.5 shrink-0 rounded-full bg-amber-500/80" />
          <KindIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
          <span className="text-foreground/90 truncate text-[12.5px] font-medium">
            {perm.title}
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          <span className="text-muted-foreground/75 text-[11px] font-medium">
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
        <div className="rounded-[10px] bg-black/[0.028] px-3 py-2 dark:bg-white/[0.035]">
          <div className="selectable-text text-foreground/88 line-clamp-2 font-mono text-[12.5px] leading-[1.5] break-all">
            {target.isShell && (
              <span className="text-muted-foreground/65 mr-1.5 select-none">$</span>
            )}
            <span>{target.preview}</span>
          </div>
        </div>

        {target.expandableContent && (
          <div className="mt-1.5">
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 rounded-md px-1 py-0.5 text-[11.5px] font-medium transition-colors"
            >
              <ChevronDown
                className={cn(
                  "size-3.5 stroke-[1.75] transition-transform duration-150",
                  expanded && "rotate-180",
                )}
              />
              <span>{expanded ? "Hide" : target.expandLabel}</span>
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

      <div className="flex flex-wrap items-center justify-end gap-1.5 border-t border-black/[0.05] px-4 py-2.5 dark:border-white/[0.055]">
        {decisions.map((dec) => {
          const isDeny = isDenyDecision(dec);
          const isAlways = isAlwaysDecision(dec);
          const isPrimaryAllow = dec === primaryAllowChoice;
          const isMatchedAlways = dec === primaryAlwaysChoice;
          const isMatchedDeny = dec === primaryDenyChoice;
          const label = formatDecisionLabel(dec);
          const disabled = submittingDecision !== null;

          const buttonEl = (
            <button
              key={dec}
              type="button"
              disabled={disabled}
              onClick={() => void handleResolve(dec)}
              className={cn(
                "orbit-press inline-flex h-7.5 cursor-pointer items-center gap-1.5 rounded-full text-[12.5px] font-medium disabled:cursor-not-allowed disabled:opacity-50",
                isDeny
                  ? "text-muted-foreground px-3 hover:bg-rose-500/[0.08] hover:text-rose-600 dark:hover:text-rose-400"
                  : isAlways
                    ? "text-foreground/85 border border-black/[0.09] px-3 hover:bg-black/[0.04] dark:border-white/[0.11] dark:hover:bg-white/[0.06]"
                    : "bg-[#141416] px-3.5 text-white hover:bg-[#27272A] dark:bg-[#F4F4F6] dark:text-[#141416] dark:hover:bg-white",
              )}
            >
              <span>{label}</span>
              {isPrimaryAllow && (
                <span className="ml-0.5 text-[11px] font-normal opacity-55">{mod}Y</span>
              )}
            </button>
          );

          if (isMatchedAlways) {
            return (
              <SimpleTooltip key={dec} label={label} shortcut={`${mod}⇧Y`} side="top">
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
  hostId,
  agentId,
  requestId,
  questions,
  queueIndex,
  queueTotal,
  onStepPrev,
  onStepNext,
}: {
  hostId: string;
  agentId: string;
  requestId: string;
  questions: QuestionItem[];
  queueIndex: number;
  queueTotal: number;
  onStepPrev: () => void;
  onStepNext: () => void;
}) {
  const mod = modSymbol();
  const [selections, setSelections] = useState<Record<number, string[]>>({});
  const [customAnswers, setCustomAnswers] = useState<Record<number, string>>({});
  const [submitting, setSubmitting] = useState(false);
  const inputRefs = useRef<Array<HTMLInputElement | null>>([]);

  useEffect(() => {
    if (!submitting) return;
    const id = window.setTimeout(() => setSubmitting(false), 2500);
    return () => window.clearTimeout(id);
  }, [submitting]);

  const toggleOption = (qIdx: number, opt: string, multi: boolean) => {
    if (submitting) return;
    if (!multi) {
      setCustomAnswers((prev) => (prev[qIdx] ? { ...prev, [qIdx]: "" } : prev));
    }
    setSelections((prev) => {
      const cur = prev[qIdx] ?? [];
      if (!multi) return { ...prev, [qIdx]: [opt] };
      return {
        ...prev,
        [qIdx]: cur.includes(opt) ? cur.filter((x) => x !== opt) : [...cur, opt],
      };
    });
  };

  const handleCustomChange = (qIdx: number, value: string, multi: boolean) => {
    if (submitting) return;
    setCustomAnswers((prev) => ({ ...prev, [qIdx]: value }));
    if (!multi && value.trim().length > 0) {
      setSelections((prev) => (prev[qIdx]?.length ? { ...prev, [qIdx]: [] } : prev));
    }
  };

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
    } catch (err) {
      setSubmitting(false);
      orbitActions.showToast(`Failed to submit response: ${String(err)}`);
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
        if (e.key === "Enter" && (e.metaKey || e.ctrlKey) && canSubmit && !submitting) {
          e.preventDefault();
          void handleSubmit();
        }
      }}
      className="approval-dock-surface orbit-dock-enter overflow-hidden rounded-[18px]"
    >
      <div className="flex items-center justify-between gap-3 border-b border-black/[0.055] px-4 py-2.5 dark:border-white/[0.06]">
        <div className="flex min-w-0 items-center gap-2">
          <span className="size-1.5 shrink-0 rounded-full bg-amber-500/80" />
          <MessageCircleQuestion className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
          <span className="text-foreground/90 truncate text-[12.5px] font-medium">
            Clarification needed
          </span>
        </div>

        <div className="flex shrink-0 items-center gap-2">
          {questions.length > 1 && (
            <span className="text-muted-foreground/75 tabular-nums text-[11.5px] font-medium">
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
        <div className="space-y-4">
          {questions.map((q, qIdx) => {
            const cleanHeader = q.header?.trim();
            const showHeader = Boolean(cleanHeader && cleanHeader !== q.question.trim());
            const customVal = customAnswers[qIdx] ?? "";
            const hasCustomText = customVal.trim().length > 0;

            return (
              <div
                key={q.id || qIdx}
                className={cn(
                  "space-y-2",
                  qIdx > 0 && "border-t border-black/[0.05] pt-3.5 dark:border-white/[0.055]",
                )}
              >
                <div className="space-y-0.5">
                  {showHeader && (
                    <div className="text-muted-foreground/75 text-[11px] font-medium tracking-wide uppercase">
                      {cleanHeader}
                    </div>
                  )}
                  <div className="selectable-text text-foreground text-[13.5px] leading-[1.45] font-medium">
                    {questions.length > 1 && (
                      <span className="text-muted-foreground/65 tabular-nums mr-1.5">
                        {qIdx + 1}.
                      </span>
                    )}
                    <span>{q.question}</span>
                  </div>
                  {q.is_multi_select && (
                    <div className="text-muted-foreground text-[11.5px]">Select all that apply</div>
                  )}
                </div>

                <div className="divide-y divide-black/[0.05] overflow-hidden rounded-[12px] border border-black/[0.07] dark:divide-white/[0.055] dark:border-white/[0.085]">
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
                            ? "bg-black/[0.048] text-foreground font-medium dark:bg-white/[0.075]"
                            : "text-foreground/82 bg-transparent hover:bg-black/[0.025] dark:hover:bg-white/[0.035]",
                        )}
                      >
                        <span
                          className={cn(
                            "mt-[2.5px] flex size-[15px] shrink-0 items-center justify-center border transition-colors",
                            q.is_multi_select ? "rounded-[4px]" : "rounded-full",
                            chosen
                              ? "border-[#141416] bg-[#141416] dark:border-[#F4F4F6] dark:bg-[#F4F4F6]"
                              : "border-black/22 dark:border-white/25",
                          )}
                        >
                          {chosen &&
                            (q.is_multi_select ? (
                              <Check className="size-2.5 stroke-[2.6] text-white dark:text-[#141416]" />
                            ) : (
                              <span className="size-[5px] rounded-full bg-white dark:bg-[#141416]" />
                            ))}
                        </span>
                        <span className="min-w-0 flex-1 break-words">
                          <span>{label}</span>
                          {recommended && (
                            <span className="text-muted-foreground ml-1.5 inline-flex items-center rounded-full bg-black/[0.055] px-1.5 py-0.5 align-middle text-[10.5px] leading-none font-medium dark:bg-white/[0.085]">
                              Recommended
                            </span>
                          )}
                        </span>
                      </button>
                    );
                  })}

                  <div
                    className={cn(
                      "flex items-center gap-2.5 px-3 py-2 transition-colors",
                      hasCustomText && "bg-black/[0.048] dark:bg-white/[0.075]",
                    )}
                  >
                    <PenLine
                      className={cn(
                        "size-3.5 shrink-0 stroke-[1.75]",
                        hasCustomText ? "text-foreground" : "text-muted-foreground/55",
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
                      onKeyDown={(e) => {
                        if (e.key === "Enter" && !e.shiftKey && !(e.metaKey || e.ctrlKey)) {
                          e.preventDefault();
                          if (canSubmit) {
                            void handleSubmit();
                          } else {
                            focusNextUnansweredInput(qIdx);
                          }
                        }
                      }}
                      placeholder={q.options.length > 0 ? "Other…" : "Type your answer…"}
                      className="text-foreground placeholder:text-muted-foreground/55 w-full bg-transparent text-[13px] leading-[1.45] outline-none disabled:cursor-not-allowed disabled:opacity-60"
                    />
                    {customVal.length > 0 && !submitting && (
                      <button
                        type="button"
                        aria-label="Clear custom answer"
                        onClick={() => handleCustomChange(qIdx, "", q.is_multi_select)}
                        className="text-muted-foreground hover:text-foreground inline-flex size-4 shrink-0 cursor-pointer items-center justify-center rounded-full"
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

      <div className="flex items-center justify-between gap-2 border-t border-black/[0.05] px-4 py-2.5 dark:border-white/[0.055]">
        <span className="text-muted-foreground/70 text-[11.5px]">
          {canSubmit ? `${mod}↵ to submit` : "Choose an option or write a reply"}
        </span>
        <button
          type="button"
          disabled={!canSubmit || submitting}
          onClick={() => void handleSubmit()}
          className="orbit-press inline-flex h-7.5 cursor-pointer items-center rounded-full bg-[#141416] px-3.5 text-[12.5px] font-medium text-white hover:bg-[#27272A] disabled:cursor-not-allowed disabled:opacity-35 dark:bg-[#F4F4F6] dark:text-[#141416] dark:hover:bg-white"
        >
          {submitting ? "Submitting…" : "Submit"}
        </button>
      </div>
    </div>
  );
}

export const StickyApprovalDock = memo(
  function StickyApprovalDock({
    hostId,
    agentId,
    cwd,
    permissions,
    questions,
  }: {
    hostId: string;
    agentId: string;
    cwd: string;
    permissions: PendingPermissionItem[];
    questions: PendingQuestionItem[];
  }) {
    const [focusedRequestId, setFocusedRequestId] = useState<string | null>(null);
    const [fallbackSlotIndex, setFallbackSlotIndex] = useState<number>(0);

    const queue = useMemo<QueuedDecision[]>(() => {
      const seen = new Set<string>();
      const merged: QueuedDecision[] = [];

      for (const item of permissions) {
        const key = `permission:${item.request_id}`;
        if (!seen.has(key)) {
          seen.add(key);
          merged.push({ kind: "permission", seq: item.seq, item });
        }
      }
      for (const item of questions) {
        const key = `question:${item.request_id}`;
        if (!seen.has(key)) {
          seen.add(key);
          merged.push({ kind: "question", seq: item.seq, item });
        }
      }

      merged.sort((a, b) => a.seq - b.seq);
      return merged;
    }, [permissions, questions]);

    const activeIndex = useMemo(() => {
      if (queue.length === 0) return 0;
      if (focusedRequestId) {
        const idx = queue.findIndex((entry) => entry.item.request_id === focusedRequestId);
        if (idx >= 0) return idx;
      }
      return Math.min(Math.max(0, fallbackSlotIndex), queue.length - 1);
    }, [queue, focusedRequestId, fallbackSlotIndex]);

    const activeEntry = queue[activeIndex] ?? null;

    useEffect(() => {
      if (!activeEntry) {
        setFocusedDecisionTarget(null);
        return;
      }
      setFocusedDecisionTarget({
        kind: activeEntry.kind,
        requestId: activeEntry.item.request_id,
      });
      return () => {
        setFocusedDecisionTarget(null);
      };
    }, [activeEntry]);

    if (!activeEntry) return null;

    const handleStep = (delta: number) => {
      if (queue.length <= 1) return;
      const nextIdx = (activeIndex + delta + queue.length) % queue.length;
      setFallbackSlotIndex(nextIdx);
      setFocusedRequestId(queue[nextIdx].item.request_id);
    };

    return (
      <div className="w-full px-[24px] pb-2.5">
        <div className="mx-auto w-full max-w-[var(--orbit-stage-max-w,768px)]">
          {activeEntry.kind === "permission" ? (
            <PermissionApprovalCard
              key={activeEntry.item.request_id}
              hostId={hostId}
              agentId={agentId}
              cwd={cwd}
              perm={activeEntry.item}
              queueIndex={activeIndex}
              queueTotal={queue.length}
              onStepPrev={() => handleStep(-1)}
              onStepNext={() => handleStep(1)}
            />
          ) : (
            <StickyQuestionDockCard
              key={activeEntry.item.request_id}
              hostId={hostId}
              agentId={agentId}
              requestId={activeEntry.item.request_id}
              questions={activeEntry.item.questions}
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
    prev.permissions.every((p, i) => p.request_id === next.permissions[i]?.request_id) &&
    prev.questions.every((q, i) => q.request_id === next.questions[i]?.request_id),
);
