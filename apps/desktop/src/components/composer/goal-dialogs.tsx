import { memo, useEffect, useState } from "react";
import { AlertCircle, Loader2, Target } from "lucide-react";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Textarea } from "@/components/ui/textarea";
import { useI18n } from "@/lib/i18n";
import type { GoalRunRecord } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

function EditGoalModal({
  hostId,
  goal,
  onClose,
}: {
  hostId: string;
  goal: GoalRunRecord;
  onClose: () => void;
}) {
  const { t } = useI18n();
  const [objective, setObjective] = useState(goal.objective);
  const [criteriaText, setCriteriaText] = useState(
    goal.criteria.map((c) => c.label).join("\n"),
  );
  const [verifyCommand, setVerifyCommand] = useState(goal.verify_command ?? "");
  const [maxIterations, setMaxIterations] = useState(String(goal.max_iterations || 25));
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    setObjective(goal.objective);
    setCriteriaText(goal.criteria.map((c) => c.label).join("\n"));
    setVerifyCommand(goal.verify_command ?? "");
    setMaxIterations(String(goal.max_iterations || 25));
    setError(null);
    setSaving(false);
  }, [goal.goal_id, goal.updated_at_ms, goal.objective, goal.criteria, goal.verify_command, goal.max_iterations]);

  const handleSave = async () => {
    const trimmedObjective = objective.trim();
    if (!trimmedObjective) {
      setError(t("goal.editObjectiveRequired"));
      return;
    }
    const parsedIterations = Number.parseInt(maxIterations.trim(), 10);
    const validIterations =
      Number.isFinite(parsedIterations) && parsedIterations >= 1
        ? Math.min(200, Math.max(1, parsedIterations))
        : null;
    const criteriaLines = criteriaText
      .split(/\r?\n/)
      .map((line) => line.trim())
      .filter(Boolean);

    setSaving(true);
    setError(null);
    try {
      const updated = await orbitActions.updateGoalRun({
        hostId,
        goalId: goal.goal_id,
        objective: trimmedObjective,
        criteria: criteriaLines,
        verifyCommand: verifyCommand.trim(),
        maxIterations: validIterations,
      });
      if (!updated) {
        setError(t("goal.editSaveFailed"));
        setSaving(false);
        return;
      }
      orbitActions.showToast(t("goal.editSavedToast"));
      onClose();
    } catch (err) {
      setError(String(err));
      setSaving(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(open) => {
        if (!open && !saving) onClose();
      }}
    >
      <DialogContent className="sm:max-w-[520px]">
        <DialogHeader>
          <DialogTitle className="flex items-center gap-2 text-[15px]">
            <Target className="text-ink-secondary size-4 shrink-0" />
            <span>{t("goal.editTitle")}</span>
          </DialogTitle>
          <DialogDescription>{t("goal.editDescription")}</DialogDescription>
        </DialogHeader>

        <div className="space-y-3.5 py-1">
          <div className="space-y-1.5">
            <Label htmlFor="orbit-goal-objective">{t("goal.editObjectiveLabel")}</Label>
            <Textarea
              id="orbit-goal-objective"
              autoFocus
              rows={4}
              value={objective}
              disabled={saving}
              onChange={(e) => setObjective(e.target.value)}
              onKeyDown={(e) => {
                if ((e.metaKey || e.ctrlKey) && e.key === "Enter") {
                  e.preventDefault();
                  void handleSave();
                }
              }}
              placeholder={t("composer.placeholderGoal")}
              className="resize-y text-[13px] leading-[1.45]"
            />
          </div>

          <div className="space-y-1.5">
            <Label htmlFor="orbit-goal-criteria">{t("goal.editCriteriaLabel")}</Label>
            <Textarea
              id="orbit-goal-criteria"
              rows={3}
              value={criteriaText}
              disabled={saving}
              onChange={(e) => setCriteriaText(e.target.value)}
              placeholder={t("goal.editCriteriaPlaceholder")}
              className="resize-y text-[12.5px] leading-[1.45]"
            />
          </div>

          <div className="grid grid-cols-[1fr_116px] gap-3">
            <div className="space-y-1.5">
              <Label htmlFor="orbit-goal-verify">{t("goal.editVerifyLabel")}</Label>
              <Input
                id="orbit-goal-verify"
                value={verifyCommand}
                disabled={saving}
                onChange={(e) => setVerifyCommand(e.target.value)}
                placeholder={t("goal.editVerifyPlaceholder")}
                className="font-mono text-[12px]"
              />
            </div>
            <div className="space-y-1.5">
              <Label htmlFor="orbit-goal-max-turns">{t("goal.editMaxTurnsLabel")}</Label>
              <Input
                id="orbit-goal-max-turns"
                type="number"
                min={1}
                max={200}
                value={maxIterations}
                disabled={saving}
                onChange={(e) => setMaxIterations(e.target.value)}
                className="font-mono text-[12px] tabular-nums"
              />
            </div>
          </div>

          {error && (
            <div
              role="alert"
              className="border-signal-danger-border bg-signal-danger-bg text-signal-danger flex items-start gap-2 rounded-[8px] border px-2.5 py-2 text-[12px]"
            >
              <AlertCircle className="mt-0.5 size-3.5 shrink-0" />
              <span className="min-w-0 break-words">{error}</span>
            </div>
          )}
        </div>

        <DialogFooter>
          <Button type="button" variant="outline" size="sm" disabled={saving} onClick={onClose}>
            {t("common.cancel")}
          </Button>
          <Button type="button" size="sm" disabled={saving || !objective.trim()} onClick={() => void handleSave()}>
            {saving && <Loader2 className="mr-1.5 size-3.5 animate-spin" />}
            {t("goal.editSave")}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

export const GoalDialogs = memo(function GoalDialogs() {
  const { t } = useI18n();
  const editTarget = useOrbitSelector((s) => s.goalEditDialogTarget);
  const replaceTarget = useOrbitSelector((s) => s.goalReplaceConfirmTarget);

  return (
    <>
      {editTarget && (
        <EditGoalModal
          hostId={editTarget.hostId}
          goal={editTarget.goal}
          onClose={() => orbitActions.closeGoalEditDialog()}
        />
      )}
      <ConfirmDialog
        open={Boolean(replaceTarget)}
        onOpenChange={(open) => {
          if (!open) orbitActions.closeGoalReplaceConfirm();
        }}
        title={t("goal.replaceTitle")}
        description={t("goal.replaceDesc")}
        detail={
          replaceTarget ? (
            <div className="space-y-1.5 text-[12px] leading-[1.45]">
              <div className="text-ink-tertiary line-clamp-2">
                <span className="text-ink-muted mr-1.5 font-medium">{t("goal.replaceCurrentLabel")}</span>
                {replaceTarget.currentGoal.objective}
              </div>
              <div className="text-ink-primary line-clamp-2 font-medium">
                <span className="text-ink-secondary mr-1.5">{t("goal.replaceNewLabel")}</span>
                {replaceTarget.newObjective}
              </div>
            </div>
          ) : undefined
        }
        confirmLabel={t("goal.replaceConfirm")}
        onConfirm={async () => {
          const target = replaceTarget;
          orbitActions.closeGoalReplaceConfirm();
          if (target) {
            await target.onConfirm();
          }
        }}
      />
    </>
  );
});
