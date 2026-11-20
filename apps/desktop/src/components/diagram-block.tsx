import {
  type PointerEvent as ReactPointerEvent,
  type WheelEvent as ReactWheelEvent,
  memo,
  useCallback,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { createPortal } from "react-dom";
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
import { ScrollArea } from "@/components/ui/scroll-area";
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

function diagramCacheKey(kind: DiagramKind, theme: DiagramTheme, source: string): string {
  return `${kind}::${theme}::${source}`;
}

function getCachedDiagram(key: string): RenderDiagramView | undefined {
  const hit = diagramViewCache.get(key);
  if (hit !== undefined) {
    // Refresh LRU recency
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
      return <Workflow className="size-3.5 text-zinc-500 dark:text-zinc-400" />;
    case "graphviz":
      return <GitFork className="size-3.5 text-zinc-500 dark:text-zinc-400" />;
    case "svg":
      return <ImageIcon className="size-3.5 text-zinc-500 dark:text-zinc-400" />;
  }
}

const DiagramLightbox = memo(function DiagramLightbox({
  title,
  svg,
  naturalWidth,
  rawSource,
  kind,
  onClose,
}: {
  title: string;
  svg: string;
  naturalWidth: number;
  rawSource: string;
  kind: DiagramKind;
  onClose: () => void;
}) {
  const [zoom, setZoom] = useState(1);
  const [pan, setPan] = useState<{ x: number; y: number }>({ x: 0, y: 0 });
  const [copied, setCopied] = useState(false);
  const dragRef = useRef<{
    active: boolean;
    pointerId: number;
    startX: number;
    startY: number;
    origX: number;
    origY: number;
  }>({
    active: false,
    pointerId: -1,
    startX: 0,
    startY: 0,
    origX: 0,
    origY: 0,
  });

  useEffect(() => {
    const onKeyDown = (ev: KeyboardEvent) => {
      if (ev.key === "Escape") {
        ev.preventDefault();
        onClose();
      } else if (ev.key === "+" || ev.key === "=") {
        setZoom((z) => Math.min(4, Math.round((z + 0.2) * 100) / 100));
      } else if (ev.key === "-") {
        setZoom((z) => Math.max(0.25, Math.round((z - 0.2) * 100) / 100));
      } else if (ev.key === "0") {
        setZoom(1);
        setPan({ x: 0, y: 0 });
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [onClose]);

  const handleWheel = useCallback((ev: ReactWheelEvent<HTMLDivElement>) => {
    if (ev.ctrlKey || ev.metaKey || Math.abs(ev.deltaY) > 0) {
      ev.preventDefault();
      const step = ev.deltaY < 0 ? 0.12 : -0.12;
      setZoom((z) => Math.min(4, Math.max(0.25, Math.round((z + step) * 100) / 100)));
    }
  }, []);

  const handlePointerDown = useCallback(
    (ev: ReactPointerEvent<HTMLDivElement>) => {
      if (ev.button !== 0) return;
      ev.currentTarget.setPointerCapture(ev.pointerId);
      dragRef.current = {
        active: true,
        pointerId: ev.pointerId,
        startX: ev.clientX,
        startY: ev.clientY,
        origX: pan.x,
        origY: pan.y,
      };
    },
    [pan],
  );

  const handlePointerMove = useCallback((ev: ReactPointerEvent<HTMLDivElement>) => {
    const d = dragRef.current;
    if (!d.active || d.pointerId !== ev.pointerId) return;
    setPan({
      x: d.origX + (ev.clientX - d.startX),
      y: d.origY + (ev.clientY - d.startY),
    });
  }, []);

  const handlePointerUp = useCallback((ev: ReactPointerEvent<HTMLDivElement>) => {
    if (dragRef.current.pointerId === ev.pointerId) {
      dragRef.current.active = false;
    }
  }, []);

  const handleDownloadSvg = useCallback(() => {
    const blob = new Blob([svg], { type: "image/svg+xml;charset=utf-8" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `orbit-${kind}-diagram.svg`;
    a.click();
    window.setTimeout(() => URL.revokeObjectURL(url), 1000);
  }, [svg, kind]);

  const handleCopySource = useCallback(async () => {
    await navigator.clipboard.writeText(rawSource);
    setCopied(true);
    window.setTimeout(() => setCopied(false), 1500);
  }, [rawSource]);

  return createPortal(
    <div
      className="fixed inset-0 z-50 flex flex-col bg-black/65 backdrop-blur-xs"
      onClick={(ev) => {
        if (ev.target === ev.currentTarget) onClose();
      }}
    >
      <div className="flex shrink-0 items-center justify-between border-b border-white/10 bg-zinc-950/85 px-4 py-2.5 text-zinc-100 select-none">
        <div className="flex items-center gap-2 text-[13px] font-medium">
          <DiagramKindIcon kind={kind} />
          <span>{title}</span>
        </div>
        <div className="flex items-center gap-1.5">
          <button
            type="button"
            onClick={() => setZoom((z) => Math.max(0.25, Math.round((z - 0.2) * 100) / 100))}
            className="orbit-press inline-flex size-7 cursor-pointer items-center justify-center rounded-md text-zinc-300 hover:bg-white/10 hover:text-white"
            title="Zoom out (-)"
          >
            <ZoomOut className="size-3.5" />
          </button>
          <span className="tabular-nums min-w-[44px] text-center font-mono text-[11.5px] text-zinc-300">
            {Math.round(zoom * 100)}%
          </span>
          <button
            type="button"
            onClick={() => setZoom((z) => Math.min(4, Math.round((z + 0.2) * 100) / 100))}
            className="orbit-press inline-flex size-7 cursor-pointer items-center justify-center rounded-md text-zinc-300 hover:bg-white/10 hover:text-white"
            title="Zoom in (+)"
          >
            <ZoomIn className="size-3.5" />
          </button>
          <button
            type="button"
            onClick={() => {
              setZoom(1);
              setPan({ x: 0, y: 0 });
            }}
            className="orbit-press inline-flex size-7 cursor-pointer items-center justify-center rounded-md text-zinc-300 hover:bg-white/10 hover:text-white"
            title="Reset view (0)"
          >
            <RotateCcw className="size-3.5" />
          </button>
          <div className="mx-1 h-4 w-px bg-white/15" />
          <button
            type="button"
            onClick={handleDownloadSvg}
            className="orbit-press inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2 text-[12px] text-zinc-300 hover:bg-white/10 hover:text-white"
          >
            <Download className="size-3.5" />
            <span>SVG</span>
          </button>
          <button
            type="button"
            onClick={() => void handleCopySource()}
            className="orbit-press inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2 text-[12px] text-zinc-300 hover:bg-white/10 hover:text-white"
          >
            {copied ? <Check className="size-3.5 text-emerald-400" /> : <Copy className="size-3.5" />}
            <span>{copied ? "Copied" : "Copy"}</span>
          </button>
          <button
            type="button"
            onClick={onClose}
            className="orbit-press ml-1 inline-flex size-7 cursor-pointer items-center justify-center rounded-md text-zinc-300 hover:bg-white/10 hover:text-white"
            title="Close (Esc)"
          >
            <X className="size-4" />
          </button>
        </div>
      </div>

      <div
        className="flex min-h-0 flex-1 cursor-grab items-center justify-center overflow-hidden p-8 active:cursor-grabbing"
        onWheel={handleWheel}
        onPointerDown={handlePointerDown}
        onPointerMove={handlePointerMove}
        onPointerUp={handlePointerUp}
        onClick={(ev) => {
          if (ev.target === ev.currentTarget && pan.x === 0 && pan.y === 0) onClose();
        }}
      >
        <div
          className="border-border bg-card rounded-2xl border p-6 shadow-2xl select-none [&>svg]:block [&>svg]:h-auto [&>svg]:max-w-none"
          style={{
            minWidth: Math.min(Math.max(naturalWidth, 320), 960),
            transform: `translate3d(${pan.x}px, ${pan.y}px, 0) scale(${zoom})`,
            transformOrigin: "center center",
            willChange: "transform",
          }}
          dangerouslySetInnerHTML={{ __html: svg }}
        />
      </div>
    </div>,
    document.body,
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
    const [nearViewport, setNearViewport] = useState<boolean>(() => getCachedDiagram(key) !== undefined);
    const [viewMode, setViewMode] = useState<"diagram" | "code">("diagram");
    const [rendered, setRendered] = useState<RenderDiagramView | null>(
      () => getCachedDiagram(key) ?? null,
    );
    const [lightboxOpen, setLightboxOpen] = useState(false);
    const [copied, setCopied] = useState(false);

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
        });
      return () => {
        cancelled = true;
      };
    }, [isStreamingBlock, nearViewport, key, kind, block.raw_code, theme]);

    const handleCopy = async () => {
      await navigator.clipboard.writeText(block.raw_code);
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    };

    const handleDownloadSvg = () => {
      if (!rendered?.svg) return;
      const blob = new Blob([rendered.svg], { type: "image/svg+xml;charset=utf-8" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `orbit-${kind}-diagram.svg`;
      a.click();
      window.setTimeout(() => URL.revokeObjectURL(url), 1000);
    };

    const baseTitle = kindBaseTitle(kind);
    const headerTitle = rendered?.diagram_type_label
      ? rendered.diagram_type_label === baseTitle
        ? baseTitle
        : `${baseTitle} · ${rendered.diagram_type_label}`
      : baseTitle;

    const svgReady = !isStreamingBlock && rendered?.svg != null && rendered.error == null;
    const showDiagram = viewMode === "diagram" && svgReady;

    const estimatedIntrinsicHeight = Math.min(
      Math.max(rendered ? Math.round(rendered.height * 0.65 + 48) : 220, 140),
      560,
    );

    return (
      <div
        ref={cardRef}
        style={
          !isStreamingBlock
            ? { containIntrinsicBlockSize: `auto ${estimatedIntrinsicHeight}px` }
            : undefined
        }
        className={cn(
          "border-border my-3 overflow-hidden rounded-xl border bg-zinc-50 text-zinc-800 dark:bg-[#141519] dark:text-zinc-200",
          !isStreamingBlock ? "orbit-heavy-block-settled" : "[contain:layout_style_paint]",
        )}
      >
        <div className="border-border text-muted-foreground flex items-center justify-between gap-2 border-b bg-black/[0.02] px-3.5 py-1.5 text-xs select-none dark:bg-white/[0.02]">
          <div className="flex min-w-0 items-center gap-2">
            <DiagramKindIcon kind={kind} />
            <span className="truncate font-mono text-[11.5px]">{headerTitle}</span>
            {isStreamingBlock && (
              <span className="rounded-full bg-black/[0.06] px-2 py-0.5 text-[10.5px] text-zinc-500 dark:bg-white/[0.08] dark:text-zinc-400">
                Streaming…
              </span>
            )}
            {!isStreamingBlock && rendered?.error && (
              <span
                title={rendered.error}
                className="rounded-full bg-amber-500/12 px-2 py-0.5 text-[10.5px] text-amber-700 dark:text-amber-300"
              >
                Code view
              </span>
            )}
          </div>

          <div className="flex shrink-0 items-center gap-1">
            {svgReady && (
              <>
                <div className="border-border/80 mr-1 inline-flex items-center rounded-md border bg-black/[0.03] p-0.5 dark:bg-white/[0.04]">
                  <button
                    type="button"
                    onClick={() => setViewMode("diagram")}
                    className={cn(
                      "orbit-press inline-flex cursor-pointer items-center gap-1 rounded-[4px] px-1.5 py-0.5 text-[11px] transition-colors",
                      viewMode === "diagram"
                        ? "bg-background text-foreground font-medium shadow-2xs"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    <Eye className="size-3" />
                    <span>Diagram</span>
                  </button>
                  <button
                    type="button"
                    onClick={() => setViewMode("code")}
                    className={cn(
                      "orbit-press inline-flex cursor-pointer items-center gap-1 rounded-[4px] px-1.5 py-0.5 text-[11px] transition-colors",
                      viewMode === "code"
                        ? "bg-background text-foreground font-medium shadow-2xs"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    <Code2 className="size-3" />
                    <span>Code</span>
                  </button>
                </div>

                <button
                  type="button"
                  onClick={handleDownloadSvg}
                  title="Download SVG"
                  className="text-muted-foreground hover:text-foreground orbit-press inline-flex size-6 cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] dark:hover:bg-white/[0.06]"
                >
                  <Download className="size-3.5" />
                </button>
                <button
                  type="button"
                  onClick={() => setLightboxOpen(true)}
                  title="Fullscreen interactive view"
                  className="text-muted-foreground hover:text-foreground orbit-press inline-flex size-6 cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.05] dark:hover:bg-white/[0.06]"
                >
                  <Maximize2 className="size-3.5" />
                </button>
              </>
            )}

            <button
              type="button"
              onClick={() => void handleCopy()}
              className="text-muted-foreground hover:text-foreground orbit-press inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] hover:bg-black/[0.05] dark:hover:bg-white/[0.06]"
            >
              <span className="relative inline-flex size-3.5 items-center justify-center">
                <Copy
                  className={`size-3.5 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] ${
                    copied ? "scale-50 -rotate-12 opacity-0" : "scale-100 rotate-0 opacity-100"
                  }`}
                />
                <Check
                  className={`absolute size-3.5 text-emerald-600 transition-[transform,opacity] duration-150 ease-[var(--ease-out-expo)] dark:text-emerald-400 ${
                    copied ? "scale-100 rotate-0 opacity-100" : "scale-50 rotate-12 opacity-0"
                  }`}
                />
              </span>
              <span>{copied ? "Copied" : "Copy"}</span>
            </button>
          </div>
        </div>

        {showDiagram && rendered?.svg ? (
          <ScrollArea
            orientation="horizontal"
            viewportClassName="p-4"
          >
            <div
              onDoubleClick={() => setLightboxOpen(true)}
              title="Double-click to expand"
              className="flex min-w-full cursor-zoom-in justify-center transition-opacity duration-150 ease-out [&>svg]:block [&>svg]:h-auto [&>svg]:max-w-full"
              style={{
                minHeight: Math.min(Math.max(rendered.height * 0.4, 96), 240),
              }}
              dangerouslySetInnerHTML={{ __html: rendered.svg }}
            />
          </ScrollArea>
        ) : (
          <HighlightedCodeBlock
            kind="code_block"
            language={block.language}
            raw_code={block.raw_code}
            lines={block.lines}
            isDark={isDark}
            embedded
          />
        )}

        {lightboxOpen && rendered?.svg && (
          <DiagramLightbox
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
