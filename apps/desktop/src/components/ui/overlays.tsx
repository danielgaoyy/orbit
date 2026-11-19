import * as React from "react";
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { Popover as BasePopover } from "@base-ui/react/popover";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { Kbd, KbdGroup, ShortcutKbd } from "@/components/ui/kbd";
import { cn } from "@/lib/utils";

export { Kbd, KbdGroup, ShortcutKbd };

function resolveTriggerRender(
  asChild: boolean | undefined,
  children: React.ReactNode,
  explicitRender: React.ReactElement | undefined,
): {
  render?: React.ReactElement;
  children?: React.ReactNode;
  nativeButton?: boolean;
} {
  if (explicitRender) {
    const isNonNativeButton =
      typeof explicitRender.type === "string" && explicitRender.type !== "button";
    return {
      render: explicitRender,
      children,
      ...(isNonNativeButton ? { nativeButton: false } : {}),
    };
  }
  if (asChild && React.isValidElement(children)) {
    const isNonNativeButton =
      typeof children.type === "string" && children.type !== "button";
    return {
      render: children,
      ...(isNonNativeButton ? { nativeButton: false } : {}),
    };
  }
  return { children };
}

/* ============================================================================
 * Popover (Base UI)
 * ============================================================================ */

export const Popover = BasePopover.Root;

export type PopoverTriggerProps = Omit<
  React.ComponentProps<typeof BasePopover.Trigger>,
  "render"
> & {
  asChild?: boolean;
  render?: React.ReactElement;
};

export function PopoverTrigger({
  asChild,
  render,
  children,
  nativeButton,
  ...props
}: PopoverTriggerProps) {
  const resolved = resolveTriggerRender(asChild, children, render);
  return (
    <BasePopover.Trigger
      data-slot="popover-trigger"
      render={resolved.render}
      nativeButton={nativeButton ?? resolved.nativeButton}
      {...props}
    >
      {resolved.children}
    </BasePopover.Trigger>
  );
}

export interface PopoverContentProps
  extends Omit<React.ComponentProps<typeof BasePopover.Popup>, "initialFocus" | "finalFocus"> {
  side?: React.ComponentProps<typeof BasePopover.Positioner>["side"];
  align?: React.ComponentProps<typeof BasePopover.Positioner>["align"];
  sideOffset?: number;
  alignOffset?: number;
  collisionPadding?: React.ComponentProps<typeof BasePopover.Positioner>["collisionPadding"];
  initialFocus?: React.ComponentProps<typeof BasePopover.Popup>["initialFocus"];
  finalFocus?: React.ComponentProps<typeof BasePopover.Popup>["finalFocus"];
}

export function PopoverContent({
  className,
  side = "bottom",
  align = "start",
  sideOffset = 6,
  alignOffset = 0,
  collisionPadding,
  initialFocus,
  finalFocus,
  ...props
}: PopoverContentProps) {
  return (
    <BasePopover.Portal>
      <BasePopover.Positioner
        side={side}
        align={align}
        sideOffset={sideOffset}
        alignOffset={alignOffset}
        collisionPadding={collisionPadding}
        className="z-50 outline-none"
      >
        <BasePopover.Popup
          data-slot="popover-content"
          initialFocus={initialFocus}
          finalFocus={finalFocus}
          className={cn(
            "bg-popover text-popover-foreground floating-surface orbit-floating-content w-72 rounded-[14px] border border-black/[0.09] p-3 outline-none dark:border-white/[0.10]",
            className,
          )}
          {...props}
        />
      </BasePopover.Positioner>
    </BasePopover.Portal>
  );
}

/* ============================================================================
 * DropdownMenu (Base UI Menu)
 * ============================================================================ */

export const DropdownMenu = BaseMenu.Root;

export type DropdownMenuTriggerProps = Omit<
  React.ComponentProps<typeof BaseMenu.Trigger>,
  "render"
> & {
  asChild?: boolean;
  render?: React.ReactElement;
};

