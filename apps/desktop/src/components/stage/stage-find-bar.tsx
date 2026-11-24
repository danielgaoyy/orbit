import { memo, useEffect, useRef, useState } from "react";
import { ChevronDown, ChevronUp, History, Search, X } from "lucide-react";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { cn } from "@/lib/utils";

export interface StageFindBarProps {
  open: boolean;
  focusNonce: number;
  query: string;
  onQueryChange: (nextQuery: string) => void;
  activeIndex: number;
  totalMatches: number;
  hasUnloadedMatches: boolean;
  isActiveMatchUnloaded: boolean;
  isRemoteSearching: boolean;
  isPaginatingJump: boolean;
  overviewReservePx: number;
  overviewReserveTransition?: string;
  onStep: (delta: 1 | -1) => void;
  onClose: () => void;
}

const EXIT_ANIMATION_MS = 145;

/**
 * Floating capsule Find Bar anchored top-right inside `StreamStage` (Orbital Quiet Precision).
 * - Zero layout shift: positioned absolutely above the scroll viewport and tracks `overviewReservePx`.
 * - Unified counter (`3 / 12`) spanning instant loaded matches + remote daemon full-history matches.
 * - `Enter` / `↓` next match, `Shift+Enter` / `↑` previous match, `Esc` closes and restores focus.
 */
export const StageFindBar = memo(function StageFindBar({
  open,
  focusNonce,
  query,
  onQueryChange,
  activeIndex,
  totalMatches,
  hasUnloadedMatches,
  isActiveMatchUnloaded,
  isRemoteSearching,
  isPaginatingJump,
  overviewReservePx,
  overviewReserveTransition,
  onStep,
  onClose,
}: StageFindBarProps) {
  const { t } = useI18n();
  const ime = useImeCompositionGuard();
  const inputRef = useRef<HTMLInputElement | null>(null);
  const [rendered, setRendered] = useState(open);
  const [exiting, setExiting] = useState(false);

  useEffect(() => {
    if (open) {
      setRendered(true);
      setExiting(false);
      return;
    }
    if (!rendered) return;
    const prefersReducedMotion =
      typeof window !== "undefined" &&
      window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
    if (prefersReducedMotion) {
      setRendered(false);
      setExiting(false);
      return;
    }
    setExiting(true);
    const timer = window.setTimeout(() => {
      setRendered(false);
      setExiting(false);
    }, EXIT_ANIMATION_MS);
    return () => window.clearTimeout(timer);
  }, [open, rendered]);

  // Autofocus & select existing query on open or repeated `⌘F`
  useEffect(() => {
    if (!open) return;
    const raf = window.requestAnimationFrame(() => {
      const el = inputRef.current;
      if (!el) return;
      el.focus({ preventScroll: true });
      el.select();
    });
    return () => window.cancelAnimationFrame(raf);
  }, [open, focusNonce]);

  if (!rendered) return null;

  const trimmed = query.trim();
  const hasQuery = trimmed.length > 0;
  const isBusy = isRemoteSearching || isPaginatingJump;
  const canNavigate = totalMatches > 0;
  const displayPos = canNavigate ? Math.min(Math.max(activeIndex, 0) + 1, totalMatches) : 0;
  const noResultsSettled = hasQuery && totalMatches === 0 && !isBusy;

  const handleKeyDown = (e: React.KeyboardEvent<HTMLInputElement>) => {
    if (ime.shouldIgnoreKeyDown(e)) return;

    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      onClose();
      return;
    }

    if (e.key === "Enter" || e.key === "ArrowDown") {
      e.preventDefault();
      if (canNavigate) {
        onStep(e.key === "Enter" && e.shiftKey ? -1 : 1);
      }
      return;
    }

    if (e.key === "ArrowUp") {
      e.preventDefault();
      if (canNavigate) {
        onStep(-1);
      }
    }
  };

  return (
    <div
      role="search"
      aria-label={t("stage.findInConversation")}
      data-orbit-stage-find="true"
      data-orbit-find-ignore="true"
      style={{
        right: `${16 + overviewReservePx}px`,
        transition: overviewReserveTransition,
      }}
      className={cn(
        "floating-surface absolute top-3 z-30 flex h-[36px] w-[336px] max-w-[calc(100%-32px)] items-center gap-1.5 rounded-xl border px-2.5",
        exiting ? "orbit-find-bar-exit pointer-events-none" : "orbit-find-bar-enter",
      )}
    >
      <Search className="text-ink-tertiary size-[14px] shrink-0" aria-hidden="true" />

      <input
        ref={inputRef}
        type="text"
        value={query}
        onChange={(e) => onQueryChange(e.target.value)}
        {...ime.compositionProps}
        onKeyDown={handleKeyDown}
        placeholder={t("stage.findPlaceholder")}
        aria-label={t("stage.findPlaceholder")}
        spellCheck={false}
        autoCorrect="off"
        className="text-ink-primary placeholder:text-ink-muted min-w-0 flex-1 bg-transparent text-[13px] leading-[1.45] outline-none"
      />

      {hasQuery && (
        <div className="flex shrink-0 items-center gap-1.5">
          {isBusy && (
            <span
              title={
                isPaginatingJump
                  ? t("stage.findLoadingHistoryTurn")
                  : t("stage.findSearchingHistory")
              }
              className="text-ink-tertiary inline-flex items-center"
            >
              <OrbitSpinner size={12} />
            </span>
          )}
          {!isBusy && (isActiveMatchUnloaded || hasUnloadedMatches) && (
            <span
              title={t("stage.findIncludesEarlierHistory")}
              className="text-ink-muted inline-flex items-center"
            >
              <History className="size-[11px]" aria-hidden="true" />
            </span>
          )}
          <span
            aria-live="polite"
            className={cn(
              "font-mono text-[11px] leading-[1.35] tabular-nums select-none",
              noResultsSettled ? "text-signal-danger" : "text-ink-tertiary",
            )}
          >
            {canNavigate ? `${displayPos} / ${totalMatches}` : isBusy ? "…" : "0 / 0"}
          </span>
        </div>
      )}

      <span
        aria-hidden="true"
        className="bg-border-subtle mx-0.5 h-[14px] w-px shrink-0"
      />

      <div className="flex shrink-0 items-center gap-0.5">
        <IconButton
          size="xs"
          tooltip={t("stage.findPrevious")}
          shortcut="⇧↵"
          disabled={!canNavigate}
          onClick={() => onStep(-1)}
        >
          <ChevronUp />
        </IconButton>
        <IconButton
          size="xs"
          tooltip={t("stage.findNext")}
          shortcut="↵"
          disabled={!canNavigate}
          onClick={() => onStep(1)}
        >
          <ChevronDown />
        </IconButton>
        <IconButton
          size="xs"
          tooltip={t("common.close")}
          shortcut="Esc"
          onClick={onClose}
        >
          <X />
        </IconButton>
      </div>
    </div>
  );
});
