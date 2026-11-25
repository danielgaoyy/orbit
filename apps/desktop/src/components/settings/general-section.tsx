import { memo, useEffect, useMemo, useRef } from "react";
import { ArrowRight, Compass } from "lucide-react";
import {
  SettingCard, SettingGroupLabel, SettingRow, SettingSectionHeader, SettingSelect,
  ToggleSwitch, type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { LOCALES, useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { isWebRuntime, normalizeSemver, type ExternalEditorKind } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const languageOptions = LOCALES.map((item) => ({
  value: item.id,
  label: item.nativeName,
  description: item.nativeName !== item.englishName ? item.englishName : undefined,
}));

export const GeneralSection = memo(function GeneralSection() {
  const { locale, t } = useI18n();
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const worktreeBranchPrefix = useOrbitSelector((s) => s.worktreeBranchPrefix);
  const coalesceExploration = useOrbitSelector((s) => s.coalesceExploration);
  const externalEditors = useOrbitSelector((s) => s.externalEditors);
  const preferredEditor = useOrbitSelector((s) => s.preferredEditor);
  const storeAppVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const checkingUpdate = useOrbitSelector((s) => s.checkingReleaseUpdate);
  const releaseCheckError = useOrbitSelector((s) => s.releaseCheckError);
  const isBrowserOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const mod = modSymbol();

  const prefixOptions = useMemo<SettingSelectOption<string>[]>(
    () => [
      { value: "auto", label: t("composer.modeAuto"), description: t("settings.branchPrefixAutoDesc") },
      { value: "orbit/", label: "orbit/", mono: true },
      { value: "feat/", label: "feat/", mono: true },
      { value: "fix/", label: "fix/", mono: true },
      { value: "chore/", label: "chore/", mono: true },
    ],
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [locale],
  );

  const appVersion = normalizeSemver(storeAppVersion) ?? "0.11.2";
  const autoCheckedRef = useRef(false);

  // Gently check for the latest release once when opening General settings
  useEffect(() => {
    if (isWebRuntime() || autoCheckedRef.current || releaseUpdateInfo || checkingUpdate || isBrowserOffline) return;
    autoCheckedRef.current = true;
    void orbitActions.checkFleetUpdates({ silent: true });
  }, [releaseUpdateInfo, checkingUpdate, isBrowserOffline]);

  // Synced preferences may contain a custom prefix from another client.
  const branchOptions = useMemo(
    () =>
      prefixOptions.some((item) => item.value === worktreeBranchPrefix)
        ? prefixOptions
        : [...prefixOptions, { value: worktreeBranchPrefix, label: worktreeBranchPrefix, mono: true }],
    [prefixOptions, worktreeBranchPrefix],
  );

  const installedEditors = useMemo(
    () => externalEditors.filter((ed) => ed.installed),
    [externalEditors],
  );

  const effectivePreferred: ExternalEditorKind | null =
    (preferredEditor && installedEditors.some((ed) => ed.kind === preferredEditor)
      ? preferredEditor
      : installedEditors[0]?.kind) ?? null;

  const editorOptions = useMemo<SettingSelectOption<ExternalEditorKind>[]>(
    () =>
      installedEditors.map((ed) => ({
        value: ed.kind,
        label: ed.label,
      })),
    [installedEditors],
  );

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title={t("settings.generalTitle")}
        description={t("settings.generalDesc")}
      />

      {/* Interface Language */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupInterface")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.language"
            title={t("settings.language")}
            description={t("settings.languageDesc")}
          >
            <SettingSelect
              ariaLabel={t("settings.language")}
              value={locale}
              options={languageOptions}
              onChange={(nextLocale) => orbitActions.setLocale(nextLocale)}
              menuClassName="w-[216px]"
            />
          </SettingRow>
        </SettingCard>
      </div>

      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupComposer")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.sendWithEnter"
            title={t("settings.sendWithEnter")}
            description={
              enterToSend
                ? t("settings.sendWithEnterOnDesc")
                : t("settings.sendWithEnterOffDesc", { mod })
            }
          >
            <ToggleSwitch
              ariaLabel={t("settings.sendWithEnter")}
              checked={enterToSend}
              onChange={(v) => orbitActions.setEnterToSend(v)}
            />
          </SettingRow>

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

          {!isWebRuntime() && (
            <SettingRow
              settingId="settings.externalEditor"
              title={t("settings.externalEditor")}
              description={t("settings.externalEditorDesc", { mod })}
            >
              <SettingSelect
                ariaLabel={t("settings.externalEditor")}
                value={effectivePreferred}
                options={editorOptions}
                onChange={(kind) => orbitActions.setPreferredEditor(kind)}
                placeholder={t("settings.noEditorsFound")}
                disabled={editorOptions.length === 0}
              />
            </SettingRow>
          )}
        </SettingCard>
      </div>

      {/* Git & Worktrees */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupGitWorktrees")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.isolateWorktrees"
            title={t("settings.isolateWorktrees")}
            description={t("settings.isolateWorktreesDesc")}
          >
            <ToggleSwitch
              ariaLabel={t("settings.isolateWorktrees")}
              checked={draftUseWorktree}
              onChange={(v) => orbitActions.setDraftConfig({ useWorktree: v })}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.branchPrefix"
            title={t("settings.branchPrefix")}
            description={t("settings.branchPrefixDesc")}
            disabled={!draftUseWorktree}
          >
            <SettingSelect
              ariaLabel={t("settings.branchPrefix")}
              value={worktreeBranchPrefix}
              options={branchOptions}
              disabled={!draftUseWorktree}
              onChange={(pfx) => orbitActions.setWorktreePrefs({ worktreeBranchPrefix: pfx })}
              menuClassName="w-[200px]"
            />
          </SettingRow>
        </SettingCard>
      </div>

      {!isWebRuntime() && (
        <div className="space-y-2">
          <SettingGroupLabel>Orbit Desktop</SettingGroupLabel>
          <SettingCard>
            <SettingRow
              settingId="settings.appVersion"
              title={t("settings.appVersion")}
              description={
                checkingUpdate ? t("settings.checking") : releaseCheckError ||
                (releaseUpdateInfo?.has_newer_version
                  ? t("settings.latestBadge", { version: releaseUpdateInfo.latest_version })
                  : releaseUpdateInfo ? t("common.upToDate") : undefined)
              }
            >
              <div className="flex items-center gap-3">
                <span className="font-mono text-[13px] text-ink-secondary">
                  v{appVersion}
                </span>
                <Button variant="outline" size="sm" className="text-[13px]" onClick={() => orbitActions.openSettings("hosts")}>
                  {t("settings.manageFleet")} <ArrowRight aria-hidden className="size-[13px]" />
                </Button>
              </div>
            </SettingRow>
          </SettingCard>
        </div>
      )}

      {/* Getting Started / Onboarding Guide */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupGettingStarted")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.setupGuide"
            title={t("settings.setupGuide")}
            description={t("settings.setupGuideDesc")}
          >
            <Button
              variant="outline" size="sm"
              onClick={() => orbitActions.openOnboardingGuide()}
              className="text-[13px]"
            >
              <Compass className="text-ink-muted size-[13px]" />
              <span>{t("settings.openSetupGuide")}</span>
            </Button>
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
});
