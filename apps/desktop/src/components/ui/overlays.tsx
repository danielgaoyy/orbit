import * as React from "react";
import { Menu as BaseMenu } from "@base-ui/react/menu";
import { Popover as BasePopover } from "@base-ui/react/popover";
import { Tooltip as BaseTooltip } from "@base-ui/react/tooltip";
import { ChevronRight } from "lucide-react";
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
            "text-ink-primary floating-surface orbit-floating-content w-72 rounded-[14px] border p-3 outline-none",
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
export const DropdownMenuGroup = BaseMenu.Group;

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
            "text-ink-primary floating-surface orbit-floating-content w-max min-w-[180px] max-w-[calc(100vw-16px)] overflow-hidden rounded-[10px] border p-1 outline-none",
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
  variant?: "default" | "destructive";
}

export function DropdownMenuItem({
  className,
  variant = "default",
  onSelect,
  onClick,
  ...props
}: DropdownMenuItemProps) {
  return (
    <BaseMenu.Item
      data-slot="dropdown-menu-item"
      data-variant={variant}
      onClick={(e) => {
        onSelect?.(e);
        onClick?.(e);
      }}
      className={cn(
        "relative flex min-h-[28px] cursor-pointer items-center justify-between gap-2 rounded-[6px] px-2 py-1 text-[12.5px] transition-colors duration-100 outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0",
        variant === "destructive"
          ? "text-signal-danger focus:bg-signal-danger-bg focus:text-signal-danger data-[highlighted]:bg-signal-danger-bg data-[highlighted]:text-signal-danger"
          : "focus:bg-accent focus:text-ink-primary data-[highlighted]:bg-accent data-[highlighted]:text-ink-primary",
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
      className={cn("bg-border-subtle -mx-1 my-1 h-px", className)}
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
      className={cn("text-ink-muted px-2 py-1 text-[11px] font-medium select-none", className)}
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
      className={cn("text-ink-faint ml-auto text-[11px] tracking-widest", className)}
      {...props}
    />
  );
}

export const DropdownMenuSub = BaseMenu.SubmenuRoot;

export interface DropdownMenuSubTriggerProps
  extends React.ComponentProps<typeof BaseMenu.SubmenuTrigger> {}

export function DropdownMenuSubTrigger({
  className,
  children,
  ...props
}: DropdownMenuSubTriggerProps) {
  return (
    <BaseMenu.SubmenuTrigger
      data-slot="dropdown-menu-sub-trigger"
      className={cn(
        "focus:bg-accent focus:text-ink-primary data-[highlighted]:bg-accent data-[highlighted]:text-ink-primary data-[popup-open]:bg-accent data-[popup-open]:text-ink-primary relative flex min-h-[28px] cursor-pointer items-center justify-between gap-2 rounded-[6px] px-2 py-1 text-[12.5px] transition-colors duration-100 outline-none select-none data-[disabled]:pointer-events-none data-[disabled]:opacity-45 [&_svg]:pointer-events-none [&_svg]:shrink-0",
        className,
      )}
      {...props}
    >
      {children}
      <ChevronRight className="text-ink-muted ml-auto h-[13px] w-[13px] shrink-0 stroke-[1.75]" />
    </BaseMenu.SubmenuTrigger>
  );
}

export interface DropdownMenuSubContentProps
  extends Omit<React.ComponentProps<typeof BaseMenu.Popup>, "finalFocus"> {
  side?: React.ComponentProps<typeof BaseMenu.Positioner>["side"];
  align?: React.ComponentProps<typeof BaseMenu.Positioner>["align"];
  sideOffset?: number;
  alignOffset?: number;
  collisionPadding?: React.ComponentProps<typeof BaseMenu.Positioner>["collisionPadding"];
  finalFocus?: React.ComponentProps<typeof BaseMenu.Popup>["finalFocus"];
}

export function DropdownMenuSubContent({
  className,
  side = "right",
  align = "start",
  sideOffset = 4,
  alignOffset = -4,
  collisionPadding = 8,
  finalFocus,
  ...props
}: DropdownMenuSubContentProps) {
  return (
    <BaseMenu.Portal>
      <BaseMenu.Positioner
        side={side}
        align={align}
        sideOffset={sideOffset}
        alignOffset={alignOffset}
        collisionPadding={collisionPadding}
        className="z-50 outline-none"
      >
        <BaseMenu.Popup
          data-slot="dropdown-menu-sub-content"
          finalFocus={finalFocus}
          className={cn(
            "text-ink-primary floating-surface orbit-floating-content w-max min-w-[172px] max-w-[260px] overflow-hidden rounded-[10px] border p-1 outline-none",
            className,
          )}
          {...props}
        />
      </BaseMenu.Positioner>
    </BaseMenu.Portal>
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
          <BaseTooltip.Popup
            data-slot="tooltip-content"
            className="text-ink-primary floating-surface orbit-tooltip-content flex items-center gap-1.5 rounded-[8px] border px-2.5 py-1 text-[12px] select-none"
          >
            <span>{label}</span>
            {shortcut && <ShortcutKbd keys={shortcut} size="sm" />}
          </BaseTooltip.Popup>
        </BaseTooltip.Positioner>
      </BaseTooltip.Portal>
    </BaseTooltip.Root>
  );
}
