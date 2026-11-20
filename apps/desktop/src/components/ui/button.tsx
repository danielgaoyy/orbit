import * as React from "react";
import { useRender } from "@base-ui/react/use-render";
import { cva, type VariantProps } from "class-variance-authority";
import { cn } from "@/lib/utils";

const buttonVariants = cva(
  "inline-flex items-center justify-center gap-2 whitespace-nowrap rounded-lg text-[13px] font-medium transition-[color,background-color,border-color,opacity,transform] disabled:pointer-events-none disabled:opacity-50 [&_svg]:pointer-events-none [&_svg:not([class*='size-'])]:size-4 shrink-0 outline-none focus-visible:ring-2 focus-visible:ring-ring/50 cursor-pointer",
  {
    variants: {
      variant: {
        default: "bg-foreground text-background shadow-2xs hover:opacity-90",
        destructive: "bg-destructive text-white shadow-xs hover:bg-destructive/90",
        outline:
          "border border-border bg-background shadow-2xs hover:bg-accent hover:text-accent-foreground",
        secondary: "bg-secondary text-secondary-foreground shadow-2xs hover:bg-secondary/80",
        ghost: "hover:bg-accent hover:text-accent-foreground",
        link: "text-foreground underline-offset-4 hover:underline",
      },
      size: {
        default: "h-[34px] px-3.5 py-1.5",
        sm: "h-[30px] rounded-lg gap-1.5 px-3 text-[12.5px]",
        xs: "h-[26px] rounded-md px-2.5 text-[12px]",
        lg: "h-[38px] rounded-lg px-5 text-[13.5px]",
        icon: "size-[32px]",
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
