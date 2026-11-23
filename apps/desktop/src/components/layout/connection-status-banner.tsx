import { memo, useEffect, useState } from "react";
import { AlertTriangle, RefreshCw, WifiOff, X } from "lucide-react";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
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
    const offlineText = t("banner.networkOffline");
    return (
      <div
        role="status"
        className="@container orbit-stage-enter flex min-h-[30px] min-w-0 shrink-0 items-center justify-between gap-2 overflow-hidden border-b border-amber-500/20 bg-amber-500/[0.055] px-3 py-1 text-[12px] text-amber-800 dark:text-amber-200 @[560px]:px-4"
      >
        <div className="flex min-w-0 items-center gap-2">
          <WifiOff className="size-3.5 shrink-0 text-amber-600 dark:text-amber-400" />
          <span title={offlineText} className="truncate">
            {offlineText}
          </span>
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
        className="@container orbit-stage-enter border-border/70 bg-canvas text-muted-foreground flex min-h-[30px] min-w-0 shrink-0 items-center justify-between gap-2 overflow-hidden border-b px-3 py-1 text-[12px] @[560px]:px-4"
      >
        <div className="flex min-w-0 items-center gap-2">
          <OrbitSpinner size={12} className="shrink-0 text-sky-500" />
          <span title={label} className="truncate">
            {label}
          </span>
        </div>
        <button
          type="button"
          onClick={() => orbitActions.openSettings("general")}
          className="text-foreground/80 hover:text-foreground shrink-0 cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
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
    const upgradingText = t("banner.updatingHostInBackground", { host: selectedHost.display_name });
    return (
      <div
        role="status"
        className="@container orbit-stage-enter border-border/70 bg-canvas text-muted-foreground flex min-h-[30px] min-w-0 shrink-0 items-center gap-2 overflow-hidden border-b px-3 py-1 text-[12px] @[560px]:px-4"
      >
        <OrbitSpinner size={12} className="text-muted-foreground/80 shrink-0" />
        <span title={upgradingText} className="truncate">
          {upgradingText}
        </span>
      </div>
    );
  }

  // 3. Selected host disconnected / reconnecting
  if (selectedHost && selectedHost.connection_state === "disconnected") {
    const errSnippet = selectedHost.last_error?.trim() || null;
    const disconnectedText = `${t("banner.disconnectedFrom", { host: selectedHost.display_name })}${errSnippet ? ` · ${errSnippet}` : ""}`;
    const reconnectLabel = reconnecting ? t("banner.reconnecting") : t("banner.reconnect");
    return (
      <div
        role="status"
        className="@container orbit-stage-enter flex min-h-[30px] min-w-0 shrink-0 items-center justify-between gap-2 overflow-hidden border-b border-rose-500/20 bg-rose-500/[0.055] px-3 py-1 text-[12px] text-rose-800 dark:text-rose-200 @[560px]:px-4"
      >
        <div className="flex min-w-0 items-center gap-2">
          <AlertTriangle className="size-3.5 shrink-0 text-rose-600 dark:text-rose-400" />
          <span title={disconnectedText} className="truncate">
            {disconnectedText}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <SimpleTooltip label={reconnectLabel}>
            <button
              type="button"
              aria-label={reconnectLabel}
              disabled={reconnecting}
              onClick={() => {
                if (!selectedHostId) return;
                setReconnecting(true);
                void orbitActions.reconnectHost(selectedHostId).finally(() => {
                  setReconnecting(false);
                });
              }}
              className="inline-flex h-[22px] cursor-pointer items-center justify-center gap-1 rounded-md bg-rose-500/[0.11] px-1.5 text-[11.5px] font-medium text-rose-700 transition-colors hover:bg-rose-500/[0.18] disabled:opacity-60 dark:text-rose-200 @[460px]:px-2"
            >
              <RefreshCw className={reconnecting ? "size-3 shrink-0 animate-spin" : "size-3 shrink-0"} />
              <span className="hidden @[460px]:inline">{reconnectLabel}</span>
            </button>
          </SimpleTooltip>
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
    const connectingText = t("banner.connectingTo", {
      host: selectedHost.display_name,
      mode: modeLabel,
    });
    return (
      <div
        role="status"
        className="@container orbit-stage-enter border-border/70 bg-canvas text-muted-foreground flex min-h-[30px] min-w-0 shrink-0 items-center gap-2 overflow-hidden border-b px-3 py-1 text-[12px] @[560px]:px-4"
      >
        <OrbitSpinner size={12} className="text-muted-foreground/80 shrink-0" />
        <span title={connectingText} className="truncate">
          {connectingText}
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
    const mismatchText = t("banner.hostOlderVersion", {
      host: selectedHost.display_name,
      version: normDaemon,
    });
    const updateAllFull = t("banner.updateAllCount", { count: outdatedFleetHostsCount });
    return (
      <div
        role="status"
        className="@container orbit-stage-enter border-border/70 bg-canvas text-muted-foreground flex min-h-[30px] min-w-0 shrink-0 items-center justify-between gap-2 overflow-hidden border-b px-3 py-1 text-[12px] @[560px]:px-4"
      >
        <div className="flex min-w-0 items-center gap-2">
          <span title={mismatchText} className="truncate">
            {mismatchText}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-0.5 @[520px]:gap-1">
          {outdatedFleetHostsCount > 1 && (
            <SimpleTooltip label={updateAllFull}>
              <button
                type="button"
                aria-label={updateAllFull}
                onClick={() => void orbitActions.upgradeAllConnectedHosts()}
                className="inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11.5px] font-medium text-sky-600 transition-colors hover:bg-sky-500/12 dark:text-sky-400 @[520px]:px-2"
              >
                <span className="hidden @[560px]:inline">{updateAllFull}</span>
                <span className="@[560px]:hidden">
                  {t("banner.updateAllShort", { count: outdatedFleetHostsCount })}
                </span>
              </button>
            </SimpleTooltip>
          )}
          <button
            type="button"
            onClick={() =>
              void orbitActions.upgradeHostDaemon(selectedHost.host_id, {
                force: true,
                targetVersion,
              })
            }
            className="text-foreground/85 hover:text-foreground inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11.5px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06] @[520px]:px-2"
          >
            <span>
              {upgradeState?.status === "failed" ? t("banner.retryUpdate") : t("common.update")}
            </span>
          </button>
          <SimpleTooltip label={t("banner.dismissNotice")}>
            <button
              type="button"
              aria-label={t("banner.dismissNotice")}
              onClick={() => orbitActions.dismissVersionMismatch(selectedHost.host_id, normDaemon)}
              className="text-muted-foreground hover:text-foreground inline-flex size-5 cursor-pointer items-center justify-center rounded-md transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              <X className="size-3" />
            </button>
          </SimpleTooltip>
        </div>
      </div>
    );
  }

  return null;
});