export function DropdownMenuTrigger({
  asChild,
  render,
  children,
  nativeButton,
  ...props
}: DropdownMenuTriggerProps) {
  const resolved = resolveTriggerRender(asChild, children, render);
  return (
    <BaseMenu.Trigger
      data-slot="dropdown-menu-trigger"
      render={resolved.render}
      nativeButton={nativeButton ?? resolved.nativeButton}
      {...props}
    >
      {resolved.children}
    </BaseMenu.Trigger>
  );
}

export interface DropdownMenuContentProps
  extends Omit<React.ComponentProps<typeof BaseMenu.Popup>, "finalFocus"> {
  side?: React.ComponentProps<typeof BaseMenu.Positioner>["side"];
  align?: React.ComponentProps<typeof BaseMenu.Positioner>["align"];
  sideOffset?: number;
  alignOffset?: number;
  finalFocus?: React.ComponentProps<typeof BaseMenu.Popup>["finalFocus"];
}

export function DropdownMenuContent({
  className,
  side = "bottom",
  align = "end",
  sideOffset = 6,
  alignOffset = 0,
  finalFocus,
  ...props
}: DropdownMenuContentProps) {
  return (
    <BaseMenu.Portal>
      <BaseMenu.Positioner
        side={side}
        align={align}
        sideOffset={sideOffset}
        alignOffset={alignOffset}
        className="z-50 outline-none"
      >
        <BaseMenu.Popup
          data-slot="dropdown-menu-content"
          finalFocus={finalFocus}
          className={cn(
            "bg-popover text-popover-foreground floating-surface orbit-floating-content min-w-44 overflow-hidden rounded-[14px] border border-black/[0.09] p-1.5 outline-none dark:border-white/[0.10]",
            className,
          )}
          {...props}
        />
      </BaseMenu.Positioner>
    </BaseMenu.Portal>
  );
}

export interface DropdownMenuItemProps
  extends Omit<React.ComponentProps<typeof BaseMenu.Item>, "onSelect"> {
  onSelect?: (event: React.MouseEvent<HTMLDivElement>) => void;
}

export function DropdownMenuItem({
  className,
  onSelect,
  onClick,
  ...props
}: DropdownMenuItemProps) {
  return (
    <BaseMenu.Item
      data-slot="dropdown-menu-item"
      onClick={(e) => {
        onSelect?.(e);
        onClick?.(e);
      }}
      className={cn(
        "focus:bg-accent focus:text-accent-foreground data-[highlighted]:bg-accent data-[highlighted]:text-accent-foreground relative flex cursor-pointer items-center justify-between gap-3 rounded-[9px] px-2.5 py-1.5 text-[13px] transition-colors duration-100 outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-45",
        className,
      )}
      {...props}
    />
  );
}

export function DropdownMenuSeparator({
  className,
  ...props
}: React.ComponentProps<typeof BaseMenu.Separator>) {
  return (
    <BaseMenu.Separator
      data-slot="dropdown-menu-separator"
      className={cn("bg-border -mx-1 my-1 h-px", className)}
      {...props}
    />
  );
}

export function DropdownMenuLabel({
  className,
  ...props
}: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="dropdown-menu-label"
      className={cn("text-muted-foreground px-2.5 py-1 text-[12px] font-medium select-none", className)}
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
      className={cn("text-muted-foreground ml-auto text-[11.5px] tracking-widest", className)}
      {...props}
    />
  );
}

/* ============================================================================
 * Tooltip (Base UI)
 * ============================================================================ */

export function TooltipProvider({ children }: { children: React.ReactNode }) {
  return (
    <BaseTooltip.Provider delay={320} timeout={400}>
      {children}
    </BaseTooltip.Provider>
  );
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
  const resolved = resolveTriggerRender(true, children, undefined);
  return (
    <BaseTooltip.Root>
      <BaseTooltip.Trigger render={resolved.render}>{resolved.children}</BaseTooltip.Trigger>
      <BaseTooltip.Portal>
        <BaseTooltip.Positioner side={side} sideOffset={6} className="z-50 pointer-events-none">
          <BaseTooltip.Popup className="bg-popover text-popover-foreground floating-surface orbit-tooltip-content flex items-center gap-1.5 rounded-lg border border-black/[0.08] px-2.5 py-1 text-[12px] select-none dark:border-white/[0.11]">
            <span>{label}</span>
            {shortcut && <ShortcutKbd keys={shortcut} />}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}
