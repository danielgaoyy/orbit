import { useEffect, useRef, useState } from "react";
import {
  AlertTriangle,
  ArrowRight,
  GitBranch,
  Loader2,
  ShieldCheck,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Button } from "@/components/ui/button";
import { Field, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { ToggleSwitch } from "@/components/settings/settings-primitives";
import {
  type WorktreeHandoffMode,
  type WorktreeHandoffPreview,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  type WorktreeHandoffDialogTarget,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const MODE_OPTIONS: ReadonlyArray<{ id: WorktreeHandoffMode; label: string }> = [
  { id: "apply", label: "Apply" },
  { id: "move", label: "Move" },
];

function statusToneClass(status: string, hasConflict: boolean): string {
  if (hasConflict) return "text-amber-600 dark:text-amber-400";
  const s = status.trim().toUpperCase();
  if (s === "A" || s === "??") return "text-emerald-600 dark:text-emerald-400";
  if (s === "D") return "text-rose-600 dark:text-rose-400";
  if (s === "M" || s === "R") return "text-amber-600 dark:text-amber-400";
  return "text-muted-foreground";
}

function shortBackupLabel(backupRef: string): string {
  const parts = backupRef.split("/");
  return parts[parts.length - 1] || backupRef;
}

function targetIdentityKey(target: WorktreeHandoffDialogTarget): string {
  return [
    target.hostId,
    target.workspaceId,
    target.direction,
    target.targetWorkspaceId ?? "",
    target.defaultMode ?? "",
  ].join("::");
}

export function WorktreeHandoffModal() {
  const target = useOrbitSelector((s) => s.handoffDialogTarget);
  const [executing, setExecuting] = useState(false);
  const open = Boolean(target);

  useEffect(() => {
    if (!target) {
      setExecuting(false);
    }
  }, [target]);

  return (
    <Dialog
      open={open}
      onOpenChange={(nextOpen) => {
        if (!nextOpen && !executing) {
          orbitActions.closeWorktreeHandoffDialog();
        }
      }}
    >
      {target && (
        <WorktreeHandoffModalBody
          key={targetIdentityKey(target)}
          target={target}
          executing={executing}
          setExecuting={setExecuting}
        />
      )}
    </Dialog>
  );
}

function WorktreeHandoffModalBody({
  target,
  executing,
  setExecuting,
}: {
  target: WorktreeHandoffDialogTarget;
  executing: boolean;
  setExecuting: (next: boolean) => void;
}) {
  const { hostId, workspaceId, direction } = target;
  const targetWorkspaceId = target.targetWorkspaceId ?? null;
  const activeAgentInWs = useOrbitSelector((s) => {
    if (!s.selectedAgentId || s.selectedHostId !== hostId) return null;
    const ag = s.hostDetails[hostId]?.agents.find((a) => a.agent_id === s.selectedAgentId);
    return ag && ag.workspace_id === workspaceId ? ag.agent_id : null;
  });
  const sourceAgentId = target.sourceAgentId ?? activeAgentInWs;
  const identity = targetIdentityKey(target);

  const sourceWorkspace = useOrbitSelector(
    (s) =>
      s.hostDetails[hostId]?.workspaces.find((w) => w.workspace_id === workspaceId) ?? null,
  );

  const [mode, setMode] = useState<WorktreeHandoffMode>(
    () => target.defaultMode ?? (direction === "local_to_worktree" ? "move" : "apply"),
  );
  const [includeCommitted, setIncludeCommitted] = useState<boolean>(
    () => direction === "worktree_to_local",
  );
  const [branchName, setBranchName] = useState("");
  const [allowThreeWay, setAllowThreeWay] = useState(false);
  const [preview, setPreview] = useState<WorktreeHandoffPreview | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [previewRefreshNonce, setPreviewRefreshNonce] = useState(0);

  const previewSeqRef = useRef(0);
  const execSeqRef = useRef(0);
  const mountedRef = useRef(true);
  const activeIdentityRef = useRef(identity);
  const radioGroupRef = useRef<HTMLDivElement | null>(null);

  activeIdentityRef.current = identity;

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
      previewSeqRef.current += 1;
      execSeqRef.current += 1;
    };
  }, []);

  // Live non-mutating dry-run preview (freshly keyed per target — zero stale double-RPC)
  useEffect(() => {
    const seq = ++previewSeqRef.current;
    setLoading(true);
    setError(null);
    void orbitIpc
      .previewWorktreeHandoff({
        host_id: hostId,
        workspace_id: workspaceId,
        source_agent_id: sourceAgentId,
        direction,
        target_workspace_id: targetWorkspaceId,
        include_committed: direction === "worktree_to_local" ? includeCommitted : false,
      })
      .then((res) => {
        if (!mountedRef.current || previewSeqRef.current !== seq) return;
        setPreview(res);
        if (res.can_apply_cleanly) {
          setAllowThreeWay(false);
        }
      })
      .catch((err) => {
        if (!mountedRef.current || previewSeqRef.current !== seq) return;
        setPreview(null);
        setError(err instanceof Error ? err.message : String(err));
      })
      .finally(() => {
        if (mountedRef.current && previewSeqRef.current === seq) {
          setLoading(false);
        }
      });
  }, [hostId, workspaceId, direction, targetWorkspaceId, includeCommitted, previewRefreshNonce]);

  const handleModeKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (executing) return;
    if (e.key !== "ArrowLeft" && e.key !== "ArrowRight") return;
    e.preventDefault();
    const next: WorktreeHandoffMode = mode === "apply" ? "move" : "apply";
    setMode(next);
    radioGroupRef.current
      ?.querySelector<HTMLButtonElement>(`[data-handoff-mode="${next}"]`)
      ?.focus();
  };

  const hasConflicts = Boolean(preview && preview.conflicts.length > 0);
  const hasHardBlocker = Boolean(
    preview && !preview.can_apply_cleanly && preview.conflicts.length === 0,
  );
  const hasUntrackedConflict = Boolean(
    preview?.files.some((f) => f.is_untracked && f.has_conflict),
  );
  const canOfferThreeWay = hasConflicts && !hasUntrackedConflict && !hasHardBlocker;
  const fileCount = preview?.files.length ?? 0;
  const canSubmit =
    Boolean(preview) &&
    !loading &&
    !executing &&
    !hasHardBlocker &&
    !hasUntrackedConflict &&
    (fileCount > 0 || (direction === "local_to_worktree" && !targetWorkspaceId)) &&
    (!hasConflicts || (canOfferThreeWay && allowThreeWay));

  const handleExecute = async () => {
    if (!canSubmit) return;
    const mySeq = ++execSeqRef.current;
    const myIdentity = identity;
    setExecuting(true);
    setError(null);
    try {
      const outcome = await orbitIpc.executeWorktreeHandoff({
        host_id: hostId,
        workspace_id: workspaceId,
        source_agent_id: sourceAgentId,
        direction,
        mode,
        target_workspace_id: targetWorkspaceId,
        include_committed: direction === "worktree_to_local" ? includeCommitted : false,
        branch:
          direction === "local_to_worktree" && !targetWorkspaceId
            ? branchName.trim() || null
            : null,
        allow_three_way_conflicts: canOfferThreeWay && allowThreeWay,
      });

      // Immediately refresh Git status & diff for both source & target checkouts
      const srcCwd = preview?.source_cwd || sourceWorkspace?.cwd || target.sourceCwd || null;
      const refreshes: Array<Promise<void>> = [];
      if (srcCwd) {
        refreshes.push(orbitActions.refreshGit(hostId, srcCwd, null, false));
      }
      if (outcome.target_cwd && outcome.target_cwd !== srcCwd) {
        refreshes.push(orbitActions.refreshGit(hostId, outcome.target_cwd, null, false));
      }
      if (outcome.target_workspace) {
        refreshes.push(orbitIpc.refreshHost(hostId));
      }
      void Promise.allSettled(refreshes);

      const backupSuffix = outcome.backup_ref
        ? ` · backup ${shortBackupLabel(outcome.backup_ref)}`
        : "";
      orbitActions.showToast(`${outcome.summary}${backupSuffix}`);
      if (
        mountedRef.current &&
        execSeqRef.current === mySeq &&
        activeIdentityRef.current === myIdentity
      ) {
        orbitActions.closeWorktreeHandoffDialog();
      }
    } catch (err) {
      if (
        mountedRef.current &&
        execSeqRef.current === mySeq &&
        activeIdentityRef.current === myIdentity
      ) {
        const message = err instanceof Error ? err.message : String(err);
        setError(message);
        if (message.includes("handoff_conflict")) {
          setPreviewRefreshNonce((n) => n + 1);
        }
      }
    } finally {
      if (
        mountedRef.current &&
        execSeqRef.current === mySeq &&
        activeIdentityRef.current === myIdentity
      ) {
        setExecuting(false);
      }
    }
  };

  const sourceLabel =
    direction === "worktree_to_local"
      ? preview?.source_branch || sourceWorkspace?.git_branch || "worktree"
      : `Local${preview?.source_branch || sourceWorkspace?.git_branch ? ` (${preview?.source_branch || sourceWorkspace?.git_branch})` : ""}`;

  const targetLabel =
    direction === "worktree_to_local"
      ? `Local${preview?.target_branch ? ` (${preview.target_branch})` : ""}`
      : targetWorkspaceId
        ? preview?.target_branch || "Worktree"
        : branchName.trim()
          ? `New worktree (${branchName.trim()})`
          : "New worktree";

  const primaryActionLabel =
    canOfferThreeWay && allowThreeWay
      ? "Apply with 3-way markers"
      : direction === "worktree_to_local"
        ? mode === "move"
          ? "Move to Local"
          : "Apply to Local"
        : mode === "move"
          ? "Move to Worktree"
          : "Apply to Worktree";

  // Surface non-conflict preview warnings (e.g., skipped escaping symlinks or blocked tracked symlinks)
  // even when changed files exist in the preview list.
  const showStandaloneWarning = Boolean(
    preview?.warning &&
      !hasConflicts &&
      (preview.files.length > 0 || hasHardBlocker),
  );

  return (
    <DialogContent closeDisabled={executing} className="max-w-[460px] gap-4 p-5 select-none">
      <DialogHeader className="gap-1.5 pr-6">
        <DialogTitle className="text-[15px]">Handoff</DialogTitle>
        <DialogDescription className="bg-muted/70 text-foreground/85 inline-flex w-fit max-w-full items-center gap-1.5 rounded-lg px-2.5 py-1 font-mono text-[11.5px]">
          <GitBranch className="text-muted-foreground h-[12px] w-[12px] shrink-0" />
          <span className="truncate">{sourceLabel}</span>
          <ArrowRight className="text-muted-foreground h-[12px] w-[12px] shrink-0" />
          <span className="truncate font-medium">{targetLabel}</span>
        </DialogDescription>
      </DialogHeader>

      {/* Mode segmented control */}
      <Field>
        <div className="flex items-center justify-between">
          <FieldLabel>Mode</FieldLabel>
          <span className="text-ink-muted text-[11.5px]">
            {mode === "apply"
              ? "Keep source intact"
              : "Reset uncommitted source after transfer"}
          </span>
        </div>
        <div
          ref={radioGroupRef}
          role="radiogroup"
          aria-label="Handoff mode"
          onKeyDown={handleModeKeyDown}
          className="bg-muted grid grid-cols-2 gap-1 rounded-[10px] p-1"
        >
          {MODE_OPTIONS.map((opt) => {
            const active = mode === opt.id;
            return (
              <button
                key={opt.id}
                type="button"
                role="radio"
                aria-checked={active}
                tabIndex={active ? 0 : -1}
                data-handoff-mode={opt.id}
                disabled={executing}
                onClick={() => setMode(opt.id)}
                className={cn(
                  "focus-visible:ring-foreground/25 cursor-pointer rounded-[7px] py-1.5 text-[12.5px] font-medium transition-colors outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50",
                  active
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                {opt.label}
              </button>
            );
          })}
        </div>
      </Field>

      {/* Conditional options: Include commits (Worktree -> Local) or Branch name (Local -> New worktree) */}
      {direction === "worktree_to_local" && (preview?.commit_count ?? 0) > 0 && (
        <div className="flex items-center justify-between gap-3 rounded-[11px] border border-black/[0.07] px-3 py-2.5 dark:border-white/[0.08]">
          <div className="min-w-0">
            <div className="text-[13px] font-normal">Include commits</div>
            <div className="text-muted-foreground text-[11.5px]">
              {preview?.commit_count}{" "}
              {preview?.commit_count === 1 ? "commit" : "commits"} ahead of local
            </div>
          </div>
          <ToggleSwitch
            checked={includeCommitted}
            onChange={setIncludeCommitted}
            disabled={executing}
            ariaLabel="Include commits"
          />
        </div>
      )}

      {direction === "local_to_worktree" && !targetWorkspaceId && (
        <Field>
          <FieldLabel htmlFor="orbit-handoff-branch">
            Branch name <span className="text-ink-muted font-normal">(optional)</span>
          </FieldLabel>
          <Input
            id="orbit-handoff-branch"
            type="text"
            value={branchName}
            disabled={executing}
            onChange={(e) => setBranchName(e.target.value)}
            placeholder="Auto-generated branch"
            className="h-[34px] px-3 font-mono"
          />
        </Field>
      )}

      {/* Live dry-run diff preview */}
      <div className="overflow-hidden rounded-[11px] border border-black/[0.07] dark:border-white/[0.08]">
        <div className="border-b border-black/[0.06] px-3 py-2 text-[12px] dark:border-white/[0.07]">
          <div className="flex items-center justify-between gap-2">
            <span className="text-muted-foreground inline-flex items-center gap-1.5 font-medium">
              <span>
                {loading && !preview
                  ? "Checking changes…"
                  : `${fileCount} ${fileCount === 1 ? "file" : "files"}`}
              </span>
              {loading && preview && (
                <Loader2 className="h-3 w-3 animate-spin opacity-70" />
              )}
            </span>
            {preview && (
              <div className="flex items-center gap-2 font-mono text-[11.5px] tabular-nums">
                {preview.target_dirty && (
                  <span className="text-muted-foreground font-sans text-[11px]">
                    Target dirty
                  </span>
                )}
                <span className="text-emerald-600 dark:text-emerald-400">
                  +{preview.total_insertions}
                </span>
                <span className="text-rose-600 dark:text-rose-400">
                  -{preview.total_deletions}
                </span>
              </div>
            )}
          </div>
        </div>

        {loading && !preview ? (
          <div className="text-muted-foreground flex items-center justify-center gap-2 py-6 text-[12.5px]">
            <Loader2 className="h-3.5 w-3.5 animate-spin" />
            <span>Analyzing diff…</span>
          </div>
        ) : preview && preview.files.length > 0 ? (
          <ScrollArea className="max-h-[180px]">
            <div className="divide-y divide-black/[0.04] dark:divide-white/[0.05]">
              {preview.files.map((file) => (
                <div
                  key={file.path}
                  className="flex items-center justify-between gap-2.5 px-3 py-1.5 text-[12px]"
                >
                  <div className="flex min-w-0 items-center gap-2 font-mono">
                    <span
                      className={cn(
                        "w-[18px] shrink-0 text-[11px] font-semibold",
                        statusToneClass(file.status, file.has_conflict),
                      )}
                    >
                      {file.status}
                    </span>
                    <span
                      title={file.path}
                      className={cn(
                        "truncate text-[12px]",
                        file.has_conflict
                          ? "text-amber-700 dark:text-amber-300"
                          : "text-foreground/90",
                      )}
                    >
                      {file.path}
                    </span>
                  </div>
                  <div className="flex shrink-0 items-center gap-1.5 font-mono text-[11px] tabular-nums">
                    {file.has_conflict && (
                      <span className="rounded bg-amber-500/15 px-1.5 py-0.5 font-sans text-[10px] font-medium text-amber-700 dark:text-amber-300">
                        Conflict
                      </span>
                    )}
                    {file.insertions > 0 && (
                      <span className="text-emerald-600 dark:text-emerald-400">
                        +{file.insertions}
                      </span>
                    )}
                    {file.deletions > 0 && (
                      <span className="text-rose-600 dark:text-rose-400">
                        -{file.deletions}
                      </span>
                    )}
                  </div>
                </div>
              ))}
            </div>
          </ScrollArea>
        ) : (
          <div className="text-muted-foreground px-3 py-5 text-center text-[12.5px]">
            {preview?.warning || "No changes to transfer."}
          </div>
        )}
      </div>

      {/* Standalone preview warning (e.g. skipped escaping symlinks or blocked tracked symlinks) */}
      {showStandaloneWarning && preview?.warning && (
        <div className="flex items-start gap-2 rounded-[11px] border border-amber-500/25 bg-amber-500/[0.08] px-3 py-2 text-[12px] text-amber-800 dark:text-amber-300">
          <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
          <span className="min-w-0 leading-snug">{preview.warning}</span>
        </div>
      )}

      {/* Amber conflict warning & explicit 3-way opt-in (only when no colliding untracked files) */}
      {hasConflicts && preview && (
        <div className="space-y-2.5 rounded-[11px] border border-amber-500/25 bg-amber-500/[0.08] px-3 py-2.5 text-[12px] text-amber-800 dark:text-amber-300">
          <div className="flex items-start gap-2">
            <AlertTriangle className="mt-0.5 h-3.5 w-3.5 shrink-0" />
            <div className="min-w-0 space-y-0.5">
              <div className="font-medium">
                {preview.conflicts.length}{" "}
                {preview.conflicts.length === 1 ? "file conflicts" : "files conflict"} with target
              </div>
              <div className="truncate font-mono text-[11px] opacity-85">
                {preview.conflicts.slice(0, 3).join(", ")}
                {preview.conflicts.length > 3 ? ` +${preview.conflicts.length - 3} more` : ""}
              </div>
              {hasUntrackedConflict && (
                <div className="pt-0.5 text-[11.5px] opacity-90">
                  Resolve or move differing untracked files in target before handoff.
                </div>
              )}
            </div>
          </div>
          {canOfferThreeWay && (
            <div className="flex items-center justify-between gap-2 border-t border-amber-500/20 pt-2">
              <span className="text-[12px] font-medium">
                Apply with 3-way conflict markers
              </span>
              <ToggleSwitch
                checked={allowThreeWay}
                onChange={setAllowThreeWay}
                disabled={executing}
                ariaLabel="Apply with 3-way conflict markers"
              />
            </div>
          )}
        </div>
      )}

      {error && (
        <p className="rounded-lg bg-rose-500/10 px-3 py-2 text-[12px] text-rose-600 dark:text-rose-400">
          {error}
        </p>
      )}

      {/* Quiet reassurance line & primary action */}
      <div className="flex items-center justify-between gap-3 pt-0.5">
        <span className="text-muted-foreground inline-flex min-w-0 items-center gap-1.5 text-[11.5px]">
          <ShieldCheck className="h-3.5 w-3.5 shrink-0 opacity-75" />
          <span className="truncate">Backup snapshot saved automatically</span>
        </span>

        <div className="flex shrink-0 items-center gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={executing}
            onClick={() => orbitActions.closeWorktreeHandoffDialog()}
            className="h-[31px] rounded-[8px] px-3 text-[12.5px]"
          >
            Cancel
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={!canSubmit}
            onClick={() => void handleExecute()}
            className="h-[31px] gap-1.5 rounded-[8px] px-3.5 text-[12.5px]"
          >
            {executing && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
            <span>{primaryActionLabel}</span>
          </Button>
        </div>
      </div>
    </DialogContent>
  );
}
