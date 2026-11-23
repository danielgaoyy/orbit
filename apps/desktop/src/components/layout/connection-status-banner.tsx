import { memo, useEffect, useState } from "react";
import { AlertTriangle, RefreshCw, WifiOff, X } from "lucide-react";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { compareSemver, normalizeSemver } from "@/lib/orbit-ipc";
import {
  orbitActions,
  resolveFleetTargetVersion,
  useOrbitSelector,
} from "@/state/orbit-store";

export const ConnectionStatusBanner = memo(function ConnectionStatusBanner() {
  const { t } = useI18n();
  const isBrowserOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const fleet = useOrbitSelector((s) => s.fleet);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);
  const hostUpgradeStates = useOrbitSelector((s) => s.hostUpgradeStates);
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
          <span className="truncate">{t("banner.networkOffline")}</span>
        </div>
      </div>
    );
  }

  // 1.5 Active fleet-wide update progress (persists even after closing Settings)
  if (
    fleetUpdate.phase === "upgrading_hosts" ||
    fleetUpdate.phase === "upgrading_desktop"
  ) {
    const upgradingCount = Object.values(hostUpgradeStates).filter(
      (st) => st.status === "upgrading",
    ).length;
    const label =
      fleetUpdate.phase === "upgrading_desktop"
        ? (fleetUpdate.desktopStatus ?? t("banner.updatingDesktop"))
        : upgradingCount > 0
          ? t("banner.updatingHosts", {
              count: upgradingCount,
              version: fleetUpdate.targetVersion ? ` (v${fleetUpdate.targetVersion})` : "",
            })
          : (fleetUpdate.summaryMessage ?? t("banner.updatingFleet"));
    return (
      <div
        role="status"
        className="orbit-stage-enter border-border/80 bg-muted/40 text-muted-foreground flex shrink-0 items-center justify-between gap-3 border-b px-4 py-1.5 text-[12px]"
      >
        <div className="flex min-w-0 items-center gap-2">
          <OrbitSpinner size={12} className="shrink-0 text-sky-500" />
          <span className="truncate">{label}</span>
        </div>
        <button
          type="button"
          onClick={() => orbitActions.openSettings("general")}
          className="text-foreground/80 hover:text-foreground hover:bg-accent shrink-0 cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors"
        >
          {t("banner.viewProgress")}
        </button>
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
          {t("banner.updatingHostInBackground", { host: selectedHost.display_name })}
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
            {t("banner.disconnectedFrom", { host: selectedHost.display_name })}
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
            <span>{reconnecting ? t("banner.reconnecting") : t("banner.reconnect")}</span>
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
      selectedHost.connection_state === "connecting_relay"
        ? t("banner.viaRelay")
        : t("banner.directly");
    return (
      <div
        role="status"
        className="orbit-stage-enter border-border/80 bg-muted/45 text-muted-foreground flex shrink-0 items-center gap-2 border-b px-4 py-1.5 text-[12.5px]"
      >
        <OrbitSpinner size={12} className="text-muted-foreground/80 shrink-0" />
        <span className="truncate">
          {t("banner.connectingTo", { host: selectedHost.display_name, mode: modeLabel })}
        </span>
      </div>
    );
  }

  // 4. Protocol-lagging daemon fallback (ONLY when protocol is behind AND automatic upgrade hasn't completed)
  // Always offers a direct 1-click Update button — never asks the user to copy/paste shell commands.
  const targetVersion = resolveFleetTargetVersion(appVersion, releaseUpdateInfo);
  const normApp = normalizeSemver(appVersion);
  const normDaemon = normalizeSemver(selectedHost?.daemon_version);
  const isOnline =
    selectedHost?.connection_state === "online_direct" ||
    selectedHost?.connection_state === "online_relay";
  const daemonPv = selectedHost?.daemon_protocol_version ?? null;
  const clientPv = selectedHost?.client_protocol_version ?? null;
  const isProtocolBehind =
    daemonPv !== null && clientPv !== null && clientPv > 0 && daemonPv < clientPv;
  const isDaemonOlder = Boolean(
    (targetVersion ?? normApp) &&
      normDaemon &&
      compareSemver(normDaemon, (targetVersion ?? normApp) as string) < 0,
  );
  const outdatedFleetHostsCount = targetVersion
    ? fleet.hosts.filter((h) => {
        const online =
          h.connection_state === "online_direct" || h.connection_state === "online_relay";
        if (!online && !h.ssh_alias) return false;
        const v = normalizeSemver(h.daemon_version);
        return Boolean(v && compareSemver(v, targetVersion) < 0);
      }).length
    : 0;

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
            {t("banner.hostOlderVersion", {
              host: selectedHost.display_name,
              version: normDaemon,
            })}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          {outdatedFleetHostsCount > 1 && (
            <button
              type="button"
              onClick={() => void orbitActions.upgradeAllConnectedHosts()}
              className="text-sky-600 hover:bg-sky-500/12 dark:text-sky-400 inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors"
            >
              <span>{t("banner.updateAllCount", { count: outdatedFleetHostsCount })}</span>
            </button>
          )}
          <button
            type="button"
            onClick={() =>
              void orbitActions.upgradeHostDaemon(selectedHost.host_id, {
                force: true,
                targetVersion,
              })
            }
            className="text-foreground/85 hover:text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors"
          >
            <span>
              {upgradeState?.status === "failed" ? t("banner.retryUpdate") : t("common.update")}
            </span>
          </button>
          <button
            type="button"
            aria-label={t("banner.dismissNotice")}
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
