import { memo, useMemo } from "react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  ToggleSwitch,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { Segmented, type SegmentedOption } from "@/components/ui/segmented";
import { useI18n } from "@/lib/i18n";
import { type ThemeMode, orbitActions, useOrbitSelector } from "@/state/orbit-store";

function ThemeMiniSwatch({ mode }: { mode: ThemeMode }) {
  const halves = mode === "system" ? (["light", "dark"] as const) : ([mode] as const);
  return (
    <span
      aria-hidden
      className="border-border-default flex size-[14px] shrink-0 overflow-hidden rounded-full border"
    >
      {halves.map((tone) => (
        <span
          key={tone}
          data-orbit-theme-preview={tone}
          className="bg-surface-base block h-full flex-1"
        />
      ))}
    </span>
  );
}

export const AppearanceSection = memo(function AppearanceSection() {
  const { t } = useI18n();
  const theme = useOrbitSelector((s) => s.theme);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);

  const themeOptions = useMemo<readonly SegmentedOption<ThemeMode>[]>(
    () => [
      { value: "system", label: t("settings.themeSystem"), icon: <ThemeMiniSwatch mode="system" /> },
      { value: "light", label: t("settings.themeLight"), icon: <ThemeMiniSwatch mode="light" /> },
      { value: "dark", label: t("settings.themeDark"), icon: <ThemeMiniSwatch mode="dark" /> },
    ],
    [t],
  );

  const diffLayoutOptions = useMemo<SettingSelectOption<"unified" | "split">[]>(
    () => [
      {
        value: "unified",
        label: t("settings.diffUnified"),
        description: t("settings.diffUnifiedDesc"),
      },
      {
        value: "split",
        label: t("settings.diffSplit"),
        description: t("settings.diffSplitDesc"),
      },
    ],
    [t],
  );

  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("settings.appearanceTitle")}
        description={t("settings.appearanceDesc")}
      />

      {/* Theme */}
      <div className="flex flex-col gap-3">
        <SettingGroupLabel>{t("settings.groupTheme")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.groupTheme"
            title={t("settings.groupTheme")}
          >
            <Segmented<ThemeMode>
              ariaLabel={t("settings.groupTheme")}
              value={theme}
              options={themeOptions}
              onValueChange={(next) => orbitActions.setTheme(next)}
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Code Review */}
      <div className="flex flex-col gap-3">
        <SettingGroupLabel>{t("settings.groupCodeReview")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.diffLayout"
            title={t("settings.diffLayout")}
            description={t("settings.diffLayoutDesc")}
          >
            <SettingSelect
              ariaLabel={t("settings.diffLayout")}
              value={diffLayout}
              options={diffLayoutOptions}
              onChange={(layout) => orbitActions.setDiffLayout(layout)}
              menuClassName="w-[216px]"
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Display */}
      <div className="flex flex-col gap-3">
        <SettingGroupLabel>{t("settings.groupDisplay")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.collapseExploration"
            title={t("settings.collapseExploration")}
            description={t("settings.collapseExplorationDesc")}
          >
            <ToggleSwitch
              ariaLabel={t("settings.collapseExploration")}
              checked={coalesceExploration}
              onChange={(v) => orbitActions.setWorktreePrefs({ coalesceExploration: v })}
            />
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
});
