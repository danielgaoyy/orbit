import { memo, useCallback, useEffect, useMemo, useState } from "react";
import {
  ArrowLeftRight,
  ArrowUp,
  Check,
  ChevronDown,
  ChevronRight,
  ChevronsDownUp,
  ChevronsUpDown,
  Columns2,
  GitBranch,
  GitCommitHorizontal,
  GitPullRequest,
  Loader2,
  Minus,
  Plus,
  RefreshCw,
  RotateCcw,
  Rows3,
  Search,
  Send,
  WrapText,
} from "lucide-react";
import { DiffFilesViewer, type ReviewCommentDraft } from "@/components/diff-viewer";
import { CreatePrDialog } from "@/components/side-panel/create-pr-dialog";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Textarea } from "@/components/ui/textarea";
import {
  buildSingleHunkPatch,
  deriveDefaultPrTitle,
  extractFileHunks,
  type IndexedDiffHunk,
} from "@/lib/git-patch";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import {
  orbitIpc,
  type DiffFileView,
  type GitCreatePrResult,
  type GitFileEntry,
} from "@/lib/orbit-ipc";
import { normalizeProjectPath } from "@/lib/projects";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import { cn } from "@/lib/utils";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const EMPTY_DIFFS: DiffFileView[] = [];
const EMPTY_STATUS_FILES: GitFileEntry[] = [];

type PendingDiscardTarget =
  | { kind: "file"; path: string; untracked: boolean }
  | { kind: "hunk"; path: string; hunk: IndexedDiffHunk; hunkKey: string };

function statusBadgeClass(status: string): string {
  const s = status.trim().toUpperCase();
  if (s === "A" || s === "??") return "text-emerald-600 dark:text-emerald-400";
  if (s === "D") return "text-rose-600 dark:text-rose-400";
  if (s === "M" || s === "R" || s === "E") return "text-amber-600 dark:text-amber-400";
  return "text-muted-foreground";
}

function shortFileName(path: string): string {
  return path.split("/").pop() || path;
}

function formatErrorDetail(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  return raw.trim() || "Unknown error";
}

