import { memo, useCallback, useEffect, useState } from "react";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  ChevronRight,
  Edit3,
  GitMerge,
  Loader2,
  Play,
  SkipForward,
  XCircle,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Textarea } from "@/components/ui/textarea";
import {
  countTotalConflictBlocks,
  defaultManualResolutionText,
  hasActiveConflictOrRebase,
} from "@/lib/git-collab";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  orbitIpc,
  type GitConflictBlock,
  type GitConflictFile,
  type GitConflictResolution,
  type GitRebaseStatus,
} from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";

interface GitConflictResolverProps {
  hostId: string;
  cwd: string;
  visible: boolean;
  rebaseStatus: GitRebaseStatus | null;
  conflictFiles: readonly GitConflictFile[];
  onStateChanged: () => Promise<void>;
}

function formatErrorDetail(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  return raw.trim() || "Unknown error";
}

export const GitConflictResolver = memo(function GitConflictResolver({
  hostId,
  cwd,
  visible,
  rebaseStatus,
  conflictFiles,
  onStateChanged,
}: GitConflictResolverProps) {
  const { t } = useI18n();
  const [expandedFiles, setExpandedFiles] = useState<Set<string>>(() => new Set());
  const [busyKey, setBusyKey] = useState<string | null>(null);
  const [rebaseBusy, setRebaseBusy] = useState<"continue" | "skip" | "abort" | null>(null);
  const [pendingRebaseConfirm, setPendingRebaseConfirm] = useState<
    | { action: "abort" }
    | { action: "skip"; expectedStep: number | null }
    | null
  >(null);
  const [editingBlock, setEditingBlock] = useState<{
    key: string;
    path: string;
    blockIndex: number;
    draft: string;
  } | null>(null);
  const manualIme = useImeCompositionGuard();

  // Auto-expand the first conflicted file when conflicts appear.
  useEffect(() => {
    if (conflictFiles.length === 0) {
      setExpandedFiles(new Set());
      setEditingBlock(null);
      return;
    }
    setExpandedFiles((prev) => {
      if (prev.size > 0) return prev;
      return new Set([conflictFiles[0].path]);
    });
  }, [conflictFiles]);

  const active = hasActiveConflictOrRebase(rebaseStatus, conflictFiles);

  const toggleFile = useCallback((path: string) => {
    setExpandedFiles((prev) => {
      const next = new Set(prev);
      if (next.has(path)) next.delete(path);
      else next.add(path);
      return next;
    });
  }, []);

  const handleResolve = useCallback(
    async (
      file: GitConflictFile,
      blockIndex: number | null,
      resolution: GitConflictResolution,
      actionKey: string,
    ) => {
      if (busyKey) return;
      setBusyKey(actionKey);
      try {
        const res = await orbitIpc.gitResolveConflict({
          hostId,
          cwd,
          path: file.path,
          blockIndex,
          resolution,
          autoStage: true,
          expectedSha256: file.content_sha256 ?? null,
        });
        if (editingBlock && editingBlock.path === file.path) {
          setEditingBlock(null);
        }
        await onStateChanged();
        orbitActions.showToast(
          res.staged
            ? t("review.conflictResolvedStagedToast", { path: file.path })
            : t("review.conflictBlockResolvedToast", { remaining: res.remaining_blocks }),
        );
      } catch (err) {
        orbitActions.showToast(
          t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }),
        );
      } finally {
        setBusyKey(null);
      }
    },
    [busyKey, hostId, cwd, editingBlock, onStateChanged, t],
  );

  const handleRebaseContinue = useCallback(async () => {
    if (rebaseBusy) return;
    setRebaseBusy("continue");
    try {
      const res = await orbitIpc.gitRebaseContinue({ hostId, cwd });
      await onStateChanged();
      orbitActions.showToast(res.summary);
    } catch (err) {
      orbitActions.showToast(
        t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }),
      );
    } finally {
      setRebaseBusy(null);
    }
  }, [rebaseBusy, hostId, cwd, onStateChanged, t]);

  const handleConfirmRebaseAction = useCallback(async () => {
    if (!pendingRebaseConfirm) return;
    const pending = pendingRebaseConfirm;
    setRebaseBusy(pending.action);
    try {
      const res =
        pending.action === "abort"
          ? await orbitIpc.gitRebaseAbort({ hostId, cwd })
          : await orbitIpc.gitRebaseSkip({
              hostId,
              cwd,
              expectedStep: pending.expectedStep,
            });
      await onStateChanged();
      orbitActions.showToast(res.summary);
    } finally {
      setRebaseBusy(null);
    }
  }, [pendingRebaseConfirm, hostId, cwd, onStateChanged]);

  // Orbital Quiet Precision: render zero chrome when no conflicts or active rebase exist.
  if (!active) {
    return null;
  }

  const inRebase = Boolean(rebaseStatus?.in_progress);
  const unresolvedFileCount = conflictFiles.length;
  const totalBlocks = countTotalConflictBlocks(conflictFiles);
  const canContinueRebase = inRebase && unresolvedFileCount === 0 && rebaseBusy === null;

  const renderBlock = (file: GitConflictFile, block: GitConflictBlock) => {
    const blockKey = `${file.path}::${block.index}`;
    const isEditing = editingBlock?.key === blockKey;
    const blockBusy = busyKey === blockKey || busyKey === file.path;

    return (
      <div
        key={blockKey}
        className="border-border bg-surface-base flex flex-col overflow-hidden rounded-lg border"
      >
        <div className="border-border-subtle bg-surface-raised flex flex-wrap items-center justify-between gap-1.5 border-b px-2.5 py-1.5">
          <span className="text-muted-foreground tabular-nums font-mono text-[11px]">
            {t("review.conflictBlockHeader", {
              index: block.index + 1,
              start: block.start_line,
              end: block.end_line,
            })}
          </span>
          <div className="flex flex-wrap items-center gap-1">
            <button
              type="button"
              disabled={Boolean(busyKey)}
              onClick={() =>
                void handleResolve(file, block.index, { strategy: "ours" }, blockKey)
              }
              className="border-border text-muted-foreground hover:border-emerald-500/35 hover:bg-emerald-500/[0.10] hover:text-emerald-700 dark:hover:text-emerald-300 inline-flex h-[20px] cursor-pointer items-center gap-1 rounded border px-1.5 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
            >
              {blockBusy && <Loader2 className="size-[10px] animate-spin" />}
              <span>{t("review.acceptOurs")}</span>
            </button>
            <button
              type="button"
              disabled={Boolean(busyKey)}
              onClick={() =>
                void handleResolve(file, block.index, { strategy: "theirs" }, blockKey)
              }
              className="border-border text-muted-foreground hover:border-sky-500/35 hover:bg-sky-500/[0.10] hover:text-sky-700 dark:hover:text-sky-300 inline-flex h-[20px] cursor-pointer items-center rounded border px-1.5 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
            >
              {t("review.acceptTheirs")}
            </button>
            <button
              type="button"
              disabled={Boolean(busyKey)}
              onClick={() =>
                void handleResolve(file, block.index, { strategy: "both" }, blockKey)
              }
              className="border-border text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[20px] cursor-pointer items-center rounded border px-1.5 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
            >
              {t("review.acceptBoth")}
            </button>
            <button
              type="button"
              disabled={Boolean(busyKey)}
              onClick={() =>
                setEditingBlock(
                  isEditing
                    ? null
                    : {
                        key: blockKey,
                        path: file.path,
                        blockIndex: block.index,
                        draft: defaultManualResolutionText(block),
                      },
                )
              }
              className={cn(
                "border-border inline-flex h-[20px] cursor-pointer items-center gap-1 rounded border px-1.5 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45",
                isEditing
                  ? "bg-accent text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              <Edit3 className="size-[10px]" />
              <span>{t("review.editManually")}</span>
            </button>
          </div>
        </div>

        {/* 3-Way Code Panes (Ours / optional diff3 Base / Theirs) */}
        <div className="bg-surface-recessed flex flex-col font-mono text-[11.5px] leading-[18px]">
          {block.context_before.length > 0 && (
            <pre className="text-muted-foreground/70 overflow-x-auto px-2.5 py-1 whitespace-pre">
              {block.context_before.join("\n")}
            </pre>
          )}

          {/* Ours */}
          <div className="border-l-2 border-l-emerald-500/70 bg-emerald-500/[0.06] px-2.5 py-1.5">
            <div className="mb-0.5 font-sans text-[10px] font-medium tracking-wide text-emerald-700 uppercase dark:text-emerald-400">
              {t("review.conflictOursLabel")}
              {block.ours_label ? ` · ${block.ours_label}` : ""}
            </div>
            <pre className="text-foreground overflow-x-auto whitespace-pre">
              {block.ours || " "}
            </pre>
          </div>

          {/* Common Ancestor Base (only when diff3 / zdiff3 markers are present) */}
          {block.base != null && (
            <div className="border-border-subtle border-y border-l-2 border-l-muted-foreground/45 bg-muted/45 px-2.5 py-1.5">
              <div className="text-muted-foreground mb-0.5 font-sans text-[10px] font-medium tracking-wide uppercase">
                {t("review.conflictBaseLabel")}
                {block.base_label ? ` · ${block.base_label}` : ""}
              </div>
              <pre className="text-muted-foreground overflow-x-auto whitespace-pre">
                {block.base || " "}
              </pre>
            </div>
          )}

          {/* Theirs */}
          <div className="border-l-2 border-l-sky-500/70 bg-sky-500/[0.06] px-2.5 py-1.5">
            <div className="mb-0.5 font-sans text-[10px] font-medium tracking-wide text-sky-700 uppercase dark:text-sky-400">
              {t("review.conflictTheirsLabel")}
              {block.theirs_label ? ` · ${block.theirs_label}` : ""}
            </div>
            <pre className="text-foreground overflow-x-auto whitespace-pre">
              {block.theirs || " "}
            </pre>
          </div>

          {block.context_after.length > 0 && (
            <pre className="text-muted-foreground/70 overflow-x-auto px-2.5 py-1 whitespace-pre">
              {block.context_after.join("\n")}
            </pre>
          )}
        </div>

        {/* Inline Manual Resolution Editor */}
        {isEditing && editingBlock && (
          <div className="border-border-subtle bg-surface-raised flex flex-col gap-1.5 border-t p-2">
            <Textarea
              autoFocus
              rows={4}
              value={editingBlock.draft}
              onChange={(e) =>
                setEditingBlock((prev) => (prev ? { ...prev, draft: e.target.value } : null))
              }
              {...manualIme.compositionProps}
              onKeyDown={(e) => {
                if (manualIme.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Escape") {
                  e.preventDefault();
                  setEditingBlock(null);
                }
              }}
              className="min-h-[76px] resize-y font-mono text-[11.5px] leading-[18px]"
            />
            <div className="flex items-center justify-end gap-1.5">
              <Button
                type="button"
                variant="ghost"
                size="xs"
                onClick={() => setEditingBlock(null)}
              >
                {t("common.cancel")}
              </Button>
              <Button
                type="button"
                size="xs"
                disabled={Boolean(busyKey)}
                onClick={() =>
                  void handleResolve(
                    file,
                    block.index,
                    { strategy: "manual_content", content: editingBlock.draft },
                    blockKey,
                  )
                }
              >
                {t("review.applyManualResolution")}
              </Button>
            </div>
          </div>
        )}
      </div>
    );
  };

  return (
    <div className="border-border bg-surface-raised/75 flex max-h-[52%] shrink-0 flex-col gap-2 overflow-y-auto border-b px-3 py-2.5">
      {/* Rebase / Conflict Header Banner */}
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex min-w-0 flex-1 items-center gap-1.5">
          {inRebase ? (
            <GitMerge className="size-[13.5px] shrink-0 text-amber-600 dark:text-amber-400" />
          ) : (
            <AlertTriangle className="size-[13.5px] shrink-0 text-amber-600 dark:text-amber-400" />
          )}
          <div className="flex min-w-0 flex-col">
            <div className="flex flex-wrap items-center gap-1.5 text-[12px] font-medium">
              <span className="text-foreground truncate">
                {inRebase
                  ? rebaseStatus?.onto
                    ? t("review.rebasingOnto", {
                        head: rebaseStatus.head_name ?? "HEAD",
                        onto: rebaseStatus.onto,
                      })
                    : t("review.rebaseInProgress")
                  : rebaseStatus?.merge_in_progress
                    ? t("review.mergeInProgress")
                    : rebaseStatus?.cherry_pick_in_progress
                      ? t("review.cherryPickInProgress")
                      : t("review.conflictsDetectedTitle", { count: unresolvedFileCount })}
              </span>
              {inRebase &&
                rebaseStatus?.current_step != null &&
                rebaseStatus?.total_steps != null && (
                  <span className="border-amber-500/30 bg-amber-500/10 text-amber-700 dark:text-amber-300 tabular-nums rounded border px-1.5 py-0.25 font-mono text-[10.5px]">
                    {t("review.rebaseStepCounter", {
                      current: rebaseStatus.current_step,
                      total: rebaseStatus.total_steps,
                    })}
                  </span>
                )}
              {totalBlocks > 0 && (
                <span className="text-muted-foreground tabular-nums font-mono text-[10.5px]">
                  {t("review.conflictBlockCount", { count: totalBlocks })}
                </span>
              )}
            </div>
            {inRebase && rebaseStatus?.stopped_message && (
              <span className="text-muted-foreground truncate text-[11px]">
                {rebaseStatus.stopped_message}
              </span>
            )}
          </div>
        </div>

        {inRebase && (
          <div className="flex shrink-0 items-center gap-1">
            <Button
              type="button"
              size="xs"
              disabled={!canContinueRebase}
              title={
                unresolvedFileCount > 0
                  ? t("review.rebaseResolveBeforeContinueHint")
                  : t("review.rebaseContinue")
              }
              onClick={() => void handleRebaseContinue()}
              className="h-[24px] gap-1 px-2 text-[11px]"
            >
              {rebaseBusy === "continue" ? (
                <Loader2 className="size-[11px] animate-spin" />
              ) : (
                <Play className="size-[10.5px]" />
              )}
              <span>{t("review.rebaseContinue")}</span>
            </Button>
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={rebaseBusy !== null}
              onClick={() =>
                setPendingRebaseConfirm({
                  action: "skip",
                  expectedStep: rebaseStatus?.current_step ?? null,
                })
              }
              className="h-[24px] gap-1 px-2 text-[11px]"
            >
              <SkipForward className="size-[11px]" />
              <span>{t("review.rebaseSkip")}</span>
            </Button>
            <Button
              type="button"
              variant="outline"
              size="xs"
              disabled={rebaseBusy !== null}
              onClick={() => setPendingRebaseConfirm({ action: "abort" })}
              className="hover:border-rose-500/35 hover:bg-rose-500/[0.10] hover:text-rose-600 dark:hover:text-rose-300 h-[24px] gap-1 px-2 text-[11px]"
            >
              <XCircle className="size-[11px]" />
              <span>{t("review.rebaseAbort")}</span>
            </Button>
          </div>
        )}
      </div>

      {/* Conflicted Files + 3-Way Blocks */}
      {conflictFiles.length > 0 && (
        <div className="flex flex-col gap-2">
          {conflictFiles.map((file) => {
            const expanded = expandedFiles.has(file.path);
            const fileBusy = busyKey === file.path;
            return (
              <div
                key={file.path}
                className="border-border bg-surface-base flex flex-col overflow-hidden rounded-lg border"
              >
                <div className="bg-surface-raised/80 flex flex-wrap items-center justify-between gap-2 px-2.5 py-1.5">
                  <button
                    type="button"
                    onClick={() => toggleFile(file.path)}
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
                  >
                    {expanded ? (
                      <ChevronDown className="text-muted-foreground size-[13px] shrink-0" />
                    ) : (
                      <ChevronRight className="text-muted-foreground size-[13px] shrink-0" />
                    )}
                    <span className="text-foreground truncate font-mono text-[11.5px] font-medium">
                      {file.path}
                    </span>
                    <span className="tabular-nums shrink-0 rounded bg-amber-500/12 px-1.5 py-0.25 font-mono text-[10px] text-amber-700 dark:text-amber-300">
                      {file.is_binary
                        ? t("review.conflictBinaryBadge")
                        : file.blocks.length > 0
                          ? t("review.conflictBlockCount", { count: file.blocks.length })
                          : t("review.conflictReadyToStageBadge")}
                    </span>
                  </button>

                  <div className="flex shrink-0 items-center gap-1">
                    <button
                      type="button"
                      disabled={Boolean(busyKey)}
                      onClick={() =>
                        void handleResolve(file, null, { strategy: "ours" }, file.path)
                      }
                      className="border-border text-muted-foreground hover:border-emerald-500/35 hover:bg-emerald-500/[0.10] hover:text-emerald-700 dark:hover:text-emerald-300 inline-flex h-[20px] cursor-pointer items-center gap-1 rounded border px-1.5 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                    >
                      {fileBusy && <Loader2 className="size-[10px] animate-spin" />}
                      <span>{t("review.acceptAllOurs")}</span>
                    </button>
                    <button
                      type="button"
                      disabled={Boolean(busyKey)}
                      onClick={() =>
                        void handleResolve(file, null, { strategy: "theirs" }, file.path)
                      }
                      className="border-border text-muted-foreground hover:border-sky-500/35 hover:bg-sky-500/[0.10] hover:text-sky-700 dark:hover:text-sky-300 inline-flex h-[20px] cursor-pointer items-center rounded border px-1.5 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                    >
                      {t("review.acceptAllTheirs")}
                    </button>
                    {!file.is_binary && file.blocks.length === 0 && (
                      <button
                        type="button"
                        disabled={Boolean(busyKey)}
                        onClick={() =>
                          void handleResolve(file, null, { strategy: "both" }, file.path)
                        }
                        className="border-emerald-500/35 bg-emerald-500/12 text-emerald-700 hover:bg-emerald-500/20 dark:text-emerald-300 inline-flex h-[20px] cursor-pointer items-center gap-1 rounded border px-1.5 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                      >
                        <Check className="size-[10.5px]" />
                        <span>{t("review.markResolved")}</span>
                      </button>
                    )}
                  </div>
                </div>

                {expanded && file.blocks.length > 0 && (
                  <div className="border-border-subtle flex flex-col gap-2 border-t p-2">
                    {file.blocks.map((block) => renderBlock(file, block))}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* Guarded Confirmation for Rebase Abort / Skip */}
      <ConfirmDialog
        open={visible && pendingRebaseConfirm !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPendingRebaseConfirm(null);
        }}
        tone={pendingRebaseConfirm?.action === "abort" ? "danger" : "default"}
        defaultAction="cancel"
        title={
          pendingRebaseConfirm?.action === "abort"
            ? t("review.rebaseAbortConfirmTitle")
            : t("review.rebaseSkipConfirmTitle")
        }
        description={
          pendingRebaseConfirm?.action === "abort"
            ? t("review.rebaseAbortConfirmDesc")
            : t("review.rebaseSkipConfirmDesc")
        }
        confirmLabel={
          pendingRebaseConfirm?.action === "abort"
            ? t("review.rebaseAbort")
            : t("review.rebaseSkip")
        }
        onConfirm={handleConfirmRebaseAction}
      />
    </div>
  );
});
