import * as React from "react";
import { cn } from "@/lib/utils";

export interface ScrollAreaProps extends React.HTMLAttributes<HTMLDivElement> {
  orientation?: "vertical" | "horizontal" | "both";
  viewportRef?: React.Ref<HTMLDivElement>;
  viewportClassName?: string;
  contentClassName?: string;
  onViewportScroll?: React.UIEventHandler<HTMLDivElement>;
}

/**
 * Native Composited ScrollArea:
 * Runs 100% on the OS / WebKit compositor thread (native macOS overlay NSScroller, standard CSS thin scrollbars elsewhere)
 * with zero JS scrollbar tracking, zero `getComputedStyle` layout reads, and zero inherited CSS variable invalidation storms.
 */
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
  return (
    <div
      data-slot="scroll-area"
      className={cn("relative overflow-hidden", className)}
      {...props}
    >
      <div
        ref={viewportRef}
        data-slot="scroll-area-viewport"
        data-orientation={orientation}
        onScroll={onViewportScroll}
        className={cn(
          "orbit-native-scroll size-full rounded-[inherit] outline-none",
          orientation === "vertical" && "overflow-x-hidden overflow-y-auto",
          orientation === "horizontal" &&
            "overflow-x-auto overflow-y-hidden overscroll-x-contain",
          orientation === "both" && "overflow-auto overscroll-contain",
          viewportClassName,
        )}
      >
        <div
          data-slot="scroll-area-content"
          style={
            orientation === "vertical"
              ? { minWidth: 0, maxWidth: "100%" }
              : { minWidth: "fit-content" }
          }
          className={cn(
            orientation === "vertical" ? "w-full min-w-0" : "min-w-full",
            contentClassName,
          )}
        >
          {children}
        </div>
      </div>
    </div>
  );
}

/** Compatibility stub retained for callers importing `ScrollBar`; native composited scrollbars require no extra DOM element. */
function ScrollBar(_props: {
  className?: string;
  orientation?: "vertical" | "horizontal";
}) {
  return null;
}

export { ScrollArea, ScrollBar };
