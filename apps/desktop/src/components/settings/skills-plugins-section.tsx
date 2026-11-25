import { HostPluginsSection } from "@/components/settings/host-plugins-section";
import { HostSkillsSection } from "@/components/settings/host-skills-section";
import { SettingPageHeader } from "@/components/settings/settings-primitives";
import { useI18n } from "@/lib/i18n";

export function SkillsPluginsSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  return (
    <div key={hostId} className="flex flex-col gap-6">
      <SettingPageHeader
        title={t("settings.nav4Skills")}
        description={t("managedSkills.pageDesc")}
      />
      <HostSkillsSection hostId={hostId} />
      <HostPluginsSection hostId={hostId} />
    </div>
  );
}
