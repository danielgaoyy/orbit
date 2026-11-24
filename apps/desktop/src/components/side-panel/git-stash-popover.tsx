import { memo, useCallback, useState } from "react";
import {
  Archive,
  ChevronDown,
  ChevronRight,
  GitBranch,
  GitMerge,
  Loader2,
  Play,
  Plus,
  Trash2,
  Undo2,
} from "lucide-react";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Separator } from "@/components/ui/separator";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { orbitIpc, type GitStashEntry } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { orbitActions } from "@/state/orbit-store";

interface GitStashPopoverProps {
  hostId: string;
  cwd: string;
  visible: boolean;
  stashes: readonly GitStashEntry[];
  hasLocalChanges: boolean;
  rebaseInProgress: boolean;
  onRefreshAll: () => Promise<void>;
}

function formatErrorDetail(err: unknown): string {
  const raw = err instanceof Error ? err.message : String(err);
  return raw.trim() || "Unknown error";
}

export const GitStashPopover = memo(function GitStashPopover({
  hostId,
  cwd,
  visible,
  stashes,
  hasLocalChanges,
  rebaseInProgress,
  onRefreshAll,
}: GitStashPopoverProps) {
  const { t } = useI18n();
  const [open, setOpen] = useState(false);
  const [stashMessage, setStashMessage] = useState("");
  const [includeUntracked, setIncludeUntracked] = useState(false);
  const [busyAction, setBusyAction] = useState<string | null>(null);
  const [pendingDrop, setPendingDrop] = useState<GitStashEntry | null>(null);
  const [rebaseExpanded, setRebaseExpanded] = useState(false);
  const [rebaseOntoRef, setRebaseOntoRef] = useState("");
  const stashIme = useImeCompositionGuard();
  const rebaseIme = useImeCompositionGuard();

  const handlePush = useCallback(async () => {
    if (busyAction || !hasLocalChanges) return;
    setBusyAction("push");
    try {
      const res = await orbitIpc.gitStashPush({
        hostId,
        cwd,
        message: stashMessage.trim() || null,
        includeUntracked,
      });
      setStashMessage("");
      await onRefreshAll();
      orbitActions.showToast(res.summary || t("review.stashPushedToast"));
    } catch (err) {
      orbitActions.showToast(
        t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }),
      );
    } finally {
      setBusyAction(null);
    }
  }, [busyAction, hasLocalChanges, hostId, cwd, stashMessage, includeUntracked, onRefreshAll, t]);

  const handleApplyOrPop = useCallback(
    async (mode: "apply" | "pop", entry: GitStashEntry) => {
      if (busyAction) return;
      setBusyAction(`${mode}:${entry.stash_ref}`);
      try {
        const res =
          mode === "apply"
            ? await orbitIpc.gitStashApply({
                hostId,
                cwd,
                stashRef: entry.stash_ref,
                expectedOid: entry.oid ?? null,
              })
            : await orbitIpc.gitStashPop({
                hostId,
                cwd,
                stashRef: entry.stash_ref,
                expectedOid: entry.oid ?? null,
              });
        await onRefreshAll();
        orbitActions.showToast(res.summary);
        if (res.had_conflicts) {
          setOpen(false);
        }
      } catch (err) {
        orbitActions.showToast(
          t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }),
        );
      } finally {
        setBusyAction(null);
      }
    },
    [busyAction, hostId, cwd, onRefreshAll, t],
  );

  const handleConfirmDrop = useCallback(async () => {
    if (!pendingDrop) return;
    const target = pendingDrop;
    setBusyAction(`drop:${target.stash_ref}`);
    try {
      const res = await orbitIpc.gitStashDrop({
        hostId,
        cwd,
        stashRef: target.stash_ref,
        expectedOid: target.oid ?? null,
      });
      await onRefreshAll();
      orbitActions.showToast(res.summary || t("review.stashDroppedToast", { ref: target.stash_ref }));
    } finally {
      setBusyAction(null);
    }
  }, [pendingDrop, hostId, cwd, onRefreshAll, t]);

  const handleStartRebase = useCallback(async () => {
    const trimmed = rebaseOntoRef.trim();
    if (!trimmed || busyAction || rebaseInProgress) return;
    setBusyAction("rebase");
    try {
      const res = await orbitIpc.gitRebaseStart({
        hostId,
        cwd,
        ontoRef: trimmed,
      });
      setRebaseOntoRef("");
      setRebaseExpanded(false);
      setOpen(false);
      await onRefreshAll();
      orbitActions.showToast(res.summary);
    } catch (err) {
      orbitActions.showToast(
        t("review.gitActionFailedToast", { detail: formatErrorDetail(err) }),
      );
    } finally {
      setBusyAction(null);
    }
  }, [rebaseOntoRef, busyAction, rebaseInProgress, hostId, cwd, onRefreshAll, t]);

  return (
    <>
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            title={
              stashes.length > 0
                ? t("review.stashButtonWithCountTitle", { count: stashes.length })
                : t("review.stashButtonTitle")
            }
            className={cn(
              "inline-flex h-[26px] cursor-pointer items-center justify-center gap-1 rounded-lg px-1.5 transition-colors",
              open
                ? "bg-accent text-foreground"
                : "text-muted-foreground hover:bg-accent hover:text-foreground",
            )}
          >
            <Archive className="size-[13px] shrink-0" />
            {stashes.length > 0 && (
              <span className="tabular-nums font-mono text-[10.5px] font-medium">
                {stashes.length}
              </span>
            )}
          </button>
        </PopoverTrigger>

        <PopoverContent
          align="end"
          sideOffset={6}
          className="flex w-[340px] flex-col gap-3 p-3"
        >
          {/* Push New Stash */}
          <div className="flex flex-col gap-2">
            <div className="flex items-center justify-between gap-2">
              <span className="text-foreground text-[12px] font-semibold">
                {t("review.stashTitle")}
              </span>
              {stashes.length > 0 && (
                <span className="text-muted-foreground tabular-nums font-mono text-[10.5px]">
                  {stashes.length}
                </span>
              )}
            </div>

            <div className="flex items-center gap-1.5">
              <Input
                type="text"
                value={stashMessage}
                disabled={busyAction !== null || !hasLocalChanges}
                onChange={(e) => setStashMessage(e.target.value)}
                {...stashIme.compositionProps}
                onKeyDown={(e) => {
                  if (stashIme.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void handlePush();
                  }
                }}
                placeholder={t("review.stashMessagePlaceholder")}
                className="h-[28px] flex-1 text-[12px]"
              />
              <Button
                type="button"
                size="xs"
                disabled={busyAction !== null || !hasLocalChanges}
                onClick={() => void handlePush()}
                className="h-[28px] shrink-0 gap-1 px-2.5 text-[11.5px]"
              >
                {busyAction === "push" ? (
                  <Loader2 className="size-[11px] animate-spin" />
                ) : (
                  <Plus className="size-[11px]" />
                )}
                <span>{t("review.stashPush")}</span>
              </Button>
            </div>

            <label className="text-muted-foreground hover:text-foreground flex cursor-pointer items-center gap-1.5 text-[11.5px] select-none">
              <Checkbox
                size="sm"
                checked={includeUntracked}
                disabled={busyAction !== null || !hasLocalChanges}
                onCheckedChange={setIncludeUntracked}
              />
              <span>{t("review.stashIncludeUntracked")}</span>
            </label>
          </div>

          <Separator />

          {/* Saved Stashes List */}
          {stashes.length === 0 ? (
            <p className="text-muted-foreground py-2 text-center text-[11.5px]">
              {t("review.stashEmpty")}
            </p>
          ) : (
            <div className="flex max-h-[210px] flex-col gap-1.5 overflow-y-auto">
              {stashes.map((entry) => {
                const rowBusy =
                  busyAction === `apply:${entry.stash_ref}` ||
                  busyAction === `pop:${entry.stash_ref}` ||
                  busyAction === `drop:${entry.stash_ref}`;
                return (
                  <div
                    key={entry.stash_ref}
                    className="border-border-subtle bg-surface-recessed flex flex-col gap-1.5 rounded-lg border px-2.5 py-2"
                  >
                    <div className="flex items-start justify-between gap-2">
                      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                        <div className="flex items-center gap-1.5">
                          <span className="text-foreground tabular-nums font-mono text-[11px] font-medium">
                            {entry.stash_ref}
                          </span>
                          {entry.branch && (
                            <span className="text-muted-foreground inline-flex min-w-0 items-center gap-0.5 font-mono text-[10.5px]">
                              <GitBranch className="size-[10px] shrink-0" />
                              <span className="truncate">{entry.branch}</span>
                            </span>
                          )}
                        </div>
                        <span className="text-muted-foreground truncate text-[11.5px]">
                          {entry.message}
                        </span>
                      </div>
                    </div>

                    <div className="flex items-center justify-end gap-1">
                      <button
                        type="button"
                        disabled={busyAction !== null}
                        onClick={() => void handleApplyOrPop("apply", entry)}
                        className="border-border bg-background text-foreground/85 hover:bg-accent hover:text-foreground inline-flex h-[22px] cursor-pointer items-center gap-1 rounded border px-2 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                      >
                        {rowBusy && busyAction?.startsWith("apply:") ? (
                          <Loader2 className="size-[10px] animate-spin" />
                        ) : (
                          <Play className="size-[10px]" />
                        )}
                        <span>{t("review.stashApply")}</span>
                      </button>
                      <button
                        type="button"
                        disabled={busyAction !== null}
                        onClick={() => void handleApplyOrPop("pop", entry)}
                        className="border-border bg-background text-foreground/85 hover:bg-accent hover:text-foreground inline-flex h-[22px] cursor-pointer items-center gap-1 rounded border px-2 text-[10.5px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
                      >
                        {rowBusy && busyAction?.startsWith("pop:") ? (
                          <Loader2 className="size-[10px] animate-spin" />
                        ) : (
                          <Undo2 className="size-[10px]" />
                        )}
                        <span>{t("review.stashPop")}</span>
                      </button>
                      <button
                        type="button"
                        disabled={busyAction !== null}
                        title={t("review.stashDrop")}
                        onClick={() => {
                          setOpen(false);
                          setPendingDrop(entry);
                        }}
                        className="border-border bg-background text-muted-foreground hover:border-rose-500/35 hover:bg-rose-500/[0.10] hover:text-rose-600 dark:hover:text-rose-300 inline-flex size-[22px] cursor-pointer items-center justify-center rounded border transition-colors disabled:pointer-events-none disabled:opacity-45"
                      >
                        <Trash2 className="size-[10.5px]" />
                      </button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}

          {/* Subtle On-Demand Rebase Launcher (zero idle chrome in main toolbar) */}
          {!rebaseInProgress && (
            <>
              <Separator />
              <div className="flex flex-col gap-2">
                <button
                  type="button"
                  onClick={() => setRebaseExpanded((v) => !v)}
                  className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 text-[11.5px] font-medium transition-colors"
                >
                  {rebaseExpanded ? (
                    <ChevronDown className="size-[12px]" />
                  ) : (
                    <ChevronRight className="size-[12px]" />
                  )}
                  <GitMerge className="size-[12px]" />
                  <span>{t("review.rebaseOntoToggle")}</span>
                </button>
                {rebaseExpanded && (
                  <div className="flex items-center gap-1.5">
                    <Input
                      type="text"
                      value={rebaseOntoRef}
                      disabled={busyAction !== null}
                      onChange={(e) => setRebaseOntoRef(e.target.value)}
                      {...rebaseIme.compositionProps}
                      onKeyDown={(e) => {
                        if (rebaseIme.shouldIgnoreKeyDown(e)) return;
                        if (e.key === "Enter") {
                          e.preventDefault();
                          void handleStartRebase();
                        }
                      }}
                      placeholder={t("review.rebaseOntoPlaceholder")}
                      className="h-[28px] flex-1 font-mono text-[11.5px]"
                    />
                    <Button
                      type="button"
                      size="xs"
                      disabled={busyAction !== null || !rebaseOntoRef.trim()}
                      onClick={() => void handleStartRebase()}
                      className="h-[28px] shrink-0 gap-1 px-2.5 text-[11.5px]"
                    >
                      {busyAction === "rebase" ? (
                        <Loader2 className="size-[11px] animate-spin" />
                      ) : (
                        <GitMerge className="size-[11px]" />
                      )}
                      <span>{t("review.rebaseStart")}</span>
                    </Button>
                  </div>
                )}
              </div>
            </>
          )}
        </PopoverContent>
      </Popover>

      {/* Destructive Confirmation Dialog for Dropping a Stash */}
      <ConfirmDialog
        open={visible && pendingDrop !== null}
        onOpenChange={(nextOpen) => {
          if (!nextOpen) setPendingDrop(null);
        }}
        tone="danger"
        defaultAction="cancel"
        title={
          pendingDrop
            ? t("review.stashDropConfirmTitle", { ref: pendingDrop.stash_ref })
            : ""
        }
        description={t("review.stashDropConfirmDesc")}
        detail={
          pendingDrop ? (
            <div className="font-mono text-[11.5px] break-words">
              {pendingDrop.message}
            </div>
          ) : undefined
        }
        confirmLabel={t("review.stashDrop")}
        onConfirm={handleConfirmDrop}
      />
    </>
  );
});
