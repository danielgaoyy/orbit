import * as React from "react";
import { ScrollArea as BaseScrollArea } from "@base-ui/react/scroll-area";
import { cn } from "@/lib/utils";

export interface ScrollAreaProps
  extends React.ComponentPropsWithoutRef<typeof BaseScrollArea.Root> {
  orientation?: "vertical" | "horizontal" | "both";
  viewportRef?: React.Ref<HTMLDivElement>;
  viewportClassName?: string;
  contentClassName?: string;
  onViewportScroll?: React.UIEventHandler<HTMLDivElement>;
}

function ScrollArea({
  className,
  children,
  orientation = "vertical",
  viewportRef,
  viewportClassName,
  contentClassName,
  onViewportScroll,
  ...props
}: ScrollAreaProps) {
  const showVertical = orientation === "vertical" || orientation === "both";
  const showHorizontal = orientation === "horizontal" || orientation === "both";

  return (
    <BaseScrollArea.Root
      data-slot="scroll-area"
      className={cn("relative overflow-hidden", className)}
      {...props}
    >
      <BaseScrollArea.Viewport
        ref={viewportRef}
        data-slot="scroll-area-viewport"
        onScroll={onViewportScroll}
        className={cn(
          "size-full rounded-[inherit] outline-none",
          orientation === "vertical" && "overflow-x-hidden overflow-y-auto",
          orientation === "horizontal" && "overflow-x-auto overflow-y-hidden",
          orientation === "both" && "overflow-auto",
          viewportClassName,
        )}
      >
        <BaseScrollArea.Content
          data-slot="scroll-area-content"
          className={cn(
            orientation === "vertical" ? "w-full min-w-0" : "min-w-full",
            contentClassName,
          )}
        >
          {children}
        </BaseScrollArea.Content>
      </BaseScrollArea.Viewport>
      {showVertical && <ScrollBar orientation="vertical" />}
      {showHorizontal && <ScrollBar orientation="horizontal" />}
      {showVertical && showHorizontal && <BaseScrollArea.Corner />}
    </BaseScrollArea.Root>
  );
}

function ScrollBar({
  className,
  orientation = "vertical",
  ...props
}: React.ComponentPropsWithoutRef<typeof BaseScrollArea.Scrollbar>) {
  return (
    <BaseScrollArea.Scrollbar
      data-slot="scroll-area-scrollbar"
      orientation={orientation}
      className={cn(
        "pointer-events-none z-30 flex touch-none select-none opacity-0 transition-opacity duration-150 delay-300 data-[hovering]:pointer-events-auto data-[hovering]:opacity-100 data-[hovering]:delay-0 data-[scrolling]:pointer-events-auto data-[scrolling]:opacity-100 data-[scrolling]:delay-0 data-[scrolling]:duration-75",
        orientation === "vertical" && "h-full w-2.5 border-l border-l-transparent p-[2px]",
        orientation === "horizontal" && "h-2.5 flex-col border-t border-t-transparent p-[2px]",
        className,
      )}
      {...props}
    >
      <BaseScrollArea.Thumb
        data-slot="scroll-area-thumb"
        className="bg-foreground/15 hover:bg-foreground/28 active:bg-foreground/38 relative flex-1 rounded-full transition-colors duration-150"
      />
    </BaseScrollArea.Scrollbar>
  );
}

export { ScrollArea, ScrollBar };
