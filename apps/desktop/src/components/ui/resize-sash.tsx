import { useCallback, useRef, type CSSProperties } from "react";
import { DragShield } from "@/components/ui/drag-feedback";
import { DRAG_THRESHOLD_RESIZE_PX } from "@/lib/drag/drag-constants";
import { usePointerDrag } from "@/lib/drag/use-pointer-drag";
import { cn } from "@/lib/utils";

export interface ResizeSashProps<TSnapshot = number> {
  /**
   * Matches WAI-ARIA `aria-orientation`:
   * - `"vertical"` = vertical divider between left/right panes (`cursor-col-resize`)
   * - `"horizontal"` = horizontal divider between top/bottom panes (`cursor-row-resize`)
   */
  orientation: "vertical" | "horizontal";
  /**
   * - `"edge-end"`: overlaid on the right edge of a left pane (Sidebar)
   * - `"edge-start"`: overlaid on the left edge of a right pane (Side Panel)
   * - `"split"`: in-flow divider centered between two sibling flex panes
   */
  variant: "edge-start" | "edge-end" | "split";
  ariaLabel: string;
  /** Tooltip / double-click reset hint */
  title?: string;
  /** Capture starting dimension/ratio snapshot on drag activation. Return null to abort. */
  onDragStart: () => TSnapshot | null;
  /** Called in rAF with cumulative pixel delta along the resize axis (positive = right / down). */
  onDragMove: (deltaPx: number, snapshot: TSnapshot) => void;
  /** Called once on normal pointer release. */
  onDragCommit?: (deltaPx: number, snapshot: TSnapshot) => void;
  /** Called on Escape / pointercancel / window blur to revert back to snapshot. */
  onDragCancel?: (snapshot: TSnapshot) => void;
  /** Optional hook to sync external motion controllers (e.g. `setIsResizingLeft/Right`). */
  onDraggingChange?: (dragging: boolean) => void;
  /** Double-click (or keyboard Home) reset action. */
  onReset?: () => void;
  /** Keyboard nudging (`ArrowLeft/Right` for vertical sash, `ArrowUp/Down` for horizontal sash). */
  onKeyboardStep?: (stepMultiplier: number) => void;
  style?: CSSProperties;
  className?: string;
}

/**
 * Canonical monochrome resize sash shared across Sidebar, Side Panel, Side Panel Dual Split,
 * and Terminal Multi-Pane Splits.
 *
 * Contract:
 * - 6px invisible hit area + crisp 1.5px visual line (monochrome `--border-strong` hover, `--ink-muted` active).
 * - Controlled `isDragging` visual highlight stays lit even when pointer drifts off the 6px track.
 * - Unified 2px threshold, double-click pass-through, rAF coalescing, DragShield, Escape rollback, and keyboard steps.
 */
