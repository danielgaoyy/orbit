import { memo, type ReactNode } from "react";
import { AlarmClock, ChevronRight, X } from "lucide-react";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface StageRailHeaderProps {
  title: string;
  leading?: ReactNode;
  actions?: ReactNode;
  closeTooltip: string;
  closeShortcut?: string;
  onClose: () => void;
  /** Optional fixed quick-action bar rendered inside the non-scrolling rail header (e.g. Overview tools). */
  children?: ReactNode;
  className?: string;
}

/**
 * Shared fixed top header for 304px right-side stage rails (`StageOverviewPanel` and
 * `PersonalAgentBriefingRail`), guaranteeing identical height, padding, title typography,
 * and close-button ergonomics.
 */
export const StageRailHeader = memo(function StageRailHeader({
  title,
  leading,
  actions,
  closeTooltip,
  closeShortcut,
  onClose,
  children,
  className,
}: StageRailHeaderProps) {
  return (
    <div className={cn("shrink-0 px-2 pt-2 pb-1.5 select-none", className)}>
      <div
        data-slot="stage-rail-header"
        className={cn(
          "flex h-[26px] items-center justify-between pr-0.5 pl-2",
          children && "pb-0.5",
        )}
      >
        <span className="text-ink-primary flex min-w-0 items-center gap-1.5 text-[13px] font-medium">
          {leading && <span className="flex shrink-0 items-center">{leading}</span>}
          <span className="truncate">{title}</span>
        </span>
        <div className="flex shrink-0 items-center gap-0.5">
          {actions}
          <IconButton
            size="xs"
            tooltip={closeTooltip}
            shortcut={closeShortcut}
            aria-label={closeTooltip}
            onClick={onClose}
            className="text-ink-muted hover:text-ink-primary"
          >
            <X className="size-3 stroke-[1.85]" />
          </IconButton>
        </div>
      </div>
      {children}
    </div>
  );
});

/**
 * Shared scrollable body region beneath `StageRailHeader` with pinned 1px top divider.
 */
export const StageRailScrollBody = memo(function StageRailScrollBody({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      data-slot="stage-rail-scroll-body"
      className={cn(
        "orbit-native-scroll border-border-subtle min-h-0 shrink overflow-y-auto overscroll-contain border-t select-none",
        className,
      )}
    >
      {children}
    </div>
  );
});

export interface StageRailSectionProps {
  label: string;
  count?: number;
  collapsed: boolean;
  onOpenChange: (open: boolean) => void;
  tone?: "default" | "warning";
  isRunning?: boolean;
  hasAlert?: boolean;
  runningTone?: "spinner" | "emerald" | "info" | "timer";
  trailingMeta?: ReactNode;
  headerAction?: ReactNode;
  headerActionVisibility?: "hover" | "always";
  footer?: ReactNode;
  children: ReactNode;
}

/**
 * Single canonical collapsible section primitive shared by `StageOverviewPanel` (`OverviewSection`)
 * and `PersonalAgentBriefingRail`, uniting eyebrow typography, mono counters, collapsed live-status
 * indicators, hover-revealed header actions, and chevron motion.
 */
