import { memo, useEffect, useState } from "react";
import {
  FileDiff,
  FileText,
  Film,
  GitCompareArrows,
  Image as ImageIcon,
  Loader2,
  Music,
  Quote,
  SquareTerminal,
  X,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { getCachedPromptImage, loadPromptImage } from "@/lib/prompt-image-cache";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { symbolKindGlyph } from "@/lib/symbol-mentions";
import {
  type ContextMentionItem,
  type DiffQuoteSnippet,
  type PastedAttachment,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";

interface ContextTrayProps {
  contexts: readonly ContextMentionItem[];
  quotes: readonly DiffQuoteSnippet[];
  pastes: readonly PastedAttachment[];
}

const ComposerImageThumb = memo(function ComposerImageThumb({
  img,
  hostId,
  cwd,
}: {
  img: Extract<ContextMentionItem, { kind: "image" }>;
  hostId: string | null;
  cwd: string;
}) {
  const effectiveHostId = img.stagedHostId || hostId || "";
  const resolveSyncUrl = (): string | null => {
    if (img.previewUrl) return img.previewUrl;
    if (effectiveHostId && img.path) {
      const cached = getCachedPromptImage(effectiveHostId, img.path);
      if (cached?.status === "ready") return cached.url;
    }
    return null;
  };

  const [resolvedUrl, setResolvedUrl] = useState<string | null>(resolveSyncUrl);
  const [loadFailed, setLoadFailed] = useState(false);

  useEffect(() => {
    const sync = resolveSyncUrl();
    if (sync) {
      setResolvedUrl(sync);
      setLoadFailed(false);
      return;
    }
    if (!effectiveHostId || !img.path) {
      setResolvedUrl(null);
      setLoadFailed(true);
      return;
    }
    let cancelled = false;
    setLoadFailed(false);
    void loadPromptImage(effectiveHostId, cwd || "~", img.path).then((entry) => {
      if (cancelled) return;
      if (entry.status === "ready") {
        setResolvedUrl(entry.url);
      } else {
        setLoadFailed(true);
      }
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [img.previewUrl, img.path, effectiveHostId, cwd]);

  const sizeHint =
    img.sizeBytes > 0 ? ` (${Math.max(1, Math.round(img.sizeBytes / 1024))} KB)` : "";

  return (
    <div
      title={`${img.filename}${sizeHint}${img.uploading ? " · Uploading…" : ""}`}
      className="orbit-pill-enter group border-border bg-muted/50 relative size-12 shrink-0 overflow-hidden rounded-xl border"
    >
      {resolvedUrl ? (
        <img
          src={resolvedUrl}
          alt={img.filename}
          className="size-full object-cover"
          onError={() => {
            setResolvedUrl(null);
            setLoadFailed(true);
          }}
        />
      ) : (
        <div className="text-muted-foreground/70 flex size-full items-center justify-center">
          {loadFailed ? (
            <ImageIcon className="size-4 stroke-[1.6]" />
          ) : (
            <Loader2 className="size-3.5 animate-spin" />
          )}
        </div>
      )}
      {img.uploading && (
        <div className="absolute inset-0 flex items-center justify-center bg-black/45">
          <Loader2 className="size-4 animate-spin text-white" />
        </div>
      )}
      <button
        type="button"
        onClick={() => orbitActions.removeContextMention(img.id)}
        aria-label={`Remove ${img.filename}`}
        className="bg-background/90 text-foreground border-border absolute top-0.5 right-0.5 flex size-4.5 cursor-pointer items-center justify-center rounded-full border opacity-0 shadow-xs transition-opacity group-hover:opacity-100"
      >
        <X className="size-2.5" />
      </button>
    </div>
  );
});

function splitPathDisplay(rawPath: string): { basename: string; dirHint: string | null } {
  const normalized = rawPath.replace(/\\/g, "/");
  const idx = normalized.lastIndexOf("/");
  if (idx <= 0) {
    return { basename: normalized, dirHint: null };
  }
  const basename = normalized.slice(idx + 1);
  const parent = normalized.slice(0, idx);
  const parts = parent.split("/").filter(Boolean);
  const shortDir = parts.length > 2 ? `…/${parts.slice(-2).join("/")}` : parent;
  return { basename, dirHint: shortDir };
}

export const ContextTray = memo(function ContextTray({
  contexts,
  quotes,
  pastes,
}: ContextTrayProps) {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  if (contexts.length === 0 && quotes.length === 0 && pastes.length === 0) {
    return null;
  }

  const images = contexts.filter(
    (c): c is Extract<ContextMentionItem, { kind: "image" }> => c.kind === "image",
  );
  const nonImages = contexts.filter((c) => c.kind !== "image");

  return (
    <div className="flex flex-wrap items-center gap-1.5 px-[14px] pt-2.5 pb-0.5">
      {/* 1. Image Thumbnails (48×48) */}
      {images.map((img) => (
        <ComposerImageThumb
          key={img.id}
          img={img}
          hostId={selectedHostId}
          cwd={selectedCwd || "~"}
        />
      ))}

      {/* 2. Structured Context Pills (@file, @diff, @terminal) */}
      {nonImages.map((item) => {
        if (item.kind === "file") {
          const displayTarget = item.filename || item.path || "Attachment";
          const { basename, dirHint } = splitPathDisplay(
            item.mediaKind ? displayTarget : item.path || item.filename || "Attachment",
          );
          const MediaIcon =
            item.mediaKind === "pdf"
              ? FileText
              : item.mediaKind === "video"
                ? Film
                : item.mediaKind === "audio"
                  ? Music
                  : resolveSemanticFileIcon(basename);
          const isPreviewableMedia = Boolean(item.mediaKind);
          const metaLabelParts: string[] = [];
          if (item.mediaKind === "pdf" && item.pdfPageCount && item.pdfPageCount > 0) {
            metaLabelParts.push(
              item.pdfPageCount === 1 ? "1p" : `${item.pdfPageCount}p`,
            );
          }
          if (item.sizeBytes != null && item.sizeBytes > 0) {
            metaLabelParts.push(formatFileSizeCompact(item.sizeBytes));
          }
          const metaSummary = metaLabelParts.join(" · ");

          if (isPreviewableMedia && item.mediaKind) {
            const mediaKind = item.mediaKind;
            return (
              <span
                key={item.id}
                title={
                  item.uploading
                    ? `Uploading ${basename}…`
                    : `${basename}${metaSummary ? ` (${metaSummary})` : ""} — click to preview`
                }
                className="orbit-pill-enter border-border bg-muted/65 text-foreground hover:bg-accent/70 inline-flex max-w-76 items-center gap-1.5 rounded-lg border px-2 py-1 text-[12px] transition-colors"
              >
                <button
                  type="button"
                  onClick={() =>
                    orbitActions.openMediaPreview({
                      kind: mediaKind,
                      filename: basename,
                      path: item.path || null,
                      hostId: item.stagedHostId ?? selectedHostId,
                      cwd: selectedCwd || "~",
                      localUrl: item.previewUrl ?? null,
                      mimeType: item.mimeType ?? null,
                      sizeBytes: item.sizeBytes ?? null,
                      pdfPageCount: item.pdfPageCount ?? null,
                    })
                  }
                  className="flex min-w-0 cursor-pointer items-center gap-1.5 text-left"
                >
                  {item.uploading ? (
                    <Loader2 className="text-muted-foreground size-3.5 shrink-0 animate-spin stroke-[1.75]" />
                  ) : (
                    <MediaIcon className="text-foreground/70 size-3.5 shrink-0 stroke-[1.75]" />
                  )}
                  <span className="truncate font-mono font-medium">{basename}</span>
                  {metaSummary && (
                    <span className="text-muted-foreground tabular-nums shrink-0 text-[11px]">
                      {metaSummary}
                    </span>
                  )}
                </button>
                <button
                  type="button"
                  aria-label={`Remove ${basename}`}
                  onClick={(e) => {
                    e.stopPropagation();
                    orbitActions.removeContextMention(item.id);
                  }}
                  className="text-muted-foreground hover:text-foreground ml-0.5 cursor-pointer rounded-xs"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          return (
            <span
              key={item.id}
              title={item.uploading ? `Uploading ${basename}…` : displayTarget}
              className="orbit-pill-enter border-border bg-muted/65 text-foreground inline-flex max-w-68 items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px] transition-colors"
            >
              {item.uploading ? (
                <Loader2 className="text-muted-foreground size-3.5 shrink-0 animate-spin stroke-[1.75]" />
              ) : (
                <MediaIcon className="text-foreground/65 size-3.5 shrink-0 stroke-[1.75]" />
              )}
              <span className="truncate font-mono font-medium">{basename}</span>
              {dirHint && (
                <span className="text-muted-foreground hidden truncate font-mono text-[11px] sm:inline">
                  {dirHint}
                </span>
              )}
              <button
                type="button"
                onClick={() => orbitActions.removeContextMention(item.id)}
                className="text-muted-foreground hover:text-foreground ml-0.5 cursor-pointer rounded-xs"
              >
                <X className="size-3" />
              </button>
            </span>
          );
        }

        if (item.kind === "git_diff") {
          const DiffIcon = item.staged ? GitCompareArrows : FileDiff;
          return (
            <span
              key={item.id}
              className="orbit-pill-enter border-border bg-muted/65 text-foreground inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px]"
            >
              <DiffIcon className="text-foreground/65 size-3.5 shrink-0 stroke-[1.75]" />
              <span className="font-medium">{item.label}</span>
              <button
                type="button"
                onClick={() => orbitActions.removeContextMention(item.id)}
                className="text-muted-foreground hover:text-foreground ml-0.5 cursor-pointer rounded-xs"
              >
                <X className="size-3" />
              </button>
            </span>
          );
        }

        if (item.kind === "terminal_output") {
          return (
            <span
              key={item.id}
              className="orbit-pill-enter border-border bg-muted/65 text-foreground inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px]"
            >
              <SquareTerminal className="text-foreground/65 size-3.5 shrink-0 stroke-[1.75]" />
              <span className="font-medium">{item.label}</span>
              <button
                type="button"
                onClick={() => orbitActions.removeContextMention(item.id)}
                className="text-muted-foreground hover:text-foreground ml-0.5 cursor-pointer rounded-xs"
              >
                <X className="size-3" />
              </button>
            </span>
          );
        }

        if (item.kind === "code_symbol") {
          const glyphMeta = symbolKindGlyph(item.symbolKind);
          return (
            <span
              key={item.id}
              title={item.signature || `${item.symbolName} (${item.path}:L${item.startLine})`}
              className="orbit-pill-enter border-border bg-muted/65 text-foreground inline-flex max-w-68 items-center gap-1.5 rounded-lg border px-2 py-1 text-[12px]"
            >
              <span className="bg-background/80 text-muted-foreground inline-flex size-4 shrink-0 items-center justify-center rounded font-mono text-[10.5px] font-semibold">
                {glyphMeta.glyph}
              </span>
              <span className="truncate font-mono font-medium">{item.symbolName}</span>
              <span className="text-muted-foreground shrink-0 font-mono text-[11px]">
                :L{item.startLine}
              </span>
              <button
                type="button"
                onClick={() => orbitActions.removeContextMention(item.id)}
                className="text-muted-foreground hover:text-foreground ml-0.5 cursor-pointer rounded-xs"
              >
                <X className="size-3" />
              </button>
            </span>
          );
        }

        return null;
      })}

      {/* 3. Diff Line Quotes */}
      {quotes.map((q) => (
        <span
          key={q.id}
          title={q.code}
          className="orbit-pill-enter border-border bg-muted/65 text-foreground inline-flex max-w-68 items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px]"
        >
          <Quote className="text-foreground/65 size-3 shrink-0" />
          <span className="truncate font-mono">
            {q.path}
            {q.lineNumber !== null ? `:${q.lineNumber}` : ""}
          </span>
          <button
            type="button"
            onClick={() => orbitActions.removeDiffQuote(q.id)}
            className="text-muted-foreground hover:text-foreground ml-0.5 cursor-pointer rounded-xs"
          >
            <X className="size-3" />
          </button>
        </span>
      ))}

      {/* 4. Collapsed Large Pasted Snippets & Attached Text Files */}
      {pastes.map((p) => {
        const PasteIcon = p.filename ? resolveSemanticFileIcon(p.filename) : FileText;
        return (
          <span
            key={p.id}
            title={`${p.content.slice(0, 280)}${p.content.length > 280 ? "…" : ""}`}
            className="orbit-pill-enter border-border bg-muted/65 text-foreground inline-flex items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px]"
          >
            <PasteIcon className="text-foreground/65 size-3.5 shrink-0 stroke-[1.75]" />
            <span className="font-medium">{p.label}</span>
            <button
              type="button"
              onClick={() => orbitActions.removePastedAttachment(p.id)}
              className="text-muted-foreground hover:text-foreground ml-0.5 cursor-pointer rounded-xs"
            >
              <X className="size-3" />
            </button>
          </span>
        );
      })}
    </div>
  );
});
