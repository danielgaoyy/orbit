import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { Minus, Plus } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  ToggleSwitch,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Segmented, type SegmentedOption } from "@/components/ui/segmented";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { isMacDesktop, isTauriRuntime, orbitIpc } from "@/lib/orbit-ipc";
import {
  CODE_SIZES,
  DEFAULT_CODE_SIZE,
  DEFAULT_READ_SIZE,
  FONT_PRESETS_BY_SLOT,
  READ_SIZES,
  ZOOM_DEFAULT_PERCENT,
  ZOOM_MAX_PERCENT,
  ZOOM_MIN_PERCENT,
  type FontSlot,
  isFontInstalled,
  resolveFontPreviewStack,
  sanitizeFontName,
} from "@/lib/typography";
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

const ZoomStepper = memo(function ZoomStepper() {
  const { t } = useI18n();
  const zoomPercent = useOrbitSelector((s) => s.uiZoomPercent);
  const atMin = zoomPercent <= ZOOM_MIN_PERCENT;
  const atMax = zoomPercent >= ZOOM_MAX_PERCENT;
  const isDefault = zoomPercent === ZOOM_DEFAULT_PERCENT;

  const handleStep = useCallback((delta: -1 | 1) => {
    void orbitIpc
      .stepUiZoom(delta, "settings")
      .then((next) => {
        orbitActions.applyUiZoomState(next);
      })
      .catch(() => undefined);
  }, []);

  const handleActualSize = useCallback(() => {
    void orbitIpc
      .setUiZoom(ZOOM_DEFAULT_PERCENT, "settings")
      .then((next) => {
        orbitActions.applyUiZoomState(next);
      })
      .catch(() => undefined);
  }, []);

  return (
    <div className="flex items-center gap-2">
      <div
        data-slot="zoom-stepper"
        className="border-border-default bg-surface-base inline-flex h-[30px] items-center rounded-lg border p-0.5"
      >
        <button
          type="button"
          disabled={atMin}
          aria-label={t("settings.zoomOut")}
          onClick={() => handleStep(-1)}
          className="text-ink-secondary hover:bg-accent hover:text-ink-primary focus-visible:ring-ring/50 inline-flex size-[24px] cursor-pointer items-center justify-center rounded-[6px] transition-colors focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-35"
        >
          <Minus className="size-[13px] stroke-[2]" />
        </button>
        <span
          data-slot="zoom-stepper-value"
          className="text-ink-primary min-w-[46px] px-1 text-center text-[13px] font-medium tabular-nums select-none"
        >
          {zoomPercent}%
        </span>
        <button
          type="button"
          disabled={atMax}
          aria-label={t("settings.zoomIn")}
          onClick={() => handleStep(1)}
          className="text-ink-secondary hover:bg-accent hover:text-ink-primary focus-visible:ring-ring/50 inline-flex size-[24px] cursor-pointer items-center justify-center rounded-[6px] transition-colors focus-visible:ring-1 disabled:pointer-events-none disabled:opacity-35"
        >
          <Plus className="size-[13px] stroke-[2]" />
        </button>
      </div>
      {!isDefault && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          onClick={handleActualSize}
          className="text-ink-secondary hover:text-ink-primary h-[30px] px-2.5 text-[13px]"
        >
          {t("settings.actualSize")}
        </Button>
      )}
    </div>
  );
});

const TypographyPreviewStrip = memo(function TypographyPreviewStrip() {
  const { t } = useI18n();
  return (
    <div
      data-slot="typography-preview"
      className="border-border-subtle bg-surface-recessed flex flex-col gap-2.5 rounded-[12px] border px-4 py-3.5"
    >
      <div className="text-ink-muted text-[11px] font-semibold tracking-[0.05em] uppercase">
        {t("settings.typographyPreviewLabel")}
      </div>
      <div
        data-typography-preview="ui"
        className="text-ink-secondary font-(family-name:--orbit-font-ui) text-[13px] leading-[1.45]"
      >
        {t("settings.typographyPreviewUi")}
      </div>
      <div
        data-typography-preview="conversation"
        className="text-ink-primary font-(family-name:--orbit-font-content) text-(length:--orbit-read-body) leading-(--orbit-read-leading)"
      >
        {t("settings.typographyPreviewConversation")}
      </div>
      <div
        data-typography-preview="code"
        className="border-border-subtle bg-surface-base text-ink-primary overflow-x-auto rounded-[8px] border px-3 py-2 font-(family-name:--orbit-font-code) text-(length:--orbit-code-size) leading-(--orbit-code-line)"
      >
        {t("settings.typographyPreviewCode")}
      </div>
    </div>
  );
});

