import { memo, useMemo, useState } from "react";
import {
  CheckCircle2,
  ChevronDown,
  ChevronRight,
  Clock,
  ExternalLink,
  GitPullRequest,
  MessageSquareText,
  MinusCircle,
  RefreshCw,
  Wrench,
  XCircle,
} from "lucide-react";
import {
  buildFixFailingCiPrompt,
  buildFixPrCommentsPrompt,
  formatPathLineRange,
  getUnresolvedPrComments,
} from "@/lib/git-collab";
import { useI18n } from "@/lib/i18n";
import { orbitIpc, type GitCiCheckState, type GitPrStatus } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";

interface GitPrStatusBarProps {
  prStatus: GitPrStatus | null;
  isLoading: boolean;
  onRefresh: () => void;
  onOpenFile?: (path: string, focusLine?: number | null) => void;
}

function CheckStateIcon({ state }: { state: GitCiCheckState }) {
  switch (state) {
    case "passing":
      return <CheckCircle2 className="size-[12px] shrink-0 text-emerald-600 dark:text-emerald-400" />;
    case "failing":
      return <XCircle className="size-[12px] shrink-0 text-rose-600 dark:text-rose-400" />;
    case "pending":
      return <Clock className="size-[12px] shrink-0 text-amber-600 dark:text-amber-400" />;
    default:
      return <MinusCircle className="text-muted-foreground size-[12px] shrink-0" />;
  }
}

