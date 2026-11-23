import * as React from "react";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";

/**
 * Official shadcn/ui `<Input>` primitive styled with Orbit's surface & border tokens.
 */
function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "border-border-subtle bg-surface-base text-ink-primary placeholder:text-ink-faint focus-visible:border-border-strong focus-visible:ring-ring/30 flex h-[32px] w-full min-w-0 rounded-[8px] border px-2.5 py-1 text-[12.5px] shadow-2xs transition-colors outline-none focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Input, Textarea };
