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
  ...props
}: React.ComponentProps<typeof BaseDialog.Popup>) {
  return (
    <BaseDialog.Portal>
      <BaseDialog.Backdrop className="orbit-dialog-overlay fixed inset-0 z-50 bg-black/45 backdrop-blur-[2px]" />
      <BaseDialog.Popup
        data-slot="dialog-content"
        className={cn(
          "bg-elevated text-foreground floating-surface orbit-dialog-content fixed top-1/2 left-1/2 z-50 flex w-[calc(100vw-2rem)] max-w-lg -translate-x-1/2 -translate-y-1/2 flex-col gap-4 overflow-hidden rounded-[16px] border border-black/[0.09] p-6 outline-none dark:border-white/[0.10]",
          className,
        )}
        {...props}
      >
        {children}
        <BaseDialog.Close className="text-muted-foreground hover:text-foreground absolute top-4 right-4 cursor-pointer rounded-md opacity-70 transition-opacity hover:opacity-100">
          <X className="h-[16px] w-[16px]" />
          <span className="sr-only">Close</span>
        </BaseDialog.Close>
      </BaseDialog.Popup>
    </BaseDialog.Portal>
  );
}

function DialogHeader({ className, ...props }: React.ComponentProps<"div">) {
  return <div className={cn("flex flex-col gap-1.5 text-left", className)} {...props} />;
}

function DialogTitle({ className, ...props }: React.ComponentProps<typeof BaseDialog.Title>) {
  return (
    <BaseDialog.Title
      className={cn("text-[16px] font-semibold tracking-[-0.01em]", className)}
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
      className={cn("text-muted-foreground text-[13px]", className)}
      {...props}
    />
  );
}

function Tabs({ className, ...props }: React.ComponentProps<typeof BaseTabs.Root>) {
  return <BaseTabs.Root className={cn("flex flex-col", className)} {...props} />;
}

function TabsList({ className, ...props }: React.ComponentProps<typeof BaseTabs.List>) {
  return (
    <BaseTabs.List
      className={cn(
        "bg-muted text-muted-foreground inline-flex h-[32px] w-fit items-center justify-center rounded-[10px] p-1",
        className,
      )}
      {...props}
    />
  );
}

function TabsTrigger({ className, ...props }: React.ComponentProps<typeof BaseTabs.Tab>) {
  return (
    <BaseTabs.Tab
      className={cn(
        "data-[active]:bg-background data-[active]:text-foreground inline-flex cursor-pointer items-center justify-center gap-1.5 rounded-[7px] px-3 py-1 text-[12.5px] font-medium whitespace-nowrap transition-colors data-[active]:shadow-2xs",
        className,
      )}
      {...props}
    />
  );
}

function TabsContent({ className, ...props }: React.ComponentProps<typeof BaseTabs.Panel>) {
  return <BaseTabs.Panel className={cn("flex-1 outline-none", className)} {...props} />;
}

export {
  Dialog,
  DialogTrigger,
  DialogContent,
  DialogHeader,
  DialogTitle,
  DialogDescription,
  Tabs,
  TabsList,
  TabsTrigger,
  TabsContent,
};
