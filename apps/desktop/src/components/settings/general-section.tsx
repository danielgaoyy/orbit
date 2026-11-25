import { memo, useEffect, useMemo, useRef } from "react";
import { ArrowRight, Compass } from "lucide-react";
import { HostMigrationSection } from "@/components/settings/host-security-ops-panels";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { LOCALES, useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { isWebRuntime, normalizeSemver } from "@/lib/orbit-ipc";
import {
  type ActiveTurnSendBehavior,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";

const languageOptions = LOCALES.map((item) => ({
  value: item.id,
  label: item.nativeName,
  description: item.nativeName !== item.englishName ? item.englishName : undefined,
}));

export const GeneralSection = memo(function GeneralSection({
  hostId,
}: {
  hostId: string | null;
}) {
  const { locale, t } = useI18n();
  const enterToSend = useOrbitSelector((s) => s.enterToSend);
  const activeTurnSendBehavior = useOrbitSelector((s) => s.activeTurnSendBehavior);
  const storeAppVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const checkingUpdate = useOrbitSelector((s) => s.checkingReleaseUpdate);
  const releaseCheckError = useOrbitSelector((s) => s.releaseCheckError);
  const isBrowserOffline = useOrbitSelector((s) => s.isBrowserOffline);
  const online = useHostIsOnline(hostId);
  const projectCwd = useOrbitSelector((s) =>
    s.selectedHostId === hostId ? s.selectedWorkspaceCwd : null,
  );
  const mod = modSymbol();
  const modEnterLabel = mod === "⌘" ? "⌘ + Enter" : "Ctrl + Enter";

  const sendShortcutOptions = useMemo<SettingSelectOption<"enter" | "mod_enter">[]>(
    () => [
      {
        value: "enter",
        label: "Enter",
        description: t("settings.sendShortcutEnterDesc"),
      },
      {
        value: "mod_enter",
        label: modEnterLabel,
        description: t("settings.sendShortcutModEnterDesc"),
      },
    ],
    [modEnterLabel, t],
  );

  const activeTurnSendOptions = useMemo<SettingSelectOption<ActiveTurnSendBehavior>[]>(
    () => [
      {
        value: "queue",
        label: t("settings.activeTurnSendQueue"),
        description: t("settings.activeTurnSendQueueDesc"),
      },
      {
        value: "steer",
        label: t("settings.activeTurnSendSteer"),
        description: t("settings.activeTurnSendSteerDesc"),
      },
    ],
    [t],
  );

  const appVersion = normalizeSemver(storeAppVersion) ?? "0.11.5";
  const autoCheckedRef = useRef(false);

  // Gently check for the latest release once when opening General settings
  useEffect(() => {
    if (isWebRuntime() || autoCheckedRef.current || releaseUpdateInfo || checkingUpdate || isBrowserOffline) return;
    autoCheckedRef.current = true;
    void orbitActions.checkFleetUpdates({ silent: true });
  }, [releaseUpdateInfo, checkingUpdate, isBrowserOffline]);

  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("settings.generalTitle")}
        description={t("settings.generalDesc")}
      />

      {/* General */}
      <div className="flex flex-col gap-3">
        <SettingGroupLabel>{t("settings.groupGeneral")}</SettingGroupLabel>
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

          <SettingRow
            settingId="settings.setupGuide"
            title={t("settings.setupGuide")}
            description={t("settings.setupGuideDesc")}
          >
            <Button
              variant="outline"
              size="sm"
              onClick={() => orbitActions.openOnboardingGuide()}
              className="text-[13px]"
            >
              <Compass className="text-ink-muted size-[13px]" />
              <span>{t("settings.openSetupGuide")}</span>
            </Button>
          </SettingRow>
        </SettingCard>
      </div>

      {/* Send */}
      <div className="flex flex-col gap-3">
        <SettingGroupLabel>{t("settings.groupSend")}</SettingGroupLabel>
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
            <SettingSelect<"enter" | "mod_enter">
              ariaLabel={t("settings.sendWithEnter")}
              value={enterToSend ? "enter" : "mod_enter"}
              options={sendShortcutOptions}
              onChange={(val) => orbitActions.setEnterToSend(val === "enter")}
              menuClassName="w-[216px]"
            />
          </SettingRow>

          <SettingRow
            settingId="settings.activeTurnSendBehavior"
            title={t("settings.activeTurnSendBehavior")}
            description={t("settings.activeTurnSendBehaviorDesc")}
          >
            <SettingSelect<ActiveTurnSendBehavior>
              ariaLabel={t("settings.activeTurnSendBehavior")}
              value={activeTurnSendBehavior}
              options={activeTurnSendOptions}
              onChange={(val) => orbitActions.setActiveTurnSendBehavior(val)}
              menuClassName="w-[248px]"
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* App Version */}
      {!isWebRuntime() && (
        <div className="flex flex-col gap-3">
          <SettingGroupLabel>{t("settings.groupAppVersion")}</SettingGroupLabel>
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

      {/* Import from IDE / CLI */}
      {hostId && (
        <HostMigrationSection
          key={JSON.stringify([hostId, projectCwd])}
          hostId={hostId}
          isOnline={online}
        />
      )}
    </div>
  );
});
