import * as React from "react";
import { cn } from "@/lib/utils";

export interface ListRowProps extends React.ComponentProps<"button"> {
  /** Compact (30px) for Overview & dense toolbars; default (34px) for Sidebar & Palette */
  density?: "compact" | "default";
  selected?: boolean;
  leading?: React.ReactNode;
  trailing?: React.ReactNode;
  subtitle?: React.ReactNode;
}

/**
 * Unified interactive list row primitive for Sidebar, Overview HUD, Command Palette, and Settings.
 */
export const ListRow = React.forwardRef<HTMLButtonElement, ListRowProps>(function ListRow(
  {
    className,
    density = "compact",
    selected = false,
    leading,
    trailing,
    subtitle,
    children,
    type = "button",
    ...props
  },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      data-selected={selected ? "true" : undefined}
      className={cn(
        "group/listrow flex w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 text-left transition-colors outline-none focus-visible:ring-1 focus-visible:ring-ring/45 disabled:pointer-events-none disabled:opacity-45",
        subtitle
          ? "min-h-[40px] py-1.5"
          : density === "compact"
            ? "h-[30px]"
            : "h-[34px]",
        selected
          ? "bg-accent text-ink-primary font-medium"
          : "text-ink-secondary hover:bg-accent/70 hover:text-ink-primary",
        className,
      )}
      {...props}
    >
      <span className="flex min-w-0 flex-1 items-center gap-2.5">
        {leading && (
          <span
            className={cn(
              "flex shrink-0 items-center justify-center transition-colors",
              selected
                ? "text-ink-primary"
                : "text-ink-tertiary group-hover/listrow:text-ink-primary",
            )}
          >
            {leading}
          </span>
        )}
        <span className="min-w-0 flex-1">
          <span className="block truncate text-[13px] leading-[18px]">{children}</span>
          {subtitle && (
            <span className="text-ink-tertiary block truncate text-[11px] leading-[15px] font-normal">
              {subtitle}
            </span>
          )}
        </span>
      </span>
      {trailing && <span className="flex shrink-0 items-center gap-1">{trailing}</span>}
    </button>
  );
});
