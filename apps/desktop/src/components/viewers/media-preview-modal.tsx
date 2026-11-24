import { memo, useEffect } from "react";
import { FileWarning } from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { MediaPlayer } from "@/components/viewers/media-player";
import { PdfViewer } from "@/components/viewers/pdf-viewer";
import { basenameOfPath } from "@/lib/media-utils";
import { useDelayedLoading } from "@/lib/use-delayed-loading";
import { fileCacheKey, orbitActions, useOrbitSelector } from "@/state/orbit-store";

export const MediaPreviewModal = memo(function MediaPreviewModal() {
  const target = useOrbitSelector((s) => s.mediaPreviewTarget);

  const hostId = target?.hostId ?? "";
  const cwd = target?.cwd ?? "~";
  const path = target?.path ?? "";
  const needsRemoteLoad = Boolean(target && !target.localUrl && hostId && path);
  const cKey = needsRemoteLoad ? fileCacheKey(hostId, cwd, path) : "";
  const cacheEntry = useOrbitSelector((s) => (cKey ? (s.fileViewCache[cKey] ?? null) : null));

  useEffect(() => {
    if (needsRemoteLoad && !cacheEntry) {
      void orbitActions.loadWorkspaceFile(hostId, cwd, path, false);
    }
  }, [needsRemoteLoad, hostId, cwd, path, cacheEntry]);

  const close = () => orbitActions.closeMediaPreview();

  const handleCite = (citation: string) => {
    orbitActions.insertCitationIntoComposer(citation);
    close();
  };

  const handleOpenInSidePanel =
    target && target.hostId && target.path
      ? () => {
          const p = target.path!;
          const h = target.hostId!;
          const c = target.cwd ?? "~";
          close();
          orbitActions.openFileInSidePanel(p, { hostId: h, cwd: c });
        }
      : undefined;

  const data = cacheEntry?.data ?? null;
  const isInitialLoading = Boolean(
    needsRemoteLoad && (!cacheEntry || cacheEntry.status === "loading") && !data,
  );
  const showLoadingIndicator = useDelayedLoading(isInitialLoading);
  const effectiveKind =
    data && (data.content_kind === "pdf" || data.content_kind === "audio" || data.content_kind === "video")
      ? data.content_kind
      : target?.kind ?? "pdf";

  const displayName = target ? basenameOfPath(target.filename || target.path || "Attachment") : "";
  const isAudioOnly = effectiveKind === "audio";

  const viewerKey = `${hostId}:${cwd}:${path}:${target?.localUrl ?? ""}`;

  return (
    <Dialog open={target !== null} onOpenChange={(open) => !open && close()}>
      <DialogContent
        className={
          isAudioOnly
            ? "w-[calc(100vw-2rem)] max-w-2xl gap-0 overflow-hidden p-0"
            : "flex h-[84vh] w-[calc(100vw-2.5rem)] max-w-5xl flex-col gap-0 overflow-hidden p-0"
        }
      >
        <DialogHeader className="sr-only">
          <DialogTitle>{displayName}</DialogTitle>
          <DialogDescription>{target?.path ?? displayName}</DialogDescription>
        </DialogHeader>

        {target && (
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden pr-0">
            {isInitialLoading ? (
              showLoadingIndicator ? (
                <div className="orbit-stage-enter text-muted-foreground flex flex-1 items-center justify-center gap-2 py-20 text-[13px]">
                  <OrbitSpinner size={15} />
                  <span>Loading {displayName}…</span>
                </div>
              ) : (
                <div className="flex flex-1 py-20" />
              )
            ) : needsRemoteLoad && cacheEntry?.status === "error" && !data ? (
              <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 px-6 py-20 text-center">
                <FileWarning className="size-5 opacity-65" />
                <div className="text-foreground/85 text-[13px] font-medium">
                  Unable to load media attachment
                </div>
                <div className="max-w-md font-mono text-[11.5px] opacity-75">
                  {cacheEntry.error}
                </div>
                <button
                  type="button"
                  onClick={() => void orbitActions.loadWorkspaceFile(hostId, cwd, path, true)}
                  className="border-border hover:bg-accent text-foreground mt-1 cursor-pointer rounded-lg border px-3 py-1 text-[12px] transition-colors"
                >
                  Retry
                </button>
              </div>
            ) : effectiveKind === "pdf" ? (
              <PdfViewer
                key={viewerKey}
                filename={displayName}
                path={target.path}
                hostId={target.hostId}
                cwd={target.cwd}
                sourceUrl={target.localUrl}
                base64Content={data?.content ?? null}
                sizeBytes={data?.size_bytes ?? target.sizeBytes}
                truncated={Boolean(data?.truncated)}
                pageCountHint={target.pdfPageCount ?? null}
                onCitePage={handleCite}
                onOpenInSidePanel={handleOpenInSidePanel}
                className="min-h-0 flex-1 [&>div:first-child]:pr-10"
              />
            ) : (
              <MediaPlayer
                key={viewerKey}
                kind={effectiveKind}
                filename={displayName}
                path={target.path}
                hostId={target.hostId}
                cwd={target.cwd}
                sourceUrl={target.localUrl}
                base64Content={data?.content ?? null}
                mimeType={data?.mime_type ?? target.mimeType}
                sizeBytes={data?.size_bytes ?? target.sizeBytes}
                truncated={Boolean(data?.truncated)}
                onCiteTimestamp={handleCite}
                onOpenInSidePanel={handleOpenInSidePanel}
                className="min-h-0 flex-1 [&>div:first-child]:pr-10"
              />
            )}
          </div>
        )}
      </DialogContent>
    </Dialog>
  );
});
