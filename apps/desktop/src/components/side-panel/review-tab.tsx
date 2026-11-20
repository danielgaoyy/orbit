import { memo, useCallback, useMemo, useState } from "react";
import {
  Check,
  ChevronsDownUp,
  ChevronsUpDown,
  Columns2,
  GitBranch,
  RefreshCw,
  Rows3,
  Search,
  Send,
  WrapText,
} from "lucide-react";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import { DiffFilesViewer, type ReviewCommentDraft } from "@/components/diff-viewer";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { DiffFileView } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_DIFFS: DiffFileView[] = [];

function statusBadgeClass(status: string): string {
  const s = status.trim().toUpperCase();
  if (s === "A" || s === "??") return "text-emerald-600 dark:text-emerald-400";
  if (s === "D") return "text-rose-600 dark:text-rose-400";
  if (s === "M" || s === "R" || s === "E") return "text-amber-600 dark:text-amber-400";
  return "text-muted-foreground";
}

export const ReviewTab = memo(function ReviewTab({
  selectedHostId,
}: {
  selectedHostId: string;
}) {
  const reviewScope = useOrbitSelector((s) => s.reviewScope);
  const diffStaged = useOrbitSelector((s) => s.diffStaged);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);
  const selectedDiffPath = useOrbitSelector((s) => s.selectedDiffPath);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  const gitReview = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedWorkspaceCwd
      ? (s.gitReviews[`${s.selectedHostId}::${s.selectedWorkspaceCwd}`] ?? null)
      : null,
  );
  // Only read session_turn_diffs reference; never trigger working_tree re-renders on unrelated streaming deltas.
  const sessionTurnDiffs = useOrbitSelector((s) => {
    if (!s.selectedHostId || !s.selectedAgentId) return EMPTY_DIFFS;
    return s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.session_turn_diffs ?? EMPTY_DIFFS;
  });

  const [wrapLines, setWrapLines] = useState(false);
  const [fileQuery, setFileQuery] = useState("");
  const [isRefreshing, setIsRefreshing] = useState(false);
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(() => new Set());
  const [viewedPaths, setViewedPaths] = useState<Set<string>>(() => new Set());
  const [draftComments, setDraftComments] = useState<ReviewCommentDraft[]>([]);

  const displayedDiffFiles = useMemo(() => {
    const base =
      reviewScope === "session_turns" ? sessionTurnDiffs : (gitReview?.diff_files ?? EMPTY_DIFFS);
    if (!selectedDiffPath) {
      if (!fileQuery.trim()) return base;
      const q = fileQuery.trim().toLowerCase();
      return base.filter((f) => f.path.toLowerCase().includes(q));
    }
    const targetNorm = stripWorkspacePrefix(selectedDiffPath, selectedCwd).replace(/^\.\//, "");
    return base.filter(
      (f) =>
        f.path === selectedDiffPath ||
        stripWorkspacePrefix(f.path, selectedCwd).replace(/^\.\//, "") === targetNorm,
    );
  }, [reviewScope, sessionTurnDiffs, selectedDiffPath, selectedCwd, gitReview, fileQuery]);

  const diffStatsByPath = useMemo(() => {
    const map = new Map<string, { additions: number; deletions: number }>();
    const source =
      reviewScope === "session_turns" ? sessionTurnDiffs : (gitReview?.diff_files ?? EMPTY_DIFFS);
    for (const f of source) {
      map.set(f.path, { additions: f.additions, deletions: f.deletions });
    }
    return map;
  }, [reviewScope, sessionTurnDiffs, gitReview]);

  const fileSelectorItems = useMemo(() => {
    const raw =
      reviewScope === "session_turns"
        ? Array.from(new Set(sessionTurnDiffs.map((f) => f.path))).map((path) => ({
            path,
            status: "E",
          }))
        : (gitReview?.status?.files ?? []).map((f) => ({
            path: f.path,
            status: f.status,
          }));
    const q = fileQuery.trim().toLowerCase();
    if (!q) return raw;
    return raw.filter((f) => f.path.toLowerCase().includes(q));
  }, [reviewScope, sessionTurnDiffs, gitReview, fileQuery]);

  const allCollapsed =
    displayedDiffFiles.length > 0 && displayedDiffFiles.every((f) => collapsedPaths.has(f.path));

  const handleToggleCollapse = useCallback((path: string) => {
    setCollapsedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const handleToggleViewed = useCallback((path: string) => {
    setViewedPaths((prev) => {
      const next = new Set(prev);
      const markingViewed = !next.has(path);
      if (markingViewed) next.add(path);
      else next.delete(path);
      if (markingViewed) {
        setCollapsedPaths((cPrev) => {
          const cNext = new Set(cPrev);
          cNext.add(path);
          return cNext;
        });
      }
      return next;
    });
  }, []);

  const handleToggleAllCollapse = useCallback(() => {
    if (allCollapsed) {
      setCollapsedPaths(new Set());
    } else {
      setCollapsedPaths(new Set(displayedDiffFiles.map((f) => f.path)));
    }
  }, [allCollapsed, displayedDiffFiles]);

  const handleQuoteLine = useCallback((path: string, ln: number | null, code: string) => {
    orbitActions.quoteDiffLineToComposer(path, ln, code);
  }, []);

  const handleOpenFile = useCallback(
    (path: string, focusLine?: number | null) => {
      orbitActions.openFileInSidePanel(path, {
        hostId: selectedHostId,
        cwd: selectedCwd,
        focusLine,
      });
    },
    [selectedHostId, selectedCwd],
  );

  const handleAddComment = useCallback((draft: Omit<ReviewCommentDraft, "id">) => {
    setDraftComments((prev) => [
      ...prev,
      { ...draft, id: `rc_${Date.now()}_${Math.random().toString(36).slice(2, 6)}` },
    ]);
  }, []);

  const handleDeleteComment = useCallback((id: string) => {
    setDraftComments((prev) => prev.filter((c) => c.id !== id));
  }, []);

  const handleSubmitReviewComments = useCallback(() => {
    if (draftComments.length === 0) return;
    const grouped = new Map<string, ReviewCommentDraft[]>();
    for (const c of draftComments) {
      const list = grouped.get(c.path);
      if (list) list.push(c);
      else grouped.set(c.path, [c]);
    }
    const sections: string[] = [
      `Please address the following ${draftComments.length} code review ${draftComments.length === 1 ? "comment" : "comments"}:`,
    ];
    for (const [path, list] of grouped.entries()) {
      sections.push(`\n### \`${path}\``);
      for (const item of list) {
        const loc = item.lineNumber != null ? `L${item.lineNumber}` : item.side;
        sections.push(`- **${loc}** (\`${item.code.trim().slice(0, 120)}\`):\n  ${item.comment}`);
      }
    }
    orbitActions.promoteSideQuestionToComposer(sections.join("\n"));
    setDraftComments([]);
    orbitActions.showToast(
      `Inserted ${draftComments.length} review ${draftComments.length === 1 ? "comment" : "comments"} into Composer`,
    );
  }, [draftComments]);

  const handleRefresh = useCallback(async () => {
    if (isRefreshing) return;
    setIsRefreshing(true);
    try {
      await orbitActions.refreshGit(selectedHostId, selectedCwd, selectedDiffPath, diffStaged);
    } finally {
      setIsRefreshing(false);
    }
  }, [isRefreshing, selectedHostId, selectedCwd, selectedDiffPath, diffStaged]);

  const emptyMessage =
    reviewScope === "session_turns"
      ? "No file edits recorded in this session yet."
      : diffStaged
        ? "No staged changes in the Git index."
        : "Working tree clean — no changes to review.";

  return (
    <div className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Compact Scope & View Sub-Bar */}
      <div className="border-border flex h-[36px] shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="bg-muted inline-flex h-[26px] items-center rounded-lg p-0.5 text-[11.5px]">
          <button
            type="button"
            onClick={() => orbitActions.setReviewScope("working_tree")}
            className={cn(
              "inline-flex h-[22px] cursor-pointer items-center rounded-md px-2 font-medium transition-colors",
              reviewScope === "working_tree"
                ? "bg-background text-foreground shadow-2xs"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            Working Tree
          </button>
          <button
            type="button"
            onClick={() => orbitActions.setReviewScope("session_turns")}
            className={cn(
              "inline-flex h-[22px] cursor-pointer items-center rounded-md px-2 font-medium transition-colors",
              reviewScope === "session_turns"
                ? "bg-background text-foreground shadow-2xs"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            Session ({sessionTurnDiffs.length})
          </button>
        </div>

        <div className="flex items-center gap-1">
          {draftComments.length > 0 && (
            <button
              type="button"
              onClick={handleSubmitReviewComments}
              title="Send accumulated review comments to Composer"
              className="bg-foreground text-background inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11px] font-medium transition-opacity hover:opacity-90"
            >
              <Send className="h-[11px] w-[11px]" />
              <span>Review ({draftComments.length})</span>
            </button>
          )}

          <button
            type="button"
            title={wrapLines ? "Disable line wrapping" : "Wrap long diff lines"}
            onClick={() => setWrapLines((v) => !v)}
            className={cn(
              "inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-lg transition-colors",
              wrapLines
                ? "bg-accent text-foreground"
                : "text-muted-foreground hover:text-foreground hover:bg-accent",
            )}
          >
            <WrapText className="h-[13px] w-[13px]" />
          </button>

          {displayedDiffFiles.length > 1 && (
            <button
              type="button"
              title={allCollapsed ? "Expand all files" : "Collapse all files"}
              onClick={handleToggleAllCollapse}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-lg transition-colors"
            >
              {allCollapsed ? (
                <ChevronsUpDown className="h-[13.5px] w-[13.5px]" />
              ) : (
                <ChevronsDownUp className="h-[13.5px] w-[13.5px]" />
              )}
            </button>
          )}

          <div className="bg-muted inline-flex h-[26px] items-center rounded-lg p-0.5">
            <button
              type="button"
              title="Unified diff"
              onClick={() => orbitActions.setDiffLayout("unified")}
              className={cn(
                "inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md",
                diffLayout === "unified"
                  ? "bg-background text-foreground shadow-2xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Rows3 className="h-[13px] w-[13px]" />
            </button>
            <button
              type="button"
              title="Split side-by-side diff"
              onClick={() => orbitActions.setDiffLayout("split")}
              className={cn(
                "inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md",
                diffLayout === "split"
                  ? "bg-background text-foreground shadow-2xs"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Columns2 className="h-[13px] w-[13px]" />
            </button>
          </div>

          {reviewScope === "working_tree" && (
            <button
              type="button"
              title="Refresh Git status & diff"
              onClick={() => void handleRefresh()}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-lg transition-colors"
            >
              <RefreshCw className={cn("h-[13.5px] w-[13.5px]", isRefreshing && "animate-spin")} />
            </button>
          )}
        </div>
      </div>

      {/* Branch + Unstaged/Staged + File Filter Strip */}
      {(reviewScope === "working_tree" || fileSelectorItems.length > 0 || fileQuery.length > 0) && (
        <div className="border-border space-y-2 border-b px-3 py-2">
          {reviewScope === "working_tree" && (
            <div className="flex items-center justify-between gap-2 text-[12px]">
              <div className="text-muted-foreground flex min-w-0 items-center gap-1.5 font-mono">
                <GitBranch className="h-[13px] w-[13px] shrink-0" />
                <span className="selectable-text truncate">
                  {gitReview?.status?.branch ?? "HEAD"}
                </span>
                {(gitReview?.status?.ahead ?? 0) > 0 && (
                  <span className="text-emerald-600 dark:text-emerald-400">
                    ↑{gitReview?.status?.ahead}
                  </span>
                )}
                {(gitReview?.status?.behind ?? 0) > 0 && (
                  <span className="text-amber-600 dark:text-amber-400">
                    ↓{gitReview?.status?.behind}
                  </span>
                )}
              </div>

              <div className="flex items-center gap-1">
                <button
                  type="button"
                  onClick={() => orbitActions.setDiffStaged(false)}
                  className={cn(
                    "cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                    !diffStaged
                      ? "bg-accent text-foreground"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  Unstaged
                </button>
                <button
                  type="button"
                  onClick={() => orbitActions.setDiffStaged(true)}
                  className={cn(
                    "cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                    diffStaged
                      ? "bg-accent text-foreground"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  Staged
                </button>
              </div>
            </div>
          )}

          {(fileSelectorItems.length > 3 || fileQuery.length > 0) && (
            <div className="bg-muted/60 border-border/70 flex h-[26px] items-center gap-1.5 rounded-md border px-2">
              <Search className="text-muted-foreground h-[12px] w-[12px] shrink-0" />
              <input
                type="text"
                value={fileQuery}
                onChange={(e) => setFileQuery(e.target.value)}
                placeholder="Filter changed files…"
                className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent text-[11.5px] outline-none"
              />
            </div>
          )}

          {fileSelectorItems.length > 0 && (
            <ScrollArea orientation="horizontal" viewportClassName="pb-0.5">
              <div className="flex w-max items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => orbitActions.selectDiffFile(null)}
                  className={cn(
                    "shrink-0 cursor-pointer rounded-md border px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                    selectedDiffPath === null
                      ? "border-foreground/20 bg-accent text-foreground"
                      : "border-border text-muted-foreground hover:text-foreground",
                  )}
                >
                  All ({fileSelectorItems.length})
                </button>
                {fileSelectorItems.map((f) => {
                  const shortName = f.path.split("/").pop() || f.path;
                  const stats = diffStatsByPath.get(f.path);
                  const isViewed = viewedPaths.has(f.path);
                  const active =
                    selectedDiffPath === f.path ||
                    (selectedDiffPath !== null &&
                      stripWorkspacePrefix(selectedDiffPath, selectedCwd).replace(/^\.\//, "") ===
                        stripWorkspacePrefix(f.path, selectedCwd).replace(/^\.\//, ""));
                  return (
                    <button
                      key={f.path}
                      type="button"
                      title={f.path}
                      onClick={() => orbitActions.selectDiffFile(f.path)}
                      className={cn(
                        "inline-flex shrink-0 cursor-pointer items-center gap-1.5 rounded-md border px-2 py-0.5 font-mono text-[11.5px] transition-colors",
                        active
                          ? "border-foreground/20 bg-accent text-foreground font-medium"
                          : "border-border text-muted-foreground hover:text-foreground",
                        isViewed && "opacity-60",
                      )}
                    >
                      {isViewed && (
                        <Check className="h-[10.5px] w-[10.5px] text-emerald-600 dark:text-emerald-400" />
                      )}
                      <span>{shortName}</span>
                      {stats && (stats.additions > 0 || stats.deletions > 0) ? (
                        <span className="tabular-nums text-[10.5px]">
                          {stats.additions > 0 && (
                            <span className="text-emerald-600 dark:text-emerald-400">
                              +{stats.additions}
                            </span>
                          )}
                          {stats.deletions > 0 && (
                            <span className="ml-0.5 text-rose-600 dark:text-rose-400">
                              -{stats.deletions}
                            </span>
                          )}
                        </span>
                      ) : (
                        <span className={cn("text-[10.5px] font-medium", statusBadgeClass(f.status))}>
                          {f.status}
                        </span>
                      )}
                    </button>
                  );
                })}
              </div>
            </ScrollArea>
          )}
        </div>
      )}

      {/* Diff Stream Body */}
      <ScrollArea className="min-h-0 flex-1" viewportClassName="p-3">
        <DiffFilesViewer
          files={displayedDiffFiles}
          layout={diffLayout}
          wrapLines={wrapLines}
          emptyMessage={emptyMessage}
          collapsedPaths={collapsedPaths}
          viewedPaths={viewedPaths}
          comments={draftComments}
          onToggleCollapse={handleToggleCollapse}
          onToggleViewed={handleToggleViewed}
          onQuoteLine={handleQuoteLine}
          onAddComment={handleAddComment}
          onDeleteComment={handleDeleteComment}
          onOpenFile={handleOpenFile}
        />
      </ScrollArea>
    </div>
  );
});
