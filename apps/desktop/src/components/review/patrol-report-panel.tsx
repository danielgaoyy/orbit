import { memo, useEffect, useMemo, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  FileCode2,
  ShieldAlert,
  ShieldCheck,
  ShieldX,
  Wrench,
  X,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import type {
  PrPatrolCategory,
  PrPatrolFinding,
  PrPatrolReport,
  PrPatrolSeverity,
} from "@/lib/orbit-ipc";
import {
  computePatrolVerdict,
  countFindingsBySeverity,
  formatPatrolDuration,
  formatPatrolLocation,
  sortPatrolFindings,
  type PatrolVerdict,
} from "@/lib/pr-patrol";
import { cn } from "@/lib/utils";

export interface PatrolReportPanelProps {
  report: PrPatrolReport;
  onJumpToFinding: (finding: PrPatrolFinding) => void;
  onOpenFindingInEditor: (finding: PrPatrolFinding) => void;
  onFixFinding: (finding: PrPatrolFinding) => void;
  onFixActionable: (findings: readonly PrPatrolFinding[]) => void;
  onDismiss: () => void;
}

type SeverityFilter = "all" | PrPatrolSeverity;

export function patrolSeverityBadgeVariant(
  severity: PrPatrolSeverity,
): "destructive" | "warning" | "default" {
  switch (severity) {
    case "P0":
      return "destructive";
    case "P1":
      return "warning";
    default:
      return "default";
  }
}

function verdictMeta(verdict: PatrolVerdict): {
  badgeVariant: "success" | "warning" | "destructive";
  labelKey: "review.patrolVerdictClean" | "review.patrolVerdictNeedsReview" | "review.patrolVerdictBlocked";
  icon: typeof ShieldCheck;
  iconClass: string;
} {
  switch (verdict) {
    case "blocked":
      return {
        badgeVariant: "destructive",
        labelKey: "review.patrolVerdictBlocked",
        icon: ShieldX,
        iconClass: "text-signal-danger",
      };
    case "needs_review":
      return {
        badgeVariant: "warning",
        labelKey: "review.patrolVerdictNeedsReview",
        icon: ShieldAlert,
        iconClass: "text-signal-warning",
      };
    default:
      return {
        badgeVariant: "success",
        labelKey: "review.patrolVerdictClean",
        icon: ShieldCheck,
        iconClass: "text-signal-success",
      };
  }
}

function categoryLabelKey(category: PrPatrolCategory):
  | "review.patrolCategorySecurity"
  | "review.patrolCategoryCorrectness"
  | "review.patrolCategoryConcurrency"
  | "review.patrolCategoryPerformance"
  | "review.patrolCategoryMaintainability" {
  switch (category) {
    case "security":
      return "review.patrolCategorySecurity";
    case "correctness":
      return "review.patrolCategoryCorrectness";
    case "concurrency":
      return "review.patrolCategoryConcurrency";
    case "performance":
      return "review.patrolCategoryPerformance";
    default:
      return "review.patrolCategoryMaintainability";
  }
}

export const PatrolReportPanel = memo(function PatrolReportPanel({
  report,
  onJumpToFinding,
  onOpenFindingInEditor,
  onFixFinding,
  onFixActionable,
  onDismiss,
}: PatrolReportPanelProps) {
  const { t } = useI18n();
  const sortedFindings = useMemo(() => sortPatrolFindings(report.findings), [report.findings]);
  const counts = useMemo(() => countFindingsBySeverity(sortedFindings), [sortedFindings]);
  const verdict = useMemo(() => computePatrolVerdict(report), [report]);
  const meta = verdictMeta(verdict);
  const VerdictIcon = meta.icon;

  const [expanded, setExpanded] = useState(sortedFindings.length > 0);
  const [severityFilter, setSeverityFilter] = useState<SeverityFilter>("all");

  // Auto-open when a fresh report has findings; stay compact on clean reports
  useEffect(() => {
    setExpanded(report.findings.length > 0);
    setSeverityFilter("all");
  }, [report]);

  const visibleFindings = useMemo(() => {
    if (severityFilter === "all") return sortedFindings;
    return sortedFindings.filter((f) => f.severity === severityFilter);
  }, [sortedFindings, severityFilter]);

  const actionableFindings = useMemo(
    () => sortedFindings.filter((f) => f.severity === "P0" || f.severity === "P1"),
    [sortedFindings],
  );

  const distinctSeverities = useMemo(() => {
    const present: PrPatrolSeverity[] = [];
    if (counts.p0 > 0) present.push("P0");
    if (counts.p1 > 0) present.push("P1");
    if (counts.p2 > 0) present.push("P2");
    return present;
  }, [counts]);

  const durationLabel = useMemo(() => formatPatrolDuration(report.duration_ms), [report.duration_ms]);
  const shortRange =
    report.base_ref && report.head_ref
      ? `${report.base_ref.slice(0, 7)}..${report.head_ref.slice(0, 7)}`
      : null;

  return (
    <div
      data-orbit-patrol-report="true"
      className="border-border bg-surface-raised/60 flex shrink-0 flex-col gap-2 border-b px-3 py-2"
    >
      {/* Compact Patrol Summary Strip */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <button
          type="button"
          onClick={() => setExpanded((v) => !v)}
          className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
        >
          {expanded ? (
            <ChevronDown className="text-ink-tertiary size-[13px] shrink-0" />
          ) : (
            <ChevronRight className="text-ink-tertiary size-[13px] shrink-0" />
          )}
          <VerdictIcon className={cn("size-[13.5px] shrink-0", meta.iconClass)} />
          <Badge variant={meta.badgeVariant} className="h-[19px] px-1.5 text-[10.5px]">
            {t(meta.labelKey)}
          </Badge>

          {/* Tabular Severity Counters */}
          {counts.total > 0 && (
            <div className="tabular-nums flex items-center gap-1 font-mono text-[10.5px]">
              {counts.p0 > 0 && (
                <span className="text-signal-danger font-medium">P0 {counts.p0}</span>
              )}
              {counts.p1 > 0 && (
                <span className="text-signal-warning font-medium">P1 {counts.p1}</span>
              )}
              {counts.p2 > 0 && (
                <span className="text-signal-info font-medium">P2 {counts.p2}</span>
              )}
            </div>
          )}

          <span className="text-ink-tertiary tabular-nums truncate text-[11px]">
            ·{" "}
            {t("review.patrolMetaSummary", {
              files: report.files_reviewed,
              duration: durationLabel,
            })}
          </span>
        </button>

        <div className="flex shrink-0 items-center gap-1">
          {actionableFindings.length > 0 && (
            <button
              type="button"
              onClick={() => onFixActionable(actionableFindings)}
              className={cn(
                "inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px] font-medium transition-colors",
                counts.p0 > 0
                  ? "border-signal-danger-border bg-signal-danger-bg text-signal-danger hover:opacity-90"
                  : "border-border-default bg-surface-base text-ink-primary hover:bg-accent",
              )}
            >
              <Wrench className="size-[11px] shrink-0" />
              <span className="tabular-nums">
                {t("review.patrolFixAllActionable", { count: actionableFindings.length })}
              </span>
            </button>
          )}

          <SimpleTooltip label={t("review.patrolDismiss")}>
            <button
              type="button"
              aria-label={t("review.patrolDismiss")}
              onClick={onDismiss}
              className="text-ink-tertiary hover:bg-accent hover:text-ink-primary inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md transition-colors"
            >
              <X className="size-[12px]" />
            </button>
          </SimpleTooltip>
        </div>
      </div>

      {/* Expanded Patrol Detail & Findings List */}
      {expanded && (
        <div className="flex flex-col gap-1.5">
          {/* Clean Verdict Detail */}
          {counts.total === 0 ? (
            <div className="border-border-subtle bg-surface-recessed text-ink-secondary flex items-center justify-between gap-2 rounded-lg border px-2.5 py-2 text-[11.5px]">
              <div className="flex min-w-0 items-center gap-1.5">
                <CheckCircle2 className="text-signal-success size-[13px] shrink-0" />
                <span className="truncate">{report.summary || t("review.patrolCleanBody")}</span>
              </div>
              {shortRange && (
                <span className="text-ink-faint shrink-0 font-mono text-[10.5px]">
                  {shortRange}
                </span>
              )}
            </div>
          ) : (
            <>
              {/* Filter Bar + Scope Metadata */}
              <div className="flex flex-wrap items-center justify-between gap-2">
                <div className="flex items-center gap-1">
                  {distinctSeverities.length > 1 && (
                    <>
                      <button
                        type="button"
                        onClick={() => setSeverityFilter("all")}
                        className={cn(
                          "inline-flex h-[20px] cursor-pointer items-center rounded-[5px] px-1.5 text-[10.5px] font-medium transition-colors",
                          severityFilter === "all"
                            ? "bg-accent text-ink-primary"
                            : "text-ink-tertiary hover:text-ink-primary",
                        )}
                      >
                        {t("review.patrolFilterAll", { count: counts.total })}
                      </button>
                      {distinctSeverities.map((sev) => {
                        const count =
                          sev === "P0" ? counts.p0 : sev === "P1" ? counts.p1 : counts.p2;
                        const active = severityFilter === sev;
                        return (
                          <button
                            key={sev}
                            type="button"
                            onClick={() => setSeverityFilter(sev)}
                            className={cn(
                              "tabular-nums inline-flex h-[20px] cursor-pointer items-center rounded-[5px] px-1.5 font-mono text-[10.5px] font-medium transition-colors",
                              active
                                ? "bg-accent text-ink-primary"
                                : "text-ink-tertiary hover:text-ink-primary",
                            )}
                          >
                            {sev} ({count})
                          </button>
                        );
                      })}
                    </>
                  )}
                </div>

                <div className="text-ink-faint tabular-nums flex items-center gap-2 font-mono text-[10.5px]">
                  {report.dropped_false_positives > 0 && (
                    <span className="font-sans">
                      {t("review.patrolFilteredFp", {
                        count: report.dropped_false_positives,
                      })}
                    </span>
                  )}
                  {shortRange && <span>{shortRange}</span>}
                </div>
              </div>

              {/* Scrollable Findings Well */}
              <div className="border-border-subtle bg-surface-recessed flex max-h-[260px] flex-col gap-1.5 overflow-y-auto rounded-lg border p-1.5">
                {visibleFindings.map((finding) => {
                  const loc = formatPatrolLocation(finding);
                  return (
                    <div
                      key={finding.id}
                      className="border-border-subtle bg-surface-base flex flex-col gap-1.5 rounded-md border px-2.5 py-2"
                    >
                      <div className="flex items-center justify-between gap-2">
                        <div className="flex min-w-0 flex-1 items-center gap-1.5">
                          <Badge
                            variant={patrolSeverityBadgeVariant(finding.severity)}
                            className="h-[18px] px-1.5 font-mono text-[10px]"
                          >
                            {finding.severity}
                          </Badge>
                          <span className="text-ink-tertiary shrink-0 text-[10.5px] font-medium">
                            {t(categoryLabelKey(finding.category))}
                          </span>
                          <span className="text-ink-faint shrink-0 text-[10px]">·</span>
                          <SimpleTooltip label={t("review.patrolJumpToFindingTooltip")}>
                            <button
                              type="button"
                              onClick={() => onJumpToFinding(finding)}
                              className="text-ink-secondary hover:text-ink-primary min-w-0 cursor-pointer truncate font-mono text-[11px] hover:underline"
                            >
                              {loc}
                            </button>
                          </SimpleTooltip>
                        </div>

                        <div className="flex shrink-0 items-center gap-1">
                          <SimpleTooltip label={t("review.patrolOpenInEditor")}>
                            <button
                              type="button"
                              aria-label={t("review.patrolOpenInEditor")}
                              onClick={() => onOpenFindingInEditor(finding)}
                              className="text-ink-tertiary hover:bg-accent hover:text-ink-primary inline-flex size-[21px] cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                            >
                              <FileCode2 className="size-[12px]" />
                            </button>
                          </SimpleTooltip>
                          <button
                            type="button"
                            onClick={() => onFixFinding(finding)}
                            className="border-border-default bg-surface-raised text-ink-primary hover:bg-accent inline-flex h-[21px] cursor-pointer items-center gap-1 rounded-[5px] border px-1.5 text-[10.5px] font-medium transition-colors"
                          >
                            <Wrench className="size-[10.5px] shrink-0" />
                            <span>{t("review.patrolFixWithAgent")}</span>
                          </button>
                        </div>
                      </div>

                      <button
                        type="button"
                        onClick={() => onJumpToFinding(finding)}
                        className="text-ink-primary hover:text-ink-primary/85 cursor-pointer text-left text-[12px] font-medium leading-snug"
                      >
                        {finding.title}
                      </button>

                      {finding.summary.trim() && (
                        <p className="text-ink-secondary text-[11.5px] leading-snug">
                          {finding.summary}
                        </p>
                      )}

                      {finding.evidence.trim() && (
                        <pre className="border-border-subtle bg-surface-recessed text-ink-secondary selectable-text m-0 overflow-x-auto rounded border px-2 py-1 font-mono text-[11px] leading-[17px]">
                          {finding.evidence}
                        </pre>
                      )}

                      {finding.suggested_fix.trim() && (
                        <p className="text-ink-tertiary text-[11px] leading-snug">
                          <span className="text-ink-secondary font-medium">
                            {t("review.patrolSuggestedFixLabel")}{" "}
                          </span>
                          {finding.suggested_fix}
                        </p>
                      )}
                    </div>
                  );
                })}
              </div>
            </>
          )}
        </div>
      )}
    </div>
  );
});