function pathsMatch(a: string, b: string, cwd: string): boolean {
  if (a === b) return true;
  return (
    stripWorkspacePrefix(a, cwd).replace(/^\.\//, "") ===
    stripWorkspacePrefix(b, cwd).replace(/^\.\//, "")
  );
}

export const ReviewTab = memo(function ReviewTab({
  selectedHostId,
}: {
  selectedHostId: string;
}) {
  const { t } = useI18n();
  const reviewScope = useOrbitSelector((s) => s.reviewScope);
  const diffStaged = useOrbitSelector((s) => s.diffStaged);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);
  const selectedDiffPath = useOrbitSelector((s) => s.selectedDiffPath);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  const activeWorkspace = useOrbitSelector((s) => {
    const detail = s.hostDetails[selectedHostId];
    if (!detail) return null;
    const normCwd = selectedCwd ? normalizeProjectPath(selectedCwd) : "";
    if (s.selectedAgentId) {
      const ag = detail.agents.find((a) => a.agent_id === s.selectedAgentId);
      if (ag) {
        const ws = detail.workspaces.find((w) => w.workspace_id === ag.workspace_id);
        if (ws && (!normCwd || normalizeProjectPath(ws.cwd) === normCwd)) {
          return ws;
        }
      }
    }
    if (!normCwd) return null;
    return (
      detail.workspaces.find(
        (w) => !w.archived_at && normalizeProjectPath(w.cwd) === normCwd,
      ) ?? null
    );
  });

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

  // Git write workflow state (strictly scoped to `working_tree`)
  const [busyPaths, setBusyPaths] = useState<Set<string>>(() => new Set());
  const [busyHunkKeys, setBusyHunkKeys] = useState<Set<string>>(() => new Set());
  const [isBulkStaging, setIsBulkStaging] = useState(false);
  const [pendingDiscard, setPendingDiscard] = useState<PendingDiscardTarget | null>(null);
  const [commitBarExpanded, setCommitBarExpanded] = useState(true);
  const [commitMessage, setCommitMessage] = useState("");
  const [amendCommit, setAmendCommit] = useState(false);
  const [isCommitting, setIsCommitting] = useState(false);
  const [isPushing, setIsPushing] = useState(false);
  const [prDialogOpen, setPrDialogOpen] = useState(false);
  const [lastCommitSummary, setLastCommitSummary] = useState<string | null>(null);
  const commitIme = useImeCompositionGuard();

  useEffect(() => {
    setCollapsedPaths(new Set());
    setViewedPaths(new Set());
    setDraftComments([]);
    setFileQuery("");
    setBusyPaths(new Set());
    setBusyHunkKeys(new Set());
    setIsBulkStaging(false);
    setPendingDiscard(null);
    setCommitMessage("");
    setAmendCommit(false);
    setIsCommitting(false);
    setIsPushing(false);
    setPrDialogOpen(false);
    setLastCommitSummary(null);
  }, [selectedHostId, selectedCwd]);

  const isWorkingTreeWritable =
    reviewScope === "working_tree" &&
    Boolean(selectedCwd) &&
    Boolean(gitReview?.status?.is_git_repo);

  const statusFiles = gitReview?.status?.files ?? EMPTY_STATUS_FILES;

  const { unstagedEntries, stagedEntries, untrackedPaths } = useMemo(() => {
    const unstaged: GitFileEntry[] = [];
    const staged: GitFileEntry[] = [];
    const seenUnstaged = new Set<string>();
    const seenStaged = new Set<string>();
    const untracked = new Set<string>();

    for (const entry of statusFiles) {
      if (entry.untracked) {
        untracked.add(entry.path);
      }
      if (entry.staged) {
        if (!seenStaged.has(entry.path)) {
          seenStaged.add(entry.path);
          staged.push(entry);
        }
      } else {
        if (!seenUnstaged.has(entry.path)) {
          seenUnstaged.add(entry.path);
          unstaged.push(entry);
        }
      }
    }
    return {
      unstagedEntries: unstaged,
      stagedEntries: staged,
      untrackedPaths: untracked,
    };
  }, [statusFiles]);

  const unstagedCount = unstagedEntries.length;
  const stagedCount = stagedEntries.length;

  const displayedDiffFiles = useMemo(() => {
    const base =
      reviewScope === "session_turns" ? sessionTurnDiffs : (gitReview?.diff_files ?? EMPTY_DIFFS);
    if (!selectedDiffPath) {
      if (!fileQuery.trim()) return base;
      const q = fileQuery.trim().toLowerCase();
      return base.filter((f) => f.path.toLowerCase().includes(q));
    }
    return base.filter((f) => pathsMatch(f.path, selectedDiffPath, selectedCwd));
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
            untracked: false,
          }))
        : (diffStaged ? stagedEntries : unstagedEntries).map((f) => ({
            path: f.path,
            status: f.status,
            untracked: f.untracked,
          }));
    const q = fileQuery.trim().toLowerCase();
    if (!q) return raw;
    return raw.filter((f) => f.path.toLowerCase().includes(q));
  }, [reviewScope, sessionTurnDiffs, diffStaged, stagedEntries, unstagedEntries, fileQuery]);

  const allCollapsed =
    displayedDiffFiles.length > 0 && displayedDiffFiles.every((f) => collapsedPaths.has(f.path));

  const setPathBusy = useCallback((path: string, busy: boolean) => {
    setBusyPaths((prev) => {
      const next = new Set(prev);
      if (busy) next.add(path);
      else next.delete(path);
      return next;
    });
  }, []);

  const setHunkBusy = useCallback((hunkKey: string, busy: boolean) => {
    setBusyHunkKeys((prev) => {
      const next = new Set(prev);
      if (busy) next.add(hunkKey);
      else next.delete(hunkKey);
      return next;
    });
  }, []);

  const handleToggleCollapse = useCallback((path: string) => {
    setCollapsedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const handleToggleViewed = useCallback(
    (path: string) => {
      const markingViewed = !viewedPaths.has(path);
      setViewedPaths((prev) => {
        const next = new Set(prev);
        if (markingViewed) next.add(path);
        else next.delete(path);
        return next;
      });
      if (markingViewed) {
        setCollapsedPaths((cPrev) => {
          const cNext = new Set(cPrev);
          cNext.add(path);
          return cNext;
        });
      }
    },
    [viewedPaths],
  );

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
      t("sidePanel.insertedCommentsToast", { count: draftComments.length }),
    );
  }, [draftComments, t]);

  const handleRefresh = useCallback(async () => {
    if (isRefreshing || !selectedCwd) return;
    setIsRefreshing(true);
    try {
      await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
        path: selectedDiffPath,
        staged: diffStaged,
      });
    } finally {
      setIsRefreshing(false);
    }
  }, [isRefreshing, selectedHostId, selectedCwd, selectedDiffPath, diffStaged]);

  const resolveRefreshPath = useCallback(
    (clearSelection: boolean): string | null => {
      if (clearSelection) return null;
      if (selectedDiffPath && !diffStaged && untrackedPaths.has(selectedDiffPath)) {
        return selectedDiffPath;
      }
      return null;
    },
    [selectedDiffPath, diffStaged, untrackedPaths],
  );

  const handleStageFile = useCallback(
    async (path: string) => {
      if (!isWorkingTreeWritable || !selectedCwd) return;
      setPathBusy(path, true);
      try {
        await orbitIpc.gitStageFiles({
          hostId: selectedHostId,
          cwd: selectedCwd,
          paths: [path],
        });
        const shouldClear = Boolean(selectedDiffPath && pathsMatch(selectedDiffPath, path, selectedCwd));
        await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
          path: resolveRefreshPath(shouldClear),
          staged: diffStaged,
          clearSelectedPath: shouldClear,
        });
        orbitActions.showToast(t("review.stagedFileToast", { name: shortFileName(path) }));
      } catch (err) {
        orbitActions.showToast(t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }));
      } finally {
        setPathBusy(path, false);
      }
    },
    [
      isWorkingTreeWritable,
      selectedHostId,
      selectedCwd,
      selectedDiffPath,
      diffStaged,
      resolveRefreshPath,
      setPathBusy,
      t,
    ],
  );

  const handleUnstageFile = useCallback(
    async (path: string) => {
      if (!isWorkingTreeWritable || !selectedCwd) return;
      setPathBusy(path, true);
      try {
        await orbitIpc.gitUnstageFiles({
          hostId: selectedHostId,
          cwd: selectedCwd,
          paths: [path],
        });
        const shouldClear = Boolean(selectedDiffPath && pathsMatch(selectedDiffPath, path, selectedCwd));
        await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
          path: resolveRefreshPath(shouldClear),
          staged: diffStaged,
          clearSelectedPath: shouldClear,
        });
        orbitActions.showToast(t("review.unstagedFileToast", { name: shortFileName(path) }));
      } catch (err) {
        orbitActions.showToast(t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }));
      } finally {
        setPathBusy(path, false);
      }
    },
    [
      isWorkingTreeWritable,
      selectedHostId,
      selectedCwd,
      selectedDiffPath,
      diffStaged,
      resolveRefreshPath,
      setPathBusy,
      t,
    ],
  );

  const handleStageOrUnstageAll = useCallback(async () => {
    if (!isWorkingTreeWritable || !selectedCwd || isBulkStaging) return;
    setIsBulkStaging(true);
    try {
      if (diffStaged) {
        await orbitIpc.gitUnstageFiles({
          hostId: selectedHostId,
          cwd: selectedCwd,
          all: true,
        });
        await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
          staged: true,
          clearSelectedPath: true,
        });
        orbitActions.showToast(t("review.unstagedAllToast"));
      } else {
        await orbitIpc.gitStageFiles({
          hostId: selectedHostId,
          cwd: selectedCwd,
          all: true,
        });
        await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
          staged: false,
          clearSelectedPath: true,
        });
        orbitActions.showToast(t("review.stagedAllToast"));
      }
    } catch (err) {
      orbitActions.showToast(t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }));
    } finally {
      setIsBulkStaging(false);
    }
  }, [isWorkingTreeWritable, selectedHostId, selectedCwd, isBulkStaging, diffStaged, t]);

  const handleRequestDiscardFile = useCallback(
    (path: string) => {
      if (!isWorkingTreeWritable) return;
      setPendingDiscard({
        kind: "file",
        path,
        untracked: untrackedPaths.has(path),
      });
    },
    [isWorkingTreeWritable, untrackedPaths],
  );

  const shouldClearSelectedAfterHunk = useCallback(
    (path: string) => {
      if (!selectedDiffPath || !pathsMatch(selectedDiffPath, path, selectedCwd)) {
        return false;
      }
      const matchFile = (gitReview?.diff_files ?? EMPTY_DIFFS).find((f) =>
        pathsMatch(f.path, path, selectedCwd),
      );
      if (!matchFile) return false;
      return extractFileHunks(matchFile).length <= 1;
    },
    [selectedDiffPath, selectedCwd, gitReview],
  );

  const handleStageHunk = useCallback(
    async (path: string, hunk: IndexedDiffHunk) => {
      if (!isWorkingTreeWritable || !selectedCwd) return;
      const hunkKey = `${path}::${hunk.headerLineIndex}`;
      setHunkBusy(hunkKey, true);
      try {
        const patch = buildSingleHunkPatch(path, hunk);
        await orbitIpc.gitApplyHunk({
          hostId: selectedHostId,
          cwd: selectedCwd,
          patch,
          cached: true,
          reverse: false,
        });
        const shouldClear = shouldClearSelectedAfterHunk(path);
        await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
          path: resolveRefreshPath(shouldClear),
          staged: diffStaged,
          clearSelectedPath: shouldClear,
        });
        orbitActions.showToast(t("review.stagedHunkToast", { name: shortFileName(path) }));
      } catch (err) {
        orbitActions.showToast(t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }));
      } finally {
        setHunkBusy(hunkKey, false);
      }
    },
    [
      isWorkingTreeWritable,
      selectedHostId,
      selectedCwd,
      diffStaged,
      resolveRefreshPath,
      setHunkBusy,
      shouldClearSelectedAfterHunk,
      t,
    ],
  );

  const handleUnstageHunk = useCallback(
    async (path: string, hunk: IndexedDiffHunk) => {
      if (!isWorkingTreeWritable || !selectedCwd) return;
      const hunkKey = `${path}::${hunk.headerLineIndex}`;
      setHunkBusy(hunkKey, true);
      try {
        const patch = buildSingleHunkPatch(path, hunk);
        await orbitIpc.gitApplyHunk({
          hostId: selectedHostId,
          cwd: selectedCwd,
          patch,
          cached: true,
          reverse: true,
        });
        const shouldClear = shouldClearSelectedAfterHunk(path);
        await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
          path: resolveRefreshPath(shouldClear),
          staged: diffStaged,
          clearSelectedPath: shouldClear,
        });
        orbitActions.showToast(t("review.unstagedHunkToast", { name: shortFileName(path) }));
      } catch (err) {
        orbitActions.showToast(t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }));
      } finally {
        setHunkBusy(hunkKey, false);
      }
    },
    [
      isWorkingTreeWritable,
      selectedHostId,
      selectedCwd,
      diffStaged,
      resolveRefreshPath,
      setHunkBusy,
      shouldClearSelectedAfterHunk,
      t,
    ],
  );

  const handleRequestDiscardHunk = useCallback(
    (path: string, hunk: IndexedDiffHunk) => {
      if (!isWorkingTreeWritable) return;
      setPendingDiscard({
        kind: "hunk",
        path,
        hunk,
        hunkKey: `${path}::${hunk.headerLineIndex}`,
      });
    },
    [isWorkingTreeWritable],
  );

  const handleConfirmDiscard = useCallback(async () => {
    if (!pendingDiscard || !selectedCwd) return;
    if (pendingDiscard.kind === "file") {
      const { path, untracked } = pendingDiscard;
      setPathBusy(path, true);
      try {
        await orbitIpc.gitDiscardFiles({
          hostId: selectedHostId,
          cwd: selectedCwd,
          paths: [path],
          includeUntracked: untracked,
        });
        const shouldClear = Boolean(selectedDiffPath && pathsMatch(selectedDiffPath, path, selectedCwd));
        await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
          path: resolveRefreshPath(shouldClear),
          staged: diffStaged,
          clearSelectedPath: shouldClear,
        });
        orbitActions.showToast(t("review.discardedFileToast", { name: shortFileName(path) }));
      } finally {
        setPathBusy(path, false);
      }
      return;
    }

    const { path, hunk, hunkKey } = pendingDiscard;
    setHunkBusy(hunkKey, true);
    try {
      const patch = buildSingleHunkPatch(path, hunk);
      await orbitIpc.gitApplyHunk({
        hostId: selectedHostId,
        cwd: selectedCwd,
        patch,
        cached: false,
        reverse: true,
      });
      const shouldClear = shouldClearSelectedAfterHunk(path);
      await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
        path: resolveRefreshPath(shouldClear),
        staged: diffStaged,
        clearSelectedPath: shouldClear,
      });
      orbitActions.showToast(t("review.discardedHunkToast", { name: shortFileName(path) }));
    } finally {
      setHunkBusy(hunkKey, false);
    }
  }, [
    pendingDiscard,
    selectedHostId,
    selectedCwd,
    selectedDiffPath,
    diffStaged,
    resolveRefreshPath,
    setPathBusy,
    setHunkBusy,
    shouldClearSelectedAfterHunk,
    t,
  ]);

  const canCommit =
    isWorkingTreeWritable &&
    !isCommitting &&
    !isBulkStaging &&
    commitMessage.trim().length > 0 &&
    (stagedCount > 0 || amendCommit);

  const commitDisabledHint = !amendCommit && stagedCount === 0
    ? t("review.commitNeedsStagedHint")
    : commitMessage.trim().length === 0
      ? t("review.commitNeedsMessageHint")
      : undefined;

  const handleCommit = useCallback(async () => {
    if (!canCommit || !selectedCwd) return;
    setIsCommitting(true);
    try {
      const result = await orbitIpc.gitCommit({
        hostId: selectedHostId,
        cwd: selectedCwd,
        message: commitMessage,
        amend: amendCommit,
      });
      setCommitMessage("");
      setLastCommitSummary(result.summary || null);
      const wasAmend = amendCommit;
      setAmendCommit(false);
      await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
        path: resolveRefreshPath(diffStaged),
        staged: diffStaged,
        clearSelectedPath: diffStaged,
      });
      orbitActions.showToast(
        wasAmend
          ? t("review.amendedToast", { sha: result.short_sha })
          : t("review.committedToast", { sha: result.short_sha }),
      );
    } catch (err) {
      orbitActions.showToast(t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }));
    } finally {
      setIsCommitting(false);
    }
  }, [
    canCommit,
    selectedHostId,
    selectedCwd,
    commitMessage,
    amendCommit,
    diffStaged,
    resolveRefreshPath,
    t,
  ]);

  const handlePush = useCallback(async () => {
    if (!isWorkingTreeWritable || !selectedCwd || isPushing) return;
    setIsPushing(true);
    try {
      const result = await orbitIpc.gitPush({
        hostId: selectedHostId,
        cwd: selectedCwd,
        setUpstream: true,
      });
      await orbitActions.refreshGitReview(selectedHostId, selectedCwd, {
        path: resolveRefreshPath(false),
        staged: diffStaged,
      });
      orbitActions.showToast(
        t("review.pushedToast", {
          branch: result.branch,
          remote: result.remote,
        }),
      );
    } catch (err) {
      orbitActions.showToast(t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }));
    } finally {
      setIsPushing(false);
    }
  }, [
    isWorkingTreeWritable,
    selectedHostId,
    selectedCwd,
    isPushing,
    diffStaged,
    resolveRefreshPath,
    t,
  ]);

  const handlePrCreated = useCallback(
    (result: GitCreatePrResult) => {
      orbitActions.recordSessionPullRequest(result);
      orbitActions.showToast(
        result.number != null
          ? t("review.prCreatedWithNumberToast", { number: result.number })
          : t("review.prCreatedToast"),
        {
          label: t("review.openPr"),
          url: result.url,
        },
      );
    },
    [t],
  );

  const showApplyToLocal = Boolean(activeWorkspace?.is_worktree);
  const showMoveToWorktree = Boolean(
    activeWorkspace &&
      !activeWorkspace.is_worktree &&
      gitReview?.status?.is_git_repo &&
      (gitReview.status.files?.length ?? 0) > 0,
  );

  const emptyMessage =
    reviewScope === "session_turns"
      ? t("sidePanel.emptySessionDiffs")
      : diffStaged
        ? t("sidePanel.emptyStagedDiffs")
        : t("sidePanel.emptyWorkingTree");

  const aheadCount = gitReview?.status?.ahead ?? 0;
  const branchName = gitReview?.status?.branch ?? null;
  const defaultPrTitle = useMemo(
    () => deriveDefaultPrTitle(branchName, lastCommitSummary),
    [branchName, lastCommitSummary],
  );
  const commitShortcutLabel = `${modSymbol()}↵`;
  const showBulkStageButton =
    isWorkingTreeWritable && (diffStaged ? stagedCount > 0 : unstagedCount > 0);

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
            {t("sidePanel.workingTree")}
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
            {t("sidePanel.sessionCount", { count: sessionTurnDiffs.length })}
          </button>
        </div>

        <div className="flex items-center gap-1">
          {draftComments.length > 0 && (
            <button
              type="button"
              onClick={handleSubmitReviewComments}
              title={t("sidePanel.sendReviewCommentsTitle")}
              className="bg-foreground text-background inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11px] font-medium transition-opacity hover:opacity-90"
            >
              <Send className="h-[11px] w-[11px]" />
              <span>{t("sidePanel.reviewCount", { count: draftComments.length })}</span>
            </button>
          )}

          <button
            type="button"
            title={wrapLines ? t("sidePanel.disableWrap") : t("sidePanel.wrapLines")}
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
              title={allCollapsed ? t("sidePanel.expandAllFiles") : t("sidePanel.collapseAllFiles")}
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
              title={t("review.unifiedDiffTitle")}
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
              title={t("review.splitDiffTitle")}
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
              title={t("review.refreshGitTitle")}
              onClick={() => void handleRefresh()}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-lg transition-colors"
            >
              <RefreshCw className={cn("h-[13.5px] w-[13.5px]", isRefreshing && "animate-spin")} />
            </button>
          )}
        </div>
      </div>

      {/* Branch + Unstaged/Staged + Bulk Stage + File Filter Strip */}
      {(reviewScope === "working_tree" || fileSelectorItems.length > 0 || fileQuery.length > 0) && (
        <div className="border-border space-y-2 border-b px-3 py-2">
          {reviewScope === "working_tree" && (
            <div className="flex flex-wrap items-center justify-between gap-2 text-[12px]">
              <div className="text-muted-foreground flex min-w-0 items-center gap-1.5 font-mono">
                <GitBranch className="h-[13px] w-[13px] shrink-0" />
                <span className="selectable-text truncate">
                  {branchName ?? "HEAD"}
                </span>
                {aheadCount > 0 && (
                  <span className="text-emerald-600 dark:text-emerald-400">
                    ↑{aheadCount}
                  </span>
                )}
                {(gitReview?.status?.behind ?? 0) > 0 && (
                  <span className="text-amber-600 dark:text-amber-400">
                    ↓{gitReview?.status?.behind}
                  </span>
                )}
              </div>

              <div className="flex shrink-0 items-center gap-1">
                {(showApplyToLocal || showMoveToWorktree) && activeWorkspace && (
                  <button
                    type="button"
                    onClick={() =>
                      orbitActions.openWorktreeHandoffDialog({
                        hostId: selectedHostId,
                        workspaceId: activeWorkspace.workspace_id,
                        direction: showApplyToLocal ? "worktree_to_local" : "local_to_worktree",
                        defaultMode: showApplyToLocal ? "apply" : "move",
                        sourceCwd: activeWorkspace.cwd,
                      })
                    }
                    className="border-border text-foreground/80 hover:bg-accent hover:text-foreground mr-0.5 inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px] font-medium transition-colors"
                  >
                    <ArrowLeftRight className="h-[11px] w-[11px] shrink-0 opacity-75" />
                    <span>
                      {showApplyToLocal ? t("review.applyToLocal") : t("review.moveToWorktree")}
                    </span>
                  </button>
                )}

                {showBulkStageButton && (
                  <button
                    type="button"
                    disabled={isBulkStaging}
                    onClick={() => void handleStageOrUnstageAll()}
                    className="border-border text-muted-foreground hover:bg-accent hover:text-foreground mr-0.5 inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                  >
                    {isBulkStaging ? (
                      <Loader2 className="h-[11px] w-[11px] animate-spin" />
                    ) : diffStaged ? (
                      <Minus className="h-[11px] w-[11px]" />
                    ) : (
                      <Plus className="h-[11px] w-[11px]" />
                    )}
                    <span>{diffStaged ? t("review.unstageAll") : t("review.stageAll")}</span>
                  </button>
                )}

                <button
                  type="button"
                  onClick={() => orbitActions.setDiffStaged(false)}
                  className={cn(
                    "inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                    !diffStaged
                      ? "bg-accent text-foreground"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <span>{t("review.unstaged")}</span>
                  {unstagedCount > 0 && (
                    <span className="tabular-nums text-[10.5px] opacity-70">{unstagedCount}</span>
                  )}
                </button>
                <button
                  type="button"
                  onClick={() => orbitActions.setDiffStaged(true)}
                  className={cn(
                    "inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors",
                    diffStaged
                      ? "bg-accent text-foreground"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <span>{t("review.staged")}</span>
                  {stagedCount > 0 && (
                    <span className="tabular-nums text-[10.5px] opacity-70">{stagedCount}</span>
                  )}
                </button>
              </div>
            </div>
          )}

          {(fileSelectorItems.length > 3 || fileQuery.length > 0) && (
            <div className="relative">
              <Search className="text-muted-foreground pointer-events-none absolute top-1/2 left-2.5 h-[12px] w-[12px] -translate-y-1/2" />
              <Input
                type="text"
                value={fileQuery}
                onChange={(e) => setFileQuery(e.target.value)}
                placeholder={t("review.filterFilesPlaceholder")}
                className="h-[26px] rounded-[6px] pl-7 text-[11.5px]"
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
                  {t("review.allFilesCount", { count: fileSelectorItems.length })}
                </button>
                {fileSelectorItems.map((f) => {
                  const name = shortFileName(f.path);
                  const stats = diffStatsByPath.get(f.path);
                  const isViewed = viewedPaths.has(f.path);
                  const isBusy = busyPaths.has(f.path);
                  const active =
                    selectedDiffPath !== null && pathsMatch(selectedDiffPath, f.path, selectedCwd);
                  return (
                    <div
                      key={f.path}
                      className={cn(
                        "group/filepill inline-flex shrink-0 items-center rounded-md border font-mono text-[11.5px] transition-colors",
                        active
                          ? "border-foreground/20 bg-accent text-foreground font-medium"
                          : "border-border text-muted-foreground hover:text-foreground",
                        isViewed && "opacity-60",
                      )}
                    >
                      <button
                        type="button"
                        title={f.path}
                        onClick={() => orbitActions.selectDiffFile(f.path)}
                        className="inline-flex cursor-pointer items-center gap-1.5 px-2 py-0.5"
                      >
                        {isViewed && (
                          <Check className="h-[10.5px] w-[10.5px] text-emerald-600 dark:text-emerald-400" />
                        )}
                        <span>{name}</span>
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
                      {isWorkingTreeWritable && (
                        <div className="border-border/70 flex items-center gap-0.5 border-l px-1 py-0.5">
                          {diffStaged ? (
                            <button
                              type="button"
                              disabled={isBusy}
                              title={t("review.unstageFileTitle", { path: f.path })}
                              onClick={() => void handleUnstageFile(f.path)}
                              className="text-muted-foreground hover:bg-black/[0.07] hover:text-foreground dark:hover:bg-white/[0.09] inline-flex h-[16px] w-[16px] cursor-pointer items-center justify-center rounded transition-colors disabled:pointer-events-none disabled:opacity-40"
                            >
                              {isBusy ? (
                                <Loader2 className="h-[10px] w-[10px] animate-spin" />
                              ) : (
                                <Minus className="h-[10.5px] w-[10.5px]" />
                              )}
                            </button>
                          ) : (
                            <>
                              <button
                                type="button"
                                disabled={isBusy}
                                title={t("review.stageFileTitle", { path: f.path })}
                                onClick={() => void handleStageFile(f.path)}
                                className="text-muted-foreground hover:bg-emerald-500/[0.15] hover:text-emerald-700 dark:hover:text-emerald-300 inline-flex h-[16px] w-[16px] cursor-pointer items-center justify-center rounded transition-colors disabled:pointer-events-none disabled:opacity-40"
                              >
                                {isBusy ? (
                                  <Loader2 className="h-[10px] w-[10px] animate-spin" />
                                ) : (
                                  <Plus className="h-[10.5px] w-[10.5px]" />
                                )}
                              </button>
                              <button
                                type="button"
                                disabled={isBusy}
                                title={t("review.discardFileTitle", { path: f.path })}
                                onClick={() => handleRequestDiscardFile(f.path)}
                                className="text-muted-foreground hover:bg-rose-500/[0.15] hover:text-rose-600 dark:hover:text-rose-300 inline-flex h-[16px] w-[16px] cursor-pointer items-center justify-center rounded transition-colors disabled:pointer-events-none disabled:opacity-40"
                              >
                                <RotateCcw className="h-[10px] w-[10px]" />
                              </button>
                            </>
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            </ScrollArea>
          )}
        </div>
      )}

      {/* Virtualized Diff Stream Body (O(1) DOM nodes regardless of file/line count) */}
      <DiffFilesViewer
        virtualized
        files={displayedDiffFiles}
        layout={diffLayout}
        wrapLines={wrapLines}
        emptyMessage={emptyMessage}
        collapsedPaths={collapsedPaths}
        viewedPaths={viewedPaths}
        comments={draftComments}
        gitWriteMode={isWorkingTreeWritable ? (diffStaged ? "staged" : "unstaged") : null}
        busyPaths={isWorkingTreeWritable ? busyPaths : undefined}
        busyHunkKeys={isWorkingTreeWritable ? busyHunkKeys : undefined}
        untrackedPaths={isWorkingTreeWritable ? untrackedPaths : undefined}
        onToggleCollapse={handleToggleCollapse}
        onToggleViewed={handleToggleViewed}
        onQuoteLine={handleQuoteLine}
        onAddComment={handleAddComment}
        onDeleteComment={handleDeleteComment}
        onOpenFile={handleOpenFile}
        onStageFile={isWorkingTreeWritable ? (path) => void handleStageFile(path) : undefined}
        onUnstageFile={isWorkingTreeWritable ? (path) => void handleUnstageFile(path) : undefined}
        onDiscardFile={isWorkingTreeWritable ? handleRequestDiscardFile : undefined}
        onStageHunk={
          isWorkingTreeWritable ? (path, hunk) => void handleStageHunk(path, hunk) : undefined
        }
        onUnstageHunk={
          isWorkingTreeWritable ? (path, hunk) => void handleUnstageHunk(path, hunk) : undefined
        }
        onDiscardHunk={isWorkingTreeWritable ? handleRequestDiscardHunk : undefined}
      />

      {/* Restrained Bottom Commit / Push / Create PR Bar (working_tree scope only) */}
      {isWorkingTreeWritable && (
        <div className="border-border bg-background/95 shrink-0 border-t px-3 py-2">
          <div className="flex items-center justify-between gap-2">
            <button
              type="button"
              title={t("review.commitSectionToggle")}
              onClick={() => setCommitBarExpanded((v) => !v)}
              className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 text-[11.5px] font-medium transition-colors"
            >
              {commitBarExpanded ? (
                <ChevronDown className="h-[13px] w-[13px] shrink-0" />
              ) : (
                <ChevronRight className="h-[13px] w-[13px] shrink-0" />
              )}
              <span
                className={cn(
                  stagedCount > 0
                    ? "text-emerald-700 dark:text-emerald-400"
                    : "text-muted-foreground",
                )}
              >
                {stagedCount > 0
                  ? t("review.stagedCountSummary", { count: stagedCount })
                  : t("review.noStagedChanges")}
              </span>
            </button>

            {commitBarExpanded ? (
              <label
                title={t("review.amendHint")}
                className="text-muted-foreground hover:text-foreground flex cursor-pointer items-center gap-1.5 text-[11.5px] select-none"
              >
                <Checkbox
                  size="sm"
                  checked={amendCommit}
                  disabled={isCommitting}
                  onCheckedChange={setAmendCommit}
                />
                <span className={cn(amendCommit && "text-foreground font-medium")}>
                  {t("review.amend")}
                </span>
              </label>
            ) : (
              <div className="flex items-center gap-1">
                <button
                  type="button"
                  disabled={isPushing}
                  title={t("review.pushTitle")}
                  onClick={() => void handlePush()}
                  className="border-border bg-background text-foreground/85 hover:bg-accent hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                >
                  {isPushing ? (
                    <Loader2 className="h-[11px] w-[11px] animate-spin" />
                  ) : (
                    <ArrowUp className="h-[11px] w-[11px]" />
                  )}
                  <span>{t("review.push")}</span>
                  {aheadCount > 0 && (
                    <span className="tabular-nums text-[10px] text-emerald-600 dark:text-emerald-400">
                      ↑{aheadCount}
                    </span>
                  )}
                </button>
                <button
                  type="button"
                  onClick={() => setPrDialogOpen(true)}
                  className="border-border bg-background text-foreground/85 hover:bg-accent hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px] font-medium transition-colors"
                >
                  <GitPullRequest className="h-[11px] w-[11px]" />
                  <span>{t("review.createPrShort")}</span>
                </button>
              </div>
            )}
          </div>

          {commitBarExpanded && (
            <div className="mt-2 space-y-2">
              <Textarea
                rows={2}
                disabled={isCommitting}
                value={commitMessage}
                onChange={(e) => setCommitMessage(e.target.value)}
                {...commitIme.compositionProps}
                onKeyDown={(e) => {
                  if (commitIme.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) {
                    e.preventDefault();
                    void handleCommit();
                  }
                }}
                placeholder={
                  amendCommit
                    ? t("review.amendPlaceholder", { shortcut: commitShortcutLabel })
                    : t("review.commitPlaceholder", { shortcut: commitShortcutLabel })
                }
                className="max-h-[120px] min-h-[52px] resize-y px-2.5 py-1.5 text-[12px] leading-relaxed"
              />

              {!amendCommit && stagedCount === 0 && (
                <p className="text-muted-foreground text-[11px] leading-snug">
                  {t("review.commitNeedsStagedHint")}
                </p>
              )}

              <div className="flex items-center justify-between gap-2">
                <Button
                  type="button"
                  size="xs"
                  disabled={!canCommit}
                  title={commitDisabledHint}
                  onClick={() => void handleCommit()}
                  className="h-[28px] gap-1.5 px-3 text-[12px]"
                >
                  {isCommitting ? (
                    <Loader2 className="h-[12px] w-[12px] animate-spin" />
                  ) : (
                    <GitCommitHorizontal className="h-[13px] w-[13px]" />
                  )}
                  <span>{amendCommit ? t("review.amendCommit") : t("review.commit")}</span>
                </Button>

                <div className="flex items-center gap-1.5">
                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    disabled={isPushing}
                    title={t("review.pushTitle")}
                    onClick={() => void handlePush()}
                    className="h-[28px] px-2.5 text-[12px]"
                  >
                    {isPushing ? (
                      <Loader2 className="h-[12px] w-[12px] animate-spin" />
                    ) : (
                      <ArrowUp className="h-[12px] w-[12px]" />
                    )}
                    <span>{t("review.push")}</span>
                    {aheadCount > 0 && (
                      <span className="tabular-nums text-[10.5px] text-emerald-600 dark:text-emerald-400">
                        ↑{aheadCount}
                      </span>
                    )}
                  </Button>

                  <Button
                    type="button"
                    variant="outline"
                    size="xs"
                    onClick={() => setPrDialogOpen(true)}
                    className="h-[28px] gap-1.5 px-2.5 text-[12px]"
                  >
                    <GitPullRequest className="h-[12px] w-[12px]" />
                    <span>{t("review.createPr")}</span>
                  </Button>
                </div>
              </div>
            </div>
          )}
        </div>
      )}

      {/* Unified Destructive Confirmation Dialog for Discard File & Discard Hunk */}
      <ConfirmDialog
        open={pendingDiscard !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPendingDiscard(null);
        }}
        tone="danger"
        defaultAction="cancel"
        title={
          pendingDiscard?.kind === "hunk"
            ? t("review.discardHunkConfirmTitle", { name: shortFileName(pendingDiscard.path) })
            : pendingDiscard
              ? t("review.discardFileConfirmTitle", { name: shortFileName(pendingDiscard.path) })
              : ""
        }
        description={
          pendingDiscard?.kind === "hunk"
            ? t("review.discardHunkConfirmDesc")
            : pendingDiscard?.untracked
              ? t("review.discardUntrackedDesc")
              : t("review.discardTrackedDesc")
        }
        detail={
          pendingDiscard ? (
            <div className="space-y-1 font-mono text-[11.5px]">
              <div className="text-foreground/90 break-all">{pendingDiscard.path}</div>
              {pendingDiscard.kind === "hunk" && pendingDiscard.hunk.header && (
                <div className="text-muted-foreground truncate">{pendingDiscard.hunk.header}</div>
              )}
            </div>
          ) : undefined
        }
        confirmLabel={
          pendingDiscard?.kind === "hunk"
            ? t("review.discardHunkConfirmAction")
            : t("review.discardFileConfirmAction")
        }
        onConfirm={handleConfirmDiscard}
      />

      {/* Restrained Create Pull Request Dialog */}
      {isWorkingTreeWritable && selectedCwd && (
        <CreatePrDialog
          open={prDialogOpen}
          onOpenChange={setPrDialogOpen}
          hostId={selectedHostId}
          cwd={selectedCwd}
          branch={branchName}
          defaultTitle={defaultPrTitle}
          onCreated={handlePrCreated}
        />
      )}
    </div>
  );
});