export const StageRailSection = memo(function StageRailSection({
  label,
  count,
  collapsed,
  onOpenChange,
  tone = "default",
  isRunning = false,
  hasAlert = false,
  runningTone = "spinner",
  trailingMeta,
  headerAction,
  headerActionVisibility = "hover",
  footer,
  children,
}: StageRailSectionProps) {
  const { t } = useI18n();
  const hoverReplacesMeta = Boolean(headerAction && headerActionVisibility === "hover");

  const collapsedStatusIndicator = collapsed && (hasAlert || isRunning) && (
    <span className="flex items-center">
      {hasAlert ? (
        <span className="bg-signal-warning size-[6px] shrink-0 rounded-full" />
      ) : runningTone === "emerald" ? (
        <span className="bg-signal-success size-[6px] shrink-0 rounded-full" />
      ) : runningTone === "info" ? (
        <span className="bg-signal-info size-[6px] shrink-0 rounded-full" />
      ) : runningTone === "timer" ? (
        <AlarmClock className="text-ink-faint size-[11.5px] shrink-0 stroke-[1.7]" />
      ) : (
        <OrbitSpinner size={12} className="text-ink-secondary" />
      )}
    </span>
  );

  return (
    <Collapsible
      open={!collapsed}
      onOpenChange={onOpenChange}
      data-slot="stage-rail-section"
      className={cn(
        "border-border-subtle border-t px-2 pt-1.5 first:border-t-0",
        !collapsed && footer ? "pb-0" : "pb-1.5",
      )}
    >
      <div className="group/sechead hover:bg-accent/65 flex h-[28px] w-full items-center rounded-[8px] pr-1 transition-colors">
        <CollapsibleTrigger className="group/trigger focus-visible:ring-ring/50 flex h-full min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-[8px] pr-1 pl-2 text-left outline-none focus-visible:ring-2">
          <span
            className={cn(
              "truncate text-[11px] font-medium tracking-[0.03em] uppercase transition-colors",
              tone === "warning"
                ? "text-signal-warning"
                : "text-ink-tertiary group-hover/sechead:text-ink-primary",
            )}
          >
            {label}
          </span>
          {typeof count === "number" && (
            <span
              className={cn(
                "font-mono text-[11px] tabular-nums",
                tone === "warning" ? "text-signal-warning/85 font-medium" : "text-ink-muted",
              )}
            >
              {count}
            </span>
          )}
        </CollapsibleTrigger>

        <div className="flex shrink-0 items-center gap-1">
          {hoverReplacesMeta ? (
            <div className="grid items-center justify-items-end">
              {(trailingMeta || collapsedStatusIndicator) && (
                <div className="col-start-1 row-start-1 flex items-center gap-1 transition-opacity duration-150 motion-reduce:transition-none group-focus-within/sechead:pointer-events-none group-focus-within/sechead:opacity-0 group-hover/sechead:pointer-events-none group-hover/sechead:opacity-0 [@media(hover:none)]:pointer-events-none [@media(hover:none)]:opacity-0">
                  {trailingMeta && <span className="flex items-center">{trailingMeta}</span>}
                  {collapsedStatusIndicator}
                </div>
              )}
              <div className="pointer-events-none col-start-1 row-start-1 flex items-center opacity-0 transition-opacity duration-150 motion-reduce:transition-none group-focus-within/sechead:pointer-events-auto group-focus-within/sechead:opacity-100 group-hover/sechead:pointer-events-auto group-hover/sechead:opacity-100 [@media(hover:none)]:pointer-events-auto [@media(hover:none)]:opacity-100">
                {headerAction}
              </div>
            </div>
          ) : (
            <>
              {headerAction && <div className="flex items-center">{headerAction}</div>}
              {trailingMeta && <span className="flex items-center">{trailingMeta}</span>}
              {collapsedStatusIndicator}
            </>
          )}
          <CollapsibleTrigger
            aria-label={t("overview.toggleSection", { label })}
            className="group/trigger text-ink-muted inline-flex size-[20px] cursor-pointer items-center justify-center rounded-[6px] outline-none"
          >
            <ChevronRight className="size-3 shrink-0 transition-transform duration-[180ms] ease-[var(--ease-panel)] motion-reduce:transition-none group-data-[panel-open]/trigger:rotate-90" />
          </CollapsibleTrigger>
        </div>
      </div>

      <CollapsibleContent>
        <div className="pt-1">{children}</div>
        {footer}
      </CollapsibleContent>
    </Collapsible>
  );
});

/**
 * Shared overflow disclosure footer (`Show N more` / `Show less`) with sticky collapse bar
 * when expanded.
 */
export const StageRailExpandFooter = memo(function StageRailExpandFooter({
  expanded,
  hiddenCount,
  onToggle,
}: {
  expanded: boolean;
  hiddenCount: number;
  onToggle: () => void;
}) {
  const { t } = useI18n();
  return (
    <div
      data-slot="stage-rail-expand-footer"
      className={cn(
        expanded
          ? "bg-[var(--overview-surface,var(--surface-raised))] sticky bottom-0 z-[2] px-2 pt-1 pb-1.5"
          : "px-2 pt-0.5 pb-1.5",
      )}
    >
      <button
        type="button"
        onClick={onToggle}
        className="text-ink-tertiary hover:text-ink-primary inline-flex cursor-pointer items-center py-0.5 text-[11px] font-medium transition-colors"
      >
        {expanded ? t("overview.showLess") : t("overview.showMore", { count: hiddenCount })}
      </button>
    </div>
  );
});

/**
 * Shared quiet empty state for stage rails when no work sections are active.
 */
export const StageRailEmptyState = memo(function StageRailEmptyState({
  children,
  className,
}: {
  children: ReactNode;
  className?: string;
}) {
  return (
    <div
      data-slot="stage-rail-empty-state"
      className={cn("text-ink-tertiary px-3 py-4 text-[13px]", className)}
    >
      {children}
    </div>
  );
});