const FontChoiceRow = memo(function FontChoiceRow({
  slot,
  settingId,
  title,
  description,
  fontId,
  customValue,
}: {
  slot: FontSlot;
  settingId: string;
  title: string;
  description: string;
  fontId: string;
  customValue: string | null;
}) {
  const { t } = useI18n();
  const ime = useImeCompositionGuard();
  const [draft, setDraft] = useState(customValue ?? "");

  useEffect(() => {
    setDraft(customValue ?? "");
  }, [customValue]);

  const options = useMemo<SettingSelectOption<string>[]>(() => {
    const presets = FONT_PRESETS_BY_SLOT[slot];
    const notInstalledBadge = t("settings.fontNotInstalled");
    const items: SettingSelectOption<string>[] = presets.map((preset) => {
      const label =
        preset.id === "default"
          ? t("settings.fontDefault")
          : preset.id === "match-ui"
            ? t("settings.fontMatchInterface")
            : preset.id === "system-serif"
              ? t("settings.fontSystemSerif")
              : (preset.family ?? preset.id);
      const installed = preset.family ? isFontInstalled(preset.family) : true;
      const previewFamily = resolveFontPreviewStack(slot, preset.id);
      return {
        value: preset.id,
        label,
        badge: installed ? undefined : notInstalledBadge,
        style: {
          fontFamily: previewFamily,
          ...(installed ? {} : { opacity: 0.58 }),
        },
      };
    });

    items.push({
      value: "custom",
      label: t("settings.fontCustom"),
    });
    return items;
  }, [slot, t]);

  const commitCustom = useCallback(
    (raw: string) => {
      const cleaned = sanitizeFontName(raw);
      setDraft(cleaned ?? "");
      orbitActions.setFontChoice(slot, "custom", cleaned);
    },
    [slot],
  );

  const sanitizedCustom = fontId === "custom" ? sanitizeFontName(customValue) : null;
  const customNotInstalled = Boolean(sanitizedCustom && !isFontInstalled(sanitizedCustom));

  return (
    <div data-setting-id={settingId}>
      <SettingRow title={title} description={description}>
        <SettingSelect
          ariaLabel={title}
          value={fontId}
          options={options}
          onChange={(nextId) => {
            orbitActions.setFontChoice(slot, nextId);
          }}
          triggerClassName="min-w-[176px]"
          menuClassName="w-[236px]"
        />
      </SettingRow>
      {fontId === "custom" && (
        <div className="flex flex-col gap-1.5 px-4 pb-3.5">
          <Input
            aria-label={`${title} (${t("settings.fontCustom")})`}
            value={draft}
            placeholder={t("settings.fontCustomPlaceholder")}
            onChange={(e) => setDraft(e.target.value)}
            onBlur={() => commitCustom(draft)}
            {...ime.compositionProps}
            onKeyDown={(e) => {
              if (ime.shouldIgnoreKeyDown(e)) return;
              if (e.key === "Enter") {
                e.preventDefault();
                commitCustom(draft);
              }
            }}
            className="h-[30px] max-w-[320px] text-[13px] shadow-none"
          />
          {customNotInstalled && (
            <p className="text-signal-warning text-[11px] leading-[1.4]">
              {t("settings.fontNotInstalledHint")}
            </p>
          )}
        </div>
      )}
    </div>
  );
});

