import { memo, useMemo } from "react";
import { Columns2, GitBranch, RefreshCw, Rows3 } from "lucide-react";
import { processTimelineForStage } from "@/lib/timeline-coalesce";
import { DiffFilesViewer } from "@/components/diff-viewer";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

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
  const activeTimeline = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? (s.timelines[`${s.selectedHostId}::${s.selectedAgentId}`] ?? null)
      : null,
  );

  const sessionTurnDiffs = useMemo(() => {
    if (!activeTimeline) return [];
    return processTimelineForStage(activeTimeline.items).sessionTurnDiffs;
  }, [activeTimeline]);

  const displayedDiffFiles = useMemo(() => {
    if (reviewScope === "session_turns") {
      if (!selectedDiffPath) return sessionTurnDiffs;
      return sessionTurnDiffs.filter((f) => f.path === selectedDiffPath);
    }
    return gitReview?.diff_files ?? [];
  }, [reviewScope, sessionTurnDiffs, selectedDiffPath, gitReview]);

  const fileSelectorItems = useMemo(() => {
    if (reviewScope === "session_turns") {
      const unique = new Map<string, string>();
      for (const f of sessionTurnDiffs) {
        unique.set(f.path, "E");
      }
      return Array.from(unique.entries()).map(([path, status]) => ({ path, status }));
    }
    return (gitReview?.status?.files ?? []).map((f) => ({
      path: f.path,
      status: f.status,
    }));
  }, [reviewScope, sessionTurnDiffs, gitReview]);

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
              onClick={() =>
                void orbitActions.refreshGit(
                  selectedHostId,
                  selectedCwd,
                  selectedDiffPath,
                  diffStaged,
                )
              }
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-lg transition-colors"
            >
              <RefreshCw className="h-[13.5px] w-[13.5px]" />
            </button>
          )}
        </div>
      </div>

      {/* Branch + Unstaged/Staged + File Filter Strip */}
      {(reviewScope === "working_tree" || fileSelectorItems.length > 0) && (
        <div className="border-border space-y-2 border-b px-3 py-2">
          {reviewScope === "working_tree" && (
            <div className="flex items-center justify-between text-[12px]">
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
                  const active = selectedDiffPath === f.path;
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
                      )}
                    >
                      <span>{shortName}</span>
                      <span className="text-[10.5px] opacity-65">{f.status}</span>
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
          onQuoteLine={(path, ln, code) => orbitActions.quoteDiffLineToComposer(path, ln, code)}
          onOpenFile={(path) =>
            orbitActions.openFileInSidePanel(path, {
              hostId: selectedHostId,
              cwd: selectedCwd,
            })
          }
        />
      </ScrollArea>
    </div>
  );
});
