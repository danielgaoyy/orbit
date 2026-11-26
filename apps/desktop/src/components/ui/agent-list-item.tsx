import { memo, type MouseEvent, type ReactNode } from "react";
import { ProviderIcon } from "@/components/ui/provider-icons";
import {
  SubagentAvatar,
  type SubagentAvatarSize,
  type SubagentAvatarStatus,
} from "@/components/ui/subagent-avatar";
import {
  type AgentListItemStatus,
  formatAgentListItemStatusLabel,
  isAgentListItemActiveStatus,
} from "@/lib/agent-list-item";
import { useI18n } from "@/lib/i18n";
import type { ProviderKind } from "@/lib/orbit-ipc";
import type { SubagentCharacterSpec } from "@/lib/subagent-avatar";
import { cn } from "@/lib/utils";

export type { AgentListItemStatus };

export interface AgentListItemProps {
  /** Primary display title (single-line truncated) */
  title: string;
  /** Unified lifecycle status driving portrait animation, status badge, highlight, and default label */
  status: AgentListItemStatus;
  /** Pre-assigned collision-free animal portrait (falls back to deterministic `avatarSeedKey`) */
  character?: SubagentCharacterSpec | null;
  /** Deterministic seed (typically `agentId`) when `character` is not pre-assigned */
  avatarSeedKey?: string;
  /** Portrait size (defaults to "sm" when indented, "md" otherwise) */
  avatarSize?: SubagentAvatarSize;
  /** Show corner status badge on terminal states (defaults to `!isActive`) */
  showStatusBadge?: boolean;

  /** Provider engine icon rendered at 11px before model/agent metadata */
  provider?: ProviderKind | null;
  /** Formatted compact model label (e.g. "Gemini 4 Neon", "Sonnet 4.6") */
  modelLabel?: string | null;
  /** Formatted thinking/reasoning effort (e.g. "Extra High") — paired with model without a dot */
  effortLabel?: string | null;
  /** Fallback agent/provider label (e.g. "Antigravity", "Claude Code") when model is unknown or hidden */
  agentLabel?: string | null;
  /** Prefer `agentLabel` over `modelLabel` (e.g. deep nested subagents `depth >= 4`) */
  preferAgentLabel?: boolean;
  /** Custom localized status text (defaults to canonical status label for `status`) */
  statusLabel?: string;
  /** Semantic tone override for status label and surface accent */
  tone?: "default" | "warning" | "danger";
  /** Optional trailing metadata token after status (e.g. remote host name) */
  secondaryMeta?: string | null;

  /** Optional compact right-aligned element on the title row (e.g. DiffStat, counter, elapsed chip) */
  titleTrailing?: ReactNode;
  /** Optional sibling trailing control outside the main click trigger (avoids nested buttons) */
  trailing?: ReactNode;
  /** Optional composite footer rendered inside `variant="card"` (e.g. approval summary + action buttons) */
  footer?: ReactNode;

  /** Nesting indentation level (0 = root, 1..5 = indented child with subtle left hairline) */
  indentLevel?: number;
  /** Highlight row as currently selected/focused */
  selected?: boolean;
  /** Surface treatment: "row" (default soft hover row) or "card" (bordered container for composite cards) */
  variant?: "row" | "card";
  /** Native tooltip string */
  tooltip?: string;
  /** Accessible label override for the primary trigger */
  ariaLabel?: string;
  /** Primary activation click handler */
  onClick?: (e: MouseEvent<HTMLButtonElement>) => void;
  /** Extra outer container className (layout/spacing) */
  className?: string;
  /** Extra inner row className */
  rowClassName?: string;
}

/**
 * Single canonical desktop list-item component for all running, awaiting, idle, and settled
 * agents, subagents, workers, and group peers across Session Overview HUD, Personal Agent
 * Briefing Rail, and Group Notes.
 *
 * Core visual grammar (strict 2-line layout):
 *   [Circular Animal Portrait + corner status badge]
 *   Line 1: Single-line truncated Title (+ optional compact titleTrailing)
 *   Line 2: [ProviderIcon] [Model Effort | Agent] · [Semantic Status] (· optional secondaryMeta)
 *   Extra context (step previews, worktree branch, lineage) belongs in hover `tooltip`, never a 3rd line.
 */
