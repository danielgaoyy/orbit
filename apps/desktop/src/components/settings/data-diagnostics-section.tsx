import { memo } from "react";
import { ArchivedSection } from "@/components/settings/archived-section";
import { HostDiagnosticsAndMigrationPanel } from "@/components/settings/host-security-ops-panels";
import {
  SettingPageHeader,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { useI18n } from "@/lib/i18n";
import { useOrbitSelector } from "@/state/orbit-store";

export const DataDiagnosticsSection = memo(function DataDiagnosticsSection({
  hostId,
}: {
  hostId: string | null;
}) {
  const { t } = useI18n();
  const online = useHostIsOnline(hostId);
  const projectCwd = useOrbitSelector((s) =>
    s.selectedHostId === hostId ? s.selectedWorkspaceCwd : null,
  );
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
    </div>
  );
});
