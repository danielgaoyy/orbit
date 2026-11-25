import * as React from "react";
import { useRender } from "@base-ui/react/use-render";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "orbit-press inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-[8px] text-[13px] font-medium disabled:pointer-events-none disabled:opacity-45 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 outline-none focus-visible:ring-1 focus-visible:ring-ring/45 cursor-pointer",
  {
    variants: {
      variant: {
        default: "bg-primary text-primary-foreground hover:opacity-90",
        destructive: "bg-signal-danger text-white hover:opacity-90",
        outline:
          "border border-border-default bg-surface-raised text-ink-primary hover:bg-accent",
        secondary:
          "bg-surface-recessed text-ink-primary border border-border-subtle hover:bg-accent",
        ghost: "text-ink-secondary hover:bg-accent hover:text-ink-primary",
        link: "text-ink-primary underline-offset-4 hover:underline",
      },
      size: {
        default: "h-[34px] px-3.5 py-1.5",
        sm: "h-[30px] rounded-[8px] gap-1.5 px-3 text-[12px]",
        xs: "h-[26px] rounded-[6px] px-2.5 text-[12px]",
        lg: "h-[38px] rounded-[10px] px-5 text-[13px]",
        icon: "size-[28px] rounded-[6px]",
        "icon-xs": "size-6 rounded-[6px]",
        "icon-sm": "size-7 rounded-[7px]",
      },
    },
    defaultVariants: {
      variant: "default",
      size: "default",
    },
  },
);

function Button({
  className,
  variant,
  size,
  asChild = false,
  render,
  children,
  ...props
}: React.ComponentProps<"button"> &
  VariantProps<typeof buttonVariants> & {
    asChild?: boolean;
    render?: useRender.RenderProp<Record<string, never>>;
  }) {
  const effectiveRender =
    render ?? (asChild && React.isValidElement(children) ? children : undefined);

  return useRender({
    defaultTagName: "button",
    render: effectiveRender,
    props: {
      "data-slot": "button",
      className: cn(buttonVariants({ variant, size, className })),
      children: asChild && React.isValidElement(children) ? undefined : children,
      ...props,
    },
  });
}

export { Button, buttonVariants };
