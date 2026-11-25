import { Plus } from "lucide-react";
import { AddHostSection } from "@/components/settings/add-host-section";
import { FleetUpgrades } from "@/components/settings/fleet-upgrades";
import { HostConnectionSection } from "@/components/settings/host-connection-section";
import { SettingDrawer, SettingPageHeader } from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import type { HostCardView } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions } from "@/state/orbit-store";

export function HostsFleetSection({
  hosts,
  hostId,
  view,
  onSelectHost,
  onOverview,
}: {
  hosts: HostCardView[];
  hostId: string | null;
  view: "overview" | "connection" | "add_host";
  onSelectHost: (id: string) => void;
  onOverview: () => void;
}) {
  const { t } = useI18n();
  const localOnly = hosts.length === 1 && isLocalHostCard(hosts[0]);
  const detail = view === "connection" ? hosts.find((h) => h.host_id === hostId) : null;
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
  const visibleHost = detail ?? (localOnly ? hosts[0] : null);
  return (
    <div className="flex flex-col gap-[24px]">
      <SettingPageHeader
        title={detail ? detail.display_name : t("settings.nav4Hosts")}
        breadcrumb={
          detail ? (
            <nav
              className="flex items-center gap-2 text-[11px]"
              aria-label={t("settings.connectionTitle")}
            >
              <Button variant="link" className="h-auto p-0 text-[11px]" onClick={onOverview}>
                {t("settings.nav4Hosts")}
              </Button>
              <span aria-hidden="true">/</span>
              <span aria-current="page" className="truncate">
                {detail.display_name}
              </span>
            </nav>
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
            >
              <Plus data-icon="inline-start" />
              {t(localOnly ? "settings.fleetConnectRemote" : "settings.fleetConnectHost")}
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
}
