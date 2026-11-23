import { useMemo } from "react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { LOCALES, type OrbitLocale, useI18n } from "@/lib/i18n";
import {
  type TerminalCursorStyle,
  type ThemeMode,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const MIN_TERMINAL_FONT_PX = 10;
const MAX_TERMINAL_FONT_PX = 18;

function ThemePreviewSwatch({ mode }: { mode: ThemeMode }) {
  if (mode === "light") {
    return (
      <div className="flex h-[42px] w-full overflow-hidden rounded-[8px] border border-black/[0.08] bg-white">
        <div className="w-[30%] border-r border-black/[0.06] bg-[#F6F7F9] p-1.5">
          <div className="h-1 w-4 rounded-full bg-black/15" />
          <div className="mt-1.5 h-1 w-6 rounded-full bg-black/10" />
        </div>
        <div className="flex-1 p-2">
          <div className="h-1.5 w-10 rounded-full bg-black/18" />
          <div className="mt-1.5 h-1 w-14 rounded-full bg-black/10" />
        </div>
      </div>
    );
  }
  if (mode === "dark") {
    return (
      <div className="flex h-[42px] w-full overflow-hidden rounded-[8px] border border-white/[0.10] bg-[#1A1C20]">
        <div className="w-[30%] border-r border-white/[0.08] bg-[#14161A] p-1.5">
          <div className="h-1 w-4 rounded-full bg-white/25" />
          <div className="mt-1.5 h-1 w-6 rounded-full bg-white/14" />
        </div>
        <div className="flex-1 p-2">
          <div className="h-1.5 w-10 rounded-full bg-white/28" />
          <div className="mt-1.5 h-1 w-14 rounded-full bg-white/14" />
        </div>
      </div>
    );
  }
  return (
    <div className="flex h-[42px] w-full overflow-hidden rounded-[8px] border border-black/[0.10] dark:border-white/[0.10]">
      <div className="flex w-1/2 bg-white">
        <div className="w-[45%] border-r border-black/[0.06] bg-[#F6F7F9] p-1.5">
          <div className="h-1 w-3 rounded-full bg-black/15" />
        </div>
        <div className="flex-1 p-1.5">
          <div className="h-1.5 w-5 rounded-full bg-black/18" />
        </div>
      </div>
      <div className="flex w-1/2 bg-[#1A1C20]">
        <div className="flex-1 p-1.5">
          <div className="h-1.5 w-6 rounded-full bg-white/28" />
          <div className="mt-1.5 h-1 w-8 rounded-full bg-white/14" />
        </div>
      </div>
    </div>
  );
}

export function AppearanceSection() {
  const { locale, t } = useI18n();
  const theme = useOrbitSelector((s) => s.theme);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);
  const terminalFontSize = useOrbitSelector((s) => s.terminalFontSize);
  const terminalCursorStyle = useOrbitSelector((s) => s.terminalCursorStyle);
  const terminalScrollback = useOrbitSelector((s) => s.terminalScrollback);

  const languageOptions = useMemo<SettingSelectOption<OrbitLocale>[]>(
    () =>
      LOCALES.map((item) => ({
        value: item.id,
        label: item.nativeName,
        description: item.nativeName !== item.englishName ? item.englishName : undefined,
      })),
    [],
  );

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

  const cursorStyleOptions: SettingSelectOption<TerminalCursorStyle>[] = [
    { value: "bar", label: t("settings.cursorBar") },
    { value: "block", label: t("settings.cursorBlock") },
    { value: "underline", label: t("settings.cursorUnderline") },
  ];

  const scrollbackOptions: SettingSelectOption<number>[] = [
    { value: 2000, label: t("settings.scrollbackLines", { count: "2,000" }), mono: true },
    { value: 5000, label: t("settings.scrollbackLines", { count: "5,000" }), mono: true },
    { value: 10000, label: t("settings.scrollbackLines", { count: "10,000" }), mono: true },
    { value: 25000, label: t("settings.scrollbackLines", { count: "25,000" }), mono: true },
  ];

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title={t("settings.appearanceTitle")}
        description={t("settings.appearanceDesc")}
      />

      {/* Interface Language */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupInterface")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={t("settings.language")}
            description={t("settings.languageDesc")}
          >
            <SettingSelect
              value={locale}
              options={languageOptions}
              onChange={(nextLocale) => orbitActions.setLocale(nextLocale)}
              menuClassName="w-[216px]"
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Theme */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupTheme")}</SettingGroupLabel>
        <SettingCard>
          <div className="grid grid-cols-3 gap-3 p-4">
            {themeOptions.map((opt) => {
              const active = theme === opt.id;
              return (
                <button
                  key={opt.id}
                  type="button"
                  onClick={() => orbitActions.setTheme(opt.id)}
                  className={cn(
                    "flex cursor-pointer flex-col items-center gap-2 rounded-[11px] border p-2.5 transition-colors",
                    active
                      ? "border-foreground/35 bg-accent/60 text-foreground"
                      : "border-border hover:border-foreground/18 text-muted-foreground hover:text-foreground",
                  )}
                >
                  <ThemePreviewSwatch mode={opt.id} />
                  <span className={cn("text-[12.5px]", active ? "font-medium" : "font-normal")}>
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
            title={t("settings.diffLayout")}
            description={t("settings.diffLayoutDesc")}
          >
            <SettingSelect
              value={diffLayout}
              options={diffLayoutOptions}
              onChange={(layout) => orbitActions.setDiffLayout(layout)}
              menuClassName="w-[216px]"
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Integrated Terminal */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupTerminal")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={t("settings.fontSize")}
            description={t("settings.fontSizeDesc")}
          >
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                disabled={terminalFontSize <= MIN_TERMINAL_FONT_PX}
                onClick={() => orbitActions.setTerminalPrefs({ fontSize: terminalFontSize - 1 })}
                className="border-border hover:bg-accent inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-lg border font-mono text-[13px] transition-colors disabled:pointer-events-none disabled:opacity-35"
              >
                -
              </button>
              <span className="tabular-nums w-14 text-center font-mono text-[12.5px] font-medium">
                {terminalFontSize}px
              </span>
              <button
                type="button"
                disabled={terminalFontSize >= MAX_TERMINAL_FONT_PX}
                onClick={() => orbitActions.setTerminalPrefs({ fontSize: terminalFontSize + 1 })}
                className="border-border hover:bg-accent inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-lg border font-mono text-[13px] transition-colors disabled:pointer-events-none disabled:opacity-35"
              >
                +
              </button>
            </div>
          </SettingRow>

          <SettingRow
            title={t("settings.cursorStyle")}
            description={t("settings.cursorStyleDesc")}
          >
            <SettingSelect
              value={terminalCursorStyle}
              options={cursorStyleOptions}
              onChange={(cursorStyle) => orbitActions.setTerminalPrefs({ cursorStyle })}
            />
          </SettingRow>

          <SettingRow
            title={t("settings.scrollback")}
            description={t("settings.scrollbackDesc")}
          >
            <SettingSelect
              value={terminalScrollback}
              options={scrollbackOptions}
              onChange={(scrollback) => orbitActions.setTerminalPrefs({ scrollback })}
            />
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
}