export function ResizeSash<TSnapshot = number>({
  orientation,
  variant,
  ariaLabel,
  title,
  onDragStart,
  onDragMove,
  onDragCommit,
  onDragCancel,
  onDraggingChange,
  onReset,
  onKeyboardStep,
  style,
  className,
}: ResizeSashProps<TSnapshot>) {
  const { isDragging, start } = usePointerDrag();
  const snapshotRef = useRef<TSnapshot | null>(null);
  const lastDeltaRef = useRef(0);

  const isVertical = orientation === "vertical";
  const cursor = isVertical ? "col-resize" : "row-resize";

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      start(e, {
        thresholdPx: DRAG_THRESHOLD_RESIZE_PX,
        thresholdAxis: isVertical ? "x" : "y",
        cursor,
        preventDefaultOnDown: true,
        ignoreDoubleClick: true,
        raf: true,
        onActivate: () => {
          const snap = onDragStart();
          if (snap === null) return false;
          snapshotRef.current = snap;
          lastDeltaRef.current = 0;
          onDraggingChange?.(true);
          return true;
        },
        onMove: (frame) => {
          if (snapshotRef.current === null) return;
          const delta = isVertical ? frame.deltaX : frame.deltaY;
          lastDeltaRef.current = delta;
          onDragMove(delta, snapshotRef.current);
        },
        onCommit: (frame) => {
          const snap = snapshotRef.current;
          if (snap === null) return;
          const delta = frame ? (isVertical ? frame.deltaX : frame.deltaY) : lastDeltaRef.current;
          onDragCommit?.(delta, snap);
        },
        onCancel: () => {
          const snap = snapshotRef.current;
          if (snap === null) return;
          onDragCancel?.(snap);
        },
        onEnd: () => {
          snapshotRef.current = null;
          lastDeltaRef.current = 0;
          onDraggingChange?.(false);
        },
      });
    },
    [
      start,
      isVertical,
      cursor,
      onDragStart,
      onDragMove,
      onDragCommit,
      onDragCancel,
      onDraggingChange,
    ],
  );

  const handleDoubleClick = useCallback(
    (e: React.MouseEvent<HTMLDivElement>) => {
      if (!onReset) return;
      e.preventDefault();
      e.stopPropagation();
      onReset();
    },
    [onReset],
  );

  const handleKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      if (e.key === "Home" || e.key === "Enter") {
        if (onReset) {
          e.preventDefault();
          onReset();
        }
        return;
      }
      if (!onKeyboardStep) return;
      const mult = e.shiftKey ? 4 : 1;
      if (isVertical) {
        if (e.key === "ArrowLeft") {
          e.preventDefault();
          onKeyboardStep(-mult);
        } else if (e.key === "ArrowRight") {
          e.preventDefault();
          onKeyboardStep(mult);
        }
      } else {
        if (e.key === "ArrowUp") {
          e.preventDefault();
          onKeyboardStep(-mult);
        } else if (e.key === "ArrowDown") {
          e.preventDefault();
          onKeyboardStep(mult);
        }
      }
    },
    [isVertical, onReset, onKeyboardStep],
  );

  const isEdge = variant === "edge-start" || variant === "edge-end";

  return (
    <>
      <div
        role="separator"
        tabIndex={0}
        aria-orientation={orientation}
        aria-label={ariaLabel}
        title={title}
        data-dragging={isDragging ? "true" : undefined}
        style={style}
        onPointerDown={handlePointerDown}
        onDragStart={(e) => e.preventDefault()}
        onDoubleClick={handleDoubleClick}
        onKeyDown={handleKeyDown}
        className={cn(
          "group/sash z-20 touch-none select-none outline-none [-webkit-user-drag:none]",
          isVertical ? "cursor-col-resize" : "cursor-row-resize",
          variant === "edge-end" && "absolute top-0 right-0 flex h-full w-[6px] justify-end",
          variant === "edge-start" && "absolute top-0 left-0 flex h-full w-[6px] justify-start",
          variant === "split" &&
            (isVertical
              ? "relative flex h-full w-[6px] shrink-0 items-center justify-center"
              : "relative flex h-[6px] w-full shrink-0 items-center justify-center"),
          className,
        )}
      >
        {isEdge ? (
          <div
            className={cn(
              "h-full w-[1.5px] transition-colors duration-150",
              isDragging
                ? "bg-ink-muted"
                : "group-hover/sash:bg-border-strong group-focus-visible/sash:bg-border-strong bg-transparent",
            )}
          />
        ) : (
          <div
            className={cn(
              "pointer-events-none transition-colors duration-150",
              isVertical ? "h-full w-[1.5px]" : "h-[1.5px] w-full",
              isDragging
                ? "bg-ink-muted"
                : "bg-border-subtle group-hover/sash:bg-border-strong group-focus-visible/sash:bg-border-strong",
            )}
          />
        )}
      </div>

      <DragShield active={isDragging} cursor={cursor} />
    </>
  );
}
