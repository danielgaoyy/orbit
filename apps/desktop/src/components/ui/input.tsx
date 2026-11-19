import * as React from "react";
import { cn } from "@/lib/utils";

function Input({ className, type, ...props }: React.ComponentProps<"input">) {
  return (
    <input
      type={type}
      data-slot="input"
      className={cn(
        "border-input placeholder:text-muted-foreground flex h-[34px] w-full min-w-0 rounded-lg border bg-transparent px-3 py-1 text-[13px] shadow-2xs transition-colors outline-none focus-visible:border-foreground/30 disabled:pointer-events-none disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

function Textarea({ className, ...props }: React.ComponentProps<"textarea">) {
  return (
    <textarea
      data-slot="textarea"
      className={cn(
        "border-input placeholder:text-muted-foreground flex min-h-16 w-full rounded-lg border bg-transparent px-3 py-2 text-[13px] shadow-2xs transition-colors outline-none focus-visible:border-foreground/30 disabled:opacity-50",
        className,
      )}
      {...props}
    />
  );
}

export { Input, Textarea };
