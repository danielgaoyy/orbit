import { memo, useMemo } from "react";
import { Plus } from "lucide-react";
import { AddHostSection } from "@/components/settings/add-host-section";
import { ArchivedSection } from "@/components/settings/archived-section";
import { FleetUpgrades } from "@/components/settings/fleet-upgrades";
import { HostConnectionSection } from "@/components/settings/host-connection-section";
import { HostDiagnosticsSection } from "@/components/settings/host-security-ops-panels";
import {
  SettingDrawer,
  SettingPageHeader,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export const HostsFleetSection = memo(function HostsFleetSection({
  hostId,
  view,
  onSelectHost,
  onOverview,
}: {
  hostId: string | null;
  view: "overview" | "connection" | "add_host";
  onSelectHost: (id: string) => void;
  onOverview: () => void;
}) {
  const { t } = useI18n();
  const rawHosts = useOrbitSelector((s) => s.fleet.hosts);
  const hosts = useMemo(
    () => [...rawHosts].sort((a, b) => Number(isLocalHostCard(b)) - Number(isLocalHostCard(a))),
    [rawHosts],
  );
  const localOnly = hosts.length === 1 && isLocalHostCard(hosts[0]);
  const detail = view === "connection" ? hosts.find((h) => h.host_id === hostId) : null;
  const visibleHost = detail ?? (localOnly ? hosts[0] : null);
  const opsHostId = visibleHost?.host_id ?? hostId;
  const opsHostOnline = useHostIsOnline(opsHostId);
  if (view === "connection" && !detail) {
    return (
      <SettingPageHeader
        title={t("settings.connectionTitle")}
        description={t("settings.fleetMissingHost")}
        action={
          <Button variant="outline" onClick={onOverview}>
            {t("settings.nav4Hosts")}
          </Button>
        }
      />
    );
  }
  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
        title={detail ? detail.display_name : t("settings.nav4Hosts")}
        breadcrumb={
          detail ? (
            <button
              type="button"
              onClick={onOverview}
              className="text-ink-muted hover:text-ink-primary cursor-pointer text-[13px] font-medium transition-colors"
            >
              {t("settings.nav4Hosts")}
            </button>
          ) : undefined
        }
        description={t(detail ? "settings.connectionDesc" : "settings.fleetDescription")}
        action={
          !detail && (
            <Button
              data-setting-id="settings.fleetConnectHost"
              variant="outline"
              size="sm"
              onClick={() => orbitActions.setSettingsSection("add_host")}
              className="text-[13px]"
            >
              <Plus className="size-[13px]" />
              <span>{t(localOnly ? "settings.fleetConnectRemote" : "settings.fleetConnectHost")}</span>
            </Button>
          )
        }
      />
      {!detail && (
        <FleetUpgrades hosts={hosts} onSelectHost={onSelectHost} showHostRows={!localOnly} />
      )}
      {visibleHost && (
        <HostConnectionSection
          key={visibleHost.host_id}
          hostId={visibleHost.host_id}
          onRemoved={onOverview}
          inlinePairing={localOnly}
        />
      )}
      {opsHostId && (
        <HostDiagnosticsSection
          key={`diag:${opsHostId}`}
          hostId={opsHostId}
          isOnline={opsHostOnline}
        />
      )}
      <ArchivedSection key={`archive:${opsHostId}`} hostId={opsHostId} />
      <SettingDrawer
        open={view === "add_host"}
        onOpenChange={(open) => {
          if (!open) onOverview();
        }}
        title={t("settings.fleetConnectHost")}
        description={t("settings.fleetConnectDesc")}
      >
        <AddHostSection onPaired={onSelectHost} />
      </SettingDrawer>
    </div>
  );
});
