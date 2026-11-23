import * as React from "react";
import { Switch as BaseSwitch } from "@base-ui/react/switch";
import { cn } from "@/lib/utils";

export interface SwitchProps
  extends Omit<React.ComponentProps<typeof BaseSwitch.Root>, "className"> {
  className?: string;
}

/**
 * Official shadcn/ui `<Switch>` primitive powered by `@base-ui/react/switch`.
 */
export function Switch({ className, ...props }: SwitchProps) {
  return (
    <BaseSwitch.Root
      data-slot="switch"
      className={cn(
        "peer focus-visible:ring-ring/50 bg-border-strong/65 data-[checked]:bg-action-primary-bg inline-flex h-[22px] w-[38px] shrink-0 cursor-pointer items-center rounded-full border border-transparent p-[1px] transition-colors duration-150 outline-none focus-visible:ring-2 disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
      {...props}
    >
      <BaseSwitch.Thumb
        data-slot="switch-thumb"
        className={cn(
          "bg-surface-overlay data-[checked]:bg-action-primary-fg pointer-events-none block h-[18px] w-[18px] rounded-full shadow-xs ring-0 transition-transform duration-150 data-[checked]:translate-x-[16px] data-[unchecked]:translate-x-0",
        )}
      />
    </BaseSwitch.Root>
  );
}
