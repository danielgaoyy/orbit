import { useState } from "react";
import { ArchivedSection } from "@/components/settings/archived-section";
import { HostDiagnosticsAndMigrationPanel } from "@/components/settings/host-security-ops-panels";
import { useModifiedSettings } from "@/components/settings/settings-preferences";
import {
  SettingCard,
  SettingPageHeader,
  SettingRow,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useI18n } from "@/lib/i18n";
import { useOrbitSelector } from "@/state/orbit-store";

export function DataDiagnosticsSection({ hostId }: { hostId: string | null }) {
  const { t } = useI18n();
  const state = useOrbitSelector(
    (s) => s.fleet.hosts.find((host) => host.host_id === hostId)?.connection_state,
  );
  const projectCwd = useOrbitSelector((s) =>
    s.selectedHostId === hostId ? s.selectedWorkspaceCwd : null,
  );
  const online = state === "online_direct" || state === "online_relay";
  const { resetAll, modifiedIds } = useModifiedSettings();
  const [confirmReset, setConfirmReset] = useState(false);
  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader title={t("settings.nav4Data")} description={t("dataOps.pageDesc")} />
      <ArchivedSection key={"archive:" + hostId} hostId={hostId} />
      {hostId && (
        <HostDiagnosticsAndMigrationPanel
          key={JSON.stringify([hostId, projectCwd])}
          hostId={hostId}
          isOnline={online}
        />
      )}
      <SettingCard>
        <SettingRow
          settingId="dataOps.resetPreferences"
          title={t("dataOps.resetPreferences")}
          description={t("dataOps.resetDesc")}
        >
          <Button
            variant="outline"
            size="sm"
            disabled={modifiedIds.size === 0}
            onClick={() => setConfirmReset(true)}
          >
            {t("common.reset")}
          </Button>
        </SettingRow>
      </SettingCard>
      <ConfirmDialog
        open={confirmReset}
        onOpenChange={setConfirmReset}
        tone="danger"
        title={t("dataOps.resetPreferences")}
        description={t("dataOps.resetConfirm")}
        confirmLabel={t("common.reset")}
        onConfirm={resetAll}
      />
    </div>
  );
}
