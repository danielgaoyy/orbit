import {
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Check,
  Code2,
  Copy,
  Download,
  Eye,
  GitFork,
  Image as ImageIcon,
  Maximize2,
  RotateCcw,
  Workflow,
  X,
  ZoomIn,
  ZoomOut,
} from "lucide-react";
import { HighlightedCodeBlock } from "@/components/markdown-blocks";
import { Dialog, DialogContent, DialogTitle } from "@/components/ui/dialog-tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { usePanZoom } from "@/lib/drag/use-pan-zoom";
import {
  type DiagramKind,
  type DiagramTheme,
  type RenderDiagramView,
  type RichBlockView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

const DIAGRAM_CACHE_LIMIT = 128;
const diagramViewCache = new Map<string, RenderDiagramView>();

const MIN_LIGHTBOX_ZOOM = 0.25;
const MAX_LIGHTBOX_ZOOM = 5.0;

function diagramCacheKey(kind: DiagramKind, theme: DiagramTheme, source: string): string {
  return `${kind}::${theme}::${source}`;
}

function getCachedDiagram(key: string): RenderDiagramView | undefined {
  const hit = diagramViewCache.get(key);
  if (hit !== undefined) {
    diagramViewCache.delete(key);
    diagramViewCache.set(key, hit);
  }
  return hit;
}

function setCachedDiagram(key: string, view: RenderDiagramView): void {
  if (diagramViewCache.has(key)) {
    diagramViewCache.delete(key);
  } else if (diagramViewCache.size >= DIAGRAM_CACHE_LIMIT) {
    const oldestKey = diagramViewCache.keys().next().value;
    if (oldestKey !== undefined) {
      diagramViewCache.delete(oldestKey);
    }
  }
  diagramViewCache.set(key, view);
}

function kindBaseTitle(kind: DiagramKind): string {
  switch (kind) {
    case "mermaid":
      return "Mermaid";
    case "graphviz":
      return "Graphviz";
    case "svg":
      return "SVG";
  }
}

function DiagramKindIcon({ kind }: { kind: DiagramKind }) {
  switch (kind) {
    case "mermaid":
      return <Workflow className="size-3 text-zinc-500 dark:text-zinc-400" />;
    case "graphviz":
      return <GitFork className="size-3 text-zinc-500 dark:text-zinc-400" />;
    case "svg":
      return <ImageIcon className="size-3 text-zinc-500 dark:text-zinc-400" />;
  }
}

const DiagramLightbox = memo(function DiagramLightbox({
  open,
  title,
  svg,
  naturalWidth,
  rawSource,
  kind,
  onClose,
}: {
  open: boolean;
  title: string;
  svg: string;
  naturalWidth: number;
  rawSource: string;
  kind: DiagramKind;
  onClose: () => void;
}) {
  const [copiedMode, setCopiedMode] = useState<"none" | "svg" | "source">("none");
  const copyTimerRef = useRef<number | null>(null);
  const [viewportEl, setViewportEl] = useState<HTMLDivElement | null>(null);
  const diagramCardRef = useRef<HTMLDivElement | null>(null);

  const {
    zoom,
    pan,
    isDragging,
    setZoom,
    zoomIn,
    zoomOut,
    resetTransform,
    transformStyle,
    pointerHandlers,
  } = usePanZoom({
    open,
    minZoom: MIN_LIGHTBOX_ZOOM,
    maxZoom: MAX_LIGHTBOX_ZOOM,
    allowPanAtBaseZoom: true,
    zoomStep: 1.18,
    wheelMode: "always",
    contentRef: diagramCardRef,
    viewportEl,
  });

  useEffect(() => {
    return () => {
      if (copyTimerRef.current !== null) {
        window.clearTimeout(copyTimerRef.current);
        copyTimerRef.current = null;
      }
    };
  }, []);

  useEffect(() => {
    if (!open) return;
    resetTransform();
  }, [open, resetTransform]);

  useEffect(() => {
    if (!open) return;
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.metaKey || ev.ctrlKey || ev.altKey) return;
      if (ev.key === "+" || ev.key === "=") {
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
  }, [open, zoomIn, zoomOut, resetTransform]);

  const handleDownloadSvg = useCallback(() => {
    const blob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `orbit-${kind}-diagram.svg`;
    a.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }, [svg, kind]);

  const handleCopy = useCallback(
    async (mode: "svg" | "source") => {
      try {
        await navigator.clipboard.writeText(mode === "svg" ? svg : rawSource);
        setCopiedMode(mode);
        if (copyTimerRef.current !== null) {
          window.clearTimeout(copyTimerRef.current);
        }
        copyTimerRef.current = window.setTimeout(() => {
          copyTimerRef.current = null;
          setCopiedMode("none");
        }, 1500);
      } catch {
        // Clipboard permission denied or unavailable; ignore gracefully
      }
    },
    [svg, rawSource],
  );

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <DialogContent
        showCloseButton={false}
        className="bg-surface-base border-border-default flex h-[min(88vh,920px)] w-[min(94vw,1280px)] max-w-none flex-col gap-0 overflow-hidden rounded-[16px] border p-0"
      >
        <div className="border-border-subtle bg-surface-recessed/75 text-ink-primary flex shrink-0 items-center justify-between border-b px-4 py-2 select-none">
          <DialogTitle className="text-ink-primary flex items-center gap-2 text-[13px] font-medium">
            <DiagramKindIcon kind={kind} />
            <span>{title}</span>
          </DialogTitle>
          <div className="flex items-center gap-1">
            <button
              type="button"
              onClick={zoomOut}
              className="orbit-press text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-7 cursor-pointer items-center justify-center rounded-md"
              title="Zoom out (-)"
            >
              <ZoomOut className="size-3.5" />
            </button>
            <button
              type="button"
              onClick={resetTransform}
              title="Reset zoom (0)"
              className="tabular-nums orbit-press text-ink-secondary hover:bg-accent hover:text-ink-primary min-w-[44px] cursor-pointer rounded-md px-1.5 py-1 text-center font-mono text-[11px]"
            >
              {Math.round(zoom * 100)}%
            </button>
            <button
              type="button"
              onClick={zoomIn}
              className="orbit-press text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-7 cursor-pointer items-center justify-center rounded-md"
              title="Zoom in (+)"
            >
              <ZoomIn className="size-3.5" />
            </button>
            <button
              type="button"
              onClick={resetTransform}
              className="orbit-press text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-7 cursor-pointer items-center justify-center rounded-md"
              title="Reset view (0)"
            >
              <RotateCcw className="size-3.5" />
            </button>
            <div className="bg-border-subtle mx-1.5 h-3.5 w-px" />
            <button
              type="button"
              onClick={() => void handleCopy("svg")}
              className="orbit-press text-ink-secondary hover:bg-accent hover:text-ink-primary inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2 text-[11.5px]"
              title="Copy SVG"
            >
              {copiedMode === "svg" ? (
                <Check className="text-signal-success size-3.5" />
              ) : (
                <Copy className="size-3.5" />
              )}
              <span>{copiedMode === "svg" ? "Copied" : "SVG"}</span>
            </button>
            <button
              type="button"
              onClick={handleDownloadSvg}
              className="orbit-press text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-7 cursor-pointer items-center justify-center rounded-md"
              title="Download SVG"
            >
              <Download className="size-3.5" />
            </button>
            <button
              type="button"
              onClick={() => void handleCopy("source")}
              className="orbit-press text-ink-secondary hover:bg-accent hover:text-ink-primary inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2 text-[11.5px]"
              title="Copy source"
            >
              {copiedMode === "source" ? (
                <Check className="text-signal-success size-3.5" />
              ) : (
                <Code2 className="size-3.5" />
              )}
              <span>{copiedMode === "source" ? "Copied" : "Source"}</span>
            </button>
            <button
              type="button"
              onClick={onClose}
              className="orbit-press text-ink-muted hover:bg-accent hover:text-ink-primary ml-1 inline-flex size-7 cursor-pointer items-center justify-center rounded-md"
              title="Close (Esc)"
            >
              <X className="size-4" />
            </button>
          </div>
        </div>

        <div
          ref={setViewportEl}
          className={cn(
            "bg-surface-recessed/45 flex min-h-0 flex-1 touch-none items-center justify-center overflow-hidden p-8",
            isDragging ? "cursor-grabbing" : "cursor-grab",
          )}
          {...pointerHandlers}
          onDoubleClick={() => {
            if (zoom !== 1 || pan.x !== 0 || pan.y !== 0) {
              resetTransform();
            } else {
              setZoom(1.6);
            }
          }}
        >
          <div
            ref={diagramCardRef}
            className="border-border-subtle bg-surface-raised rounded-2xl border p-6 shadow-md select-none [&>svg]:block [&>svg]:h-auto [&>svg]:max-w-none"
            style={{
              minWidth: Math.min(Math.max(naturalWidth, 320), 960),
              ...transformStyle,
              transformOrigin: "center center",
              willChange: "transform",
            }}
            dangerouslySetInnerHTML={{ __html: svg }}
          />
        </div>
      </DialogContent>
    </Dialog>
  );
});

