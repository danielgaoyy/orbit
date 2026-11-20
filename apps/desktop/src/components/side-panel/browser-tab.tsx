import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AppWindow,
  ArrowLeft,
  ArrowRight,
  ExternalLink,
  Globe,
  Maximize2,
  Monitor,
  RotateCw,
  Smartphone,
  Tablet,
} from "lucide-react";
import { orbitIpc } from "@/lib/orbit-ipc";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type ViewportPreset = "fluid" | "desktop" | "tablet" | "mobile";

const VIEWPORT_WIDTHS: Record<ViewportPreset, number | null> = {
  fluid: null,
  desktop: 1280,
  tablet: 768,
  mobile: 390,
};

const QUICK_LOCAL_PORTS = [
  "http://localhost:3000",
  "http://localhost:5173",
  "http://localhost:8080",
];

function sanitizeHttpUrl(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  try {
    const parsed = new URL(trimmed);
    if (parsed.protocol === "http:" || parsed.protocol === "https:") {
      return parsed.toString();
    }
    return "";
  } catch {
    return "";
  }
}

function normalizeAddressInput(raw: string): string {
  const trimmed = raw.trim();
  if (!trimmed) return "";
  if (/^[a-zA-Z][a-zA-Z0-9+.-]*:/.test(trimmed)) {
    return sanitizeHttpUrl(trimmed);
  }
  if (/^:\d{2,5}(\/.*)?$/.test(trimmed)) {
    return sanitizeHttpUrl(`http://localhost${trimmed}`);
  }
  if (/^(?:localhost|127\.0\.0\.1|0\.0\.0\.0|\[::1\])(?::\d+)?(\/.*)?$/i.test(trimmed)) {
    return sanitizeHttpUrl(`http://${trimmed}`);
  }
  return sanitizeHttpUrl(`https://${trimmed}`);
}

const EMPTY_LOCAL_URLS: readonly string[] = [];

function isLocalUrl(url: string): boolean {
  try {
    const host = new URL(url).hostname.toLowerCase();
    return (
      host === "localhost" ||
      host === "127.0.0.1" ||
      host === "0.0.0.0" ||
      host === "::1" ||
      host === "[::1]" ||
      host.endsWith(".local") ||
      host.endsWith(".localhost")
    );
  } catch {
    return false;
  }
}

export interface BrowserTabOverlayState {
  /** Reserved extension hook: locks pointer interaction on the preview canvas while active */
  readOnly: boolean;
  /** Optional status label rendered in the upper overlay layer */
  statusLabel?: string;
}

