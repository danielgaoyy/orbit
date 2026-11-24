import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { ChevronsDownUp, ChevronsUpDown } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import type { StageTurnView } from "@/lib/orbit-ipc";
import { isCompactionMarkerTurn } from "./stage-helpers";

/**
 * Cohesive per-session turn collapse controller for `StreamStage`:
 * - Lets users fold individual completed turns or collapse all earlier turns in 1 click.
 * - Automatically expands any collapsed turn when `StageFindBar` matches inside it.
 */
export function useTurnCollapse({
  stageTurns,
  findOpen,
  matchedTurnKeys,
  activeFindTurnKey,
  onAutoExpandedForFind,
}: {
  stageTurns: readonly StageTurnView[];
  findOpen: boolean;
  matchedTurnKeys: ReadonlySet<string>;
  activeFindTurnKey: string | null;
  onAutoExpandedForFind: () => void;
}) {
  const [collapsedTurnKeys, setCollapsedTurnKeys] = useState<ReadonlySet<string>>(() => new Set());

  const collapsibleEarlierTurnKeys = useMemo(() => {
    if (stageTurns.length <= 1) return [] as string[];
    const keys: string[] = [];
    for (let i = 0; i < stageTurns.length - 1; i += 1) {
      const turn = stageTurns[i];
      if (!isCompactionMarkerTurn(turn)) {
        keys.push(turn.key);
      }
    }
    return keys;
  }, [stageTurns]);

  const toggleTurn = useCallback((turnKey: string) => {
    setCollapsedTurnKeys((prev) => {
      const next = new Set(prev);
      if (next.has(turnKey)) next.delete(turnKey);
      else next.add(turnKey);
      return next;
    });
  }, []);

  const expandTurn = useCallback((turnKey: string | null | undefined) => {
    if (!turnKey) return;
    setCollapsedTurnKeys((prev) => {
      if (!prev.has(turnKey)) return prev;
      const next = new Set(prev);
      next.delete(turnKey);
      return next;
    });
  }, []);

  const collapseEarlier = useCallback(() => {
    if (collapsibleEarlierTurnKeys.length === 0) return;
    setCollapsedTurnKeys((prev) => {
      const next = new Set(prev);
      for (const k of collapsibleEarlierTurnKeys) next.add(k);
      return next;
    });
  }, [collapsibleEarlierTurnKeys]);

  const expandAll = useCallback(() => {
    setCollapsedTurnKeys((prev) => (prev.size === 0 ? prev : new Set()));
  }, []);

  useEffect(() => {
    if (!findOpen) return;
    if (matchedTurnKeys.size === 0 && !activeFindTurnKey) return;
    setCollapsedTurnKeys((prev) => {
      if (prev.size === 0) return prev;
      let changed = false;
      const next = new Set(prev);
      for (const k of matchedTurnKeys) {
        if (next.delete(k)) changed = true;
      }
      if (activeFindTurnKey && next.delete(activeFindTurnKey)) {
        changed = true;
      }
      if (changed) {
        window.requestAnimationFrame(() => onAutoExpandedForFind());
      }
      return changed ? next : prev;
    });
  }, [findOpen, matchedTurnKeys, activeFindTurnKey, onAutoExpandedForFind]);

  return {
    collapsedTurnKeys,
    collapsibleEarlierTurnKeys,
    toggleTurn,
    expandTurn,
    collapseEarlier,
    expandAll,
  };
}

export const TurnCollapseUtilityBar = memo(function TurnCollapseUtilityBar({
  collapsibleEarlierTurnKeys,
  collapsedTurnKeys,
  onCollapseEarlier,
  onExpandAll,
}: {
  collapsibleEarlierTurnKeys: readonly string[];
  collapsedTurnKeys: ReadonlySet<string>;
  onCollapseEarlier: () => void;
  onExpandAll: () => void;
}) {
  const { t } = useI18n();
  if (collapsibleEarlierTurnKeys.length < 2 && collapsedTurnKeys.size === 0) {
    return null;
  }
  const hasUncollapsedEarlier = collapsibleEarlierTurnKeys.some((k) => !collapsedTurnKeys.has(k));

  return (
    <div className="mb-3 flex items-center justify-end gap-2 px-[6px]">
      {hasUncollapsedEarlier && (
        <button
          type="button"
          onClick={onCollapseEarlier}
          className="text-ink-muted hover:text-ink-primary hover:bg-accent/60 inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-[6px] px-2 text-[11px] font-medium transition-colors"
        >
          <ChevronsDownUp className="size-3" />
          <span>
            {t("stage.collapseEarlierTurns", { count: collapsibleEarlierTurnKeys.length })}
          </span>
        </button>
      )}
      {collapsedTurnKeys.size > 0 && (
        <button
          type="button"
          onClick={onExpandAll}
          className="text-ink-tertiary hover:text-ink-primary hover:bg-accent/60 inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-[6px] px-2 text-[11px] font-medium transition-colors"
        >
          <ChevronsUpDown className="size-3" />
          <span>{t("stage.expandAllTurns", { count: collapsedTurnKeys.size })}</span>
        </button>
      )}
    </div>
  );
});
