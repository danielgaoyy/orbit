import { useMemo } from "react";
import {
  Columns2,
  GitBranch,
  RefreshCw,
  Rows3,
  X,
} from "lucide-react";
import { processTimelineForStage } from "@/lib/timeline-coalesce";
import { DiffFilesViewer } from "@/components/diff-viewer";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export function ReviewInspector() {
  const inspectorOpen = useOrbitSelector((s) => s.inspectorOpen);
  const inspectorWidth = useOrbitSelector((s) => s.inspectorWidth);
  const reviewScope = useOrbitSelector((s) => s.reviewScope);
  const diffStaged = useOrbitSelector((s) => s.diffStaged);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);
  const selectedDiffPath = useOrbitSelector((s) => s.selectedDiffPath);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
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

  if (!inspectorOpen || !selectedHostId) return null;

  const startResize = (e: React.MouseEvent<HTMLDivElement>) => {
    e.preventDefault();
    const startX = e.clientX;
    const startW = inspectorWidth;

    const onMove = (ev: MouseEvent) => {
      const delta = startX - ev.clientX;
      orbitActions.setInspectorWidth(startW + delta);
    };
    const onUp = () => {
      window.removeEventListener("mousemove", onMove);
      window.removeEventListener("mouseup", onUp);
    };
    window.addEventListener("mousemove", onMove);
    window.addEventListener("mouseup", onUp);
  };

  return (
    <aside
      style={{ width: `${inspectorWidth}px` }}
      className="border-border bg-sidebar relative flex h-full shrink-0 flex-col border-l select-none"
    >
      {/* Left Resize Drag Handle */}
      <div
        onMouseDown={startResize}
        className="hover:bg-primary/40 absolute top-0 left-0 z-20 h-full w-1 cursor-col-resize transition-colors"
      />

      {/* Inspector Header */}
      <div className="border-border flex h-9.5 shrink-0 items-center justify-between gap-2 border-b px-3">
        {/* Scope Toggle: Working Tree vs Session Edits */}
        <div className="bg-muted inline-flex h-6.5 items-center rounded-md p-0.5 text-[11px]">
          <button
            type="button"
            onClick={() => orbitActions.setReviewScope("working_tree")}
            className={cn(
              "cursor-pointer rounded px-2 py-0.5 font-medium transition-colors",
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
              "cursor-pointer rounded px-2 py-0.5 font-medium transition-colors",
              reviewScope === "session_turns"
                ? "bg-background text-foreground shadow-2xs"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            Session ({sessionTurnDiffs.length})
          </button>
        </div>

        <div className="flex items-center gap-1">
          {/* Unified vs Split View */}
          <div className="bg-muted inline-flex h-6.5 items-center rounded-md p-0.5">
            <button
              type="button"
              title="Unified diff"
              onClick={() => orbitActions.setDiffLayout("unified")}
              className={cn(
                "inline-flex size-5.5 cursor-pointer items-center justify-center rounded",
                diffLayout === "unified"
                  ? "bg-background text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Rows3 className="size-3" />
            </button>
            <button
              type="button"
              title="Split side-by-side diff"
              onClick={() => orbitActions.setDiffLayout("split")}
              className={cn(
                "inline-flex size-5.5 cursor-pointer items-center justify-center rounded",
                diffLayout === "split"
                  ? "bg-background text-foreground"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              <Columns2 className="size-3" />
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
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-6.5 cursor-pointer items-center justify-center rounded-md"
            >
              <RefreshCw className="size-3" />
            </button>
          )}

          <button
            type="button"
            title="Close inspector"
            onClick={() => orbitActions.toggleInspector(false)}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-6.5 cursor-pointer items-center justify-center rounded-md"
          >
            <X className="size-3.5" />
          </button>
        </div>
      </div>

      {/* Sub-bar: Branch + Unstaged/Staged Toggle + Compact File Filter Strip */}
      <div className="border-border space-y-2 border-b px-3 py-2">
        {reviewScope === "working_tree" && (
          <div className="flex items-center justify-between text-[11px]">
            <div className="text-muted-foreground flex min-w-0 items-center gap-1.5 font-mono">
              <GitBranch className="size-3 shrink-0" />
              <span className="truncate">{gitReview?.status?.branch ?? "HEAD"}</span>
              {(gitReview?.status?.ahead ?? 0) > 0 && (
                <span className="text-emerald-400">↑{gitReview?.status?.ahead}</span>
              )}
              {(gitReview?.status?.behind ?? 0) > 0 && (
                <span className="text-amber-400">↓{gitReview?.status?.behind}</span>
              )}
            </div>

            <div className="flex items-center gap-1">
              <button
                type="button"
                onClick={() => orbitActions.setDiffStaged(false)}
                className={cn(
                  "cursor-pointer rounded px-1.5 py-0.5 text-[10.5px] font-medium",
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
                  "cursor-pointer rounded px-1.5 py-0.5 text-[10.5px] font-medium",
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
          <div className="flex items-center gap-1 overflow-x-auto pb-0.5">
            <button
              type="button"
              onClick={() => orbitActions.selectDiffFile(null)}
              className={cn(
                "shrink-0 cursor-pointer rounded-md border px-2 py-0.5 text-[11px] font-medium transition-colors",
                selectedDiffPath === null
                  ? "border-primary/45 bg-primary/12 text-foreground"
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
                    "inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[11px] transition-colors",
                    active
                      ? "border-primary/45 bg-primary/12 text-foreground"
                      : "border-border text-muted-foreground hover:text-foreground",
                  )}
                >
                  <span>{shortName}</span>
                  <span className="text-[9.5px] opacity-65">{f.status}</span>
                </button>
              );
            })}
          </div>
        )}
      </div>

      {/* Diff Stream Body */}
      <div className="flex-1 overflow-y-auto p-3">
        <DiffFilesViewer
          files={displayedDiffFiles}
          layout={diffLayout}
          onQuoteLine={(path, ln, code) => orbitActions.quoteDiffLineToComposer(path, ln, code)}
        />
      </div>
    </aside>
  );
}
