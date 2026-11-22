import { memo, useEffect, useRef, useState } from "react";
import {
  Check,
  Download,
  ExternalLink,
  FileSymlink,
  FileText,
  FileWarning,
  Quote,
} from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger, SimpleTooltip } from "@/components/ui/overlays";
import {
  base64ToBlobUrl,
  basenameOfPath,
  downloadBlobUrl,
  estimatePdfPageCount,
  parsePdfPageCitation,
} from "@/lib/media-utils";
import { orbitIpc } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export interface PdfViewerProps {
  filename: string;
  path?: string | null;
  hostId?: string | null;
  cwd?: string | null;
  /** Caller-owned object URL (e.g. local unsent attachment); NOT revoked by PdfViewer. */
  sourceUrl?: string | null;
  /** Base64-encoded PDF payload; converted to an owned Blob URL and revoked on cleanup. */
  base64Content?: string | null;
  sizeBytes?: number | null;
  truncated?: boolean;
  pageCountHint?: number | null;
  /** Hide filename/size in toolbar when parent header already shows breadcrumb. */
  hideHeaderTitle?: boolean;
  onCitePage?: (citation: string) => void;
  onOpenInSidePanel?: () => void;
  className?: string;
}

export const PdfViewer = memo(function PdfViewer({
  filename,
  path,
  hostId,
  cwd,
  sourceUrl,
  base64Content,
  sizeBytes,
  truncated = false,
  pageCountHint = null,
  hideHeaderTitle = false,
  onCitePage,
  onOpenInSidePanel,
  className,
}: PdfViewerProps) {
  const isLocalHost = useOrbitSelector((s) => {
    const host = hostId ? s.hostDetails[hostId]?.host : null;
    return host ? isLocalHostCard(host) : false;
  });

  const pageCountHintRef = useRef<number | null>(pageCountHint);
  pageCountHintRef.current = pageCountHint;
  const justCitedTimerRef = useRef<number | null>(null);

  const [ownedBlobUrl, setOwnedBlobUrl] = useState<string | null>(null);
  const [detectedPages, setDetectedPages] = useState<number | null>(pageCountHint);
  const [decodeError, setDecodeError] = useState<string | null>(null);
  const [citeOpen, setCiteOpen] = useState(false);
  const [pageInput, setPageInput] = useState("1");
  const [citeError, setCiteError] = useState<string | null>(null);
  const [justCited, setJustCited] = useState(false);

  useEffect(() => {
    return () => {
      if (justCitedTimerRef.current !== null) {
        window.clearTimeout(justCitedTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    if (sourceUrl || !base64Content || truncated) {
      setOwnedBlobUrl(null);
      setDecodeError(null);
      setDetectedPages(pageCountHintRef.current);
      return;
    }

    let createdUrl: string | null = null;
    try {
      const { url, bytes } = base64ToBlobUrl(base64Content, "application/pdf");
      createdUrl = url;
      setOwnedBlobUrl(url);
      setDecodeError(null);
      if (pageCountHintRef.current == null) {
        setDetectedPages(estimatePdfPageCount(bytes));
      }
    } catch (err) {
      setOwnedBlobUrl(null);
      setDecodeError(String(err));
    }

    return () => {
      if (createdUrl) {
        URL.revokeObjectURL(createdUrl);
      }
    };
  }, [sourceUrl, base64Content, truncated]);

  const activeUrl = sourceUrl || ownedBlobUrl;
  const displayName = basenameOfPath(filename || path || "document.pdf");
  const effectivePages = pageCountHint ?? detectedPages;
  const canOpenExternally = Boolean(isLocalHost && hostId && path);

  const handleOpenExternally = async () => {
    if (!hostId || !path) return;
    try {
      await orbitIpc.openLocalFileExternally(hostId, cwd || "~", path);
    } catch (err) {
      orbitActions.showToast(`Could not open externally: ${String(err)}`);
    }
  };

  const handleDownload = () => {
    if (!activeUrl) return;
    downloadBlobUrl(activeUrl, displayName);
  };

  const handleSubmitCitation = (e: React.FormEvent) => {
    e.preventDefault();
    const citation = parsePdfPageCitation(displayName, pageInput);
    if (!citation) {
      setCiteError("Enter a page or range (e.g. 4 or 4-6)");
      return;
    }
    setCiteError(null);
    if (onCitePage) {
      onCitePage(citation);
    } else {
      orbitActions.insertCitationIntoComposer(citation);
    }
    setCiteOpen(false);
    setJustCited(true);
    if (justCitedTimerRef.current !== null) {
      window.clearTimeout(justCitedTimerRef.current);
    }
    justCitedTimerRef.current = window.setTimeout(() => {
      justCitedTimerRef.current = null;
      setJustCited(false);
    }, 1400);
  };

  return (
    <div className={cn("flex min-h-0 flex-1 flex-col overflow-hidden", className)}>
      {/* Restrained Compact Toolbar */}
      <div className="border-border bg-elevated/55 flex h-[38px] shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          {!hideHeaderTitle && (
            <>
              <FileText className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
              <span
                title={path || displayName}
                className="text-foreground/90 truncate font-mono text-[12px] font-medium"
              >
                {displayName}
              </span>
            </>
          )}
          {effectivePages != null && effectivePages > 0 && (
            <span className="bg-muted text-muted-foreground tabular-nums shrink-0 rounded-md px-1.5 py-0.5 text-[11px] font-medium">
              {effectivePages === 1 ? "1 page" : `${effectivePages} pages`}
            </span>
          )}
          {!hideHeaderTitle && sizeBytes != null && sizeBytes > 0 && (
            <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[11px]">
              {formatFileSizeCompact(sizeBytes)}
            </span>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1">
          <Popover
            open={citeOpen}
            onOpenChange={(open) => {
              setCiteOpen(open);
              if (open) setCiteError(null);
            }}
          >
            <PopoverTrigger asChild>
              <button
                type="button"
                title="Cite page or page range in Composer"
                className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors"
              >
                {justCited ? (
                  <Check className="size-3 text-emerald-500" />
                ) : (
                  <Quote className="size-3 stroke-[1.8]" />
                )}
                <span>Cite page</span>
              </button>
            </PopoverTrigger>
            <PopoverContent align="end" side="bottom" className="w-60 p-2.5">
              <form onSubmit={handleSubmitCitation} className="space-y-2">
                <div className="text-foreground/85 text-[11.5px] font-medium">
                  Cite page in Composer
                </div>
                <div className="flex items-center gap-1.5">
                  <input
                    type="text"
                    value={pageInput}
                    onChange={(e) => {
                      setPageInput(e.target.value);
                      if (citeError) setCiteError(null);
                    }}
                    placeholder="4 or 4-6"
                    autoFocus
                    className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/70 h-[28px] min-w-0 flex-1 rounded-md border px-2 font-mono text-[12px] outline-none focus:border-foreground/35"
                  />
                  <button
                    type="submit"
                    className="bg-foreground text-background hover:opacity-90 h-[28px] shrink-0 cursor-pointer rounded-md px-2.5 text-[11.5px] font-medium transition-opacity"
                  >
                    Insert
                  </button>
                </div>
                {citeError ? (
                  <div className="text-[11px] text-rose-500">{citeError}</div>
                ) : (
                  <div className="text-muted-foreground text-[11px]">
                    Inserts{" "}
                    <span className="font-mono">
                      {parsePdfPageCitation(displayName, pageInput) ?? `${displayName} (p. 4)`}
                    </span>
                  </div>
                )}
              </form>
            </PopoverContent>
          </Popover>

          {onOpenInSidePanel && (
            <SimpleTooltip label="Open in Side Panel">
              <button
                type="button"
                onClick={onOpenInSidePanel}
                className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex size-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
              >
                <FileSymlink className="size-[13px] stroke-[1.75]" />
              </button>
            </SimpleTooltip>
          )}

          {canOpenExternally && (
            <SimpleTooltip label="Open externally">
              <button
                type="button"
                onClick={() => void handleOpenExternally()}
                className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex size-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
              >
                <ExternalLink className="size-[13px] stroke-[1.75]" />
              </button>
            </SimpleTooltip>
          )}

          {activeUrl && (
            <SimpleTooltip label="Download PDF">
              <button
                type="button"
                onClick={handleDownload}
                className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex size-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
              >
                <Download className="size-[13px] stroke-[1.75]" />
              </button>
            </SimpleTooltip>
          )}
        </div>
      </div>

      {/* Document Viewport */}
      {truncated ? (
        <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2.5 px-6 text-center">
          <FileWarning className="size-5 opacity-65" />
          <div className="text-foreground/85 text-[13px] font-medium">
            PDF exceeds 25 MiB inline preview limit
          </div>
          <div className="text-[12px]">
            {displayName}
            {sizeBytes != null ? ` (${formatFileSizeCompact(sizeBytes)})` : ""}
          </div>
          {canOpenExternally && (
            <button
              type="button"
              onClick={() => void handleOpenExternally()}
              className="border-border hover:bg-accent text-foreground mt-1 inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-[12px] font-medium transition-colors"
            >
              <ExternalLink className="size-3.5" />
              Open externally
            </button>
          )}
        </div>
      ) : decodeError || !activeUrl ? (
        <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
          <FileWarning className="size-5 opacity-65" />
          <div className="text-foreground/85 text-[13px] font-medium">Unable to render PDF</div>
          {decodeError && (
            <div className="max-w-sm font-mono text-[11.5px] opacity-75">{decodeError}</div>
          )}
        </div>
      ) : (
        <div className="bg-muted/25 relative min-h-0 flex-1">
          <object
            data={activeUrl}
            type="application/pdf"
            aria-label={displayName}
            className="h-full w-full"
          >
            <div className="text-muted-foreground flex h-full flex-col items-center justify-center gap-2.5 p-6 text-center">
              <FileText className="size-6 opacity-60" />
              <div className="text-foreground/85 text-[13px] font-medium">
                Inline PDF preview unavailable in this WebView
              </div>
              <div className="flex items-center gap-2">
                {canOpenExternally && (
                  <button
                    type="button"
                    onClick={() => void handleOpenExternally()}
                    className="border-border hover:bg-accent text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-[12px] font-medium transition-colors"
                  >
                    <ExternalLink className="size-3.5" />
                    Open externally
                  </button>
                )}
                <button
                  type="button"
                  onClick={handleDownload}
                  className="border-border hover:bg-accent text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-3 py-1.5 text-[12px] font-medium transition-colors"
                >
                  <Download className="size-3.5" />
                  Download
                </button>
              </div>
            </div>
          </object>
        </div>
      )}
    </div>
  );
});