export const BrowserTab = memo(function BrowserTab({
  tab,
  visible = true,
  overlayState,
}: {
  tab: SidePanelTab;
  visible?: boolean;
  overlayState?: BrowserTabOverlayState;
}) {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const detectedUrls = useOrbitSelector((s) =>
    s.selectedHostId && s.selectedAgentId
      ? (s.stages[`${s.selectedHostId}::${s.selectedAgentId}`]?.insights.detected_local_urls ??
        EMPTY_LOCAL_URLS)
      : EMPTY_LOCAL_URLS,
  );
  const hostCard = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.fleet.hosts.find((h) => h.host_id === s.selectedHostId) ?? null)
      : null,
  );

  const initialUrl = sanitizeHttpUrl(tab.url ?? "");
  const [navState, setNavState] = useState<{ entries: string[]; idx: number }>(() =>
    initialUrl ? { entries: [initialUrl], idx: 0 } : { entries: [], idx: -1 },
  );
  const [addressInput, setAddressInput] = useState(initialUrl);
  const [reloadNonce, setReloadNonce] = useState(0);
  const [viewportPreset, setViewportPreset] = useState<ViewportPreset>("fluid");
  const [isLoading, setIsLoading] = useState(false);
  const [dismissedFrameTip, setDismissedFrameTip] = useState(false);
  const [stageWidth, setStageWidth] = useState(0);

  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const addressInputRef = useRef<HTMLInputElement | null>(null);
  const activeUrl = navState.idx >= 0 ? sanitizeHttpUrl(navState.entries[navState.idx] ?? "") : "";

  useEffect(() => {
    const el = stageRef.current;
    if (!el) return;
    const ro = new ResizeObserver((entries) => {
      const w = entries[0]?.contentRect.width ?? 0;
      if (w > 0) setStageWidth(w);
    });
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  // Clear loading spinner if a blocked/unreachable origin never triggers iframe `onLoad`
  useEffect(() => {
    if (!isLoading) return;
    const timer = window.setTimeout(() => setIsLoading(false), 10_000);
    return () => window.clearTimeout(timer);
  }, [isLoading, activeUrl, reloadNonce]);

  // Sync external navigation requests (e.g. clicking a localhost link from Overview/Stage)
  useEffect(() => {
    const incoming = sanitizeHttpUrl(tab.url ?? "");
    if (!incoming || incoming === activeUrl) return;
    setNavState((prev) => {
      const base = prev.idx >= 0 ? prev.entries.slice(0, prev.idx + 1) : [];
      if (base[base.length - 1] === incoming) return prev;
      const nextEntries = [...base, incoming];
      return { entries: nextEntries, idx: nextEntries.length - 1 };
    });
    setAddressInput(incoming);
    setDismissedFrameTip(false);
    setIsLoading(true);
  }, [tab.url]); // eslint-disable-line react-hooks/exhaustive-deps

  const navigateTo = (rawUrl: string) => {
    const normalized = normalizeAddressInput(rawUrl);
    if (!normalized) return;
    if (normalized === activeUrl) {
      setIsLoading(true);
      setReloadNonce((n) => n + 1);
      return;
    }
    setNavState((prev) => {
      const base = prev.idx >= 0 ? prev.entries.slice(0, prev.idx + 1) : [];
      const nextEntries = [...base, normalized];
      return { entries: nextEntries, idx: nextEntries.length - 1 };
    });
    setAddressInput(normalized);
    setDismissedFrameTip(false);
    setIsLoading(true);
    orbitActions.updateBrowserTabUrl(tab.id, normalized);
  };

  const canBack = navState.idx > 0;
  const canForward = navState.idx >= 0 && navState.idx < navState.entries.length - 1;

  const goBack = () => {
    if (!canBack) return;
    const nextIdx = navState.idx - 1;
    const nextUrl = navState.entries[nextIdx];
    setNavState((prev) => ({ ...prev, idx: nextIdx }));
    setAddressInput(nextUrl);
    orbitActions.updateBrowserTabUrl(tab.id, nextUrl);
  };

  const goForward = () => {
    if (!canForward) return;
    const nextIdx = navState.idx + 1;
    const nextUrl = navState.entries[nextIdx];
    setNavState((prev) => ({ ...prev, idx: nextIdx }));
    setAddressInput(nextUrl);
    orbitActions.updateBrowserTabUrl(tab.id, nextUrl);
  };

  const reload = () => {
    if (!activeUrl) return;
    setIsLoading(true);
    setReloadNonce((n) => n + 1);
  };

  const openPopout = () => {
    if (!activeUrl) return;
    void orbitIpc.openBrowserPopoutWindow(activeUrl, activeUrl).catch((err) => {
      orbitActions.showToast(String(err));
    });
  };

  const openExternal = () => {
    if (!activeUrl) return;
    void orbitIpc.openExternalBrowserUrl(activeUrl).catch((err) => {
      orbitActions.showToast(String(err));
    });
  };

  const presetWidth = VIEWPORT_WIDTHS[viewportPreset];
  const isReadOnlyOverlay = Boolean(overlayState?.readOnly);
  const showExternalHint = Boolean(activeUrl && !isLocalUrl(activeUrl) && !dismissedFrameTip);

  const suggestedUrls = useMemo(() => {
    const merged = new Set<string>(detectedUrls);
    for (const fallback of QUICK_LOCAL_PORTS) {
      merged.add(fallback);
    }
    return Array.from(merged).slice(0, 6);
  }, [detectedUrls]);

  const presetScale =
    presetWidth !== null && stageWidth > 32
      ? Math.min(1, (stageWidth - 32) / presetWidth)
      : 1;
  const isRemoteHost =
    Boolean(hostCard) &&
    (Boolean(hostCard?.ssh_alias) ||
      !hostCard?.direct_endpoints.some(
        (e) => e.includes("127.0.0.1") || e.includes("localhost"),
      ));

  return (
    <div
      className={cn(
        "orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden",
        !visible && "hidden",
      )}
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "l") {
          e.preventDefault();
          addressInputRef.current?.focus();
          addressInputRef.current?.select();
        } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r" && activeUrl) {
          e.preventDefault();
          setIsLoading(true);
          setReloadNonce((n) => n + 1);
        }
      }}
    >
      {/* Slim Browser Navigation Toolbar */}
      <div className="border-border bg-sidebar flex h-[38px] shrink-0 items-center gap-1.5 border-b px-2.5">
        <div className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            title="Back"
            disabled={!canBack}
            onClick={goBack}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <ArrowLeft className="h-[13.5px] w-[13.5px]" />
          </button>
          <button
            type="button"
            title="Forward"
            disabled={!canForward}
            onClick={goForward}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <ArrowRight className="h-[13.5px] w-[13.5px]" />
          </button>
          <button
            type="button"
            title="Reload"
            disabled={!activeUrl}
            onClick={reload}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <RotateCw className={cn("h-[13px] w-[13px]", isLoading && "animate-spin")} />
          </button>
        </div>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            navigateTo(addressInput);
          }}
          className="border-border bg-canvas focus-within:border-foreground/20 flex h-[26px] min-w-0 flex-1 items-center gap-1.5 rounded-lg border px-2 transition-colors"
        >
          <Globe className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0 opacity-75" />
          <input
            ref={addressInputRef}
            type="text"
            value={addressInput}
            onChange={(e) => setAddressInput(e.target.value)}
            placeholder="localhost:5173 or https://…"
            className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent font-mono text-[11.5px] outline-none"
          />
        </form>

        {/* Responsive Viewport Presets */}
        <div className="bg-muted hidden h-[26px] shrink-0 items-center rounded-lg p-0.5 sm:inline-flex">
          {(
            [
              { id: "fluid", icon: Maximize2, label: "Fluid width" },
              { id: "desktop", icon: Monitor, label: "Desktop (1280px)" },
              { id: "tablet", icon: Tablet, label: "Tablet (768px)" },
              { id: "mobile", icon: Smartphone, label: "Mobile (390px)" },
            ] as const
          ).map((preset) => {
            const Icon = preset.icon;
            return (
              <button
                key={preset.id}
                type="button"
                title={preset.label}
                onClick={() => setViewportPreset(preset.id)}
                className={cn(
                  "inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md transition-colors",
                  viewportPreset === preset.id
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Icon className="h-[12px] w-[12px]" />
              </button>
            );
          })}
        </div>

        {/* Native Popout Window & External Browser */}
        <div className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            title="Open in native Orbit window (bypasses iframe restrictions)"
            disabled={!activeUrl}
            onClick={openPopout}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <AppWindow className="h-[13.5px] w-[13.5px]" />
          </button>
          <button
            type="button"
            title="Open in system browser"
            disabled={!activeUrl}
            onClick={openExternal}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <ExternalLink className="h-[13px] w-[13px]" />
          </button>
        </div>
      </div>

      {/* Subtle Notice for External Websites That Restrict Embedded Frames */}
      {showExternalHint && (
        <div className="border-border bg-elevated/60 text-muted-foreground flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1.5 text-[11.5px]">
          <span className="truncate">
            If this site blocks embedded preview, open it in a native window.
          </span>
          <div className="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              onClick={openPopout}
              className="text-foreground hover:bg-accent cursor-pointer rounded-md px-2 py-0.5 font-medium transition-colors"
            >
              Native Window
            </button>
            <button
              type="button"
              onClick={() => setDismissedFrameTip(true)}
              className="text-muted-foreground hover:text-foreground cursor-pointer px-1 text-[11px]"
            >
              Dismiss
            </button>
          </div>
        </div>
      )}

      {/* Layered Preview Stage (Base Frame + Extensible Read-Only Overlay Slot) */}
      <div ref={stageRef} className="bg-canvas relative flex min-h-0 flex-1 flex-col overflow-auto">
        {!activeUrl ? (
          <div className="flex flex-1 flex-col items-center justify-center gap-4 px-6 py-10 text-center">
            <div className="bg-muted text-muted-foreground inline-flex h-10 w-10 items-center justify-center rounded-xl">
              <Globe className="h-5 w-5 stroke-[1.6]" />
            </div>
            <div className="space-y-1">
              <div className="text-foreground text-[13.5px] font-medium">Preview Web & Dev Servers</div>
              <div className="text-muted-foreground max-w-xs text-[12px] leading-relaxed">
                Enter a localhost port or URL above to inspect running web apps alongside your conversation.
              </div>
            </div>

            {suggestedUrls.length > 0 && (
              <div className="flex max-w-sm flex-wrap items-center justify-center gap-1.5">
                {suggestedUrls.map((url) => {
                  const isDetected = detectedUrls.includes(url);
                  return (
                    <button
                      key={url}
                      type="button"
                      onClick={() => navigateTo(url)}
                      className={cn(
                        "border-border hover:bg-accent orbit-press inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 py-1 font-mono text-[11.5px] transition-colors",
                        isDetected
                          ? "bg-elevated text-foreground"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      {isDetected && (
                        <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
                      )}
                      <span>{url.replace(/^https?:\/\//, "")}</span>
                    </button>
                  );
                })}
              </div>
            )}
          </div>
        ) : (
          <div
            className={cn(
              "relative flex min-h-full min-w-full flex-1 justify-center",
              presetWidth !== null && "bg-muted/35 p-4",
            )}
          >
            <div
              style={
                presetWidth
                  ? {
                      width: `${Math.round(presetWidth * presetScale)}px`,
                    }
                  : undefined
              }
              className={cn(
                "relative flex min-h-full shrink-0 overflow-hidden bg-white",
                presetWidth === null
                  ? "w-full flex-1"
                  : "border-border rounded-xl border shadow-sm",
              )}
            >
              <div
                style={
                  presetWidth && presetScale < 1
                    ? {
                        width: `${presetWidth}px`,
                        height: `${(100 / presetScale).toFixed(2)}%`,
                        transform: `scale(${presetScale})`,
                        transformOrigin: "top left",
                      }
                    : { width: "100%", height: "100%" }
                }
                className="relative flex shrink-0 flex-1"
              >
                <iframe
                  ref={iframeRef}
                  key={`${activeUrl}::${reloadNonce}`}
                  src={activeUrl}
                  title={`Preview ${activeUrl}`}
                  onLoad={() => setIsLoading(false)}
                  sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals allow-downloads"
                  className={cn(
                    "h-full w-full flex-1 border-0 bg-white",
                    isReadOnlyOverlay && "pointer-events-none select-none",
                  )}
                />
              </div>

              {/* Dedicated Top Stacking Layer Reserved for Future Automation Read-Only Animations */}
              {isReadOnlyOverlay && (
                <div
                  data-slot="browser-overlay-layer"
                  className="pointer-events-auto absolute inset-0 z-10 flex items-end justify-center bg-black/[0.04] p-4 backdrop-blur-[0.5px]"
                >
                  {overlayState?.statusLabel && (
                    <div className="bg-elevated/95 border-border floating-surface text-foreground inline-flex items-center gap-2 rounded-full border px-3 py-1 text-[12px] font-medium">
                      <span className="bg-primary h-2 w-2 animate-pulse rounded-full" />
                      <span>{overlayState.statusLabel}</span>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Host Context Footnote when Connected to Remote Host */}
      {selectedHostId && activeUrl && isLocalUrl(activeUrl) && hostCard && isRemoteHost && (
        <div className="border-border bg-sidebar/80 text-muted-foreground flex items-center justify-between border-t px-3 py-1 text-[11px]">
          <span>
            Preview loads from your local machine; forward remote ports via SSH if serving on{" "}
            {hostCard.display_name}.
          </span>
          {presetWidth !== null && presetScale < 1 && (
            <span className="tabular-nums font-mono">{Math.round(presetScale * 100)}%</span>
          )}
        </div>
      )}
    </div>
  );
});
