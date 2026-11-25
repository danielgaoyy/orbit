import { memo, useEffect, useState } from "react";
import { AlertTriangle, RefreshCw, WifiOff } from "lucide-react";
import { ContextBanner } from "@/components/ui/context-banner";
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

  useEffect(() => {
    if (upgradeState?.status !== "upgrading" || upgradeState.mode !== "local") {
      setShowLocalUpgradeStrip(false);
      return;
    }
    const timer = window.setTimeout(() => setShowLocalUpgradeStrip(true), 1200);
    return () => window.clearTimeout(timer);
  }, [upgradeState?.status, upgradeState?.mode, upgradeState?.startedAt]);

  if (isBrowserOffline) {
    const offlineText = t("banner.networkOffline");
    return (
      <ContextBanner
        variant="strip"
        tone="warning"
        icon={<WifiOff className="size-3.5 shrink-0" />}
      >
        <span title={offlineText}>{offlineText}</span>
      </ContextBanner>
    );
  }

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
      <ContextBanner
        variant="strip"
        tone="neutral"
        icon={<OrbitSpinner size={12} className="text-signal-info" />}
        actions={
          <button
            type="button"
            onClick={() => orbitActions.openSettings("hosts")}
            className="text-ink-primary hover:bg-accent cursor-pointer rounded-[6px] px-2 py-0.5 text-[11px] font-medium transition-colors"
          >
            {t("banner.viewProgress")}
          </button>
        }
      >
        <span title={label}>{label}</span>
      </ContextBanner>
    );
  }

  if (
    selectedHost &&
    upgradeState?.status === "upgrading" &&
    (upgradeState.mode !== "local" || showLocalUpgradeStrip)
  ) {
    const upgradingText = t("banner.updatingHostInBackground", { host: selectedHost.display_name });
    return (
      <ContextBanner
        variant="strip"
        tone="neutral"
        icon={<OrbitSpinner size={12} className="text-ink-tertiary" />}
      >
        <span title={upgradingText}>{upgradingText}</span>
      </ContextBanner>
    );
  }

  if (selectedHost && selectedHost.connection_state === "disconnected") {
    const errSnippet = selectedHost.last_error?.trim() || null;
    const disconnectedText = `${t("banner.disconnectedFrom", { host: selectedHost.display_name })}${errSnippet ? ` · ${errSnippet}` : ""}`;
    const reconnectLabel = reconnecting ? t("banner.reconnecting") : t("banner.reconnect");
    return (
      <ContextBanner
        variant="strip"
        tone="danger"
        icon={<AlertTriangle className="size-3.5 shrink-0" />}
        actions={
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
            className="bg-signal-danger/12 text-signal-danger hover:bg-signal-danger/20 inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-[6px] px-2 text-[11px] font-medium transition-colors disabled:opacity-60"
          >
            <RefreshCw className={reconnecting ? "size-3 shrink-0 animate-spin" : "size-3 shrink-0"} />
            <span className="hidden @[460px]:inline">{reconnectLabel}</span>
          </button>
        }
      >
        <span title={disconnectedText}>{disconnectedText}</span>
      </ContextBanner>
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
      <ContextBanner
        variant="strip"
        tone="neutral"
        icon={<OrbitSpinner size={12} className="text-ink-tertiary" />}
      >
        <span title={connectingText}>{connectingText}</span>
      </ContextBanner>
    );
  }

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
      <ContextBanner
        variant="strip"
        tone="neutral"
        dismissLabel={t("banner.dismissNotice")}
        onDismiss={() => orbitActions.dismissVersionMismatch(selectedHost.host_id, normDaemon)}
        actions={
          <>
            {outdatedFleetHostsCount > 1 && (
              <button
                type="button"
                aria-label={updateAllFull}
                onClick={() => void orbitActions.upgradeAllConnectedHosts()}
                className="text-signal-info hover:bg-signal-info-bg inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-[6px] px-2 text-[11px] font-medium transition-colors"
              >
                <span className="hidden @[560px]:inline">{updateAllFull}</span>
                <span className="@[560px]:hidden">
                  {t("banner.updateAllShort", { count: outdatedFleetHostsCount })}
                </span>
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
              className="text-ink-primary hover:bg-accent inline-flex h-[22px] cursor-pointer items-center rounded-[6px] px-2 text-[11px] font-medium transition-colors"
            >
              {upgradeState?.status === "failed" ? t("banner.retryUpdate") : t("common.update")}
            </button>
          </>
        }
      >
        <span title={mismatchText}>{mismatchText}</span>
      </ContextBanner>
    );
  }

  return null;
});