export const GitPrStatusBar = memo(function GitPrStatusBar({
  prStatus,
  isLoading,
  onRefresh,
  onOpenFile,
}: GitPrStatusBarProps) {
  const { t } = useI18n();
  const [expandedSection, setExpandedSection] = useState<"checks" | "comments" | null>(null);

  const unresolvedComments = useMemo(
    () => (prStatus ? getUnresolvedPrComments(prStatus) : []),
    [prStatus],
  );

  // Orbital Quiet Precision: render zero chrome when there is no active PR.
  if (!prStatus || prStatus.availability !== "available" || prStatus.number == null) {
    return null;
  }

  const totalChecks = prStatus.checks.length;
  const hasFailingCi = prStatus.failing_checks > 0;
  const hasUnresolvedComments = unresolvedComments.length > 0;

  const handleOpenExternal = (url: string | null | undefined) => {
    if (!url) return;
    void orbitIpc.openExternalBrowserUrl(url).catch(() => {});
  };

  const handleFixFailingCi = () => {
    const prompt = buildFixFailingCiPrompt(prStatus);
    if (!prompt) return;
    orbitActions.promoteSideQuestionToComposer(prompt);
    orbitActions.showToast(t("review.fixCiInsertedToast", { count: prStatus.failing_checks }));
  };

  const handleFixReviewComments = () => {
    const prompt = buildFixPrCommentsPrompt(prStatus);
    if (!prompt) return;
    orbitActions.promoteSideQuestionToComposer(prompt);
    orbitActions.showToast(
      t("review.fixPrCommentsInsertedToast", { count: unresolvedComments.length }),
    );
  };

  return (
    <div className="border-border bg-surface-raised/60 flex shrink-0 flex-col gap-2 border-b px-3 py-2">
      {/* Compact PR Summary Header */}
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 flex-1 items-center gap-1.5">
          <button
            type="button"
            title={prStatus.url ?? t("review.openPr")}
            onClick={() => handleOpenExternal(prStatus.url)}
            className="text-foreground hover:text-foreground/80 group inline-flex min-w-0 cursor-pointer items-center gap-1.5 text-left text-[12px] font-medium transition-colors"
          >
            <GitPullRequest
              className={cn(
                "size-[13px] shrink-0",
                prStatus.is_draft
                  ? "text-muted-foreground"
                  : "text-emerald-600 dark:text-emerald-400",
              )}
            />
            <span className="tabular-nums shrink-0 font-mono text-[11.5px]">
              #{prStatus.number}
            </span>
            {prStatus.title && <span className="truncate">{prStatus.title}</span>}
            {prStatus.url && (
              <ExternalLink className="text-muted-foreground size-[10.5px] shrink-0 opacity-0 transition-opacity group-hover:opacity-80" />
            )}
          </button>
          {prStatus.is_draft && (
            <span className="border-border text-muted-foreground shrink-0 rounded border px-1 py-0.25 text-[10px] font-medium">
              {t("review.prDraftBadge")}
            </span>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1">
          {totalChecks > 0 && (
            <button
              type="button"
              onClick={() =>
                setExpandedSection((prev) => (prev === "checks" ? null : "checks"))
              }
              title={t("review.prChecksToggleTitle")}
              className={cn(
                "border-border hover:bg-accent inline-flex h-[22px] cursor-pointer items-center gap-1.5 rounded-md border px-1.5 font-mono text-[11px] transition-colors",
                expandedSection === "checks" ? "bg-accent text-foreground" : "text-muted-foreground",
              )}
            >
              {prStatus.passing_checks > 0 && (
                <span className="tabular-nums inline-flex items-center gap-1 text-emerald-600 dark:text-emerald-400">
                  <span className="size-[6px] rounded-full bg-emerald-500" />
                  {prStatus.passing_checks}
                </span>
              )}
              {prStatus.failing_checks > 0 && (
                <span className="tabular-nums inline-flex items-center gap-1 text-rose-600 dark:text-rose-400">
                  <span className="size-[6px] rounded-full bg-rose-500" />
                  {prStatus.failing_checks}
                </span>
              )}
              {prStatus.pending_checks > 0 && (
                <span className="tabular-nums inline-flex items-center gap-1 text-amber-600 dark:text-amber-400">
                  <span className="size-[6px] animate-pulse rounded-full bg-amber-500" />
                  {prStatus.pending_checks}
                </span>
              )}
              {expandedSection === "checks" ? (
                <ChevronDown className="size-[11px] opacity-70" />
              ) : (
                <ChevronRight className="size-[11px] opacity-70" />
              )}
            </button>
          )}

          {hasUnresolvedComments && (
            <button
              type="button"
              onClick={() =>
                setExpandedSection((prev) => (prev === "comments" ? null : "comments"))
              }
              title={t("review.prCommentsToggleTitle")}
              className={cn(
                "border-border hover:bg-accent inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-1.5 text-[11px] transition-colors",
                expandedSection === "comments"
                  ? "bg-accent text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <MessageSquareText className="size-[11px] shrink-0" />
              <span className="tabular-nums font-mono">{unresolvedComments.length}</span>
            </button>
          )}

          <button
            type="button"
            title={t("review.refreshPrStatusTitle")}
            onClick={onRefresh}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex size-[22px] cursor-pointer items-center justify-center rounded-md transition-colors"
          >
            <RefreshCw className={cn("size-[11.5px]", isLoading && "animate-spin")} />
          </button>
        </div>
      </div>

      {/* Contextual 1-Click Auto-Fix Triggers (hidden when green and zero unresolved comments) */}
      {(hasFailingCi || hasUnresolvedComments) && (
        <div className="flex flex-wrap items-center gap-1.5">
          {hasFailingCi && (
            <button
              type="button"
              onClick={handleFixFailingCi}
              className="border-rose-500/30 bg-rose-500/[0.08] text-rose-700 hover:bg-rose-500/[0.14] dark:border-rose-400/30 dark:bg-rose-500/[0.12] dark:text-rose-300 dark:hover:bg-rose-500/[0.18] inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-md border px-2 text-[11px] font-medium transition-colors"
            >
              <Wrench className="size-[11px] shrink-0" />
              <span>{t("review.fixFailingCi")}</span>
            </button>
          )}
          {hasUnresolvedComments && (
            <button
              type="button"
              onClick={handleFixReviewComments}
              className="border-border bg-background text-foreground/90 hover:bg-accent hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-md border px-2 text-[11px] font-medium transition-colors"
            >
              <MessageSquareText className="size-[11px] shrink-0 opacity-80" />
              <span>
                {t("review.fixReviewComments", { count: unresolvedComments.length })}
              </span>
            </button>
          )}
        </div>
      )}

      {/* Expandable CI Checks List */}
      {expandedSection === "checks" && totalChecks > 0 && (
        <div className="border-border-subtle bg-surface-recessed flex max-h-[144px] flex-col gap-0.5 overflow-y-auto rounded-lg border p-1.5">
          {prStatus.checks.map((check, idx) => (
            <div
              key={`${check.name}-${idx}`}
              className="hover:bg-accent/60 flex items-center justify-between gap-2 rounded-md px-2 py-1 text-[11.5px] transition-colors"
            >
              <div
                title={[check.name, check.workflow].filter(Boolean).join(" — ")}
                className="flex min-w-0 flex-1 items-center gap-1.5"
              >
                <CheckStateIcon state={check.state} />
                <span
                  className={cn(
                    "text-foreground min-w-0 truncate font-medium",
                    check.workflow ? "max-w-[65%] shrink-0" : "flex-1",
                  )}
                >
                  {check.name}
                </span>
                {check.workflow && (
                  <span className="text-muted-foreground min-w-0 flex-1 truncate text-[10.5px]">
                    {check.workflow}
                  </span>
                )}
              </div>
              {check.details_url && (
                <button
                  type="button"
                  title={t("overview.openExternally")}
                  onClick={() => handleOpenExternal(check.details_url)}
                  className="text-muted-foreground hover:text-foreground inline-flex size-[18px] shrink-0 cursor-pointer items-center justify-center rounded transition-colors"
                >
                  <ExternalLink className="size-[11px]" />
                </button>
              )}
            </div>
          ))}
        </div>
      )}

      {/* Expandable Unresolved Review Comments List */}
      {expandedSection === "comments" && hasUnresolvedComments && (
        <div className="border-border-subtle bg-surface-recessed flex max-h-[168px] flex-col gap-1.5 overflow-y-auto rounded-lg border p-2">
          {unresolvedComments.map((item) => {
            const loc = item.path
              ? formatPathLineRange(item.path, item.start_line ?? item.line, item.line)
              : null;
            return (
              <div
                key={item.id}
                className="border-border-subtle bg-surface-base flex flex-col gap-1 rounded-md border px-2 py-1.5 text-[11.5px]"
              >
                <div className="flex items-center justify-between gap-2">
                  <div className="flex min-w-0 items-center gap-1.5">
                    <span className="text-foreground font-medium">@{item.author}</span>
                    {loc && item.path && (
                      <button
                        type="button"
                        onClick={() => onOpenFile?.(item.path!, item.line ?? item.start_line)}
                        className="text-muted-foreground hover:text-foreground truncate font-mono text-[10.5px] hover:underline"
                      >
                        {loc}
                      </button>
                    )}
                  </div>
                  {item.url && (
                    <button
                      type="button"
                      title={t("overview.openExternally")}
                      onClick={() => handleOpenExternal(item.url)}
                      className="text-muted-foreground hover:text-foreground inline-flex size-[18px] shrink-0 cursor-pointer items-center justify-center rounded"
                    >
                      <ExternalLink className="size-[10.5px]" />
                    </button>
                  )}
                </div>
                <p className="text-muted-foreground line-clamp-3 whitespace-pre-wrap break-words text-[11.5px] leading-snug">
                  {item.body}
                </p>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
});
