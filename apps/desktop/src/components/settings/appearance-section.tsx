import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { useI18n } from "@/lib/i18n";
import {
  type TerminalCursorStyle,
  type TerminalThemeMode,
  type ThemeMode,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { Button } from "@/components/ui/button";
import { resolveXtermTheme } from "@/lib/xterm-theme";
import { cn } from "@/lib/utils";

const MIN_TERMINAL_FONT_PX = 10;
const MAX_TERMINAL_FONT_PX = 18;

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
  const terminalFontSize = useOrbitSelector((s) => s.terminalFontSize);
  const terminalCursorStyle = useOrbitSelector((s) => s.terminalCursorStyle);
  const terminalScrollback = useOrbitSelector((s) => s.terminalScrollback);
  const terminalThemeMode = useOrbitSelector((s) => s.terminalThemeMode);

  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme);
  const palette = resolveXtermTheme(resolvedTheme, terminalThemeMode);

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

  const terminalThemeOptions: SettingSelectOption<TerminalThemeMode>[] = [
    {
      value: "harmonized",
      label: t("settings.terminalThemeHarmonized"),
      description: t("settings.terminalThemeHarmonizedDesc"),
    },
    {
      value: "high_contrast",
      label: t("settings.terminalThemeHighContrast"),
      description: t("settings.terminalThemeHighContrastDesc"),
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

      {/* Integrated Terminal */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupTerminal")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.terminalTheme"
            title={t("settings.terminalTheme")}
            description={t("settings.terminalThemeDesc")}
          >
            <SettingSelect
              ariaLabel={t("settings.terminalTheme")}
              value={terminalThemeMode}
              options={terminalThemeOptions}
              onChange={(themeMode) => orbitActions.setTerminalPrefs({ themeMode })}
              menuClassName="w-[256px]"
            />
          </SettingRow>

          <SettingRow
            settingId="settings.fontSize"
            title={t("settings.fontSize")}
            description={t("settings.fontSizeDesc")}
          >
            <div className="flex items-center gap-1.5">
              <Button
                variant="outline" size="icon"
                aria-label={t("settings.fontDecrease")}
                disabled={terminalFontSize <= MIN_TERMINAL_FONT_PX}
                onClick={() => orbitActions.setTerminalPrefs({ fontSize: terminalFontSize - 1 })}
                className="size-[28px] font-mono"
              >
                -
              </Button>
              <span className="tabular-nums w-14 text-center font-mono text-[13px] font-medium">
                {terminalFontSize}px
              </span>
              <Button
                variant="outline" size="icon"
                aria-label={t("settings.fontIncrease")}
                disabled={terminalFontSize >= MAX_TERMINAL_FONT_PX}
                onClick={() => orbitActions.setTerminalPrefs({ fontSize: terminalFontSize + 1 })}
                className="size-[28px] font-mono"
              >
                +
              </Button>
            </div>
          </SettingRow>

          <SettingRow
            settingId="settings.cursorStyle"
            title={t("settings.cursorStyle")}
            description={t("settings.cursorStyleDesc")}
          >
            <SettingSelect
              ariaLabel={t("settings.cursorStyle")}
              value={terminalCursorStyle}
              options={cursorStyleOptions}
              onChange={(cursorStyle) => orbitActions.setTerminalPrefs({ cursorStyle })}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.scrollback"
            title={t("settings.scrollback")}
            description={t("settings.scrollbackDesc")}
          >
            <SettingSelect
              ariaLabel={t("settings.scrollback")}
              value={terminalScrollback}
              options={scrollbackOptions}
              onChange={(scrollback) => orbitActions.setTerminalPrefs({ scrollback })}
            />
          </SettingRow>
          <figure data-setting-id="settings.terminalPreview" className="px-4 py-3">
            <figcaption className="mb-2 text-[11px] font-medium text-ink-muted">{t("settings.terminalPreview")}</figcaption>
            <div className="border-border-subtle overflow-hidden rounded-lg border px-4 py-3 font-mono leading-[1.55]"
              style={{ backgroundColor: palette.background, color: palette.foreground, fontSize: terminalFontSize }}>
              <div><span style={{ color: palette.cyan }}>~/orbit</span> $ git status --short</div>
              <div><span style={{ color: palette.green }}>A</span>  src/main.ts</div>
              <div><span style={{ color: palette.yellow }}>M</span>  README.md</div>
              <div><span style={{ color: palette.cyan }}>~/orbit</span> $ <span aria-hidden
                className={cn("inline-block align-text-bottom", {
                  "h-[1em] w-[2px]": terminalCursorStyle === "bar",
                  "h-[1em] w-[0.6em]": terminalCursorStyle === "block",
                  "h-[2px] w-[0.6em]": terminalCursorStyle === "underline",
                })}
                style={{ backgroundColor: palette.cursor }}
              /></div>
            </div>
          </figure>
        </SettingCard>
      </div>
    </div>
  );
}
