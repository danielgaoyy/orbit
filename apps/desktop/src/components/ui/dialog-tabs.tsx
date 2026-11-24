import * as React from "react";
import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { Tabs as BaseTabs } from "@base-ui/react/tabs";
import { X } from "lucide-react";
import { cn } from "@/lib/utils";

function Dialog(props: React.ComponentProps<typeof BaseDialog.Root>) {
  return <BaseDialog.Root {...props} />;
}

type DialogTriggerProps = Omit<React.ComponentProps<typeof BaseDialog.Trigger>, "render"> & {
  asChild?: boolean;
  render?: React.ReactElement;
};

function DialogTrigger({ asChild, render, children, nativeButton, ...props }: DialogTriggerProps) {
  const targetEl = render ?? (asChild && React.isValidElement(children) ? children : undefined);
  const isNonNativeButton =
    targetEl && typeof targetEl.type === "string" && targetEl.type !== "button";
  return (
    <BaseDialog.Trigger
      data-slot="dialog-trigger"
      render={targetEl}
      nativeButton={nativeButton ?? (isNonNativeButton ? false : undefined)}
      {...props}
    >
      {asChild && React.isValidElement(children) ? undefined : children}
    </BaseDialog.Trigger>
  );
}

function DialogContent({
  className,
  children,
  closeDisabled = false,
  showCloseButton = true,
  ...props
}: React.ComponentProps<typeof BaseDialog.Popup> & {
  closeDisabled?: boolean;
  showCloseButton?: boolean;
}) {
  return (
    <BaseDialog.Portal>
      <BaseDialog.Backdrop className="orbit-dialog-overlay fixed inset-0 z-50 bg-black/45 dark:bg-black/55" />
      <BaseDialog.Popup
        data-slot="dialog-content"
        className={cn(
          "bg-surface-overlay text-ink-primary border-border-default floating-surface orbit-dialog-content fixed top-1/2 left-1/2 z-50 flex w-[calc(100vw-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 flex-col gap-4 overflow-hidden rounded-[16px] border p-6 outline-none",
          className,
        )}
        {...props}
      >
        {children}
        {showCloseButton && (
          <BaseDialog.Close
            disabled={closeDisabled}
            className="text-ink-muted hover:text-ink-primary hover:bg-accent absolute top-3.5 right-3.5 inline-flex size-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors disabled:pointer-events-none disabled:opacity-30"
          >
            <X className="h-[15px] w-[15px]" />
            <span className="sr-only">Close</span>
          </BaseDialog.Close>
        )}
      </BaseDialog.Popup>
    </BaseDialog.Portal>
  );
}

function DialogHeader({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="dialog-header"
      className={cn("flex flex-col gap-1 text-left", className)}
      {...props}
    />
  );
}

function DialogFooter({ className, ...props }: React.ComponentProps<"div">) {
  return (
    <div
      data-slot="dialog-footer"
      className={cn("flex items-center justify-end gap-2", className)}
      {...props}
    />
  );
}

function DialogTitle({ className, ...props }: React.ComponentProps<typeof BaseDialog.Title>) {
  return (
    <BaseDialog.Title
      data-slot="dialog-title"
      className={cn("text-ink-primary text-[15.5px] font-semibold tracking-[-0.015em]", className)}
      {...props}
    />
  );
}

function DialogDescription({
  className,
  ...props
}: React.ComponentProps<typeof BaseDialog.Description>) {
  return (
    <BaseDialog.Description
      data-slot="dialog-description"
      className={cn("text-ink-secondary text-[12.5px]", className)}
      {...props}
    />
  );
}

function Tabs({ className, ...props }: React.ComponentProps<typeof BaseTabs.Root>) {
  return <BaseTabs.Root data-slot="tabs" className={cn("flex flex-col", className)} {...props} />;
}

function TabsList({ className, ...props }: React.ComponentProps<typeof BaseTabs.List>) {
  return (
    <BaseTabs.List
      data-slot="tabs-list"
      className={cn(
        "bg-surface-recessed border-border-subtle/70 text-ink-muted inline-flex h-[30px] w-fit items-center justify-center gap-0.5 rounded-[9px] border p-[3px]",
        className,
      )}
      {...props}
    />
  );
}

function TabsTrigger({ className, ...props }: React.ComponentProps<typeof BaseTabs.Tab>) {
  return (
    <BaseTabs.Tab
      data-slot="tabs-trigger"
      className={cn(
        "hover:text-ink-primary data-[active]:bg-surface-raised data-[active]:text-ink-primary inline-flex h-[24px] cursor-pointer items-center justify-center gap-1.5 rounded-[6px] px-2.5 py-0 text-[12px] font-medium whitespace-nowrap transition-colors data-[active]:shadow-2xs",
        className,
      )}
      {...props}
    />
  );
}

function TabsContent({ className, ...props }: React.ComponentProps<typeof BaseTabs.Panel>) {
  return (
    <BaseTabs.Panel data-slot="tabs-content" className={cn("flex-1 outline-none", className)} {...props} />
  );
}

export {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogFooter,
  DialogTitle,
  DialogDescription,
  Tabs,
  TabsList,
  TabsTrigger,
  TabsContent,
};
