import { memo } from "react";
import { cn } from "@/lib/utils";

/**
 * Canonical monospace diff stat display (`+N -M`) used consistently across
 * Overview HUD, Turn Changes Bar, Diff Review, and Touched Files.
 */
export const DiffStat = memo(function DiffStat({
  additions,
  deletions,
  showZero = false,
  size = "sm",
  className,
}: {
  additions: number;
  deletions: number;
  showZero?: boolean;
  size?: "xs" | "sm";
  className?: string;
}) {
  if (!showZero && additions === 0 && deletions === 0) return null;

  return (
    <span
      className={cn(
        "tabular-nums inline-flex shrink-0 items-center gap-1 font-mono font-medium",
        size === "xs" ? "text-[11px]" : "text-[11px]",
        className,
      )}
    >
      {(additions > 0 || showZero) && (
        <span className="text-signal-success">+{additions}</span>
      )}
      {(deletions > 0 || showZero) && (
        <span className="text-signal-danger">-{deletions}</span>
      )}
    </span>
  );
});