export const AgentListItem = memo(function AgentListItem({
  title,
  status,
  character,
  avatarSeedKey,
  avatarSize,
  showStatusBadge,
  provider,
  modelLabel,
  effortLabel,
  agentLabel,
  preferAgentLabel = false,
  statusLabel,
  tone,
  secondaryMeta,
  titleTrailing,
  trailing,
  footer,
  indentLevel = 0,
  selected = false,
  variant = "row",
  tooltip,
  ariaLabel,
  onClick,
  className,
  rowClassName,
}: AgentListItemProps) {
  const { t } = useI18n();
  const clampedIndent = Math.max(0, Math.min(indentLevel, 5));
  const isActive = isAgentListItemActiveStatus(status);
  const resolvedTone =
    tone ??
    (status === "awaiting_permission"
      ? "warning"
      : status === "failed"
        ? "danger"
        : "default");
  const isWarning = resolvedTone === "warning";
  const isDanger = resolvedTone === "danger";
  const resolvedAvatarSize: SubagentAvatarSize =
    avatarSize ?? (clampedIndent > 0 ? "sm" : "md");
  const resolvedShowBadge = showStatusBadge ?? !isActive;
  const resolvedStatusLabel = statusLabel ?? formatAgentListItemStatusLabel(status, t);
  const showModel = Boolean(modelLabel) && !preferAgentLabel;

  const coreContent = (
    <>
      <SubagentAvatar
        character={character}
        seedKey={avatarSeedKey ?? title}
        status={status as SubagentAvatarStatus}
        size={resolvedAvatarSize}
        showStatusBadge={resolvedShowBadge}
      />
      <span data-slot="agent-list-item-lines" className="min-w-0 flex-1">
        {titleTrailing ? (
          <span className="flex min-w-0 items-center justify-between gap-1.5">
            <span className="text-ink-primary min-w-0 flex-1 truncate text-[13px] leading-[18px] font-medium">
              {title}
            </span>
            <span className="flex shrink-0 items-center gap-1.5">{titleTrailing}</span>
          </span>
        ) : (
          <span className="text-ink-primary block truncate text-[13px] leading-[18px] font-medium">
            {title}
          </span>
        )}
        <span className="text-ink-tertiary flex min-w-0 items-center gap-1 overflow-hidden text-[11px] leading-[15px]">
          {provider && (
            <ProviderIcon
              provider={provider}
              className="size-[11px] shrink-0 opacity-85"
            />
          )}
          {showModel ? (
            <>
              <span className="min-w-0 truncate">{modelLabel}</span>
              {effortLabel && (
                <span className="text-ink-muted shrink-0">{effortLabel}</span>
              )}
              <span aria-hidden="true" className="text-ink-faint shrink-0">
                ·
              </span>
            </>
          ) : agentLabel ? (
            <>
              <span className="shrink-0">{agentLabel}</span>
              {effortLabel && (
                <span className="text-ink-muted shrink-0">{effortLabel}</span>
              )}
              <span aria-hidden="true" className="text-ink-faint shrink-0">
                ·
              </span>
            </>
          ) : null}
          <span
            className={cn(
              "shrink-0",
              isWarning
                ? "text-signal-warning font-medium"
                : isDanger
                  ? "text-signal-danger"
                  : undefined,
            )}
          >
            {resolvedStatusLabel}
          </span>
          {secondaryMeta && (
            <>
              <span aria-hidden="true" className="text-ink-faint shrink-0">
                ·
              </span>
              <span className="min-w-0 truncate">{secondaryMeta}</span>
            </>
          )}
        </span>
      </span>
    </>
  );

  if (variant === "card") {
    const headerRowClass = cn(
      "flex w-full min-w-0 items-center gap-2.5 text-left outline-none",
      onClick &&
        "cursor-pointer rounded-[8px] -mx-1 -mt-1 px-1 py-1 transition-colors hover:bg-accent/55 focus-visible:ring-2 focus-visible:ring-ring/50",
      rowClassName,
    );

    return (
      <div
        data-slot="agent-list-item"
        data-variant="card"
        data-status={status}
        className={cn(
          "group/agentrow border-border-default bg-surface-base/70 hover:border-border-strong flex min-w-0 flex-col gap-2 rounded-xl border p-2.5 transition-colors",
          isDanger && "border-signal-danger-border/75",
          className,
        )}
      >
        <div className="flex min-w-0 items-center gap-1.5">
          {onClick ? (
            <button
              type="button"
              title={tooltip}
              aria-label={ariaLabel}
              onClick={onClick}
              className={headerRowClass}
            >
              {coreContent}
            </button>
          ) : (
            <div title={tooltip} className={headerRowClass}>
              {coreContent}
            </div>
          )}
          {trailing && <div className="flex shrink-0 items-center gap-1">{trailing}</div>}
        </div>
        {footer}
      </div>
    );
  }

  const rowSurfaceClass = cn(
    "group/agentrow flex min-h-[42px] w-full items-center overflow-hidden rounded-[8px] px-2 py-1.5 text-left transition-colors",
    clampedIndent > 0 && "border-l border-border-subtle/80 rounded-l-[4px]",
    isWarning
      ? "bg-signal-warning-bg/75 hover:bg-signal-warning-bg"
      : selected
        ? "bg-accent"
        : "hover:bg-accent/70",
  );

  const rowNode = trailing ? (
    <div
      data-slot="agent-list-item"
      data-variant="row"
      data-status={status}
      data-selected={selected ? "true" : undefined}
      className={cn(rowSurfaceClass, "gap-1.5", rowClassName)}
    >
      {onClick ? (
        <button
          type="button"
          title={tooltip}
          aria-label={ariaLabel}
          onClick={onClick}
          className="focus-visible:ring-ring/50 flex min-w-0 flex-1 cursor-pointer items-center gap-2.5 rounded-[6px] text-left outline-none focus-visible:ring-2"
        >
          {coreContent}
        </button>
      ) : (
        <div
          title={tooltip}
          className="flex min-w-0 flex-1 items-center gap-2.5"
        >
          {coreContent}
        </div>
      )}
      <div className="flex shrink-0 items-center gap-1">{trailing}</div>
    </div>
  ) : onClick ? (
    <button
      type="button"
      data-slot="agent-list-item"
      data-variant="row"
      data-status={status}
      data-selected={selected ? "true" : undefined}
      title={tooltip}
      aria-label={ariaLabel}
      onClick={onClick}
      className={cn(
        rowSurfaceClass,
        "focus-visible:ring-ring/50 cursor-pointer gap-2.5 outline-none focus-visible:ring-2",
        rowClassName,
      )}
    >
      {coreContent}
    </button>
  ) : (
    <div
      data-slot="agent-list-item"
      data-variant="row"
      data-status={status}
      data-selected={selected ? "true" : undefined}
      title={tooltip}
      className={cn(rowSurfaceClass, "gap-2.5", rowClassName)}
    >
      {coreContent}
    </div>
  );

  if (clampedIndent > 0 || className) {
    return (
      <div
        style={clampedIndent > 0 ? { paddingLeft: `${clampedIndent * 14}px` } : undefined}
        className={cn("min-w-0", className)}
      >
        {rowNode}
      </div>
    );
  }

  return rowNode;
});
