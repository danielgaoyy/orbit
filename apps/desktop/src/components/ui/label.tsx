import * as React from "react";
import { cn } from "@/lib/utils";

/**
 * Official shadcn/ui `<Label>` primitive styled with Orbit's typography tokens.
 */
function Label({ className, ...props }: React.ComponentProps<"label">) {
  return (
    <label
      data-slot="label"
      className={cn(
        "text-ink-primary flex items-center gap-1.5 text-[12px] leading-none font-medium select-none group-data-[disabled=true]:pointer-events-none group-data-[disabled=true]:opacity-50 peer-disabled:cursor-not-allowed peer-disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Label };
