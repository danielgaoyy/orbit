import { forwardRef, memo, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { Paperclip } from "lucide-react";
import { cn } from "@/lib/utils";

/**
 * Full-viewport transparent shield mounted during active pointer drags / resizes so embedded
 * iframes (Browser Preview, Artifact Sandbox, PDF Viewer) and WebGL canvases never swallow events.
 * Portaled into `document.body` so transformed / `contain: layout` ancestor panels never clip it.
 */
export const DragShield = memo(function DragShield({
  active,
  cursor = "grabbing",
}: {
  active: boolean;
  cursor?: "grabbing" | "col-resize" | "row-resize";
}) {
  if (!active) return null;
  const cursorClass =
    cursor === "col-resize"
      ? "cursor-col-resize"
      : cursor === "row-resize"
        ? "cursor-row-resize"
        : "cursor-grabbing";
  const shield = (
    <div
      aria-hidden="true"
      onDragStart={(e) => e.preventDefault()}
      className={cn("fixed inset-0 z-50 touch-none select-none [-webkit-user-drag:none]", cursorClass)}
    />
  );
  if (typeof document !== "undefined" && document.body) {
    return createPortal(shield, document.body);
  }
  return shield;
});

/**
 * Unified 0-rerender GPU-translated floating drag preview capsule (`drag-lifted-surface`).
 * Positioned via direct `el.style.transform = translate3d(...)` from the drag controller.
 * Portaled into `document.body` so `fixed` coordinates stay viewport-accurate inside transformed panels.
 */
export const DragGhost = memo(
  forwardRef<
    HTMLDivElement,
    {
      label: string;
      icon?: ReactNode;
      subtitle?: string;
      className?: string;
    }
  >(function DragGhost({ label, icon, subtitle, className }, ref) {
    const ghost = (
      <div
        ref={ref}
        aria-hidden="true"
        className={cn(
          "drag-lifted-surface text-ink-primary pointer-events-none fixed top-0 left-0 z-50 flex max-w-[224px] items-center gap-1.5 rounded-[8px] px-2.5 py-1 text-[12.5px] font-medium opacity-0 transition-opacity duration-[110ms] ease-[var(--ease-out-quart)] will-change-transform motion-reduce:transition-none",
          className,
        )}
      >
        {icon && <span className="inline-flex size-3.5 shrink-0 items-center justify-center">{icon}</span>}
        <span className="min-w-0 truncate">{label}</span>
        {subtitle && (
          <span className="text-ink-muted max-w-[96px] shrink-0 truncate text-[11px] font-normal">
            {subtitle}
          </span>
        )}
      </div>
    );
    if (typeof document !== "undefined" && document.body) {
      return createPortal(ghost, document.body);
    }
    return ghost;
  }),
);

/**
 * Full-width lifted row clone portaled into `document.body` for tactile "drag-along" reordering
 * inside collapsible/overflow-hidden tree containers (e.g. Sidebar Conversations crossing Curated Groups).
 * Positioned at the exact source row geometry via direct DOM `translate3d` (0 React re-renders per frame)
 * and transitions smoothly (`140ms --ease-out-expo`) into the vacated target slot on settle.
 */
export const DragRowPreview = memo(
  forwardRef<
    HTMLDivElement,
    {
      label: string;
      icon?: ReactNode;
      width: number;
      height: number;
      settling?: boolean;
      className?: string;
    }
  >(function DragRowPreview({ label, icon, width, height, settling = false, className }, ref) {
    const row = (
      <div
        ref={ref}
        aria-hidden="true"
        data-drag-row-preview="true"
        data-settling={settling ? "true" : undefined}
        style={{ width, height }}
        className={cn(
          "drag-lifted-surface text-ink-primary pointer-events-none fixed top-0 left-0 z-50 flex items-center gap-2.5 rounded-[8px] px-2 text-[13px] font-medium select-none will-change-transform [-webkit-user-drag:none]",
          className,
        )}
      >
        {icon && (
          <span className="inline-flex size-4 shrink-0 items-center justify-center">{icon}</span>
        )}
        <span className="min-w-0 flex-1 truncate">{label}</span>
      </div>
    );
    if (typeof document !== "undefined" && document.body) {
      return createPortal(row, document.body);
    }
    return row;
  }),
);

/**
 * Unified 2px `--signal-info` insertion indicator for tree/list drag targets.
 */
export const DropIndicatorLine = memo(function DropIndicatorLine({
  orientation = "horizontal",
  edge,
  className,
}: {
  orientation?: "horizontal" | "vertical";
  edge: "before" | "after";
  className?: string;
}) {
  return (
    <div
      aria-hidden="true"
      className={cn(
        "bg-signal-info pointer-events-none absolute z-20 rounded-full animate-[orbit-stage-in_110ms_var(--ease-out-quart)] motion-reduce:animate-none",
        orientation === "horizontal"
          ? cn("right-1.5 left-1.5 h-[2px]", edge === "before" ? "-top-[1px]" : "-bottom-[1px]")
          : cn("top-1 bottom-1 w-[2px]", edge === "before" ? "-left-[1px]" : "-right-[1px]"),
        className,
      )}
    />
  );
});

/**
 * Restrained full-surface file drop overlay for Conversation Stage & Launchpad.
 */
export const FileDropOverlay = memo(function FileDropOverlay({
  visible,
  label,
}: {
  visible: boolean;
  label: string;
}) {
  return (
    <div
      aria-hidden={!visible}
      className={cn(
        "border-signal-info-border bg-surface-base/75 pointer-events-none absolute inset-3 z-30 flex items-center justify-center rounded-2xl border border-dashed backdrop-blur-[2px] transition-opacity duration-[140ms] ease-[var(--ease-out-quart)] select-none motion-reduce:transition-none",
        visible ? "opacity-100" : "opacity-0",
      )}
    >
      <div className="floating-surface border-signal-info-border text-ink-primary flex items-center gap-2 rounded-xl border px-4 py-2.5 text-[13px] font-medium">
        <Paperclip className="text-signal-info size-4 shrink-0 stroke-[1.8]" />
        <span>{label}</span>
      </div>
    </div>
  );
});
