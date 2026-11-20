import { memo, useState } from "react";
import { AlertTriangle, Loader2, RefreshCw, Settings2, WifiOff, X } from "lucide-react";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

function normalizeSemver(raw: string | null | undefined): string | null {
  if (!raw) return null;
  const trimmed = raw.trim().replace(/^v/i, "");
  return trimmed.length > 0 ? trimmed : null;
}

export const ConnectionStatusBanner = memo(function ConnectionStatusBanner() {
  const isBrowserOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedHost = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.fleet.hosts.find((h) => h.host_id === s.selectedHostId) ?? null)
      : null,
  );
  const dismissedDaemonVer = useOrbitSelector((s) =>
    s.selectedHostId ? (s.dismissedVersionMismatchHosts[s.selectedHostId] ?? null) : null,
  );
  const [reconnecting, setReconnecting] = useState(false);

  // 1. OS/WebView internet offline — highest priority (loopback daemon stays green even when internet drops)
  if (isBrowserOffline) {
    return (
      <div
        role="status"
        className="orbit-stage-enter flex shrink-0 items-center justify-between gap-3 border-b border-amber-500/25 bg-amber-500/[0.08] px-4 py-1.5 text-[12.5px] text-amber-800 dark:text-amber-200"
      >
        <div className="flex min-w-0 items-center gap-2">
          <WifiOff className="size-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
          <span className="truncate">
            Network offline — upstream model requests may fail or stall until connection recovers.
          </span>
        </div>
      </div>
    );
  }

  // 2. Selected host disconnected / reconnecting
  if (selectedHost && selectedHost.connection_state === "disconnected") {
    const errSnippet = selectedHost.last_error?.trim() || null;
    return (
      <div
        role="status"
        className="orbit-stage-enter flex shrink-0 items-center justify-between gap-3 border-b border-rose-500/25 bg-rose-500/[0.07] px-4 py-1.5 text-[12.5px] text-rose-800 dark:text-rose-200"
      >
        <div className="flex min-w-0 items-center gap-2">
          <AlertTriangle className="size-3.5 shrink-0 text-rose-600 dark:text-rose-400" />
          <span className="truncate">
            Disconnected from <span className="font-medium">{selectedHost.display_name}</span>
            {errSnippet ? ` · ${errSnippet}` : ""}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1.5">
          <button
            type="button"
            disabled={reconnecting}
            onClick={() => {
              if (!selectedHostId) return;
              setReconnecting(true);
              void orbitActions.reconnectHost(selectedHostId).finally(() => {
                setReconnecting(false);
              });
            }}
            className="inline-flex cursor-pointer items-center gap-1 rounded-md border border-rose-500/30 bg-background/60 px-2 py-0.5 text-[12px] font-medium transition-colors hover:bg-background disabled:opacity-60"
          >
            <RefreshCw className={reconnecting ? "size-3 animate-spin" : "size-3"} />
            <span>{reconnecting ? "Reconnecting…" : "Reconnect"}</span>
          </button>
        </div>
      </div>
    );
  }

  if (
    selectedHost &&
    (selectedHost.connection_state === "connecting_direct" ||
      selectedHost.connection_state === "connecting_relay")
  ) {
    const modeLabel =
      selectedHost.connection_state === "connecting_relay" ? "via Relay" : "directly";
    return (
      <div
        role="status"
        className="orbit-stage-enter border-border/80 bg-muted/45 text-muted-foreground flex shrink-0 items-center gap-2 border-b px-4 py-1.5 text-[12.5px]"
      >
        <Loader2 className="size-3.5 shrink-0 animate-spin opacity-80" />
        <span className="truncate">
          Connecting to <span className="text-foreground/85 font-medium">{selectedHost.display_name}</span>{" "}
          {modeLabel}…
        </span>
      </div>
    );
  }

  // 3. Online daemon version mismatch (dismissible per host + version)
  const normApp = normalizeSemver(appVersion);
  const normDaemon = normalizeSemver(selectedHost?.daemon_version);
  if (
    selectedHost &&
    (selectedHost.connection_state === "online_direct" ||
      selectedHost.connection_state === "online_relay") &&
    normApp &&
    normDaemon &&
    normApp !== normDaemon &&
    dismissedDaemonVer !== normDaemon
  ) {
    return (
      <div
        role="status"
        className="orbit-stage-enter border-border/80 bg-muted/40 text-muted-foreground flex shrink-0 items-center justify-between gap-3 border-b px-4 py-1.5 text-[12px]"
      >
        <div className="flex min-w-0 items-center gap-2">
          <AlertTriangle className="size-3.5 shrink-0 text-amber-500" />
          <span className="truncate">
            Host <span className="text-foreground/85 font-medium">{selectedHost.display_name}</span>{" "}
            is running Daemon <span className="font-mono">v{normDaemon}</span> while Desktop is{" "}
            <span className="font-mono">v{normApp}</span>. Restart or upgrade the daemon to keep
            features in sync.
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={() =>
              orbitActions.openSettings(`host:${selectedHost.host_id}:connection`)
            }
            className="text-foreground/80 hover:text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors"
          >
            <Settings2 className="size-3" />
            <span>Host settings</span>
          </button>
          <button
            type="button"
            aria-label="Dismiss version mismatch notice"
            onClick={() => orbitActions.dismissVersionMismatch(selectedHost.host_id, normDaemon)}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-5 cursor-pointer items-center justify-center rounded-md transition-colors"
          >
            <X className="size-3" />
          </button>
        </div>
      </div>
    );
  }

  return null;
});
