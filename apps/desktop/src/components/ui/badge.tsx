import * as React from "react";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

/**
 * WCAG-AA verified semantic badges for both Light and Dark surfaces.
 */
const badgeVariants = cva(
  "inline-flex items-center justify-center rounded-[6px] border px-2 py-0.5 text-[11px] font-medium w-fit whitespace-nowrap shrink-0 gap-1 transition-colors",
  {
    variants: {
      variant: {
        default: "border-signal-info-border bg-signal-info-bg text-signal-info",
        secondary: "border-transparent bg-surface-recessed text-ink-secondary",
        outline: "border-border-default text-ink-secondary bg-transparent",
        success: "border-signal-success-border bg-signal-success-bg text-signal-success",
        relay: "border-signal-relay-border bg-signal-relay-bg text-signal-relay",
        warning: "border-signal-warning-border bg-signal-warning-bg text-signal-warning",
        destructive: "border-signal-danger-border bg-signal-danger-bg text-signal-danger",
      },
    },
    defaultVariants: {
      variant: "default",
    },
  },
);

function Badge({
  className,
  variant,
  ...props
}: React.ComponentProps<"span"> & VariantProps<typeof badgeVariants>) {
  return <span data-slot="badge" className={cn(badgeVariants({ variant }), className)} {...props} />;
}

export { Badge, badgeVariants };
