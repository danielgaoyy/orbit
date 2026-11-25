import { memo, useEffect, useState } from "react";
import { Bell, Check, Send, Volume2 } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";
import { playSoftNotificationChime } from "@/lib/notification-sound";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const HOURS = Array.from({ length: 24 }, (_, h) => h);

function formatHour(hour: number): string {
  return `${String(hour).padStart(2, "0")}:00`;
}

const HOUR_OPTIONS: readonly SettingSelectOption<number>[] = HOURS.map((h) => ({
  value: h,
  label: formatHour(h),
  mono: true,
}));

function readNotificationPermission(): string {
  return typeof window !== "undefined" && "Notification" in window
    ? Notification.permission
    : "unsupported";
}

export const NotificationsSection = memo(function NotificationsSection() {
  const notifyOnApproval = useOrbitSelector((s) => s.notifyOnApproval);
  const notifyOnTurnComplete = useOrbitSelector((s) => s.notifyOnTurnComplete);
  const notifyOnAgentError = useOrbitSelector((s) => s.notifyOnAgentError);
  const notifyOnGoalFinished = useOrbitSelector((s) => s.notifyOnGoalFinished);
  const notifySoundEnabled = useOrbitSelector((s) => s.notifySoundEnabled);
  const notifyQuietHoursEnabled = useOrbitSelector((s) => s.notifyQuietHoursEnabled);
  const notifyQuietStartHour = useOrbitSelector((s) => s.notifyQuietStartHour);
  const notifyQuietEndHour = useOrbitSelector((s) => s.notifyQuietEndHour);

  const [permission, setPermission] = useState<string>(readNotificationPermission);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const syncPermission = () => {
      setPermission(readNotificationPermission());
    };
    window.addEventListener("focus", syncPermission);
    return () => {
      window.removeEventListener("focus", syncPermission);
    };
  }, []);

  const ensurePermissionIfDefault = async () => {
    if (
      typeof window === "undefined" ||
      !("Notification" in window) ||
      Notification.permission !== "default"
    ) {
      return;
    }
    try {
      const res = await Notification.requestPermission();
      setPermission(res);
    } catch {
      setPermission(readNotificationPermission());
    }
  };

  const { t } = useI18n();
  const isBlocked = permission === "denied" || permission === "unsupported";

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title={t("settings.notificationsTitle")}
        description={t("settings.notificationsDesc")}
      />

      {/* Desktop System Permission */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.notifyPermissionGroup")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.desktopNotificationPermission"
            title={t("settings.desktopNotificationPermission")}
            description={
              permission === "granted"
                ? t("settings.notifyPermissionGranted")
                : permission === "denied"
                  ? t("settings.notifyPermissionDenied")
                  : t("settings.notifyPermissionDefault")
            }
          >
            {permission === "granted" ? (
              <div className="flex items-center gap-2.5">
                <Button
                  type="button"
                  variant="outline"
                  size="xs"
                  onClick={() => orbitActions.sendTestDesktopNotification()}
                  className="h-[28px] gap-1.5 text-[13px]"
                >
                  <Send className="h-[12px] w-[12px]" />
                  <span>{t("settings.notifyTest")}</span>
                </Button>
                <span className="text-signal-success inline-flex items-center gap-1.5 text-[13px] font-medium">
                  <Check className="h-[14px] w-[14px]" />
                  {t("settings.notifyEnabled")}
                </span>
              </div>
            ) : permission === "default" ? (
              <Button
                type="button"
                size="sm"
                className="text-[13px]"
                onClick={() => void ensurePermissionIfDefault()}
              >
                <Bell className="h-[14px] w-[14px]" />
                <span>{t("settings.notifyEnable")}</span>
              </Button>
            ) : (
              <span className="text-ink-muted text-[13px]">
                {permission === "denied" ? t("settings.notifyBlocked") : t("settings.notifyUnavailable")}
              </span>
            )}
          </SettingRow>
        </SettingCard>
      </div>

      {/* Desktop Alert Categories & Sound */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.notifyDesktopEvents")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.notifyApproval"
            title={t("settings.notifyApproval")}
            description={t("settings.notifyApprovalDesc")}
            disabled={isBlocked}
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyApproval")}
              checked={notifyOnApproval}
              disabled={isBlocked}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnApproval: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.notifyTurnComplete"
            title={t("settings.notifyTurnComplete")}
            description={t("settings.notifyTurnCompleteDesc")}
            disabled={isBlocked}
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyTurnComplete")}
              checked={notifyOnTurnComplete}
              disabled={isBlocked}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnTurnComplete: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.notifyAgentError"
            title={t("settings.notifyAgentError")}
            description={t("settings.notifyAgentErrorDesc")}
            disabled={isBlocked}
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyAgentError")}
              checked={notifyOnAgentError}
              disabled={isBlocked}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnAgentError: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.notifyGoalFinished"
            title={t("settings.notifyGoalFinished")}
            description={t("settings.notifyGoalFinishedDesc")}
            disabled={isBlocked}
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyGoalFinished")}
              checked={notifyOnGoalFinished}
              disabled={isBlocked}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnGoalFinished: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.notificationSoundTitle"
            title={t("settings.notificationSoundTitle")}
            description={t("settings.notificationSoundDesc")}
            disabled={isBlocked}
          >
            <div className="flex items-center gap-2.5">
              <button
                type="button"
                disabled={isBlocked}
                onClick={() => void playSoftNotificationChime("default")}
                className="border-border-default text-ink-primary hover:bg-surface-recessed inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[13px] font-medium transition-colors disabled:pointer-events-none disabled:opacity-45"
              >
                <Volume2 className="text-ink-muted h-[13px] w-[13px]" />
                <span>{t("settings.testSound")}</span>
              </button>
              <ToggleSwitch
                ariaLabel={t("settings.notificationSoundTitle")}
                checked={notifySoundEnabled}
                disabled={isBlocked}
                onChange={(v) => orbitActions.setNotificationPrefs({ notifySoundEnabled: v })}
              />
            </div>
          </SettingRow>

          <SettingRow
            settingId="settings.notifyQuietHours"
            title={t("settings.notifyQuietHours")}
            description={t("settings.notifyQuietHoursDesc")}
            disabled={isBlocked}
          >
            <div className="flex items-center gap-2">
              {notifyQuietHoursEnabled && (
                <div className="flex items-center gap-1.5 text-[13px]">
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel={t("settings.notifyQuietStart")}
                    value={notifyQuietStartHour}
                    options={HOUR_OPTIONS}
                    disabled={isBlocked}
                    triggerClassName="min-w-[82px]"
                    menuClassName="min-w-[104px]"
                    onChange={(hour) =>
                      orbitActions.setNotificationPrefs({ notifyQuietStartHour: hour })
                    }
                  />
                  <span className="text-ink-muted" aria-hidden>–</span>
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel={t("settings.notifyQuietEnd")}
                    value={notifyQuietEndHour}
                    options={HOUR_OPTIONS}
                    disabled={isBlocked}
                    triggerClassName="min-w-[82px]"
                    menuClassName="min-w-[104px]"
                    onChange={(hour) =>
                      orbitActions.setNotificationPrefs({ notifyQuietEndHour: hour })
                    }
                  />
                </div>
              )}
              <ToggleSwitch
                ariaLabel={t("settings.notifyQuietHours")}
                checked={notifyQuietHoursEnabled}
                disabled={isBlocked}
                onChange={(v) => orbitActions.setNotificationPrefs({ notifyQuietHoursEnabled: v })}
              />
            </div>
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
});
