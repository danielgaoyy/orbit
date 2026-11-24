import * as React from "react";
import { Progress as BaseProgress } from "@base-ui/react/progress";
import { cn } from "@/lib/utils";

export interface ProgressProps
  extends Omit<React.ComponentProps<typeof BaseProgress.Root>, "className" | "children"> {
  value: number;
  max?: number;
  className?: string;
  indicatorClassName?: string;
  /** Optional overlays rendered inside the track (e.g. threshold tick marks at 70% / 85%) */
  trackOverlay?: React.ReactNode;
}

/**
 * Official shadcn/ui `<Progress>` primitive powered by `@base-ui/react/progress`.
 */
export function Progress({
  value,
  max = 100,
  className,
  indicatorClassName,
  trackOverlay,
  ...props
}: ProgressProps) {
  const safeMax = max > 0 ? max : 100;
  const clamped = Math.min(safeMax, Math.max(0, Number.isFinite(value) ? value : 0));
  const ratio = clamped / safeMax;

  return (
    <BaseProgress.Root
      data-slot="progress"
      value={clamped}
      max={safeMax}
      className="w-full"
      {...props}
    >
      <BaseProgress.Track
        data-slot="progress-track"
        className={cn(
          "bg-surface-recessed relative h-[6px] w-full overflow-hidden rounded-full",
          className,
        )}
      >
        <BaseProgress.Indicator
          data-slot="progress-indicator"
          style={{ width: "100%", transform: `scaleX(${ratio})` }}
          className={cn(
            "bg-ink-primary/80 h-full w-full origin-left rounded-full transition-transform duration-200 ease-[var(--ease-out-expo)]",
            indicatorClassName,
          )}
        />
        {trackOverlay}
      </BaseProgress.Track>
    </BaseProgress.Root>
  );
}
