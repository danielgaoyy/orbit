import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * Official shadcn/ui `<Textarea>` primitive styled with Orbit's surface & border tokens.
 */
function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "border-border-subtle bg-surface-base text-ink-primary placeholder:text-ink-faint focus-visible:border-border-strong focus-visible:ring-ring/30 flex min-h-16 w-full rounded-[8px] border px-3 py-2 text-[12.5px] shadow-2xs transition-colors outline-none focus-visible:ring-1 disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Textarea };
