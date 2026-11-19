import { useState } from "react";
import { Bell, Check } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export function NotificationsSection() {
  const notifyOnApproval = useOrbitSelector((s) => s.notifyOnApproval);
  const notifyOnTurnComplete = useOrbitSelector((s) => s.notifyOnTurnComplete);

  const [permission, setPermission] = useState<string>(() =>
    typeof window !== "undefined" && "Notification" in window
      ? Notification.permission
      : "unsupported",
  );

  const ensurePermissionIfDefault = async () => {
    if (
      typeof window === "undefined" ||
      !("Notification" in window) ||
      Notification.permission !== "default"
    ) {
      return;
    }
    const res = await Notification.requestPermission();
    setPermission(res);
  };

  const isBlocked = permission === "denied" || permission === "unsupported";

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Notifications"
        description="Desktop alerts while Orbit is in the background."
      />

      <div className="space-y-2">
        <SettingGroupLabel>System Permission</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Desktop notifications"
            description={
              permission === "granted"
                ? "Notifications are allowed for Orbit."
                : permission === "denied"
                  ? "Enable notifications for Orbit in System Settings."
                  : "Allow Orbit to deliver system notifications."
            }
          >
            {permission === "granted" ? (
              <span className="inline-flex items-center gap-1.5 text-[12.5px] font-medium text-emerald-600 dark:text-emerald-400">
                <Check className="h-[14px] w-[14px]" />
                Enabled
              </span>
            ) : permission === "default" ? (
              <button
                type="button"
                onClick={() => void ensurePermissionIfDefault()}
                className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-opacity hover:opacity-90"
              >
                <Bell className="h-[13.5px] w-[13.5px]" />
                <span>Enable</span>
              </button>
            ) : (
              <span className="text-muted-foreground text-[12.5px]">
                {permission === "denied" ? "Blocked" : "Unavailable"}
              </span>
            )}
          </SettingRow>
        </SettingCard>
      </div>

      <div className={cn("space-y-2", isBlocked && "opacity-55")}>
        <SettingGroupLabel>Alerts</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Approvals & questions"
            description="Notify when an agent waits for your input."
          >
            <ToggleSwitch
              checked={notifyOnApproval}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnApproval: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow
            title="Task completion"
            description="Notify when a background turn finishes."
          >
            <ToggleSwitch
              checked={notifyOnTurnComplete}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnTurnComplete: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>
        </SettingCard>
      </div>
    </div>
  );
}
