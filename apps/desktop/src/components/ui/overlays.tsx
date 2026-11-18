import * as React from "react";
import {
  DropdownMenu as DropdownPrimitive,
  Popover as PopoverPrimitive,
  Tooltip as TooltipPrimitive,
} from "radix-ui";
import { Kbd, KbdGroup, ShortcutKbd } from "@/components/ui/kbd";
import { cn } from "@/lib/utils";

export { Kbd, KbdGroup, ShortcutKbd };

export const Popover = PopoverPrimitive.Root;
export const PopoverTrigger = PopoverPrimitive.Trigger;

export function PopoverContent({
  className,
  align = "start",
  sideOffset = 6,
  ...props
}: React.ComponentProps<typeof PopoverPrimitive.Content>) {
  return (
    <PopoverPrimitive.Portal>
      <PopoverPrimitive.Content
        align={align}
        sideOffset={sideOffset}
        className={cn(
          "bg-popover text-popover-foreground floating-surface z-50 w-72 rounded-[14px] border border-black/[0.09] p-3 outline-none dark:border-white/[0.10]",
          className,
        )}
        {...props}
      />
    </PopoverPrimitive.Portal>
  );
}

export const DropdownMenu = DropdownPrimitive.Root;
export const DropdownMenuTrigger = DropdownPrimitive.Trigger;

export function DropdownMenuContent({
  className,
  align = "end",
  sideOffset = 6,
  ...props
}: React.ComponentProps<typeof DropdownPrimitive.Content>) {
  return (
    <DropdownPrimitive.Portal>
      <DropdownPrimitive.Content
        align={align}
        sideOffset={sideOffset}
        className={cn(
          "bg-popover text-popover-foreground floating-surface z-50 min-w-44 overflow-hidden rounded-[14px] border border-black/[0.09] p-1.5 outline-none dark:border-white/[0.10]",
          className,
        )}
        {...props}
      />
    </DropdownPrimitive.Portal>
  );
}

export function DropdownMenuItem({
  className,
  ...props
}: React.ComponentProps<typeof DropdownPrimitive.Item>) {
  return (
    <DropdownPrimitive.Item
      className={cn(
        "focus:bg-accent focus:text-accent-foreground relative flex cursor-pointer items-center justify-between gap-3 rounded-[9px] px-2.5 py-1.5 text-[13px] outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-45",
        className,
      )}
      {...props}
    />
  );
}

export function DropdownMenuSeparator({
  className,
  ...props
}: React.ComponentProps<typeof DropdownPrimitive.Separator>) {
  return (
    <DropdownPrimitive.Separator
      className={cn("bg-border -mx-1 my-1 h-px", className)}
      {...props}
    />
  );
}

export function DropdownMenuLabel({
  className,
  ...props
}: React.ComponentProps<typeof DropdownPrimitive.Label>) {
  return (
    <DropdownPrimitive.Label
      className={cn("text-muted-foreground px-2.5 py-1 text-[12px] font-medium", className)}
      {...props}
    />
  );
}

export function DropdownMenuShortcut({
  className,
  ...props
}: React.ComponentProps<"span">) {
  return (
    <span
      data-slot="dropdown-menu-shortcut"
      className={cn("text-muted-foreground ml-auto text-xs tracking-widest", className)}
      {...props}
    />
  );
}

export function TooltipProvider({ children }: { children: React.ReactNode }) {
  return <TooltipPrimitive.Provider delayDuration={320}>{children}</TooltipPrimitive.Provider>;
}

export function SimpleTooltip({
  label,
  shortcut,
  children,
  side = "bottom",
}: {
  label: string;
  shortcut?: string;
  children: React.ReactNode;
  side?: "top" | "bottom" | "left" | "right";
}) {
  return (
    <TooltipPrimitive.Root>
      <TooltipPrimitive.Trigger asChild>{children}</TooltipPrimitive.Trigger>
      <TooltipPrimitive.Portal>
        <TooltipPrimitive.Content
          side={side}
          sideOffset={6}
          className="bg-popover text-popover-foreground border-border z-50 flex items-center gap-1.5 rounded-md border px-2 py-1 text-[11px] shadow-md select-none"
        >
          <span>{label}</span>
          {shortcut && <ShortcutKbd keys={shortcut} />}
        </TooltipPrimitive.Content>
      </TooltipPrimitive.Portal>
    </TooltipPrimitive.Root>
  );
}
