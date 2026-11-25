import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { useI18n } from "@/lib/i18n";
import { type ThemeMode, orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function ThemePreviewSwatch({ mode }: { mode: ThemeMode }) {
  const halves = mode === "system" ? ["light", "dark"] : [mode];
  return (
    <span aria-hidden className="border-border-subtle flex h-[44px] w-full overflow-hidden rounded-md border">
      {halves.map((tone) => (
        <span key={tone} data-orbit-theme-preview={tone} className="bg-surface-base flex min-w-0 flex-1">
          <span className="bg-surface-recessed border-border-subtle w-1/3 shrink-0 border-r p-1.5">
            <span className="bg-ink-muted/45 block h-[3px] w-2/3 rounded-full" />
            <span className="bg-ink-muted/25 mt-1 block h-[3px] w-full rounded-full" />
          </span>
          <span className="flex-1 p-2">
            <span className="bg-ink-primary/50 block h-[4px] w-1/2 rounded-full" />
            <span className="bg-ink-muted/30 mt-1.5 block h-[3px] w-3/4 rounded-full" />
          </span>
        </span>
      ))}
    </span>
  );
}

export function AppearanceSection() {
  const { t } = useI18n();
  const theme = useOrbitSelector((s) => s.theme);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);

  const themeOptions: { id: ThemeMode; label: string }[] = [
    { id: "system", label: t("settings.themeSystem") },
    { id: "light", label: t("settings.themeLight") },
    { id: "dark", label: t("settings.themeDark") },
  ];

  const diffLayoutOptions: SettingSelectOption<"unified" | "split">[] = [
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
  ];

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title={t("settings.appearanceTitle")}
        description={t("settings.appearanceDesc")}
      />

      {/* Theme */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupTheme")}</SettingGroupLabel>
        <SettingCard settingId="settings.groupTheme">
          <div className="grid grid-cols-3 gap-3 p-4">
            {themeOptions.map((opt) => {
              const active = theme === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  aria-pressed={active}
                  onClick={() => orbitActions.setTheme(opt.id)}
                  className={cn(
                    "flex cursor-pointer flex-col items-center gap-2 rounded-[10px] border p-2.5 transition-colors",
                    active
                      ? "border-border-strong bg-surface-recessed text-ink-primary"
                      : "border-border-subtle hover:border-border-default text-ink-muted hover:text-ink-primary",
                  )}
                >
                  <ThemePreviewSwatch mode={opt.id} />
                  <span className={cn("text-[13px]", active ? "font-medium" : "font-normal")}>
                    {opt.label}
                  </span>
                </button>
              );
            })}
          </div>
        </SettingCard>
      </div>

      {/* Code Review */}
      <div className="space-y-2">
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
    </div>
  );
}
