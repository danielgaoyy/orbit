import * as React from "react";
import { X } from "lucide-react";
import { IconButton } from "@/components/ui/icon-button";
import { cn } from "@/lib/utils";

export type ContextBannerVariant = "strip" | "inline";
export type ContextBannerTone = "neutral" | "info" | "warning" | "danger" | "success";

const TONE_STYLES: Record<ContextBannerTone, { strip: string; inline: string; icon: string }> = {
  neutral: {
    strip: "border-b border-border-subtle bg-surface-base text-ink-secondary",
    inline: "border border-border-subtle bg-surface-recessed/70 text-ink-secondary",
    icon: "text-ink-tertiary",
  },
  info: {
    strip: "border-b border-signal-info-border bg-signal-info-bg text-ink-primary",
    inline: "border border-signal-info-border bg-signal-info-bg text-ink-primary",
    icon: "text-signal-info",
  },
  warning: {
    strip: "border-b border-signal-warning-border bg-signal-warning-bg text-ink-primary",
    inline: "border border-signal-warning-border bg-signal-warning-bg text-ink-primary",
    icon: "text-signal-warning",
  },
  danger: {
    strip: "border-b border-signal-danger-border bg-signal-danger-bg text-ink-primary",
    inline: "border border-signal-danger-border bg-signal-danger-bg text-ink-primary",
    icon: "text-signal-danger",
  },
  success: {
    strip: "border-b border-signal-success-border bg-signal-success-bg text-ink-primary",
    inline: "border border-signal-success-border bg-signal-success-bg text-ink-primary",
    icon: "text-signal-success",
  },
};

export interface ContextBannerProps {
  variant?: ContextBannerVariant;
  tone?: ContextBannerTone;
  icon?: React.ReactNode;
  children: React.ReactNode;
  actions?: React.ReactNode;
  onDismiss?: () => void;
  dismissLabel?: string;
  className?: string;
}

/**
 * Single unified banner primitive replacing 6 bespoke status/context strips.
 * - `variant="strip"`: full-bleed top system bar (offline, reconnecting, daemon upgrade, peer strip)
 * - `variant="inline"`: stage-embedded subtle context chip (forked from, parent thread, notices)
 */
export function ContextBanner({
  variant = "strip",
  tone = "neutral",
  icon,
  children,
  actions,
  onDismiss,
  dismissLabel = "Dismiss",
  className,
}: ContextBannerProps) {
  const styles = TONE_STYLES[tone];

  return (
    <div
      role="status"
      className={cn(
        "@container orbit-stage-enter flex min-w-0 items-center justify-between gap-2 overflow-hidden text-[13px] transition-[background-color,border-color,color] duration-[var(--dur-ui)] ease-[var(--ease-out-quart)]",
        variant === "strip"
          ? cn("min-h-[30px] shrink-0 px-3 py-1 @[560px]:px-4", styles.strip)
          : cn("min-h-[32px] rounded-[10px] px-3 py-1.5", styles.inline),
        className,
      )}
    >
      <div className="flex min-w-0 flex-1 items-center gap-2">
        {icon && <span className={cn("flex shrink-0 items-center", styles.icon)}>{icon}</span>}
        <div className="min-w-0 flex-1 truncate">{children}</div>
      </div>

      {(actions || onDismiss) && (
        <div className="flex shrink-0 items-center gap-1">
          {actions}
          {onDismiss && (
            <IconButton
              size="xs"
              tooltip={dismissLabel}
              aria-label={dismissLabel}
              onClick={onDismiss}
            >
              <X className="size-3" />
            </IconButton>
          )}
        </div>
      )}
    </div>
  );
}
