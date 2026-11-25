import { useEffect, useRef } from "react";
import { ArrowUpCircle, Laptop, RefreshCw } from "lucide-react";
import {
  HostStatusIndicator,
  SettingCard,
  SettingResourceRow,
  SettingRow,
  settingsHostTransport,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { Progress } from "@/components/ui/progress";
import { useI18n } from "@/lib/i18n";
import { type HostCardView, compareSemver, normalizeSemver } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, resolveFleetTargetVersion, useOrbitSelector } from "@/state/orbit-store";

function hostCanUpgrade(host: HostCardView) {
  return (
    isLocalHostCard(host) || Boolean(host.ssh_alias) || host.connection_state.startsWith("online_")
  );
}

export function FleetUpgrades({
  hosts,
  onSelectHost,
  showHostRows,
}: {
  hosts: HostCardView[];
  onSelectHost: (id: string) => void;
  showHostRows: boolean;
}) {
  const { t } = useI18n();
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const release = useOrbitSelector((s) => s.releaseUpdateInfo);
  const checking = useOrbitSelector((s) => s.checkingReleaseUpdate);
  const checkError = useOrbitSelector((s) => s.releaseCheckError);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);
  const hostUpdates = useOrbitSelector((s) => s.hostUpgradeStates);
  const offline = useOrbitSelector((s) => s.isBrowserOffline);
  const checked = useRef(false);

  useEffect(() => {
    if (checked.current || release || checking || offline) return;
    checked.current = true;
    void orbitActions.checkFleetUpdates({ silent: true });
  }, [release, checking, offline]);

  const target = resolveFleetTargetVersion(appVersion, release);
  const needsUpgrade = (host: HostCardView) => {
    const daemon = normalizeSemver(host.daemon_version);
    return !daemon || Boolean(target && compareSemver(daemon, target) < 0);
  };
  const pending =
    Number(Boolean(release?.has_newer_version)) +
    hosts.filter(
      (host) =>
        hostCanUpgrade(host) &&
        needsUpgrade(host) &&
        !(isLocalHostCard(host) && release?.has_newer_version),
    ).length;
  const busy =
    checking || ["checking", "upgrading_hosts", "upgrading_desktop"].includes(fleetUpdate.phase);
  const resync = Boolean(release) && !pending;
  const status =
    fleetUpdate.desktopError ??
    checkError ??
    fleetUpdate.desktopStatus ??
    fleetUpdate.summaryMessage;
  const unreachable = hosts.filter((host) => !hostCanUpgrade(host)).length;
  const latestDescription =
    target && release
      ? pending > 0
        ? t("settings.updatesAvailableDesc", { count: pending, version: target })
        : t(unreachable ? "settings.allUpToDateWithOfflineDesc" : "settings.allUpToDateDesc", {
            version: target,
            reachable: hosts.length - unreachable,
            offline: unreachable,
          })
      : t("settings.updatesDefaultDesc", { count: hosts.length });

  return (
    <SettingCard>
      <SettingRow
        settingId="settings.checkForUpdates"
        title={t("settings.groupUpdatesFleet")}
        description={status || latestDescription}
      >
        <div className="flex flex-wrap items-center gap-2">
          {fleetUpdate.phase === "paused_with_failures" ? (
            <>
              <Button
                variant="outline"
                size="sm"
                disabled={offline}
                onClick={() => void orbitActions.retryFailedFleetHosts()}
              >
                {t("settings.retryFailedHosts")}
              </Button>
              {fleetUpdate.pendingDesktopUpdate && release && (
                <Button
                  size="sm"
                  disabled={offline}
                  onClick={() => void orbitActions.installDesktopAppUpdate(release)}
                >
                  {t("settings.continueDesktopUpdate")}
                </Button>
              )}
            </>
          ) : (
            <>
              <Button
                variant="outline"
                size="sm"
                disabled={busy || offline}
                onClick={() => void orbitActions.checkFleetUpdates()}
              >
                <RefreshCw data-icon="inline-start" />
                {t(checking ? "settings.checking" : "settings.checkForUpdates")}
              </Button>
              <Button
                variant={resync ? "outline" : "default"}
                size="sm"
                disabled={busy || offline}
                title={resync ? t("settings.resyncAllTitle") : undefined}
                onClick={() => void orbitActions.upgradeAllConnectedHosts({ force: resync })}
              >
                <ArrowUpCircle data-icon="inline-start" />
                {busy
                  ? t("common.updating")
                  : resync
                    ? t("settings.resyncAll")
                    : pending
                      ? t("settings.updateAllCount", { count: pending })
                      : t("settings.updateAll")}
              </Button>
            </>
          )}
        </div>
      </SettingRow>
      <SettingResourceRow
        icon={<Laptop />}
        title="Orbit Desktop"
        description={t("settings.localClient")}
        status={
          appVersion ? `v${normalizeSemver(appVersion) ?? appVersion}` : t("common.unknownVersion")
        }
        action={
          fleetUpdate.phase === "upgrading_desktop" && fleetUpdate.desktopProgress !== null ? (
            <div className="flex items-center gap-2 font-mono text-[11px]">
              <Progress
                className="w-[64px]"
                value={fleetUpdate.desktopProgress * 100}
                aria-label={t("settings.installing")}
              />
              {Math.round(fleetUpdate.desktopProgress * 100)}%
            </div>
          ) : release?.has_newer_version && target ? (
            `→ v${target}`
          ) : undefined
        }
      />
      {showHostRows &&
        hosts.map((host) => {
          const update = hostUpdates[host.host_id];
          const version = normalizeSemver(host.daemon_version);
          const reachable = hostCanUpgrade(host);
          const transport = settingsHostTransport(host);
          const upgradeHost = () =>
            void orbitActions.upgradeHostDaemon(host.host_id, {
              force: true,
              targetVersion: target,
            });
          return (
            <SettingResourceRow
              key={host.host_id}
              icon={<HostStatusIndicator state={host.connection_state} />}
              title={host.display_name}
              description={
                update?.status === "failed" && update.error
                  ? `${transport} · ${update.error}`
                  : transport
              }
              onClick={() => onSelectHost(host.host_id)}
              status={version ? `v${version}` : t("common.unknownVersion")}
              action={
                update?.status === "upgrading" ? (
                  t("common.updating")
                ) : !reachable ? (
                  t("settings.offlineSkipped")
                ) : (
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={busy || offline}
                    aria-label={t("settings.forceRestartHost", { host: host.display_name })}
                    onClick={upgradeHost}
                  >
                    {update?.status === "failed" ? (
                      t("common.retry")
                    ) : needsUpgrade(host) && target ? (
                      `→ v${target}`
                    ) : (
                      <RefreshCw data-icon="inline-start" />
                    )}
                  </Button>
                )
              }
            />
          );
        })}
    </SettingCard>
  );
}
