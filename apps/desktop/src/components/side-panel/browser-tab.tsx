import { memo, type MouseEvent, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  AppWindow,
  ArrowLeft,
  ArrowRight,
  Camera,
  ChevronDown,
  ChevronUp,
  Crosshair,
  ExternalLink,
  Globe,
  History,
  Layers,
  Loader2,
  Maximize2,
  Monitor,
  Play,
  RotateCw,
  Smartphone,
  Tablet,
  X,
} from "lucide-react";
import { ShortcutKbd } from "@/components/ui/kbd";
import { Select } from "@/components/ui/select";
import { useI18n } from "@/lib/i18n";
import {
  type PickedElementBox,
  type RemotePreviewTunnelView,
  type WorkspaceScriptInfo,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { type SidePanelTab, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type ViewportPreset = "fluid" | "desktop" | "tablet" | "mobile";
type BrowserPanelMode = "preview" | "cdp";

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

const EMPTY_LOCAL_URLS: readonly string[] = Object.freeze([]);
const EMPTY_SCRIPTS_MAP: Readonly<Record<string, readonly WorkspaceScriptInfo[]>> = Object.freeze(
  {},
);

function extractProxyHost(rawUrl: string | null | undefined): string | null {
  if (!rawUrl) return null;
  try {
    const host = new URL(rawUrl).hostname.toLowerCase();
    return host.endsWith(".localhost") ? host : null;
  } catch {
    return null;
  }
}

interface MatchedRemoteService {
  port: number;
  proxyHost: string | null;
  serviceName: string;
}

function matchRemotePreviewService(
  rawUrl: string,
  runningServices: readonly WorkspaceScriptInfo[],
): MatchedRemoteService | null {
  if (!rawUrl || runningServices.length === 0) return null;
  let parsed: URL;
  try {
    parsed = new URL(rawUrl);
  } catch {
    return null;
  }
  const hostname = parsed.hostname.toLowerCase();
  const portNum = parsed.port ? Number.parseInt(parsed.port, 10) : 0;

  for (const svc of runningServices) {
    if (svc.port == null || svc.port <= 0) continue;
    const svcProxyHost = extractProxyHost(svc.proxy_url);
    if (svcProxyHost && hostname === svcProxyHost) {
      return { port: svc.port, proxyHost: svcProxyHost, serviceName: svc.name };
    }
  }
  if (
    portNum > 0 &&
    (hostname === "localhost" ||
      hostname === "127.0.0.1" ||
      hostname === "0.0.0.0" ||
      hostname === "::1" ||
      hostname === "[::1]")
  ) {
    for (const svc of runningServices) {
      if (svc.port === portNum) {
        return {
          port: svc.port,
          proxyHost: extractProxyHost(svc.proxy_url),
          serviceName: svc.name,
        };
      }
    }
  }
  return null;
}

function applyTunnelOrigin(originalUrl: string, tunnelLocalUrl: string): string {
  try {
    const orig = new URL(originalUrl);
    const tun = new URL(tunnelLocalUrl);
    return `${tun.origin}${orig.pathname}${orig.search}${orig.hash}`;
  } catch {
    return tunnelLocalUrl;
  }
}

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

function formatAuditTime(ms: number): string {
  try {
    return new Date(ms).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return "";
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
  const { t } = useI18n();
  const selectedHostId = tab.hostId ?? null;
  const selectedAgentId = tab.agentId ?? null;
  const selectedWorkspaceCwd = tab.cwd ?? "~";
  const detectedUrls = useOrbitSelector((s) =>
    selectedHostId && selectedAgentId
      ? (s.stages[`${selectedHostId}::${selectedAgentId}`]?.insights.detected_local_urls ??
        EMPTY_LOCAL_URLS)
      : EMPTY_LOCAL_URLS,
  );
  const hostCard = useOrbitSelector((s) =>
    selectedHostId
      ? (s.fleet.hosts.find((h) => h.host_id === selectedHostId) ?? null)
      : null,
  );
  const hostBrowser = useOrbitSelector((s) =>
    selectedHostId ? (s.browserByHost[selectedHostId] ?? null) : null,
  );
  const hostScriptsByWorkspace = useOrbitSelector((s) =>
    selectedHostId
      ? (s.hostDetails[selectedHostId]?.workspace_scripts ?? EMPTY_SCRIPTS_MAP)
      : EMPTY_SCRIPTS_MAP,
  );

  const initialUrl = sanitizeHttpUrl(tab.url ?? "");
  const [mode, setMode] = useState<BrowserPanelMode>("preview");
  const [navState, setNavState] = useState<{ entries: string[]; idx: number }>(() =>
    initialUrl ? { entries: [initialUrl], idx: 0 } : { entries: [], idx: -1 },
  );
  const [previewAddressInput, setPreviewAddressInput] = useState(initialUrl);
  const [cdpAddressInput, setCdpAddressInput] = useState(initialUrl);
  const [reloadNonce, setReloadNonce] = useState(0);
  const [viewportPreset, setViewportPreset] = useState<ViewportPreset>("fluid");
  const [isLoading, setIsLoading] = useState(false);
  const [dismissedFrameTip, setDismissedFrameTip] = useState(false);
  const [stageWidth, setStageWidth] = useState(0);
  const [auditOpen, setAuditOpen] = useState(false);
  const [consoleOpen, setConsoleOpen] = useState(false);
  const [pickElementMode, setPickElementMode] = useState(false);
  const [pickedHighlight, setPickedHighlight] = useState<{
    box: PickedElementBox;
    viewportWidth: number;
    viewportHeight: number;
  } | null>(null);
  const [remoteTunnel, setRemoteTunnel] = useState<{
    key: string;
    view: RemotePreviewTunnelView;
  } | null>(null);
  const [remoteTunnelOpeningKey, setRemoteTunnelOpeningKey] = useState<string | null>(null);

  const iframeRef = useRef<HTMLIFrameElement | null>(null);
  const stageRef = useRef<HTMLDivElement | null>(null);
  const addressInputRef = useRef<HTMLInputElement | null>(null);
  const highlightTimerRef = useRef<number | null>(null);
  const activeUrl = navState.idx >= 0 ? sanitizeHttpUrl(navState.entries[navState.idx] ?? "") : "";

  const isRemoteHost = Boolean(
    hostCard &&
      (!hostCard.is_local ||
        Boolean(hostCard.ssh_alias) ||
        !hostCard.direct_endpoints.some(
          (e) => e.includes("127.0.0.1") || e.includes("localhost"),
        )),
  );

  const runningServices = useMemo(() => {
    const list: WorkspaceScriptInfo[] = [];
    for (const scripts of Object.values(hostScriptsByWorkspace)) {
      for (const s of scripts) {
        if (s.kind === "service" && s.status === "running" && s.port != null && s.port > 0) {
          list.push(s);
        }
      }
    }
    return list;
  }, [hostScriptsByWorkspace]);

  const matchedRemoteService = useMemo(
    () => (isRemoteHost ? matchRemotePreviewService(activeUrl, runningServices) : null),
    [isRemoteHost, activeUrl, runningServices],
  );

  const remoteTunnelKey =
    selectedHostId && matchedRemoteService
      ? `${selectedHostId}::${matchedRemoteService.port}::${matchedRemoteService.proxyHost ?? ""}`
      : null;

  useEffect(() => {
    if (!visible || !remoteTunnelKey || !selectedHostId || !matchedRemoteService) {
      setRemoteTunnelOpeningKey(null);
      return;
    }
    if (remoteTunnel?.key === remoteTunnelKey) {
      setRemoteTunnelOpeningKey(null);
      return;
    }
    let cancelled = false;
    setRemoteTunnelOpeningKey(remoteTunnelKey);
    void orbitIpc
      .openRemotePreviewTunnel(
        selectedHostId,
        matchedRemoteService.port,
        matchedRemoteService.proxyHost,
      )
      .then((view) => {
        if (cancelled) return;
        setRemoteTunnel({ key: remoteTunnelKey, view });
        setRemoteTunnelOpeningKey(null);
      })
      .catch(() => {
        if (cancelled) return;
        setRemoteTunnelOpeningKey(null);
      });
    return () => {
      cancelled = true;
    };
  }, [
    visible,
    remoteTunnelKey,
    selectedHostId,
    matchedRemoteService?.port,
    matchedRemoteService?.proxyHost,
    remoteTunnel?.key,
  ]); // eslint-disable-line react-hooks/exhaustive-deps

  useEffect(() => {
    return () => {
      if (highlightTimerRef.current !== null) {
        window.clearTimeout(highlightTimerRef.current);
      }
    };
  }, []);

  const activeRemoteTunnel =
    remoteTunnelKey && remoteTunnel?.key === remoteTunnelKey ? remoteTunnel.view : null;
  const resolvedPreviewUrl =
    activeRemoteTunnel?.is_tunneled && activeUrl
      ? applyTunnelOrigin(activeUrl, activeRemoteTunnel.local_url)
      : activeUrl;

  const cdpSessions = hostBrowser?.sessions ?? [];
  const activeCdpSession = useMemo(() => {
    if (cdpSessions.length === 0) return null;
    if (hostBrowser?.activeSessionId) {
      return (
        cdpSessions.find((s) => s.session_id === hostBrowser.activeSessionId) ?? cdpSessions[0]
      );
    }
    return cdpSessions[0];
  }, [cdpSessions, hostBrowser?.activeSessionId]);

  const cdpBusy = Boolean(hostBrowser?.busy);
  const hasCdpSessions = cdpSessions.length > 0;
  const isCdpMode = mode === "cdp";

  useEffect(() => {
    const el = stageRef.current;
    if (!el) return;
    let lastW = 0;
    let rafId: number | null = null;
    const ro = new ResizeObserver((entries) => {
      const nextW = Math.round(entries[0]?.contentRect.width ?? 0);
      if (nextW <= 0 || nextW === lastW) return;
      lastW = nextW;
      if (rafId !== null) window.cancelAnimationFrame(rafId);
      rafId = window.requestAnimationFrame(() => {
        rafId = null;
        setStageWidth(nextW);
      });
    });
    ro.observe(el);
    return () => {
      ro.disconnect();
      if (rafId !== null) window.cancelAnimationFrame(rafId);
    };
  }, []);

  // Load host CDP sessions when visible or switching into CDP mode
  useEffect(() => {
    if (!visible || !selectedHostId) return;
    if (isCdpMode || !hostBrowser) {
      void orbitActions.loadBrowserSessions(selectedHostId);
    }
  }, [visible, selectedHostId, isCdpMode]); // eslint-disable-line react-hooks/exhaustive-deps

  // Sync CDP address bar with live CDP session URL without disturbing Preview address input
  useEffect(() => {
    if (!activeCdpSession) return;
    if (activeCdpSession.url && activeCdpSession.url !== "about:blank") {
      setCdpAddressInput(activeCdpSession.url);
    }
  }, [activeCdpSession?.session_id, activeCdpSession?.url]); // eslint-disable-line react-hooks/exhaustive-deps

  // Reset element picker mode whenever leaving CDP mode or losing the active session
  useEffect(() => {
    if (!isCdpMode || !activeCdpSession) {
      setPickElementMode(false);
    }
  }, [isCdpMode, activeCdpSession?.session_id]); // eslint-disable-line react-hooks/exhaustive-deps

  // Clear loading spinner if a blocked/unreachable origin never triggers iframe `onLoad`
  useEffect(() => {
    if (!isLoading) return;
    const timer = window.setTimeout(() => setIsLoading(false), 10_000);
    return () => window.clearTimeout(timer);
  }, [isLoading, activeUrl, reloadNonce]);

  // Sync external navigation requests (e.g. clicking a localhost link from Overview/Stage)
  // only into Preview address input so live CDP address input is never clobbered.
  useEffect(() => {
    const incoming = sanitizeHttpUrl(tab.url ?? "");
    if (!incoming || incoming === activeUrl) return;
    setNavState((prev) => {
      const base = prev.idx >= 0 ? prev.entries.slice(0, prev.idx + 1) : [];
      if (base[base.length - 1] === incoming) return prev;
      const nextEntries = [...base, incoming];
      return { entries: nextEntries, idx: nextEntries.length - 1 };
    });
    setPreviewAddressInput(incoming);
    setDismissedFrameTip(false);
    setIsLoading(true);
  }, [tab.url]); // eslint-disable-line react-hooks/exhaustive-deps

  const resolvedWorkspaceRoot =
    selectedWorkspaceCwd && selectedWorkspaceCwd !== "~" ? selectedWorkspaceCwd : null;

  const runCdpNavigateAndCapture = async (url: string) => {
    if (!selectedHostId) return;
    if (!activeCdpSession) {
      await orbitActions.createBrowserSession({
        hostId: selectedHostId,
        agentId: selectedAgentId ?? null,
        initialUrl: url,
        workspaceRoot: resolvedWorkspaceRoot,
      });
      return;
    }
    const navRes = await orbitActions.executeBrowserAction(
      selectedHostId,
      activeCdpSession.session_id,
      { kind: "navigate", url },
    );
    if (navRes) {
      await orbitActions.executeBrowserAction(selectedHostId, activeCdpSession.session_id, {
        kind: "screenshot",
        full_page: false,
      });
    }
  };

  const navigateTo = (rawUrl: string) => {
    const normalized = normalizeAddressInput(rawUrl);
    if (!normalized) return;
    if (isCdpMode) {
      setCdpAddressInput(normalized);
      void runCdpNavigateAndCapture(normalized);
      return;
    }
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
    setPreviewAddressInput(normalized);
    setDismissedFrameTip(false);
    setIsLoading(true);
    orbitActions.updateBrowserTabUrl(tab.id, normalized);
  };

  const canBack = isCdpMode ? Boolean(activeCdpSession && !cdpBusy) : navState.idx > 0;
  const canForward = !isCdpMode && navState.idx >= 0 && navState.idx < navState.entries.length - 1;
  const canReload = isCdpMode ? Boolean(activeCdpSession && !cdpBusy) : Boolean(activeUrl);

  const goBack = () => {
    if (isCdpMode) {
      if (!selectedHostId || !activeCdpSession || cdpBusy) return;
      void (async () => {
        const res = await orbitActions.executeBrowserAction(
          selectedHostId,
          activeCdpSession.session_id,
          { kind: "back" },
        );
        if (res) {
          await orbitActions.executeBrowserAction(selectedHostId, activeCdpSession.session_id, {
            kind: "screenshot",
            full_page: false,
          });
        }
      })();
      return;
    }
    if (!canBack) return;
    const nextIdx = navState.idx - 1;
    const nextUrl = navState.entries[nextIdx];
    setNavState((prev) => ({ ...prev, idx: nextIdx }));
    setPreviewAddressInput(nextUrl);
    orbitActions.updateBrowserTabUrl(tab.id, nextUrl);
  };

  const goForward = () => {
    if (!canForward) return;
    const nextIdx = navState.idx + 1;
    const nextUrl = navState.entries[nextIdx];
    setNavState((prev) => ({ ...prev, idx: nextIdx }));
    setPreviewAddressInput(nextUrl);
    orbitActions.updateBrowserTabUrl(tab.id, nextUrl);
  };

  const reload = () => {
    if (isCdpMode) {
      if (!selectedHostId || !activeCdpSession || cdpBusy) return;
      void (async () => {
        const res = await orbitActions.executeBrowserAction(
          selectedHostId,
          activeCdpSession.session_id,
          { kind: "reload" },
        );
        if (res) {
          await orbitActions.executeBrowserAction(selectedHostId, activeCdpSession.session_id, {
            kind: "screenshot",
            full_page: false,
          });
        }
      })();
      return;
    }
    if (!activeUrl) return;
    setIsLoading(true);
    setReloadNonce((n) => n + 1);
  };

  const captureCdpScreenshot = () => {
    if (!selectedHostId || !activeCdpSession || cdpBusy) return;
    void (async () => {
      await orbitActions.executeBrowserAction(selectedHostId, activeCdpSession.session_id, {
        kind: "snapshot",
      });
      await orbitActions.executeBrowserAction(selectedHostId, activeCdpSession.session_id, {
        kind: "screenshot",
        full_page: false,
      });
    })();
  };

  const handleScreenshotClick = (e: MouseEvent<HTMLImageElement>) => {
    if (!selectedHostId || !activeCdpSession || cdpBusy || overlayState?.readOnly) return;
    const img = e.currentTarget;
    const rect = img.getBoundingClientRect();
    const cw = img.clientWidth;
    const ch = img.clientHeight;
    const nw = img.naturalWidth || activeCdpSession.viewport_width;
    const nh = img.naturalHeight || activeCdpSession.viewport_height;
    if (rect.width <= 0 || rect.height <= 0 || cw <= 0 || ch <= 0 || nw <= 0 || nh <= 0) return;

    // Account for `object-contain` letterboxing so clicks map accurately to viewport pixels.
    const scale = Math.min(cw / nw, ch / nh);
    const renderedW = nw * scale;
    const renderedH = nh * scale;
    const offsetX = (cw - renderedW) / 2;
    const offsetY = (ch - renderedH) / 2;
    const localX = e.clientX - rect.left - offsetX;
    const localY = e.clientY - rect.top - offsetY;
    if (localX < 0 || localY < 0 || localX > renderedW || localY > renderedH) {
      return;
    }

    const x = Math.round((localX / renderedW) * activeCdpSession.viewport_width);
    const y = Math.round((localY / renderedH) * activeCdpSession.viewport_height);
    if (pickElementMode) {
      const vpW = activeCdpSession.viewport_width;
      const vpH = activeCdpSession.viewport_height;
      void (async () => {
        const pickRes = await orbitActions.executeBrowserAction(
          selectedHostId,
          activeCdpSession.session_id,
          { kind: "pick_element", x, y },
        );
        const picked = pickRes?.picked_element;
        if (!picked) {
          orbitActions.showToast(t("sidePanel.pickElementFailed"));
          return;
        }
        const labelTarget = picked.selector || `<${picked.tag}>`;
        orbitActions.addContextMention({
          id: `el_${Date.now()}_${Math.random().toString(36).slice(2, 7)}`,
          kind: "dom_element",
          selector: picked.selector,
          tag: picked.tag,
          textPreview: picked.text_preview,
          attributes: picked.attributes,
          computedBox: picked.computed_box ?? null,
          outerHtmlExcerpt: picked.outer_html_excerpt,
          pageUrl: picked.page_url || activeCdpSession.url,
        });
        setPickElementMode(false);
        if (picked.computed_box && vpW > 0 && vpH > 0) {
          setPickedHighlight({
            box: picked.computed_box,
            viewportWidth: vpW,
            viewportHeight: vpH,
          });
          if (highlightTimerRef.current !== null) {
            window.clearTimeout(highlightTimerRef.current);
          }
          highlightTimerRef.current = window.setTimeout(() => {
            setPickedHighlight(null);
            highlightTimerRef.current = null;
          }, 1_600);
        }
        orbitActions.showToast(t("sidePanel.pickedElementToast", { target: labelTarget }));
      })();
      return;
    }

    void (async () => {
      const clickRes = await orbitActions.executeBrowserAction(
        selectedHostId,
        activeCdpSession.session_id,
        { kind: "click", x, y },
      );
      if (clickRes) {
        await orbitActions.executeBrowserAction(selectedHostId, activeCdpSession.session_id, {
          kind: "screenshot",
          full_page: false,
        });
      }
    })();
  };

  const openPopout = () => {
    const target = isCdpMode
      ? sanitizeHttpUrl(activeCdpSession?.url ?? "")
      : resolvedPreviewUrl;
    if (!target) return;
    void orbitIpc.openBrowserPopoutWindow(target, target).catch((err) => {
      orbitActions.showToast(String(err));
    });
  };

  const openExternal = () => {
    const target = isCdpMode
      ? sanitizeHttpUrl(activeCdpSession?.url ?? "")
      : resolvedPreviewUrl;
    if (!target) return;
    void orbitIpc.openExternalBrowserUrl(target).catch((err) => {
      orbitActions.showToast(String(err));
    });
  };

  const presetWidth = VIEWPORT_WIDTHS[viewportPreset];
  const isReadOnlyOverlay = Boolean(overlayState?.readOnly);
  const showExternalHint = Boolean(
    !isCdpMode && activeUrl && !isLocalUrl(activeUrl) && !dismissedFrameTip,
  );

  const suggestedUrls = useMemo(() => {
    const merged = new Set<string>();
    for (const svc of runningServices) {
      const u = svc.proxy_url?.trim() || (svc.port ? `http://localhost:${svc.port}` : "");
      if (u) merged.add(u);
    }
    for (const detected of detectedUrls) {
      merged.add(detected);
    }
    for (const fallback of QUICK_LOCAL_PORTS) {
      merged.add(fallback);
    }
    return Array.from(merged).slice(0, 6);
  }, [runningServices, detectedUrls]);

  const presetScale =
    presetWidth !== null && stageWidth > 32
      ? Math.min(1, (stageWidth - 32) / presetWidth)
      : 1;

  const effectiveExternalUrl = isCdpMode
    ? sanitizeHttpUrl(activeCdpSession?.url ?? "")
    : resolvedPreviewUrl;

  return (
    <div
      className={cn(
        "orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden",
        !visible && "hidden",
      )}
      onKeyDown={(e) => {
        if (e.key === "Escape" && pickElementMode) {
          e.preventDefault();
          setPickElementMode(false);
        } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "l") {
          e.preventDefault();
          addressInputRef.current?.focus();
          addressInputRef.current?.select();
        } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "r" && canReload) {
          e.preventDefault();
          reload();
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
          {!isCdpMode && (
            <button
              type="button"
              title="Forward"
              disabled={!canForward}
              onClick={goForward}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
            >
              <ArrowRight className="h-[13.5px] w-[13.5px]" />
            </button>
          )}
          <button
            type="button"
            title="Reload"
            disabled={!canReload}
            onClick={reload}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <RotateCw
              className={cn(
                "h-[13px] w-[13px]",
                (isCdpMode ? cdpBusy : isLoading) && "animate-spin",
              )}
            />
          </button>
        </div>

        <form
          onSubmit={(e) => {
            e.preventDefault();
            navigateTo(isCdpMode ? cdpAddressInput : previewAddressInput);
          }}
          className="border-border bg-canvas focus-within:border-foreground/20 flex h-[26px] min-w-0 flex-1 items-center gap-1.5 rounded-lg border px-2 transition-colors"
        >
          <Globe className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0 opacity-75" />
          <input
            ref={addressInputRef}
            type="text"
            value={isCdpMode ? cdpAddressInput : previewAddressInput}
            onChange={(e) =>
              isCdpMode
                ? setCdpAddressInput(e.target.value)
                : setPreviewAddressInput(e.target.value)
            }
            placeholder={
              isCdpMode
                ? t("sidePanel.addressPlaceholderCdp")
                : t("sidePanel.addressPlaceholderPreview")
            }
            className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent font-mono text-[11.5px] outline-none"
          />
        </form>

        {/* Preview vs Live CDP Mode Switch */}
        <div className="bg-muted inline-flex h-[26px] shrink-0 items-center rounded-lg p-0.5 text-[11px]">
          <button
            type="button"
            onClick={() => setMode("preview")}
            className={cn(
              "inline-flex h-[22px] cursor-pointer items-center rounded-md px-2 font-medium transition-colors",
              !isCdpMode
                ? "bg-background text-foreground shadow-2xs"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {t("sidePanel.preview")}
          </button>
          <button
            type="button"
            onClick={() => setMode("cdp")}
            className={cn(
              "inline-flex h-[22px] cursor-pointer items-center gap-1.5 rounded-md px-2 font-medium transition-colors",
              isCdpMode
                ? "bg-background text-foreground shadow-2xs"
                : "text-muted-foreground hover:text-foreground",
            )}
          >
            {hasCdpSessions && <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />}
            <span>{t("sidePanel.cdp")}</span>
          </button>
        </div>

        {/* Preview Viewport Presets or CDP Capture/Close Actions */}
        {!isCdpMode ? (
          <div className="bg-muted hidden h-[26px] shrink-0 items-center rounded-lg p-0.5 sm:inline-flex">
            {(
              [
                { id: "fluid", icon: Maximize2, label: t("sidePanel.fluidWidth") },
                { id: "desktop", icon: Monitor, label: t("sidePanel.desktopViewport") },
                { id: "tablet", icon: Tablet, label: t("sidePanel.tabletViewport") },
                { id: "mobile", icon: Smartphone, label: t("sidePanel.mobileViewport") },
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
        ) : (
          <div className="flex shrink-0 items-center gap-0.5">
            <button
              type="button"
              title={t("sidePanel.pickElement")}
              aria-label={t("sidePanel.pickElement")}
              aria-pressed={pickElementMode}
              disabled={!activeCdpSession || cdpBusy || isReadOnlyOverlay}
              onClick={() => setPickElementMode((v) => !v)}
              className={cn(
                "inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35",
                pickElementMode
                  ? "bg-foreground text-background"
                  : "text-muted-foreground hover:text-foreground hover:bg-accent",
              )}
            >
              <Crosshair className="h-[13px] w-[13px]" />
            </button>
            <button
              type="button"
              title={t("sidePanel.refreshSnapshot")}
              disabled={!activeCdpSession || cdpBusy}
              onClick={captureCdpScreenshot}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
            >
              <Camera className="h-[13px] w-[13px]" />
            </button>
            <button
              type="button"
              title={t("sidePanel.closeCdpSession")}
              disabled={!activeCdpSession || cdpBusy || !selectedHostId}
              onClick={() => {
                setPickElementMode(false);
                if (selectedHostId && activeCdpSession) {
                  void orbitActions.closeBrowserSession(
                    selectedHostId,
                    activeCdpSession.session_id,
                  );
                }
              }}
              className="text-muted-foreground hover:text-destructive hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
            >
              <X className="h-[13.5px] w-[13.5px]" />
            </button>
          </div>
        )}

        {/* Native Popout Window & External Browser */}
        <div className="flex shrink-0 items-center gap-0.5">
          <button
            type="button"
            title={t("sidePanel.openNativeWindow")}
            disabled={!effectiveExternalUrl}
            onClick={openPopout}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <AppWindow className="h-[13.5px] w-[13.5px]" />
          </button>
          <button
            type="button"
            title={t("sidePanel.openSystemBrowser")}
            disabled={!effectiveExternalUrl}
            onClick={openExternal}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          >
            <ExternalLink className="h-[13px] w-[13px]" />
          </button>
        </div>
      </div>

      {/* Live CDP Status & Audit Strip */}
      {isCdpMode && activeCdpSession && (
        <div className="border-border bg-sidebar/60 flex shrink-0 items-center justify-between gap-2 border-b px-2.5 py-1 text-[11px]">
          <div className="flex min-w-0 items-center gap-2">
            {cdpSessions.length > 1 && selectedHostId ? (
              <Select<string>
                size="compact"
                align="start"
                ariaLabel="Active CDP browser session"
                value={activeCdpSession.session_id}
                options={cdpSessions.map((s) => ({
                  value: s.session_id,
                  label: s.session_id,
                  description: s.title || s.url || undefined,
                  mono: true,
                }))}
                triggerClassName="h-[22px] max-w-[164px] bg-canvas"
                menuClassName="min-w-[200px] max-w-[280px]"
                onChange={(sessionId) =>
                  orbitActions.selectBrowserSession(selectedHostId, sessionId)
                }
              />
            ) : (
              <span className="text-muted-foreground font-mono text-[10.5px]">
                {activeCdpSession.session_id}
              </span>
            )}
            <span className="text-muted-foreground truncate">
              {activeCdpSession.title || activeCdpSession.url}
            </span>
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            <span className="border-border bg-canvas text-muted-foreground inline-flex items-center gap-1 rounded-md border px-1.5 py-0.5 font-mono text-[10.5px] tabular-nums">
              <Layers className="h-3 w-3 opacity-70" />
              {activeCdpSession.element_count} refs
            </span>
            <button
              type="button"
              onClick={() => setConsoleOpen((v) => !v)}
              className={cn(
                "border-border inline-flex cursor-pointer items-center gap-1 rounded-md border px-1.5 py-0.5 font-mono text-[10.5px] tabular-nums transition-colors",
                activeCdpSession.console_errors.length > 0
                  ? "border-amber-500/40 bg-amber-500/10 text-amber-600 dark:text-amber-400"
                  : "bg-canvas text-muted-foreground hover:text-foreground",
              )}
            >
              <AlertCircle className="h-3 w-3" />
              {activeCdpSession.console_errors.length} err
            </button>
            <button
              type="button"
              onClick={() => setAuditOpen((v) => !v)}
              className={cn(
                "border-border inline-flex cursor-pointer items-center gap-1 rounded-md border px-1.5 py-0.5 text-[10.5px] transition-colors",
                auditOpen
                  ? "bg-accent text-foreground"
                  : "bg-canvas text-muted-foreground hover:text-foreground",
              )}
            >
              <History className="h-3 w-3" />
              <span>Audit</span>
              {auditOpen ? (
                <ChevronDown className="h-3 w-3" />
              ) : (
                <ChevronUp className="h-3 w-3" />
              )}
            </button>
          </div>
        </div>
      )}

      {/* Active Pick Element Mode Hint Strip */}
      {isCdpMode && activeCdpSession && pickElementMode && (
        <div className="border-border bg-elevated/80 text-foreground flex shrink-0 items-center justify-between gap-2 border-b px-2.5 py-1 text-[11px]">
          <div className="flex min-w-0 items-center gap-1.5">
            <Crosshair className="text-signal-info h-3 w-3 shrink-0" />
            <span className="truncate">{t("sidePanel.pickElementActive")}</span>
          </div>
          <button
            type="button"
            onClick={() => setPickElementMode(false)}
            aria-label={t("common.cancel")}
            className="shrink-0 cursor-pointer rounded p-0.5 transition-opacity hover:opacity-80"
          >
            <ShortcutKbd keys="Esc" size="xs" variant="subtle" />
          </button>
        </div>
      )}

      {/* Collapsible Console Errors Strip */}
      {isCdpMode && activeCdpSession && consoleOpen && (
        <div className="border-border bg-canvas/90 max-h-28 shrink-0 overflow-y-auto border-b px-3 py-1.5 font-mono text-[11px]">
          {activeCdpSession.console_errors.length === 0 ? (
            <div className="text-muted-foreground">No console errors captured.</div>
          ) : (
            <div className="space-y-1">
              {activeCdpSession.console_errors.map((err, idx) => (
                <div key={`${idx}-${err.slice(0, 24)}`} className="text-amber-600 dark:text-amber-400">
                  {err}
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Subtle Notice for External Websites That Restrict Embedded Frames */}
      {showExternalHint && (
        <div className="border-border bg-elevated/60 text-muted-foreground flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1.5 text-[11.5px]">
          <span className="truncate">
            If this site blocks embedded preview, open it in a native window or Live CDP.
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

      {/* Main Stage: Live CDP View OR Existing Embedded Web Preview */}
      <div ref={stageRef} className="bg-canvas relative flex min-h-0 flex-1 flex-col overflow-auto">
        {isCdpMode ? (
          !activeCdpSession ? (
            <div className="flex flex-1 flex-col items-center justify-center gap-4 px-6 py-10 text-center">
              <div className="bg-muted text-muted-foreground inline-flex h-10 w-10 items-center justify-center rounded-xl">
                <Monitor className="h-5 w-5 stroke-[1.6]" />
              </div>
              <div className="space-y-1">
                <div className="text-foreground text-[13.5px] font-medium">
                  Live CDP Browser Session
                </div>
                <div className="text-muted-foreground max-w-xs text-[12px] leading-relaxed">
                  Launch an isolated Chromium session with URL, SSRF, and sensitive-input guards.
                </div>
              </div>

              <div className="flex flex-col items-center gap-2">
                <button
                  type="button"
                  disabled={!selectedHostId || cdpBusy}
                  onClick={() => {
                    if (!selectedHostId) return;
                    const normalized = normalizeAddressInput(cdpAddressInput || activeUrl);
                    void orbitActions.createBrowserSession({
                      hostId: selectedHostId,
                      agentId: selectedAgentId ?? null,
                      initialUrl: normalized || null,
                      workspaceRoot: resolvedWorkspaceRoot,
                    });
                  }}
                  className="bg-foreground text-background hover:opacity-90 inline-flex cursor-pointer items-center gap-1.5 rounded-lg px-3 py-1.5 text-[12px] font-medium transition-opacity disabled:cursor-default disabled:opacity-45"
                >
                  <Play className="h-3.5 w-3.5" />
                  <span>{cdpBusy ? "Starting…" : "Start CDP Session"}</span>
                </button>
                <div className="text-muted-foreground font-mono text-[10.5px]">
                  {hostBrowser?.chromeBinary
                    ? `Binary: ${hostBrowser.chromeBinary.split(/[\\/]/).pop()}`
                    : hostBrowser?.status === "ready"
                      ? "No Chrome/Chromium binary detected on host"
                      : ""}
                </div>
              </div>

              {suggestedUrls.length > 0 && (
                <div className="flex max-w-sm flex-wrap items-center justify-center gap-1.5">
                  {suggestedUrls.map((url) => (
                    <button
                      key={url}
                      type="button"
                      disabled={!selectedHostId || cdpBusy}
                      onClick={() => {
                        setCdpAddressInput(url);
                        void runCdpNavigateAndCapture(url);
                      }}
                      className="border-border hover:bg-accent text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 py-1 font-mono text-[11.5px] transition-colors disabled:opacity-45"
                    >
                      <span>{url.replace(/^https?:\/\//, "")}</span>
                    </button>
                  ))}
                </div>
              )}
            </div>
          ) : (
            <div className="bg-muted/25 relative flex min-h-full flex-1 flex-col items-center justify-center p-3">
              {activeCdpSession.last_screenshot_base64 ? (
                <div className="relative inline-flex max-h-full max-w-full items-center justify-center">
                  <img
                    src={`data:image/png;base64,${activeCdpSession.last_screenshot_base64}`}
                    alt={activeCdpSession.title || activeCdpSession.url}
                    onClick={handleScreenshotClick}
                    className={cn(
                      "block max-h-full max-w-full rounded-lg border bg-white object-contain shadow-sm transition-colors",
                      pickElementMode ? "border-signal-info/70" : "border-border",
                      !cdpBusy && !isReadOnlyOverlay ? "cursor-crosshair" : "cursor-default",
                    )}
                  />
                  {pickedHighlight && (
                    <div
                      aria-hidden
                      style={{
                        left: `${Math.max(0, Math.min(100, (pickedHighlight.box.x / pickedHighlight.viewportWidth) * 100))}%`,
                        top: `${Math.max(0, Math.min(100, (pickedHighlight.box.y / pickedHighlight.viewportHeight) * 100))}%`,
                        width: `${Math.max(0.5, Math.min(100, (pickedHighlight.box.width / pickedHighlight.viewportWidth) * 100))}%`,
                        height: `${Math.max(0.5, Math.min(100, (pickedHighlight.box.height / pickedHighlight.viewportHeight) * 100))}%`,
                      }}
                      className="border-signal-info bg-signal-info/15 pointer-events-none absolute rounded-sm border-[1.5px] transition-opacity duration-200"
                    />
                  )}
                </div>
              ) : (
                <div className="text-muted-foreground flex flex-col items-center gap-2 text-[12px]">
                  <span>Session ready at {activeCdpSession.url}</span>
                  <button
                    type="button"
                    disabled={cdpBusy}
                    onClick={captureCdpScreenshot}
                    className="border-border hover:bg-accent text-foreground inline-flex cursor-pointer items-center gap-1.5 rounded-md border px-2.5 py-1 text-[11.5px] transition-colors"
                  >
                    <Camera className="h-3.5 w-3.5" />
                    <span>Capture Screenshot</span>
                  </button>
                </div>
              )}
            </div>
          )
        ) : !activeUrl ? (
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
                {isRemoteHost &&
                matchedRemoteService &&
                !activeRemoteTunnel &&
                remoteTunnelOpeningKey === remoteTunnelKey ? (
                  <div className="bg-canvas text-muted-foreground flex h-full w-full flex-1 flex-col items-center justify-center gap-2 text-[12px]">
                    <Loader2 className="h-4 w-4 animate-spin" />
                    <span>
                      {t("sidePanel.remoteTunnelOpening", {
                        host: hostCard?.display_name ?? selectedHostId ?? "",
                      })}
                    </span>
                  </div>
                ) : (
                  <iframe
                    ref={iframeRef}
                    key={`${resolvedPreviewUrl}::${reloadNonce}`}
                    src={resolvedPreviewUrl}
                    title={`Preview ${activeUrl}`}
                    onLoad={() => setIsLoading(false)}
                    sandbox="allow-scripts allow-same-origin allow-forms allow-popups allow-modals allow-downloads"
                    className={cn(
                      "h-full w-full flex-1 border-0 bg-white",
                      isReadOnlyOverlay && "pointer-events-none select-none",
                    )}
                  />
                )}
              </div>

              {/* Dedicated Top Stacking Layer Reserved for Future Automation Read-Only Animations */}
              {isReadOnlyOverlay && (
                <div
                  data-slot="browser-overlay-layer"
                  className="pointer-events-auto absolute inset-0 z-10 flex items-end justify-center bg-black/[0.04] p-4 backdrop-blur-[0.5px]"
                >
                  {overlayState?.statusLabel && (
                    <div className="bg-elevated/95 border-border floating-surface text-foreground inline-flex items-center gap-2 rounded-full border px-3 py-1 text-[12px] font-medium">
                      <span className="bg-signal-info h-2 w-2 animate-pulse rounded-full" />
                      <span>{overlayState.statusLabel}</span>
                    </div>
                  )}
                </div>
              )}
            </div>
          </div>
        )}
      </div>

      {/* Collapsible CDP Action Audit Drawer */}
      {isCdpMode && activeCdpSession && auditOpen && (
        <div className="border-border bg-sidebar max-h-44 shrink-0 overflow-y-auto border-t">
          <div className="border-border text-muted-foreground flex items-center justify-between border-b px-3 py-1 text-[10.5px] font-medium">
            <span>Action Audit Log ({activeCdpSession.recent_actions.length})</span>
            <span className="font-mono">
              {activeCdpSession.viewport_width}×{activeCdpSession.viewport_height}
            </span>
          </div>
          {activeCdpSession.recent_actions.length === 0 ? (
            <div className="text-muted-foreground px-3 py-2 text-[11px]">No actions yet.</div>
          ) : (
            <div className="divide-border/60 divide-y">
              {[...activeCdpSession.recent_actions].reverse().map((entry) => (
                <div
                  key={entry.id}
                  className="flex items-start gap-2 px-3 py-1.5 text-[11px]"
                >
                  <span
                    className={cn(
                      "mt-1 h-1.5 w-1.5 shrink-0 rounded-full",
                      entry.ok ? "bg-emerald-500" : "bg-rose-500",
                    )}
                  />
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-1.5">
                      <span className="text-foreground font-mono font-medium">{entry.action}</span>
                      <span className="text-muted-foreground truncate font-mono text-[10.5px]">
                        {entry.target}
                      </span>
                      <span className="text-muted-foreground ml-auto shrink-0 font-mono text-[10px] tabular-nums">
                        {formatAuditTime(entry.timestamp_ms)}
                      </span>
                    </div>
                    <div className="text-muted-foreground truncate text-[10.5px]">
                      {entry.detail}
                    </div>
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Host Context Footnote & Automatic Remote E2EE Tunnel Status */}
      {!isCdpMode && selectedHostId && activeUrl && isLocalUrl(activeUrl) && hostCard && isRemoteHost && (
        <div className="border-border bg-sidebar/80 text-muted-foreground flex items-center justify-between gap-2 border-t px-3 py-1 text-[11px]">
          <div className="flex min-w-0 items-center gap-1.5">
            {remoteTunnelOpeningKey && remoteTunnelOpeningKey === remoteTunnelKey ? (
              <>
                <Loader2 className="h-3 w-3 shrink-0 animate-spin" />
                <span className="truncate">
                  {t("sidePanel.remoteTunnelOpening", { host: hostCard.display_name })}
                </span>
              </>
            ) : activeRemoteTunnel?.is_tunneled ? (
              <>
                <span className="h-1.5 w-1.5 shrink-0 rounded-full bg-emerald-500" />
                <span className="truncate font-mono tabular-nums">
                  {t("sidePanel.remoteTunnelActive", {
                    host: hostCard.display_name,
                    localUrl: activeRemoteTunnel.local_url.replace(/^https?:\/\//, ""),
                  })}
                </span>
              </>
            ) : (
              <span className="truncate">
                {t("sidePanel.remoteTunnelFallback", { host: hostCard.display_name })}
              </span>
            )}
          </div>
          {presetWidth !== null && presetScale < 1 && (
            <span className="shrink-0 font-mono tabular-nums">{Math.round(presetScale * 100)}%</span>
          )}
        </div>
      )}
    </div>
  );
});
