import { memo } from "react";
import { Check, X } from "lucide-react";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import type { HostConnectionState } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export type OrbitLifecycleStatus =
  | "running"
  | "awaiting"
  | "completed"
  | "failed"
  | "canceled"
  | "unread"
  | "idle";

/**
 * Single source of truth for status glyphs across Sidebar, Overview, Stage, and Settings.
 * Eliminates ad-hoc `Loader2` spinners and hand-coded status dots.
 */
export const StatusIndicator = memo(function StatusIndicator({
  status,
  size = "sm",
  className,
}: {
  status: OrbitLifecycleStatus;
  size?: "xs" | "sm" | "md";
  className?: string;
}) {
  const spinnerPx = size === "xs" ? 11 : size === "md" ? 15 : 13;
  const dotClass =
    size === "xs" ? "size-[5px]" : size === "md" ? "size-[7px]" : "size-[6px]";
  const iconClass =
    size === "xs" ? "size-[11px]" : size === "md" ? "size-[14px]" : "size-[12px]";

  const dotTransition =
    "transition-[background-color,opacity] duration-150 motion-reduce:transition-none";
  const iconTransition =
    "transition-[color,opacity] duration-150 motion-reduce:transition-none";

  switch (status) {
    case "running":
      return (
        <OrbitSpinner
          size={spinnerPx}
          className={cn("text-ink-secondary shrink-0", iconTransition, className)}
        />
      );
    case "awaiting":
      return (
        <span
          aria-hidden="true"
          className={cn("bg-signal-warning shimmer shrink-0 rounded-full", dotClass, dotTransition, className)}
        />
      );
    case "completed":
      return (
        <Check
          aria-hidden="true"
          className={cn("text-signal-success shrink-0 stroke-[2.2]", iconClass, iconTransition, className)}
        />
      );
    case "failed":
      return (
        <span
          aria-hidden="true"
          className={cn("bg-signal-danger shrink-0 rounded-full", dotClass, dotTransition, className)}
        />
      );
    case "canceled":
      return (
        <X
          aria-hidden="true"
          className={cn("text-ink-muted shrink-0 stroke-[2]", iconClass, iconTransition, className)}
        />
      );
    case "unread":
      return (
        <span
          aria-hidden="true"
          className={cn("bg-signal-info shrink-0 rounded-full", dotClass, dotTransition, className)}
        />
      );
    case "idle":
    default:
      return (
        <span
          aria-hidden="true"
          className={cn("bg-ink-muted/45 shrink-0 rounded-full", dotClass, dotTransition, className)}
        />
      );
  }
});

const HOST_STATE_META: Record<
  HostConnectionState,
  { label: string; dotClass: string; textClass: string; spinning?: boolean }
> = {
  online_direct: {
    label: "Direct",
    dotClass: "bg-signal-success",
    textClass: "text-signal-success",
  },
  online_relay: {
    label: "Relay",
    dotClass: "bg-signal-relay",
    textClass: "text-signal-relay",
  },
  connecting_direct: {
    label: "Connecting",
    dotClass: "bg-signal-warning",
    textClass: "text-signal-warning",
    spinning: true,
  },
  connecting_relay: {
    label: "Connecting relay",
    dotClass: "bg-signal-relay",
    textClass: "text-signal-relay",
    spinning: true,
  },
  disconnected: {
    label: "Offline",
    dotClass: "bg-ink-muted/55",
    textClass: "text-ink-tertiary",
  },
};

/**
 * Single canonical host connectivity indicator (replaces duplicate `HostStateDot` & `HostStatusIndicator`).
 */
export const HostStatusIndicator = memo(function HostStatusIndicator({
  state,
  showLabel = false,
  className,
}: {
  state: HostConnectionState;
  showLabel?: boolean;
  className?: string;
}) {
  const meta = HOST_STATE_META[state] ?? HOST_STATE_META.disconnected;

  return (
    <span
      title={meta.label}
      className={cn("inline-flex shrink-0 items-center gap-1.5", className)}
    >
      {meta.spinning ? (
        <OrbitSpinner size={11} className={cn("transition-[color,opacity] duration-150 motion-reduce:transition-none", meta.textClass)} />
      ) : (
        <span aria-hidden="true" className={cn("size-[6px] shrink-0 rounded-full transition-[background-color,opacity] duration-150 motion-reduce:transition-none", meta.dotClass)} />
      )}
      {showLabel && (
        <span className={cn("text-[12px] leading-none font-medium transition-colors duration-150 motion-reduce:transition-none", meta.textClass)}>
          {meta.label}
        </span>
      )}
    </span>
  );
});