export interface DiagramBlockProps {
  kind: DiagramKind;
  block: Extract<RichBlockView, { kind: "code_block" }>;
  isDark: boolean;
  isStreamingBlock: boolean;
}

export const DiagramBlock = memo(
  function DiagramBlock({ kind, block, isDark, isStreamingBlock }: DiagramBlockProps) {
    const theme: DiagramTheme = isDark ? "dark" : "light";
    const key = useMemo(
      () => diagramCacheKey(kind, theme, block.raw_code),
      [kind, theme, block.raw_code],
    );

    const cardRef = useRef<HTMLDivElement | null>(null);
    const [nearViewport, setNearViewport] = useState<boolean>(
      () => getCachedDiagram(key) !== undefined,
    );
    const [viewMode, setViewMode] = useState<"diagram" | "code">("diagram");
    const [rendered, setRendered] = useState<RenderDiagramView | null>(
      () => getCachedDiagram(key) ?? null,
    );
    const [lightboxOpen, setLightboxOpen] = useState(false);
    const [copied, setCopied] = useState(false);
    const copyTimerRef = useRef<number | null>(null);

    useEffect(() => {
      return () => {
        if (copyTimerRef.current !== null) {
          window.clearTimeout(copyTimerRef.current);
          copyTimerRef.current = null;
        }
      };
    }, []);

    useEffect(() => {
      const cached = getCachedDiagram(key);
      if (cached !== undefined) {
        setRendered(cached);
        setNearViewport(true);
      }
    }, [key]);

    useEffect(() => {
      if (nearViewport) return;
      const el = cardRef.current;
      if (!el || typeof IntersectionObserver === "undefined") {
        setNearViewport(true);
        return;
      }
      const obs = new IntersectionObserver(
        (entries) => {
          if (entries.some((e) => e.isIntersecting)) {
            setNearViewport(true);
            obs.disconnect();
          }
        },
        { rootMargin: "320px 0px" },
      );
      obs.observe(el);
      return () => obs.disconnect();
    }, [nearViewport]);

    useEffect(() => {
      if (isStreamingBlock || !nearViewport) return;
      const cached = getCachedDiagram(key);
      if (cached !== undefined) {
        setRendered(cached);
        return;
      }
      let cancelled = false;
      void orbitIpc
        .renderDiagram({
          kind,
          source: block.raw_code,
          theme,
        })
        .then((res) => {
          setCachedDiagram(key, res);
          if (!cancelled) {
            setRendered(res);
          }
        })
        .catch((err: unknown) => {
          if (!cancelled) {
            setRendered({
              kind,
              theme,
              svg: null,
              width: 0,
              height: 0,
              error: err instanceof Error ? err.message : "Failed to render diagram",
              diagram_type_label: null,
            });
          }
        });
      return () => {
        cancelled = true;
      };
    }, [isStreamingBlock, nearViewport, key, kind, block.raw_code, theme]);

    const svgReady = !isStreamingBlock && rendered?.svg != null && rendered.error == null;
    const showDiagram = viewMode === "diagram" && svgReady;

    const handleCopy = async () => {
      try {
        await navigator.clipboard.writeText(block.raw_code);
        setCopied(true);
        if (copyTimerRef.current !== null) {
          window.clearTimeout(copyTimerRef.current);
        }
        copyTimerRef.current = window.setTimeout(() => {
          copyTimerRef.current = null;
          setCopied(false);
        }, 1500);
      } catch {
        // Clipboard permission denied or unavailable; ignore gracefully
      }
    };

    const baseTitle = kindBaseTitle(kind);
    const headerTitle = rendered?.diagram_type_label
      ? rendered.diagram_type_label === baseTitle
        ? baseTitle
        : `${baseTitle} · ${rendered.diagram_type_label}`
      : baseTitle;

    const estimatedIntrinsicHeight = Math.min(
      Math.max(rendered ? Math.round(rendered.height * 0.65 + 40) : 200, 136),
      560,
    );

    const keepToolbarVisible = !showDiagram;

    return (
      <div
        ref={cardRef}
        style={
          !isStreamingBlock
            ? { containIntrinsicBlockSize: `auto ${estimatedIntrinsicHeight}px` }
            : undefined
        }
        className={cn(
          "group relative my-3 overflow-hidden rounded-xl border border-black/[0.075] bg-white text-zinc-800 dark:border-white/[0.08] dark:bg-[#141519] dark:text-zinc-200",
          !isStreamingBlock ? "orbit-heavy-block-settled" : "[contain:layout_style_paint]",
        )}
      >
        {/* Subtle top-left type badge (hidden until hover in diagram view for zero clutter) */}
        <div
          className={cn(
            "pointer-events-none absolute top-2.5 left-3 z-10 flex items-center gap-1.5 transition-opacity duration-150 select-none",
            showDiagram
              ? "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100"
              : "opacity-100",
          )}
        >
          <span className="border-border/75 bg-background/88 text-muted-foreground inline-flex items-center gap-1.5 rounded-md border px-2 py-0.5 font-mono text-[10.5px] shadow-2xs backdrop-blur-md">
            <DiagramKindIcon kind={kind} />
            <span className="max-w-[180px] truncate">{headerTitle}</span>
          </span>
          {isStreamingBlock && (
            <span className="rounded-md bg-black/[0.06] px-1.5 py-0.5 text-[10.5px] text-zinc-500 dark:bg-white/[0.08] dark:text-zinc-400">
              Streaming…
            </span>
          )}
          {!isStreamingBlock && rendered?.error && (
            <span
              title={rendered.error}
              className="rounded-md bg-amber-500/12 px-1.5 py-0.5 text-[10.5px] text-amber-700 dark:text-amber-300"
            >
              Source
            </span>
          )}
        </div>

        {/* Restrained 3-action floating hover toolbar (zoom & SVG export live inside fullscreen Lightbox) */}
        <div
          className={cn(
            "absolute top-2.5 right-2.5 z-10 flex items-center gap-0.5 rounded-lg border border-black/[0.08] bg-white/90 p-0.5 shadow-xs backdrop-blur-md transition-opacity duration-150 select-none dark:border-white/[0.1] dark:bg-zinc-900/90",
            keepToolbarVisible
              ? "pointer-events-auto opacity-100"
              : "pointer-events-none opacity-0 group-focus-within:pointer-events-auto group-focus-within:opacity-100 group-hover:pointer-events-auto group-hover:opacity-100",
          )}
        >
          <button
            type="button"
            onClick={() => void handleCopy()}
            title="Copy source"
            className="text-muted-foreground hover:text-foreground orbit-press inline-flex size-6 cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] dark:hover:bg-white/[0.07]"
          >
            {copied ? (
              <Check className="size-3.5 text-emerald-600 dark:text-emerald-400" />
            ) : (
              <Copy className="size-3.5" />
            )}
          </button>

          {svgReady && (
            <button
              type="button"
              onClick={() => setViewMode((m) => (m === "diagram" ? "code" : "diagram"))}
              title={viewMode === "diagram" ? "View source" : "View diagram"}
              className={cn(
                "orbit-press inline-flex size-6 cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] dark:hover:bg-white/[0.07]",
                viewMode === "code"
                  ? "bg-black/[0.06] text-foreground dark:bg-white/[0.1]"
                  : "text-muted-foreground hover:text-foreground",
              )}
            >
              {viewMode === "diagram" ? <Code2 className="size-3.5" /> : <Eye className="size-3.5" />}
            </button>
          )}

          {showDiagram && (
            <button
              type="button"
              onClick={() => setLightboxOpen(true)}
              title="Fullscreen"
              className="text-muted-foreground hover:text-foreground orbit-press inline-flex size-6 cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] dark:hover:bg-white/[0.07]"
            >
              <Maximize2 className="size-3.5" />
            </button>
          )}
        </div>

        {showDiagram && rendered?.svg ? (
          <ScrollArea orientation="horizontal" viewportClassName="px-5 py-4">
            <div
              onDoubleClick={() => setLightboxOpen(true)}
              title="Double-click to expand"
              className="flex min-w-full cursor-zoom-in items-center justify-center transition-opacity duration-150 ease-out"
              style={{
                minHeight: Math.min(Math.max(Math.round(rendered.height * 0.4), 76), 280),
              }}
            >
              <div
                className="mx-auto flex items-center justify-center [&>svg]:block [&>svg]:h-auto [&>svg]:max-w-full"
                dangerouslySetInnerHTML={{ __html: rendered.svg }}
              />
            </div>
          </ScrollArea>
        ) : (
          <div className="pt-8">
            <HighlightedCodeBlock
              kind="code_block"
              language={block.language}
              raw_code={block.raw_code}
              lines={block.lines}
              isDark={isDark}
              embedded
            />
          </div>
        )}

        {rendered?.svg && (
          <DiagramLightbox
            open={lightboxOpen}
            title={headerTitle}
            svg={rendered.svg}
            naturalWidth={rendered.width}
            rawSource={block.raw_code}
            kind={kind}
            onClose={() => setLightboxOpen(false)}
          />
        )}
      </div>
    );
  },
  (prev, next) =>
    prev.kind === next.kind &&
    prev.isDark === next.isDark &&
    prev.isStreamingBlock === next.isStreamingBlock &&
    prev.block.language === next.block.language &&
    prev.block.raw_code === next.block.raw_code &&
    prev.block.lines.length === next.block.lines.length,
);
