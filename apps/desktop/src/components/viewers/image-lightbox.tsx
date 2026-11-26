import { memo, useCallback, useEffect, useRef, useState } from "react";
import { Dialog as BaseDialog } from "@base-ui/react/dialog";
import { ChevronLeft, ChevronRight, FileSymlink, X } from "lucide-react";
import { usePanZoom } from "@/lib/drag/use-pan-zoom";
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
  const [stageEl, setStageEl] = useState<HTMLDivElement | null>(null);
  const imgRef = useRef<HTMLImageElement | null>(null);

  const count = items.length;
  const safeIndex = count > 0 ? Math.min(Math.max(activeIndex, 0), count - 1) : 0;
  const current = count > 0 ? items[safeIndex] : null;
  const hasGallery = count > 1;
  const isOpen = open && current !== null;

  const {
    zoom,
    isDragging,
    setZoom,
    zoomIn,
    zoomOut,
    resetTransform,
    consumeMovedClick,
    transformStyle,
    pointerHandlers,
  } = usePanZoom({
    open: isOpen,
    minZoom: MIN_ZOOM,
    maxZoom: MAX_ZOOM,
    allowPanAtBaseZoom: false,
    zoomStep: 1.25,
    wheelMode: "zoomed-or-mod",
    contentRef: imgRef,
    viewportEl: stageEl,
  });

  useEffect(() => {
    if (!open) return;
    const clamped =
      items.length > 0 ? Math.min(Math.max(initialIndex, 0), items.length - 1) : 0;
    setActiveIndex(clamped);
    resetTransform();
  }, [open, initialIndex, items.length, resetTransform]);

  const stepGallery = useCallback(
    (delta: -1 | 1) => {
      if (count <= 1) return;
      setActiveIndex((prev) => (prev + delta + count) % count);
      resetTransform();
    },
    [count, resetTransform],
  );

  useEffect(() => {
    if (!isOpen) return;
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === "ArrowLeft" && count > 1) {
        ev.preventDefault();
        stepGallery(-1);
      } else if (ev.key === "ArrowRight" && count > 1) {
        ev.preventDefault();
        stepGallery(1);
      } else if (ev.key === "+" || ev.key === "=") {
        ev.preventDefault();
        zoomIn();
      } else if (ev.key === "-") {
        ev.preventDefault();
        zoomOut();
      } else if (ev.key === "0") {
        ev.preventDefault();
        resetTransform();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [isOpen, count, stepGallery, zoomIn, zoomOut, resetTransform]);

  const handleImageClick = (ev: React.MouseEvent<HTMLImageElement>) => {
    ev.stopPropagation();
    if (consumeMovedClick()) return;
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
      open={isOpen}
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
                ref={imgRef}
                src={current.url}
                alt={current.label}
                draggable={false}
                onClick={handleImageClick}
                {...pointerHandlers}
                style={transformStyle}
                className={cn(
                  "max-h-[88vh] max-w-[90vw] rounded-lg object-contain shadow-[0_24px_80px_rgba(0,0,0,0.55)] will-change-transform",
                  zoom > 1.01
                    ? cn("touch-none", isDragging ? "cursor-grabbing" : "cursor-grab")
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
