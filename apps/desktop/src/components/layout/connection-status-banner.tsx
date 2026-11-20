import { memo, useEffect, useState } from "react";
import { AlertTriangle, RefreshCw, WifiOff, X } from "lucide-react";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { compareSemver, normalizeSemver } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

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
  const upgradeState = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostUpgradeStates[s.selectedHostId] ?? null) : null,
  );

  const [reconnecting, setReconnecting] = useState(false);
  const [showLocalUpgradeStrip, setShowLocalUpgradeStrip] = useState(false);

  // Local sidecar upgrades usually complete in < 600ms; delay showing any strip so fast swaps are 100% invisible
  useEffect(() => {
    if (upgradeState?.status !== "upgrading" || upgradeState.mode !== "local") {
      setShowLocalUpgradeStrip(false);
      return;
    }
    const timer = window.setTimeout(() => setShowLocalUpgradeStrip(true), 1200);
    return () => window.clearTimeout(timer);
  }, [upgradeState?.status, upgradeState?.mode, upgradeState?.startedAt]);

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

  // 2. Active silent background daemon upgrade (remote SSH/E2EE wire or slow local fallback)
  if (
    selectedHost &&
    upgradeState?.status === "upgrading" &&
    (upgradeState.mode !== "local" || showLocalUpgradeStrip)
  ) {
    return (
      <div
        role="status"
        className="orbit-stage-enter border-border/80 bg-muted/40 text-muted-foreground flex shrink-0 items-center gap-2 border-b px-4 py-1.5 text-[12px]"
      >
        <OrbitSpinner size={12} className="text-muted-foreground/80 shrink-0" />
        <span className="truncate">
          Updating <span className="text-foreground/85 font-medium">{selectedHost.display_name}</span>{" "}
          in background… Running tasks stay uninterrupted.
        </span>
      </div>
    );
  }

  // 3. Selected host disconnected / reconnecting
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
        <OrbitSpinner size={12} className="text-muted-foreground/80 shrink-0" />
        <span className="truncate">
          Connecting to{" "}
          <span className="text-foreground/85 font-medium">{selectedHost.display_name}</span>{" "}
          {modeLabel}…
        </span>
      </div>
    );
  }

  // 4. Protocol-lagging daemon fallback (ONLY when protocol is behind AND automatic upgrade hasn't completed)
  // Always offers a direct 1-click Update button — never asks the user to copy/paste shell commands.
  const normApp = normalizeSemver(appVersion);
  const normDaemon = normalizeSemver(selectedHost?.daemon_version);
  const isOnline =
    selectedHost?.connection_state === "online_direct" ||
    selectedHost?.connection_state === "online_relay";
  const daemonPv = selectedHost?.daemon_protocol_version ?? null;
  const clientPv = selectedHost?.client_protocol_version ?? null;
  const isProtocolBehind =
    daemonPv !== null && clientPv !== null && clientPv > 0 && daemonPv < clientPv;
  const isDaemonOlder = Boolean(normApp && normDaemon && compareSemver(normDaemon, normApp) < 0);

  if (
    selectedHost &&
    isOnline &&
    isProtocolBehind &&
    isDaemonOlder &&
    normDaemon &&
    dismissedDaemonVer !== normDaemon &&
    upgradeState?.status !== "upgrading"
  ) {
    return (
      <div
        role="status"
        className="orbit-stage-enter border-border/80 bg-muted/40 text-muted-foreground flex shrink-0 items-center justify-between gap-3 border-b px-4 py-1.5 text-[12px]"
      >
        <div className="flex min-w-0 items-center gap-2">
          <span className="truncate">
            <span className="text-foreground/85 font-medium">{selectedHost.display_name}</span> is
            on an older version (<span className="font-mono">v{normDaemon}</span>) — update to unlock
            the latest features.
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <button
            type="button"
            onClick={() =>
              void orbitActions.upgradeHostDaemon(selectedHost.host_id, { force: true })
            }
            className="text-foreground/85 hover:text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors"
          >
            <span>{upgradeState?.status === "failed" ? "Retry update" : "Update"}</span>
          </button>
          <button
            type="button"
            aria-label="Dismiss update notice"
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
