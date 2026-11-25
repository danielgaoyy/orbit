import { memo, useCallback, useEffect, useRef, useState } from "react";
import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { ChevronLeft, ChevronRight, FileSymlink, X } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

export interface ImageLightboxItem {
  url: string;
  label: string;
  path?: string | null;
  hostId?: string;
  cwd?: string;
}

export interface ImageLightboxProps {
  open: boolean;
  items: readonly ImageLightboxItem[];
  initialIndex?: number;
  onClose: () => void;
  onOpenInSidePanel?: (item: ImageLightboxItem) => void;
}

const MIN_ZOOM = 1;
const MAX_ZOOM = 4;
const TOGGLE_ZOOM = 1.85;

function clampZoom(value: number): number {
  return Math.min(MAX_ZOOM, Math.max(MIN_ZOOM, value));
}

/**
 * Frameless Claude-style image preview lightbox:
 * - Full-viewport dark scrim with the image floating directly at center (zero card/dialog chrome).
 * - Click anywhere on the scrim or press `Esc` to close.
 * - Click / double-click or wheel/pinch to zoom, drag to pan when zoomed, `0` to reset.
 * - Optional gallery navigation (`←` / `→`) when multiple images are passed.
 */
export const ImageLightbox = memo(function ImageLightbox({
  open,
  items,
  initialIndex = 0,
  onClose,
  onOpenInSidePanel,
}: ImageLightboxProps) {
  const { t } = useI18n();
  const [activeIndex, setActiveIndex] = useState(initialIndex);
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [stageEl, setStageEl] = useState<HTMLDivElement | null>(null);
  const zoomRef = useRef(zoom);
  zoomRef.current = zoom;
  const dragRef = useRef<{
    active: boolean;
    pointerId: number;
    startX: number;
    startY: number;
    origX: number;
    origY: number;
    moved: boolean;
  }>({
    active: false,
    pointerId: -1,
    startX: 0,
    startY: 0,
    origX: 0,
    origY: 0,
    moved: false,
  });

  const resetTransform = useCallback(() => {
    setZoom(1);
    setPan({ x: 0, y: 0 });
    setDragging(false);
    dragRef.current.active = false;
  }, []);

  useEffect(() => {
    if (!open) return;
    const clamped =
      items.length > 0 ? Math.min(Math.max(initialIndex, 0), items.length - 1) : 0;
    setActiveIndex(clamped);
    resetTransform();
  }, [open, initialIndex, items.length, resetTransform]);

  const count = items.length;
  const safeIndex = count > 0 ? Math.min(Math.max(activeIndex, 0), count - 1) : 0;
  const current = count > 0 ? items[safeIndex] : null;
  const hasGallery = count > 1;

  const stepGallery = useCallback(
    (delta: -1 | 1) => {
      if (count <= 1) return;
      setActiveIndex((prev) => (prev + delta + count) % count);
      resetTransform();
    },
    [count, resetTransform],
  );

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === "ArrowLeft" && count > 1) {
        ev.preventDefault();
        stepGallery(-1);
      } else if (ev.key === "ArrowRight" && count > 1) {
        ev.preventDefault();
        stepGallery(1);
      } else if (ev.key === "+" || ev.key === "=") {
        ev.preventDefault();
        setZoom((z) => clampZoom(z * 1.25));
      } else if (ev.key === "-") {
        ev.preventDefault();
        setZoom((z) => {
          const next = clampZoom(z / 1.25);
          if (next <= 1.01) setPan({ x: 0, y: 0 });
          return next;
        });
      } else if (ev.key === "0") {
        ev.preventDefault();
        resetTransform();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [open, count, stepGallery, resetTransform]);

  useEffect(() => {
    if (!open || !stageEl) return;
    const handleWheel = (ev: WheelEvent) => {
      if (!ev.ctrlKey && !ev.metaKey && zoomRef.current <= 1.01) return;
      ev.preventDefault();
      const factor = ev.deltaY < 0 ? 1.14 : 1 / 1.14;
      setZoom((prev) => {
        const next = clampZoom(prev * factor);
        if (next <= 1.01) {
          setPan({ x: 0, y: 0 });
          return 1;
        }
        return next;
      });
    };
    stageEl.addEventListener("wheel", handleWheel, { passive: false });
    return () => stageEl.removeEventListener("wheel", handleWheel);
  }, [open, stageEl]);

  const handleImagePointerDown = (ev: React.PointerEvent<HTMLImageElement>) => {
    if (ev.button !== 0 || zoom <= 1.01) return;
    ev.preventDefault();
    ev.stopPropagation();
    ev.currentTarget.setPointerCapture(ev.pointerId);
    dragRef.current = {
      active: true,
      pointerId: ev.pointerId,
      startX: ev.clientX,
      startY: ev.clientY,
      origX: pan.x,
      origY: pan.y,
      moved: false,
    };
    setDragging(true);
  };

  const handleImagePointerMove = (ev: React.PointerEvent<HTMLImageElement>) => {
    const d = dragRef.current;
    if (!d.active || d.pointerId !== ev.pointerId) return;
    const dx = ev.clientX - d.startX;
    const dy = ev.clientY - d.startY;
    if (Math.abs(dx) > 3 || Math.abs(dy) > 3) {
      d.moved = true;
    }
    setPan({ x: d.origX + dx, y: d.origY + dy });
  };

  const handleImagePointerUp = (ev: React.PointerEvent<HTMLImageElement>) => {
    const d = dragRef.current;
    if (!d.active || d.pointerId !== ev.pointerId) return;
    if (ev.currentTarget.hasPointerCapture(ev.pointerId)) {
      ev.currentTarget.releasePointerCapture(ev.pointerId);
    }
    d.active = false;
    setDragging(false);
  };

  const handleImageClick = (ev: React.MouseEvent<HTMLImageElement>) => {
    ev.stopPropagation();
    if (dragRef.current.moved) {
      dragRef.current.moved = false;
      return;
    }
    if (zoom > 1.01) {
      resetTransform();
    } else {
      setZoom(TOGGLE_ZOOM);
    }
  };

  const canOpenInSidePanel = Boolean(
    current && current.path && current.hostId && onOpenInSidePanel,
  );

  return (
    <BaseDialog.Root
      open={open && current !== null}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <BaseDialog.Portal>
        <BaseDialog.Backdrop className="orbit-dialog-overlay fixed inset-0 z-50 bg-black/82 backdrop-blur-[6px]" />
        <BaseDialog.Popup
          data-slot="dialog-content"
          aria-label={current?.label || t("common.close")}
          className="orbit-dialog-content fixed inset-0 z-50 flex items-center justify-center bg-transparent p-0 outline-none select-none"
        >
          <BaseDialog.Title className="sr-only">{current?.label ?? ""}</BaseDialog.Title>
          <BaseDialog.Description className="sr-only">
            {current?.path || current?.label || ""}
          </BaseDialog.Description>

          {/* Full-viewport click-to-dismiss scrim holding only the floating image */}
          <div
            ref={setStageEl}
            onClick={onClose}
            className="relative flex size-full items-center justify-center overflow-hidden p-8 sm:p-12"
          >
            {/* Floating minimal top-right actions (Claude-style translucent circles) */}
            <div
              onClick={(e) => e.stopPropagation()}
              className="fixed top-4 right-4 z-10 flex items-center gap-1.5"
            >
              {canOpenInSidePanel && current && (
                <button
                  type="button"
                  title={t("stage.openInPanel")}
                  aria-label={t("stage.openInPanel")}
                  onClick={() => onOpenInSidePanel?.(current)}
                  className="orbit-press inline-flex size-8 cursor-pointer items-center justify-center rounded-full border border-white/12 bg-black/45 text-white/80 backdrop-blur-md transition-colors hover:bg-black/70 hover:text-white focus-visible:ring-2 focus-visible:ring-white/40 focus-visible:outline-none"
                >
                  <FileSymlink className="size-[14px] stroke-[1.8]" />
                </button>
              )}
              <button
                type="button"
                title={t("common.close")}
                aria-label={t("common.close")}
                onClick={onClose}
                className="orbit-press inline-flex size-8 cursor-pointer items-center justify-center rounded-full border border-white/12 bg-black/45 text-white/80 backdrop-blur-md transition-colors hover:bg-black/70 hover:text-white focus-visible:ring-2 focus-visible:ring-white/40 focus-visible:outline-none"
              >
                <X className="size-[15px] stroke-[1.9]" />
              </button>
            </div>

            {/* Quiet gallery arrows only when previewing a multi-image set */}
            {hasGallery && (
              <>
                <button
                  type="button"
                  aria-label="Previous image"
                  onClick={(e) => {
                    e.stopPropagation();
                    stepGallery(-1);
                  }}
                  className="orbit-press fixed top-1/2 left-4 z-10 inline-flex size-9 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border border-white/12 bg-black/45 text-white/80 backdrop-blur-md transition-colors hover:bg-black/70 hover:text-white focus-visible:ring-2 focus-visible:ring-white/40 focus-visible:outline-none"
                >
                  <ChevronLeft className="size-4.5 stroke-[1.9]" />
                </button>
                <button
                  type="button"
                  aria-label="Next image"
                  onClick={(e) => {
                    e.stopPropagation();
                    stepGallery(1);
                  }}
                  className="orbit-press fixed top-1/2 right-4 z-10 inline-flex size-9 -translate-y-1/2 cursor-pointer items-center justify-center rounded-full border border-white/12 bg-black/45 text-white/80 backdrop-blur-md transition-colors hover:bg-black/70 hover:text-white focus-visible:ring-2 focus-visible:ring-white/40 focus-visible:outline-none"
                >
                  <ChevronRight className="size-4.5 stroke-[1.9]" />
                </button>
              </>
            )}

            {/* Pure floating image — no card, no frame, no title bar */}
            {current && (
              <img
                src={current.url}
                alt={current.label}
                draggable={false}
                onClick={handleImageClick}
                onPointerDown={handleImagePointerDown}
                onPointerMove={handleImagePointerMove}
                onPointerUp={handleImagePointerUp}
                onPointerCancel={handleImagePointerUp}
                style={{
                  transform: `translate3d(${pan.x}px, ${pan.y}px, 0) scale(${zoom})`,
                  transition: dragging
                    ? "none"
                    : "transform 180ms cubic-bezier(0.22, 1, 0.36, 1)",
                }}
                className={cn(
                  "max-h-[88vh] max-w-[90vw] rounded-lg object-contain shadow-[0_24px_80px_rgba(0,0,0,0.55)] will-change-transform",
                  zoom > 1.01
                    ? dragging
                      ? "cursor-grabbing"
                      : "cursor-zoom-out"
                    : "cursor-zoom-in",
                )}
              />
            )}

            {/* Minimal index indicator only for multi-image gallery */}
            {hasGallery && (
              <div
                onClick={(e) => e.stopPropagation()}
                className="tabular-nums fixed bottom-5 left-1/2 z-10 -translate-x-1/2 rounded-full border border-white/10 bg-black/50 px-2.5 py-0.5 font-mono text-[11px] text-white/80 backdrop-blur-md"
              >
                {safeIndex + 1} / {count}
              </div>
            )}
          </div>
        </BaseDialog.Popup>
      </BaseDialog.Portal>
    </BaseDialog.Root>
  );
});
