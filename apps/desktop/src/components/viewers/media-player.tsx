import { memo, useCallback, useEffect, useRef, useState } from "react";
import {
  Check,
  Download,
  ExternalLink,
  FileSymlink,
  FileWarning,
  Film,
  Music,
  Pause,
  Play,
  Quote,
  Volume2,
  VolumeX,
} from "lucide-react";
import { SimpleTooltip } from "@/components/ui/overlays";
import {
  base64ToBlobUrl,
  basenameOfPath,
  defaultMimeForMedia,
  downloadBlobUrl,
  formatMediaDuration,
  formatTimestampCitation,
} from "@/lib/media-utils";
import { orbitIpc } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const PLAYBACK_RATES = [1, 1.25, 1.5, 2] as const;

export interface MediaPlayerProps {
  kind: "video" | "audio";
  filename: string;
  path?: string | null;
  hostId?: string | null;
  cwd?: string | null;
  /** Caller-owned object URL (e.g. local unsent attachment); NOT revoked by MediaPlayer. */
  sourceUrl?: string | null;
  /** Base64-encoded media payload; converted to an owned Blob URL and revoked on cleanup. */
  base64Content?: string | null;
  mimeType?: string | null;
  sizeBytes?: number | null;
  truncated?: boolean;
  hideHeaderTitle?: boolean;
  onCiteTimestamp?: (citation: string) => void;
  onOpenInSidePanel?: () => void;
  className?: string;
}

