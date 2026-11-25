import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { SimpleTooltip } from "@/components/ui/overlays";
import { cn } from "@/lib/utils";

const iconButtonVariants = cva(
  "orbit-press inline-flex shrink-0 cursor-pointer items-center justify-center rounded-[6px] outline-none focus-visible:ring-1 focus-visible:ring-ring/45 disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0",
  {
    variants: {
      size: {
        xs: "size-[22px] [&_svg]:size-[13px]",
        sm: "size-[26px] [&_svg]:size-[14px]",
        md: "size-[28px] [&_svg]:size-[15px]",
      },
      tone: {
        default: "text-ink-tertiary hover:text-ink-primary hover:bg-accent",
        subtle: "text-ink-muted hover:text-ink-primary hover:bg-accent",
        danger: "text-ink-tertiary hover:text-signal-danger hover:bg-signal-danger-bg",
        active: "bg-accent text-ink-primary",
      },
    },
    defaultVariants: {
      size: "md",
      tone: "default",
    },
  },
);

export interface IconButtonProps
  extends Omit<React.ComponentProps<"button">, "title">,
    VariantProps<typeof iconButtonVariants> {
  /** Highlight button as active / pressed toggle */
  active?: boolean;
  /** Optional tooltip text; when provided, wraps button in a unified SimpleTooltip */
  tooltip?: string;
  /** Optional keyboard shortcut hint inside tooltip */
  shortcut?: string;
  tooltipSide?: "top" | "bottom" | "left" | "right";
  /** Optional tiny semantic dot in top-right corner */
  indicatorTone?: "none" | "warning" | "success" | "info" | "danger";
}

export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(
  function IconButton(
    {
      className,
      size,
      tone,
      active = false,
      tooltip,
      shortcut,
      tooltipSide = "bottom",
      indicatorTone = "none",
      type = "button",
      children,
      "aria-label": ariaLabel,
      ...props
    },
    ref,
  ) {
    const resolvedTone = active ? "active" : tone;
    const buttonEl = (
      <button
        ref={ref}
        type={type}
        aria-label={ariaLabel ?? tooltip}
        data-active={active ? "true" : undefined}
        className={cn(
          "relative",
          iconButtonVariants({ size, tone: resolvedTone }),
          className,
        )}
        {...props}
      >
        {children}
        {indicatorTone !== "none" && (
          <span
            aria-hidden="true"
            className={cn(
              "ring-surface-base absolute top-[4px] right-[4px] size-[5px] rounded-full ring-[1.5px]",
              indicatorTone === "warning" && "bg-signal-warning",
              indicatorTone === "success" && "bg-signal-success",
              indicatorTone === "info" && "bg-signal-info",
              indicatorTone === "danger" && "bg-signal-danger",
            )}
          />
        )}
      </button>
    );

    if (tooltip) {
      return (
        <SimpleTooltip label={tooltip} shortcut={shortcut} side={tooltipSide}>
          {buttonEl}
        </SimpleTooltip>
      );
    }

    return buttonEl;
  },
);
