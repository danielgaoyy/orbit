import { memo, useEffect, useState } from "react";
import {
  Check,
  Copy,
  Crosshair,
  FileDiff,
  FileText,
  Film,
  Folder,
  GitCompareArrows,
  GitPullRequest,
  Globe,
  Image as ImageIcon,
  Music,
  Quote,
  Sparkles,
  SquareTerminal,
  X,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { ImageLightbox, type ImageLightboxItem } from "@/components/viewers/image-lightbox";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useI18n } from "@/lib/i18n";
import { getCachedPromptImage, loadPromptImage } from "@/lib/prompt-image-cache";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { symbolKindGlyph } from "@/lib/symbol-mentions";
import {
  type ContextMentionItem,
  type DiffQuoteSnippet,
  type InlineMentionTarget,
  type PastedAttachment,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface ContextTrayProps {
  contexts: readonly ContextMentionItem[];
  inlineMentions?: readonly InlineMentionTarget[];
  quotes: readonly DiffQuoteSnippet[];
  pastes: readonly PastedAttachment[];
  onRemoveInlineMention?: (token: string) => void;
}

interface ImagePreviewState {
  url: string;
  filename: string;
}

interface PastePreviewState {
  label: string;
  content: string;
}

const ComposerImageThumb = memo(function ComposerImageThumb({
  img,
  hostId,
  cwd,
  onPreview,
}: {
  img: Extract<ContextMentionItem, { kind: "image" }>;
  hostId: string | null;
  cwd: string;
  onPreview: (target: ImagePreviewState) => void;
}) {
  const { t } = useI18n();
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
      title={`${img.filename}${sizeHint}${img.uploading ? ` · ${t("composer.uploadingAttachments")}` : ""}`}
      className="orbit-pill-enter group border-border-default bg-surface-recessed relative size-12 shrink-0 overflow-hidden rounded-xl border"
    >
      {resolvedUrl ? (
        <button
          type="button"
          aria-label={t("composer.previewImageAria", { name: img.filename })}
          onClick={() => onPreview({ url: resolvedUrl, filename: img.filename })}
          className="block size-full cursor-zoom-in outline-none focus-visible:ring-2 focus-visible:ring-ring/50"
        >
          <img
            src={resolvedUrl}
            alt={img.filename}
            className="size-full object-cover"
            onError={() => {
              setResolvedUrl(null);
              setLoadFailed(true);
            }}
          />
        </button>
      ) : (
        <div className="text-ink-muted flex size-full items-center justify-center">
          {loadFailed ? (
            <ImageIcon className="size-4 stroke-[1.6]" />
          ) : (
            <OrbitSpinner size={14} />
          )}
        </div>
      )}
      {img.uploading && (
        <div className="pointer-events-none absolute inset-0 flex items-center justify-center bg-black/45 text-white">
          <OrbitSpinner size={15} />
        </div>
      )}
      <button
        type="button"
        onClick={(e) => {
          e.stopPropagation();
          orbitActions.removeContextMention(img.id);
        }}
        aria-label={t("composer.removeAttachmentAria", { name: img.filename })}
        className="bg-surface-overlay/95 text-ink-primary border-border-default absolute top-0.5 right-0.5 flex size-4.5 cursor-pointer items-center justify-center rounded-full border opacity-0 shadow-xs transition-opacity group-hover:opacity-100 focus-visible:opacity-100"
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
  inlineMentions = [],
  quotes,
  pastes,
  onRemoveInlineMention,
}: ContextTrayProps) {
  const { t } = useI18n();
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const [previewImage, setPreviewImage] = useState<ImagePreviewState | null>(null);
  const [previewPaste, setPreviewPaste] = useState<PastePreviewState | null>(null);
  const [copiedPaste, setCopiedPaste] = useState(false);

  if (
    contexts.length === 0 &&
    inlineMentions.length === 0 &&
    quotes.length === 0 &&
    pastes.length === 0
  ) {
    return null;
  }

  const images = contexts.filter(
    (c): c is Extract<ContextMentionItem, { kind: "image" }> => c.kind === "image",
  );
  const nonImages = contexts.filter((c) => c.kind !== "image");

  return (
    <>
      <div className="flex flex-wrap items-center gap-1.5 px-[14px] pt-2.5 pb-0.5">
        {/* 1. Image Thumbnails (48×48, clickable to preview) */}
        {images.map((img) => (
          <ComposerImageThumb
            key={img.id}
            img={img}
            hostId={selectedHostId}
            cwd={selectedCwd || "~"}
            onPreview={setPreviewImage}
          />
        ))}

        {/* 2. Structured Context Pills (@file, @diff, @terminal, @symbol) */}
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
                      ? t("composer.uploadingAttachments")
                      : `${basename}${metaSummary ? ` (${metaSummary})` : ""}`
                  }
                  className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary hover:bg-accent inline-flex max-w-76 items-center gap-1 rounded-lg border py-0.5 pr-1 pl-2 text-[13px] transition-colors"
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
                    className="flex min-w-0 cursor-pointer items-center gap-1.5 py-0.5 text-left"
                  >
                    {item.uploading ? (
                      <OrbitSpinner size={12} className="text-ink-muted shrink-0" />
                    ) : (
                      <MediaIcon className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                    )}
                    <span className="truncate font-mono font-medium">{basename}</span>
                    {metaSummary && (
                      <span className="text-ink-muted tabular-nums shrink-0 text-[11px]">
                        {metaSummary}
                      </span>
                    )}
                  </button>
                  <button
                    type="button"
                    aria-label={t("composer.removeAttachmentAria", { name: basename })}
                    onClick={(e) => {
                      e.stopPropagation();
                      orbitActions.removeContextMention(item.id);
                    }}
                    className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                  >
                    <X className="size-3" />
                  </button>
                </span>
              );
            }

            return (
              <span
                key={item.id}
                title={item.uploading ? t("composer.uploadingAttachments") : displayTarget}
                className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-lg border py-0.5 pr-1 pl-2 text-[13px] transition-colors"
              >
                {item.uploading ? (
                  <OrbitSpinner size={12} className="text-ink-muted shrink-0" />
                ) : (
                  <MediaIcon className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                )}
                <span
                  className={cn(
                    "min-w-0 truncate font-mono font-medium",
                    dirHint && "max-w-[68%] shrink-0",
                  )}
                >
                  {basename}
                </span>
                {dirHint && (
                  <span className="text-ink-muted hidden min-w-0 flex-1 truncate font-mono text-[11px] sm:inline">
                    {dirHint}
                  </span>
                )}
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: basename })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
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
                className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary inline-flex items-center gap-1.5 rounded-lg border py-0.5 pr-1 pl-2 text-[13px]"
              >
                <DiffIcon className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="font-medium">{item.label}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: item.label })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
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
                className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary inline-flex items-center gap-1.5 rounded-lg border py-0.5 pr-1 pl-2 text-[13px]"
              >
                <SquareTerminal className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="font-medium">{item.label}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: item.label })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
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
                className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-lg border py-0.5 pr-1 pl-1.5 text-[13px]"
              >
                <span className="bg-surface-base text-ink-muted inline-flex size-4 shrink-0 items-center justify-center rounded font-mono text-[11px] font-semibold">
                  {glyphMeta.glyph}
                </span>
                <span className="truncate font-mono font-medium">{item.symbolName}</span>
                <span className="text-ink-muted shrink-0 font-mono text-[11px]">
                  :L{item.startLine}
                </span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: item.symbolName })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (item.kind === "skill") {
            return (
              <span
                key={item.id}
                title={item.description ? `${item.label} — ${item.description}` : item.label}
                className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-lg border py-0.5 pr-1 pl-2 text-[13px]"
              >
                <Sparkles className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="truncate font-mono font-medium">/{item.slug}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeSkillAria", { slug: item.slug })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (item.kind === "dom_element") {
            const shortSel = item.selector || `<${item.tag}>`;
            const detailLines = [
              `Selector: ${item.selector}`,
              `Tag: <${item.tag}>`,
              item.pageUrl ? `URL: ${item.pageUrl}` : "",
              item.computedBox
                ? `Box: (${Math.round(item.computedBox.x)}, ${Math.round(item.computedBox.y)}) ${Math.round(item.computedBox.width)}×${Math.round(item.computedBox.height)}`
                : "",
              item.textPreview ? `Text: ${item.textPreview}` : "",
              item.outerHtmlExcerpt ? `\n${item.outerHtmlExcerpt}` : "",
            ]
              .filter(Boolean)
              .join("\n");
            return (
              <span
                key={item.id}
                title={item.pageUrl ? `${shortSel} — ${item.pageUrl}` : shortSel}
                className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary hover:bg-accent inline-flex max-w-72 items-center gap-1 rounded-lg border py-0.5 pr-1 pl-2 text-[13px] transition-colors"
              >
                <button
                  type="button"
                  onClick={() => {
                    setCopiedPaste(false);
                    setPreviewPaste({ label: `@element ${shortSel}`, content: detailLines });
                  }}
                  className="flex min-w-0 cursor-pointer items-center gap-1.5 py-0.5 text-left"
                >
                  <Crosshair className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                  <span className="truncate font-mono font-medium">{shortSel}</span>
                </button>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: shortSel })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (item.kind === "folder") {
            return (
              <span
                key={item.id}
                title={`${item.path}/ (${item.fileCount})`}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/80 text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <Folder className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="truncate font-mono font-medium">{item.path}/</span>
                {item.fileCount > 0 && (
                  <span className="text-ink-muted shrink-0 font-mono text-[11px] tabular-nums">
                    {item.fileCount}
                  </span>
                )}
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: `${item.path}/` })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (item.kind === "pr") {
            return (
              <span
                key={item.id}
                title={`PR #${item.number}: ${item.title}${item.headRef ? ` (${item.headRef})` : ""}`}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/80 text-ink-primary inline-flex max-w-72 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <GitPullRequest className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="shrink-0 font-mono font-medium tabular-nums">
                  #{item.number}
                </span>
                <span className="text-ink-secondary truncate text-[12px]">{item.title}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: `PR #${item.number}` })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (item.kind === "web") {
            return (
              <span
                key={item.id}
                title={item.url}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/80 text-ink-primary inline-flex max-w-72 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <Globe className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="truncate font-mono font-medium">{item.label}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: item.label })}
                  onClick={() => orbitActions.removeContextMention(item.id)}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          return null;
        })}

        {/* 2b. Atomic Inline `@` Mention Chips (monochrome hairline, 1-click removal synced with draft) */}
        {inlineMentions.map((m) => {
          const handleRemove = () => {
            if (onRemoveInlineMention) {
              onRemoveInlineMention(m.token);
            } else {
              orbitActions.removeInlineMention(m.token);
            }
          };

          if (m.kind === "file") {
            const { basename, dirHint } = splitPathDisplay(m.path);
            const FileIcon = resolveSemanticFileIcon(basename);
            return (
              <span
                key={`inl:${m.token}`}
                data-inline-mention-chip={m.token}
                title={m.path}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/70 text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <FileIcon className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span
                  className={cn(
                    "min-w-0 truncate font-mono font-medium",
                    dirHint && "max-w-[68%] shrink-0",
                  )}
                >
                  {basename}
                </span>
                {dirHint && (
                  <span className="text-ink-muted hidden min-w-0 flex-1 truncate font-mono text-[11px] sm:inline">
                    {dirHint}
                  </span>
                )}
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: m.token })}
                  onClick={handleRemove}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-4.5 shrink-0 cursor-pointer items-center justify-center rounded-[4px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (m.kind === "folder") {
            return (
              <span
                key={`inl:${m.token}`}
                data-inline-mention-chip={m.token}
                title={`${m.path}/ (${m.fileCount})`}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/70 text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <Folder className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="truncate font-mono font-medium">{m.path}/</span>
                {m.fileCount > 0 && (
                  <span className="text-ink-muted shrink-0 font-mono text-[11px] tabular-nums">
                    {m.fileCount}
                  </span>
                )}
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: m.token })}
                  onClick={handleRemove}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-4.5 shrink-0 cursor-pointer items-center justify-center rounded-[4px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (m.kind === "pr") {
            return (
              <span
                key={`inl:${m.token}`}
                data-inline-mention-chip={m.token}
                title={`PR #${m.number}: ${m.title}${m.headRef ? ` (${m.headRef})` : ""}`}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/70 text-ink-primary inline-flex max-w-72 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <GitPullRequest className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="shrink-0 font-mono font-medium tabular-nums">#{m.number}</span>
                <span className="text-ink-secondary truncate">{m.title}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: m.token })}
                  onClick={handleRemove}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-4.5 shrink-0 cursor-pointer items-center justify-center rounded-[4px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (m.kind === "web") {
            return (
              <span
                key={`inl:${m.token}`}
                data-inline-mention-chip={m.token}
                title={m.url}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/70 text-ink-primary inline-flex max-w-72 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <Globe className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="truncate font-mono font-medium">{m.label}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: m.token })}
                  onClick={handleRemove}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-4.5 shrink-0 cursor-pointer items-center justify-center rounded-[4px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (m.kind === "code_symbol") {
            const glyphMeta = symbolKindGlyph(m.symbolKind);
            return (
              <span
                key={`inl:${m.token}`}
                data-inline-mention-chip={m.token}
                title={m.signature || `${m.symbolName} (${m.path}:L${m.startLine})`}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/70 text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-1.5 text-[12px]"
              >
                <span className="bg-surface-base text-ink-muted inline-flex size-4 shrink-0 items-center justify-center rounded font-mono text-[11px] font-semibold">
                  {glyphMeta.glyph}
                </span>
                <span className="truncate font-mono font-medium">{m.symbolName}</span>
                <span className="text-ink-muted shrink-0 font-mono text-[11px] tabular-nums">
                  :L{m.startLine}
                </span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: m.token })}
                  onClick={handleRemove}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-4.5 shrink-0 cursor-pointer items-center justify-center rounded-[4px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          if (m.kind === "git_diff") {
            const DiffIcon = m.staged ? GitCompareArrows : FileDiff;
            return (
              <span
                key={`inl:${m.token}`}
                data-inline-mention-chip={m.token}
                className="orbit-pill-enter border-border-subtle bg-surface-recessed/70 text-ink-primary inline-flex items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
              >
                <DiffIcon className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="font-mono font-medium">{m.token}</span>
                <button
                  type="button"
                  aria-label={t("composer.removeAttachmentAria", { name: m.token })}
                  onClick={handleRemove}
                  className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-4.5 shrink-0 cursor-pointer items-center justify-center rounded-[4px] transition-colors"
                >
                  <X className="size-3" />
                </button>
              </span>
            );
          }

          return (
            <span
              key={`inl:${m.token}`}
              data-inline-mention-chip={m.token}
              className="orbit-pill-enter border-border-subtle bg-surface-recessed/70 text-ink-primary inline-flex items-center gap-1.5 rounded-md border py-0.5 pr-1 pl-2 text-[12px]"
            >
              <SquareTerminal className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
              <span className="font-mono font-medium">{m.label}</span>
              <button
                type="button"
                aria-label={t("composer.removeAttachmentAria", { name: m.token })}
                onClick={handleRemove}
                className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-4.5 shrink-0 cursor-pointer items-center justify-center rounded-[4px] transition-colors"
              >
                <X className="size-3" />
              </button>
            </span>
          );
        })}

        {/* 3. Diff Line Quotes */}
        {quotes.map((q) => {
          const quoteLabel = `${q.path}${q.lineNumber !== null ? `:${q.lineNumber}` : ""}`;
          return (
            <span
              key={q.id}
              title={q.code}
              className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary inline-flex max-w-68 items-center gap-1.5 rounded-lg border py-0.5 pr-1 pl-2 text-[13px]"
            >
              <Quote className="text-ink-tertiary size-3 shrink-0" />
              <span className="truncate font-mono">{quoteLabel}</span>
              <button
                type="button"
                aria-label={t("composer.removeAttachmentAria", { name: quoteLabel })}
                onClick={() => orbitActions.removeDiffQuote(q.id)}
                className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
              >
                <X className="size-3" />
              </button>
            </span>
          );
        })}

        {/* 4. Collapsed Large Pasted Snippets & Attached Text Files (clickable to inspect) */}
        {pastes.map((p) => {
          const PasteIcon = p.filename ? resolveSemanticFileIcon(p.filename) : FileText;
          return (
            <span
              key={p.id}
              className="orbit-pill-enter border-border-default bg-surface-recessed text-ink-primary hover:bg-accent inline-flex items-center gap-1 rounded-lg border py-0.5 pr-1 pl-2 text-[13px] transition-colors"
            >
              <button
                type="button"
                aria-label={t("composer.previewPasteAria", { name: p.label })}
                onClick={() => {
                  setCopiedPaste(false);
                  setPreviewPaste({ label: p.label, content: p.content });
                }}
                className="flex min-w-0 cursor-pointer items-center gap-1.5 py-0.5 text-left"
              >
                <PasteIcon className="text-ink-tertiary size-3.5 shrink-0 stroke-[1.75]" />
                <span className="font-medium">{p.label}</span>
              </button>
              <button
                type="button"
                aria-label={t("composer.removeAttachmentAria", { name: p.label })}
                onClick={() => orbitActions.removePastedAttachment(p.id)}
                className="text-ink-muted hover:text-ink-primary hover:bg-accent inline-flex size-5 shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
              >
                <X className="size-3" />
              </button>
            </span>
          );
        })}
      </div>

      {/* Frameless Claude-style Image Lightbox for attached composer images */}
      <ImageLightbox
        open={previewImage !== null}
        items={
          previewImage
            ? (() => {
                const items: ImageLightboxItem[] = [];
                for (const img of images) {
                  const effectiveHostId = img.stagedHostId || selectedHostId || "";
                  const cached =
                    !img.previewUrl && img.path && effectiveHostId
                      ? getCachedPromptImage(effectiveHostId, img.path)
                      : null;
                  const url =
                    img.previewUrl ||
                    (cached?.status === "ready" ? cached.url : null);
                  if (url) {
                    items.push({
                      url,
                      label: img.filename,
                      path: img.path ?? null,
                      hostId: effectiveHostId || undefined,
                      cwd: selectedCwd || "~",
                    });
                  }
                }
                if (items.length === 0) {
                  items.push({ url: previewImage.url, label: previewImage.filename });
                }
                return items;
              })()
            : []
        }
        initialIndex={
          previewImage
            ? Math.max(
                0,
                images
                  .map((img) => {
                    const effectiveHostId = img.stagedHostId || selectedHostId || "";
                    const cached =
                      !img.previewUrl && img.path && effectiveHostId
                        ? getCachedPromptImage(effectiveHostId, img.path)
                        : null;
                    return img.previewUrl || (cached?.status === "ready" ? cached.url : null);
                  })
                  .filter((u): u is string => Boolean(u))
                  .indexOf(previewImage.url),
              )
            : 0
        }
        onClose={() => setPreviewImage(null)}
      />

      {/* Inline Text Preview Dialog for collapsed pasted snippets & text files */}
      <Dialog open={previewPaste !== null} onOpenChange={(open) => !open && setPreviewPaste(null)}>
        <DialogContent className="flex max-h-[82vh] w-[min(90vw,720px)] flex-col gap-2.5 overflow-hidden p-4">
          <DialogHeader className="flex flex-row items-center justify-between gap-3 pr-8">
            <div className="min-w-0">
              <DialogTitle className="truncate font-mono text-[13px] font-medium">
                {previewPaste?.label ?? t("composer.pastePreviewTitle")}
              </DialogTitle>
              <DialogDescription className="sr-only">
                {t("composer.pastePreviewTitle")}
              </DialogDescription>
            </div>
            {previewPaste && (
              <Button
                type="button"
                variant="outline"
                size="xs"
                onClick={() => {
                  void navigator.clipboard.writeText(previewPaste.content).then(() => {
                    setCopiedPaste(true);
                    window.setTimeout(() => setCopiedPaste(false), 1400);
                  });
                }}
                className="shrink-0 gap-1.5"
              >
                {copiedPaste ? (
                  <Check className="text-signal-success size-3.5" />
                ) : (
                  <Copy className="size-3.5" />
                )}
                <span>{copiedPaste ? t("common.copied") : t("common.copy")}</span>
              </Button>
            )}
          </DialogHeader>
          {previewPaste && (
            <ScrollArea
              orientation="both"
              className="border-border-subtle bg-surface-recessed min-h-0 flex-1 rounded-xl border"
              viewportClassName="selectable-text max-h-[60vh] p-3"
            >
              <pre className="text-ink-primary m-0 font-mono text-[13px] leading-[1.55] whitespace-pre-wrap break-words">
                {previewPaste.content}
              </pre>
            </ScrollArea>
          )}
        </DialogContent>
      </Dialog>
    </>
  );
});