export const MediaPlayer = memo(function MediaPlayer({
  kind,
  filename,
  path,
  hostId,
  cwd,
  sourceUrl,
  base64Content,
  mimeType,
  sizeBytes,
  truncated = false,
  hideHeaderTitle = false,
  onCiteTimestamp,
  onOpenInSidePanel,
  className,
}: MediaPlayerProps) {
  const isLocalHost = useOrbitSelector((s) => {
    const host = hostId ? s.hostDetails[hostId]?.host : null;
    return host ? isLocalHostCard(host) : false;
  });

  const mediaRef = useRef<HTMLVideoElement | HTMLAudioElement | null>(null);
  const justCitedTimerRef = useRef<number | null>(null);
  const [ownedBlobUrl, setOwnedBlobUrl] = useState<string | null>(null);
  const [mediaError, setMediaError] = useState<string | null>(null);
  const [playing, setPlaying] = useState(false);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
  const [rateIdx, setRateIdx] = useState(0);
  const [volume, setVolume] = useState(1);
  const [muted, setMuted] = useState(false);
  const [justCited, setJustCited] = useState(false);

  const displayName = basenameOfPath(filename || path || (kind === "video" ? "video" : "audio"));
  const effectiveMime = mimeType || defaultMimeForMedia(displayName, kind);

  useEffect(() => {
    setPlaying(false);
    setCurrentTime(0);
    setDuration(0);
    setMediaError(null);

    if (sourceUrl || !base64Content || truncated) {
      setOwnedBlobUrl(null);
      return;
    }

    let createdUrl: string | null = null;
    try {
      const { url } = base64ToBlobUrl(base64Content, effectiveMime);
      createdUrl = url;
      setOwnedBlobUrl(url);
    } catch (err) {
      setOwnedBlobUrl(null);
      setMediaError(String(err));
    }

    return () => {
      if (createdUrl) {
        URL.revokeObjectURL(createdUrl);
      }
    };
  }, [sourceUrl, base64Content, effectiveMime, truncated]);

  const activeUrl = sourceUrl || ownedBlobUrl;
  const playbackRate = PLAYBACK_RATES[rateIdx];
  const canOpenExternally = Boolean(isLocalHost && hostId && path);

  useEffect(() => {
    const el = mediaRef.current;
    if (!el) return;
    el.playbackRate = playbackRate;
  }, [playbackRate, activeUrl]);

  useEffect(() => {
    const el = mediaRef.current;
    if (!el) return;
    el.volume = volume;
    el.muted = muted;
  }, [volume, muted, activeUrl]);

  const togglePlay = useCallback(() => {
    const el = mediaRef.current;
    if (!el) return;
    if (el.paused || el.ended) {
      void el.play().catch(() => {
        setMediaError("Playback failed — codec may not be supported by this WebView.");
      });
    } else {
      el.pause();
    }
  }, []);

  const cycleRate = useCallback(() => {
    setRateIdx((prev) => (prev + 1) % PLAYBACK_RATES.length);
  }, []);

  const handleSeek = (e: React.ChangeEvent<HTMLInputElement>) => {
    const next = Number(e.target.value);
    const el = mediaRef.current;
    if (el && Number.isFinite(next)) {
      el.currentTime = next;
      setCurrentTime(next);
    }
  };

  useEffect(() => {
    return () => {
      if (justCitedTimerRef.current !== null) {
        window.clearTimeout(justCitedTimerRef.current);
      }
    };
  }, []);

  const handleCite = () => {
    const citation = formatTimestampCitation(displayName, currentTime);
    if (onCiteTimestamp) {
      onCiteTimestamp(citation);
    } else {
      orbitActions.insertCitationIntoComposer(citation);
    }
    setJustCited(true);
    if (justCitedTimerRef.current !== null) {
      window.clearTimeout(justCitedTimerRef.current);
    }
    justCitedTimerRef.current = window.setTimeout(() => {
      justCitedTimerRef.current = null;
      setJustCited(false);
    }, 1400);
  };

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

  const progressPct = duration > 0 ? Math.min(100, Math.max(0, (currentTime / duration) * 100)) : 0;
  const KindIcon = kind === "video" ? Film : Music;

  return (
    <div
      tabIndex={0}
      onKeyDown={(e) => {
        const target = e.target;
        const isInteractiveTarget =
          target instanceof HTMLInputElement ||
          target instanceof HTMLTextAreaElement ||
          target instanceof HTMLButtonElement ||
          (target instanceof HTMLElement && target.isContentEditable);
        if (e.key === " " && !isInteractiveTarget) {
          e.preventDefault();
          togglePlay();
        }
      }}
      className={cn("flex min-h-0 flex-1 flex-col overflow-hidden outline-none", className)}
    >
      {/* Compact Header Bar */}
      <div className="border-border bg-elevated/55 flex h-[38px] shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          {!hideHeaderTitle && (
            <>
              <KindIcon className="text-muted-foreground size-3.5 shrink-0 stroke-[1.75]" />
              <span
                title={path || displayName}
                className="text-foreground/90 truncate font-mono text-[12px] font-medium"
              >
                {displayName}
              </span>
            </>
          )}
          <span className="bg-muted text-muted-foreground shrink-0 rounded-md px-1.5 py-0.5 font-mono text-[10.5px]">
            {effectiveMime}
          </span>
          {!hideHeaderTitle && sizeBytes != null && sizeBytes > 0 && (
            <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[11px]">
              {formatFileSizeCompact(sizeBytes)}
            </span>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1">
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
            <SimpleTooltip label={`Download ${kind}`}>
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

      {/* Media Viewport */}
      {truncated ? (
        <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2.5 px-6 text-center">
          <FileWarning className="size-5 opacity-65" />
          <div className="text-foreground/85 text-[13px] font-medium">
            {kind === "video" ? "Video" : "Audio"} exceeds 25 MiB inline preview limit
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
      ) : !activeUrl ? (
        <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
          <FileWarning className="size-5 opacity-65" />
          <div className="text-foreground/85 text-[13px] font-medium">
            Unable to load {kind}
          </div>
          {mediaError && <div className="max-w-sm text-[11.5px] opacity-75">{mediaError}</div>}
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col items-center justify-center gap-4 overflow-y-auto p-5">
          {kind === "video" ? (
            <div className="border-border relative flex max-h-[62vh] w-full max-w-4xl items-center justify-center overflow-hidden rounded-xl border bg-black/92 shadow-xs">
              <video
                ref={(el) => {
                  mediaRef.current = el;
                }}
                src={activeUrl}
                playsInline
                preload="metadata"
                onClick={togglePlay}
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onEnded={() => setPlaying(false)}
                onTimeUpdate={(e) => setCurrentTime(e.currentTarget.currentTime)}
                onLoadedMetadata={(e) => setDuration(e.currentTarget.duration || 0)}
                onError={() =>
                  setMediaError("Unable to decode video stream in this WebView.")
                }
                className="max-h-[62vh] w-full cursor-pointer object-contain"
              />
            </div>
          ) : (
            <div className="border-border bg-elevated/60 flex w-full max-w-xl flex-col items-center gap-3 rounded-2xl border px-6 py-7 text-center">
              <div className="bg-muted border-border flex size-12 items-center justify-center rounded-full border">
                <Music className="text-foreground/75 size-5 stroke-[1.75]" />
              </div>
              <div className="min-w-0 max-w-full">
                <div className="text-foreground truncate font-mono text-[13px] font-medium">
                  {displayName}
                </div>
                <div className="text-muted-foreground mt-0.5 text-[11.5px]">
                  {effectiveMime}
                  {sizeBytes != null && sizeBytes > 0
                    ? ` · ${formatFileSizeCompact(sizeBytes)}`
                    : ""}
                </div>
              </div>
              <audio
                ref={(el) => {
                  mediaRef.current = el;
                }}
                src={activeUrl}
                preload="metadata"
                onPlay={() => setPlaying(true)}
                onPause={() => setPlaying(false)}
                onEnded={() => setPlaying(false)}
                onTimeUpdate={(e) => setCurrentTime(e.currentTarget.currentTime)}
                onLoadedMetadata={(e) => setDuration(e.currentTarget.duration || 0)}
                onError={() =>
                  setMediaError("Unable to decode audio stream in this WebView.")
                }
                className="hidden"
              />
            </div>
          )}

          {mediaError && (
            <div className="border-border bg-muted/50 text-muted-foreground flex max-w-xl flex-wrap items-center justify-between gap-2.5 rounded-lg border px-3 py-2 text-[12px]">
              <div className="flex min-w-0 items-center gap-2">
                <FileWarning className="size-3.5 shrink-0" />
                <span>{mediaError}</span>
              </div>
              <div className="flex shrink-0 items-center gap-1.5">
                {canOpenExternally && (
                  <button
                    type="button"
                    onClick={() => void handleOpenExternally()}
                    className="border-border bg-elevated hover:bg-accent text-foreground inline-flex cursor-pointer items-center gap-1 rounded-md border px-2 py-1 text-[11.5px] font-medium transition-colors"
                  >
                    <ExternalLink className="size-3" />
                    Open externally
                  </button>
                )}
                {activeUrl && (
                  <button
                    type="button"
                    onClick={handleDownload}
                    className="border-border bg-elevated hover:bg-accent text-foreground inline-flex cursor-pointer items-center gap-1 rounded-md border px-2 py-1 text-[11.5px] font-medium transition-colors"
                  >
                    <Download className="size-3" />
                    Download
                  </button>
                )}
              </div>
            </div>
          )}

          {/* Refined Minimal Custom Control Dock */}
          <div className="border-border bg-elevated/90 w-full max-w-2xl rounded-xl border px-3 py-2.5 shadow-2xs">
            {/* Seek Track */}
            <div className="mb-2 flex items-center gap-2.5">
              <span className="text-foreground/80 tabular-nums w-11 shrink-0 font-mono text-[11px]">
                {formatMediaDuration(currentTime)}
              </span>
              <div className="focus-within:ring-foreground/30 relative flex flex-1 items-center rounded-full focus-within:ring-2">
                <div className="bg-muted h-1.5 w-full overflow-hidden rounded-full">
                  <div
                    className="bg-foreground/75 h-full rounded-full transition-[width] duration-75"
                    style={{ width: `${progressPct}%` }}
                  />
                </div>
                <input
                  type="range"
                  aria-label="Seek timeline"
                  min={0}
                  max={duration > 0 ? duration : 0}
                  step={0.1}
                  value={Math.min(currentTime, duration || 0)}
                  onChange={handleSeek}
                  className="absolute inset-0 h-full w-full cursor-pointer opacity-0"
                />
              </div>
              <span className="text-muted-foreground tabular-nums w-11 shrink-0 text-right font-mono text-[11px]">
                {formatMediaDuration(duration)}
              </span>
            </div>

            {/* Bottom Controls Row */}
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  aria-label={playing ? "Pause" : "Play"}
                  onClick={togglePlay}
                  className="bg-foreground text-background hover:opacity-90 inline-flex size-7 cursor-pointer items-center justify-center rounded-full transition-opacity"
                >
                  {playing ? (
                    <Pause className="size-3.5 fill-current stroke-[1.75]" />
                  ) : (
                    <Play className="ml-0.5 size-3.5 fill-current stroke-[1.75]" />
                  )}
                </button>

                <button
                  type="button"
                  title="Cycle playback speed"
                  onClick={cycleRate}
                  className="text-foreground/80 hover:text-foreground hover:bg-accent tabular-nums inline-flex h-[26px] min-w-[42px] cursor-pointer items-center justify-center rounded-md px-1.5 font-mono text-[11.5px] font-medium transition-colors"
                >
                  {playbackRate}×
                </button>

                <div className="flex items-center gap-1.5 pl-1">
                  <button
                    type="button"
                    aria-label={muted || volume === 0 ? "Unmute" : "Mute"}
                    onClick={() => setMuted((m) => !m)}
                    className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                  >
                    {muted || volume === 0 ? (
                      <VolumeX className="size-3.5 stroke-[1.75]" />
                    ) : (
                      <Volume2 className="size-3.5 stroke-[1.75]" />
                    )}
                  </button>
                  <input
                    type="range"
                    aria-label="Volume"
                    min={0}
                    max={1}
                    step={0.05}
                    value={muted ? 0 : volume}
                    onChange={(e) => {
                      const next = Number(e.target.value);
                      setVolume(next);
                      if (next > 0 && muted) setMuted(false);
                    }}
                    className="accent-foreground/75 h-1 w-16 cursor-pointer"
                  />
                </div>
              </div>

              <button
                type="button"
                title={`Cite ${formatTimestampCitation(displayName, currentTime)} in Composer`}
                onClick={handleCite}
                className="border-border bg-canvas/80 text-foreground/85 hover:text-foreground hover:bg-accent tabular-nums inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-md border px-2 font-mono text-[11.5px] font-medium transition-colors"
              >
                {justCited ? (
                  <Check className="size-3 text-emerald-500" />
                ) : (
                  <Quote className="text-muted-foreground size-3 stroke-[1.8]" />
                )}
                <span>Cite [{formatMediaDuration(currentTime)}]</span>
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  );
});
