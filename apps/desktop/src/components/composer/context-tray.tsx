import { memo } from "react";
import {
  FileDiff,
  FileText,
  GitCompareArrows,
  Loader2,
  Quote,
  SquareTerminal,
  X,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import {
  type ContextMentionItem,
  type DiffQuoteSnippet,
  type PastedAttachment,
  orbitActions,
} from "@/state/orbit-store";

interface ContextTrayProps {
  contexts: readonly ContextMentionItem[];
  quotes: readonly DiffQuoteSnippet[];
  pastes: readonly PastedAttachment[];
}

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
        <div
          key={img.id}
          title={`${img.filename} (${Math.max(1, Math.round(img.sizeBytes / 1024))} KB)${img.uploading ? " · Uploading…" : ""}`}
          className="orbit-pill-enter group border-border bg-muted/50 relative size-12 shrink-0 overflow-hidden rounded-xl border"
        >
          <img
            src={img.previewUrl}
            alt={img.filename}
            className="size-full object-cover"
          />
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
      ))}

      {/* 2. Structured Context Pills (@file, @diff, @terminal) */}
      {nonImages.map((item) => {
        if (item.kind === "file") {
          const displayTarget = item.path || item.filename || "Attachment";
          const { basename, dirHint } = splitPathDisplay(displayTarget);
          const FileIcon = resolveSemanticFileIcon(basename);
          return (
            <span
              key={item.id}
              title={item.uploading ? `Uploading ${basename}…` : displayTarget}
              className="orbit-pill-enter border-border bg-muted/65 text-foreground inline-flex max-w-68 items-center gap-1.5 rounded-lg border px-2.5 py-1 text-[12px] transition-colors"
            >
              {item.uploading ? (
                <Loader2 className="text-muted-foreground size-3.5 shrink-0 animate-spin stroke-[1.75]" />
              ) : (
                <FileIcon className="text-foreground/65 size-3.5 shrink-0 stroke-[1.75]" />
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