export const AppearanceSection = memo(function AppearanceSection() {
  const { t } = useI18n();
  const theme = useOrbitSelector((s) => s.theme);
  const diffLayout = useOrbitSelector((s) => s.diffLayout);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const conversationFontSize = useOrbitSelector((s) => s.conversationFontSize);
  const codeFontSize = useOrbitSelector((s) => s.codeFontSize);
  const uiFontId = useOrbitSelector((s) => s.uiFontId);
  const contentFontId = useOrbitSelector((s) => s.contentFontId);
  const codeFontId = useOrbitSelector((s) => s.codeFontId);
  const uiFontCustom = useOrbitSelector((s) => s.uiFontCustom);
  const contentFontCustom = useOrbitSelector((s) => s.contentFontCustom);
  const codeFontCustom = useOrbitSelector((s) => s.codeFontCustom);

  const isDesktop = isTauriRuntime();
  const isMac = isMacDesktop();

  const themeOptions = useMemo<readonly SegmentedOption<ThemeMode>[]>(
    () => [
      { value: "system", label: t("settings.themeSystem"), icon: <ThemeMiniSwatch mode="system" /> },
      { value: "light", label: t("settings.themeLight"), icon: <ThemeMiniSwatch mode="light" /> },
      { value: "dark", label: t("settings.themeDark"), icon: <ThemeMiniSwatch mode="dark" /> },
    ],
    [t],
  );

  const conversationSizeOptions = useMemo<SettingSelectOption<number>[]>(
    () =>
      READ_SIZES.map((size) => ({
        value: size,
        label: `${size} px`,
        badge: size === DEFAULT_READ_SIZE ? t("common.default") : undefined,
      })),
    [t],
  );

  const codeSizeOptions = useMemo<SettingSelectOption<number>[]>(
    () =>
      CODE_SIZES.map((size) => ({
        value: size,
        label: `${size} px`,
        badge: size === DEFAULT_CODE_SIZE ? t("common.default") : undefined,
      })),
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

  const zoomShortcutsHint = isMac ? "⌘+ / ⌘− / ⌘0" : "Ctrl+ / Ctrl− / Ctrl+0";

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

      {/* Text & Zoom */}
      <div className="flex flex-col gap-3">
        <SettingGroupLabel>{t("settings.groupTextZoom")}</SettingGroupLabel>
        <SettingCard>
          {isDesktop && (
            <SettingRow
              settingId="settings.windowZoom"
              title={t("settings.windowZoom")}
              description={t("settings.windowZoomDesc", { shortcuts: zoomShortcutsHint })}
            >
              <ZoomStepper />
            </SettingRow>
          )}
          <SettingRow
            settingId="settings.conversationTextSize"
            title={t("settings.conversationTextSize")}
            description={t("settings.conversationTextSizeDesc")}
          >
            <SettingSelect<number>
              ariaLabel={t("settings.conversationTextSize")}
              value={conversationFontSize}
              options={conversationSizeOptions}
              onChange={(size) => orbitActions.setConversationFontSize(size)}
              menuClassName="w-[176px]"
            />
          </SettingRow>
          <SettingRow
            settingId="settings.codeTextSize"
            title={t("settings.codeTextSize")}
            description={t("settings.codeTextSizeDesc")}
          >
            <SettingSelect<number>
              ariaLabel={t("settings.codeTextSize")}
              value={codeFontSize}
              options={codeSizeOptions}
              onChange={(size) => orbitActions.setCodeFontSize(size)}
              menuClassName="w-[176px]"
            />
          </SettingRow>
        </SettingCard>

        <TypographyPreviewStrip />
      </div>

      {/* Fonts */}
      <div className="flex flex-col gap-3">
        <SettingGroupLabel>{t("settings.groupFonts")}</SettingGroupLabel>
        <SettingCard>
          <FontChoiceRow
            slot="ui"
            settingId="settings.uiFont"
            title={t("settings.uiFont")}
            description={t("settings.uiFontDesc")}
            fontId={uiFontId}
            customValue={uiFontCustom}
          />
          <FontChoiceRow
            slot="content"
            settingId="settings.contentFont"
            title={t("settings.contentFont")}
            description={t("settings.contentFontDesc")}
            fontId={contentFontId}
            customValue={contentFontCustom}
          />
          <FontChoiceRow
            slot="code"
            settingId="settings.codeFont"
            title={t("settings.codeFont")}
            description={t("settings.codeFontDesc")}
            fontId={codeFontId}
            customValue={codeFontCustom}
          />
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
