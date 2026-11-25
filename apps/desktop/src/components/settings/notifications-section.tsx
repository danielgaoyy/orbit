import { useEffect, useMemo, useRef, useState } from "react";
import { Bell, Check, RefreshCw, Send, Shield, Smartphone, Trash2, Volume2 } from "lucide-react";
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
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Field, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { t, useI18n } from "@/lib/i18n";
import { playSoftNotificationChime } from "@/lib/notification-sound";
import type { PushPlatform, PushPreferences } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const HOURS = Array.from({ length: 24 }, (_, h) => h);

function formatHour(hour: number): string {
  return `${String(hour).padStart(2, "0")}:00`;
}

const HOUR_OPTIONS: readonly SettingSelectOption<number>[] = HOURS.map((h) => ({
  value: h,
  label: formatHour(h),
  mono: true,
}));

function formatPlatformLabel(platform: PushPlatform): string {
  switch (platform) {
    case "apns":
      return "APNs";
    case "fcm":
      return "FCM";
    case "harmony_push_kit":
      return "HarmonyOS";
  }
}

function formatTimestamp(ms: number | null | undefined): string {
  if (!ms) return t("settings.pushNever");
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return t("settings.pushUnknown");
  }
}

function readNotificationPermission(): string {
  return typeof window !== "undefined" && "Notification" in window
    ? Notification.permission
    : "unsupported";
}

const DEFAULT_PUSH_PREFS: PushPreferences = {
  enabled: true,
  notify_permission_required: true,
  notify_turn_completed: true,
  notify_agent_error: true,
  notify_goal_finished: true,
  notify_timer_fired: true,
  include_message_preview: false,
  quiet_hours_enabled: false,
  quiet_start_hour: 23,
  quiet_end_hour: 7,
  push_gateway_url: null,
  has_gateway_auth_token: false,
};

export function NotificationsSection() {
  const notifyOnApproval = useOrbitSelector((s) => s.notifyOnApproval);
  const notifyOnTurnComplete = useOrbitSelector((s) => s.notifyOnTurnComplete);
  const notifyOnAgentError = useOrbitSelector((s) => s.notifyOnAgentError);
  const notifyOnGoalFinished = useOrbitSelector((s) => s.notifyOnGoalFinished);
  const notifySoundEnabled = useOrbitSelector((s) => s.notifySoundEnabled);
  const notifyQuietHoursEnabled = useOrbitSelector((s) => s.notifyQuietHoursEnabled);
  const notifyQuietStartHour = useOrbitSelector((s) => s.notifyQuietStartHour);
  const notifyQuietEndHour = useOrbitSelector((s) => s.notifyQuietEndHour);

  const hosts = useOrbitSelector((s) => s.fleet.hosts);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const pushByHost = useOrbitSelector((s) => s.pushByHost);

  const [targetHostId, setTargetHostId] = useState<string>(
    () => selectedHostId ?? hosts[0]?.host_id ?? "",
  );

  useEffect(() => {
    if (!targetHostId && hosts.length > 0) {
      setTargetHostId(selectedHostId ?? hosts[0].host_id);
    } else if (targetHostId && hosts.length > 0 && !hosts.some((h) => h.host_id === targetHostId)) {
      setTargetHostId(selectedHostId ?? hosts[0].host_id);
    }
  }, [hosts, selectedHostId, targetHostId]);

  const activeHost = useMemo(
    () => hosts.find((h) => h.host_id === targetHostId) ?? hosts[0] ?? null,
    [hosts, targetHostId],
  );
  const activeHostId = activeHost?.host_id ?? "";
  const pushEntry = activeHostId ? pushByHost[activeHostId] : undefined;

  useEffect(() => {
    if (activeHostId) {
      void orbitActions.loadPushDevices(activeHostId);
    }
  }, [activeHostId]);

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

  // Draft state for host mobile push gateway & preferences.
  // Tracks `isDirty` so background refreshes don't clobber unsaved edits, while host switches
  // ALWAYS reset cleanly to the newly selected host's preferences (or defaults if not loaded yet).
  const [draftPrefs, setDraftPrefs] = useState<PushPreferences>(DEFAULT_PUSH_PREFS);
  const [gatewayTokenInput, setGatewayTokenInput] = useState("");
  const [clearStoredToken, setClearStoredToken] = useState(false);
  const [isDirty, setIsDirty] = useState(false);
  const [pendingRemoveDevice, setPendingRemoveDevice] = useState<{
    hostId: string;
    hostName: string;
    device_id: string;
    device_name: string;
    platform: PushPlatform;
  } | null>(null);
  const lastSyncedHostIdRef = useRef<string>(activeHostId);

  useEffect(() => {
    if (!pendingRemoveDevice) return;
    if (!hosts.some((h) => h.host_id === pendingRemoveDevice.hostId)) {
      setPendingRemoveDevice(null);
    }
  }, [hosts, pendingRemoveDevice]);

  const hostSelectOptions = useMemo<SettingSelectOption<string>[]>(
    () => hosts.map((h) => ({ value: h.host_id, label: h.display_name })),
    [hosts],
  );

  useEffect(() => {
    const hostChanged = lastSyncedHostIdRef.current !== activeHostId;
    if (hostChanged) {
      lastSyncedHostIdRef.current = activeHostId;
      setDraftPrefs(pushEntry?.preferences ?? DEFAULT_PUSH_PREFS);
      setGatewayTokenInput("");
      setClearStoredToken(false);
      setIsDirty(false);
      return;
    }
    if (!isDirty && pushEntry?.preferences) {
      setDraftPrefs(pushEntry.preferences);
      setGatewayTokenInput("");
      setClearStoredToken(false);
    }
  }, [activeHostId, pushEntry?.preferences, isDirty]);

  const updateDraftPrefs = (updater: (prev: PushPreferences) => PushPreferences) => {
    setIsDirty(true);
    setDraftPrefs(updater);
  };

  const handleSaveMobilePrefs = async () => {
    if (!activeHostId || pushEntry?.status !== "ready") return;
    const tokenTrimmed = gatewayTokenInput.trim();
    const res = await orbitActions.updatePushPreferences(activeHostId, {
      preferences: {
        ...draftPrefs,
        push_gateway_url: draftPrefs.push_gateway_url?.trim() || null,
      },
      gateway_auth_token: tokenTrimmed ? tokenTrimmed : null,
      keep_existing_gateway_token: !clearStoredToken && !tokenTrimmed,
    });
    if (res && lastSyncedHostIdRef.current === activeHostId) {
      setDraftPrefs(res.preferences);
      setGatewayTokenInput("");
      setClearStoredToken(false);
      setIsDirty(false);
    }
  };

  const { t } = useI18n();
  const isBlocked = permission === "denied" || permission === "unsupported";
  const devices = pushEntry?.devices ?? [];
  const isHostOnline =
    activeHost?.connection_state === "online_direct" ||
    activeHost?.connection_state === "online_relay";
  const isPushReady = isHostOnline && pushEntry?.status === "ready";

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
      <div className={cn("space-y-2", isBlocked && "opacity-55")}>
        <SettingGroupLabel>{t("settings.notifyDesktopEvents")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            settingId="settings.notifyApproval"
            title={t("settings.notifyApproval")}
            description={t("settings.notifyApprovalDesc")}
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyApproval")}
              checked={notifyOnApproval}
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
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyTurnComplete")}
              checked={notifyOnTurnComplete}
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
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyAgentError")}
              checked={notifyOnAgentError}
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
          >
            <ToggleSwitch
              ariaLabel={t("settings.notifyGoalFinished")}
              checked={notifyOnGoalFinished}
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
          >
            <div className="flex items-center gap-2.5">
              <button
                type="button"
                onClick={() => void playSoftNotificationChime("default")}
                className="border-border-default text-ink-primary hover:bg-surface-recessed inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[13px] font-medium transition-colors"
              >
                <Volume2 className="text-ink-muted h-[13px] w-[13px]" />
                <span>{t("settings.testSound")}</span>
              </button>
              <ToggleSwitch
                ariaLabel={t("settings.notificationSoundTitle")}
                checked={notifySoundEnabled}
                onChange={(v) => orbitActions.setNotificationPrefs({ notifySoundEnabled: v })}
              />
            </div>
          </SettingRow>

          <SettingRow
            settingId="settings.notifyQuietHours"
            title={t("settings.notifyQuietHours")}
            description={t("settings.notifyQuietHoursDesc")}
          >
            <div className="flex items-center gap-2">
              {notifyQuietHoursEnabled && (
                <div className="flex items-center gap-1.5 text-[13px]">
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel={t("settings.notifyQuietStart")}
                    value={notifyQuietStartHour}
                    options={HOUR_OPTIONS}
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
                onChange={(v) => orbitActions.setNotificationPrefs({ notifyQuietHoursEnabled: v })}
              />
            </div>
          </SettingRow>
        </SettingCard>
      </div>

      {/* Mobile Background Push & Gateway */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <SettingGroupLabel>{t("settings.pushGroup")}</SettingGroupLabel>
          {hosts.length > 1 && (
            <SettingSelect<string>
              size="compact"
              ariaLabel={t("settings.pushTarget")}
              value={activeHostId}
              options={hostSelectOptions}
              triggerClassName="h-[26px] min-w-[132px] max-w-[200px]"
              onChange={(hostId) => setTargetHostId(hostId)}
            />
          )}
        </div>

        <SettingCard>
          <SettingRow
            settingId="settings.pushEnabled"
            title={t("settings.pushEnabled")}
            description={t("settings.pushEnabledDesc")}
          >
            <ToggleSwitch
              ariaLabel={t("settings.pushEnabled")}
              checked={draftPrefs.enabled}
              disabled={!isPushReady}
              onChange={(v) => updateDraftPrefs((p) => ({ ...p, enabled: v }))}
            />
          </SettingRow>

          <SettingRow
            settingId="settings.pushMessagePreview"
            title={t("settings.pushMessagePreview")}
            description={t("settings.pushMessagePreviewDesc")}
          >
            <div className="flex items-center gap-2">
              {!draftPrefs.include_message_preview && (
                <span className="text-ink-muted inline-flex items-center gap-1 text-[11px]">
                  <Shield className="h-[12px] w-[12px]" />
                  {t("settings.pushMetadataOnly")}
                </span>
              )}
              <ToggleSwitch
                ariaLabel={t("settings.pushMessagePreview")}
                checked={draftPrefs.include_message_preview}
                disabled={!isPushReady}
                onChange={(v) => updateDraftPrefs((p) => ({ ...p, include_message_preview: v }))}
              />
            </div>
          </SettingRow>

          <SettingRow
            settingId="settings.pushEvents"
            title={t("settings.pushEvents")}
            description={t("settings.pushEventsDesc")}
          >
            <div className="flex flex-wrap justify-end gap-1.5">
              {(
                [
                  ["notify_permission_required", t("settings.pushApprovals")],
                  ["notify_turn_completed", t("settings.pushTurns")],
                  ["notify_agent_error", t("settings.pushErrors")],
                  ["notify_goal_finished", t("settings.pushGoals")],
                  ["notify_timer_fired", t("settings.pushTimers")],
                ] as const
              ).map(([field, label]) => {
                const active = draftPrefs[field];
                return (
                  <button
                    key={field}
                    aria-pressed={active}
                    type="button"
                    disabled={!isPushReady}
                    onClick={() => updateDraftPrefs((p) => ({ ...p, [field]: !p[field] }))}
                    className={cn(
                      "h-[26px] cursor-pointer rounded-md border px-2 text-[11px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                      active
                        ? "border-border-strong bg-surface-recessed text-ink-primary"
                        : "border-border-default text-ink-muted hover:text-ink-primary",
                    )}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </SettingRow>

          <SettingRow
            settingId="settings.pushQuietHours"
            title={t("settings.pushQuietHours")}
            description={t("settings.pushQuietHoursDesc")}
          >
            <div className="flex items-center gap-2">
              {draftPrefs.quiet_hours_enabled && (
                <div className="flex items-center gap-1.5 text-[13px]">
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel={t("settings.notifyQuietStart")}
                    value={draftPrefs.quiet_start_hour}
                    options={HOUR_OPTIONS}
                    disabled={!isPushReady}
                    triggerClassName="min-w-[82px]"
                    menuClassName="min-w-[104px]"
                    onChange={(hour) =>
                      updateDraftPrefs((p) => ({ ...p, quiet_start_hour: hour }))
                    }
                  />
                  <span className="text-ink-muted" aria-hidden>–</span>
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel={t("settings.notifyQuietEnd")}
                    value={draftPrefs.quiet_end_hour}
                    options={HOUR_OPTIONS}
                    disabled={!isPushReady}
                    triggerClassName="min-w-[82px]"
                    menuClassName="min-w-[104px]"
                    onChange={(hour) =>
                      updateDraftPrefs((p) => ({ ...p, quiet_end_hour: hour }))
                    }
                  />
                </div>
              )}
              <ToggleSwitch
                ariaLabel={t("settings.pushQuietHours")}
                checked={draftPrefs.quiet_hours_enabled}
                disabled={!isPushReady}
                onChange={(v) => updateDraftPrefs((p) => ({ ...p, quiet_hours_enabled: v }))}
              />
            </div>
          </SettingRow>

          <FieldGroup className="gap-3 px-4 py-3.5">
            <Field data-setting-id="settings.pushGatewayUrl">
              <FieldLabel htmlFor="push-gateway-url" className="text-ink-primary text-[13px]">
                {t("settings.pushGatewayUrl")}
              </FieldLabel>
              <Input
                id="push-gateway-url"
                type="url"
                value={draftPrefs.push_gateway_url ?? ""}
                disabled={!isPushReady}
                onChange={(e) =>
                  updateDraftPrefs((p) => ({ ...p, push_gateway_url: e.target.value }))
                }
                placeholder="https://push.example.com/v1/dispatch"
              />
            </Field>

            <Field data-setting-id="settings.pushGatewayToken">
              <div className="flex items-center justify-between">
                <FieldLabel htmlFor="push-gateway-token" className="text-ink-primary text-[13px]">
                  {t("settings.pushGatewayToken")}
                </FieldLabel>
                {draftPrefs.has_gateway_auth_token && !clearStoredToken && isPushReady && (
                  <button
                    type="button"
                    onClick={() => {
                      setIsDirty(true);
                      setClearStoredToken(true);
                    }}
                    className="text-ink-muted hover:text-ink-primary cursor-pointer text-[11px]"
                  >
                    {t("settings.pushClearToken")}
                  </button>
                )}
              </div>
              <Input
                id="push-gateway-token"
                type="password"
                value={gatewayTokenInput}
                disabled={!isPushReady}
                onChange={(e) => {
                  setIsDirty(true);
                  setGatewayTokenInput(e.target.value);
                  if (e.target.value) setClearStoredToken(false);
                }}
                placeholder={
                  draftPrefs.has_gateway_auth_token && !clearStoredToken
                    ? t("settings.pushKeepToken")
                    : t("settings.pushOptionalToken")
                }
              />
            </Field>

            <div className="flex items-center justify-between pt-1">
              <span className="text-ink-muted text-[13px]">
                {pushEntry?.error
                  ? pushEntry.error
                  : !isHostOnline
                    ? t("settings.pushHostOffline")
                    : pushEntry?.status !== "ready"
                      ? t("settings.pushHostLoading")
                      : t("settings.pushHttps")}
              </span>
              <div className="flex items-center gap-2">
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  className="text-[13px]"
                  disabled={!isPushReady || pushEntry?.testing || devices.length === 0}
                  onClick={() => void orbitActions.testPushNotification(activeHostId, null)}
                >
                  <Send className="h-[13px] w-[13px]" />
                  <span>{pushEntry?.testing ? t("settings.pushSending") : t("settings.pushTestNotification")}</span>
                </Button>
                <Button
                  type="button"
                  size="sm"
                  className="text-[13px]"
                  disabled={!isDirty || !isPushReady || pushEntry?.saving}
                  onClick={() => void handleSaveMobilePrefs()}
                >
                  <span>{pushEntry?.saving ? t("settings.pushSaving") : t("common.save")}</span>
                </Button>
              </div>
            </div>
          </FieldGroup>
        </SettingCard>
      </div>

      {/* Registered Mobile Devices */}
      <div data-setting-id="settings.pushRegisteredDevices" className="space-y-2">
        <div className="flex items-center justify-between">
          <SettingGroupLabel>{t("settings.pushRegisteredDevices")}</SettingGroupLabel>
          {activeHostId && (
            <button
              type="button"
              onClick={() => void orbitActions.loadPushDevices(activeHostId)}
              className="text-ink-muted hover:text-ink-primary inline-flex cursor-pointer items-center gap-1 text-[11px]"
            >
              <RefreshCw
                className={cn(
                  "h-[12px] w-[12px]",
                  pushEntry?.status === "loading" && "animate-spin",
                )}
              />
              <span>{t("common.refresh")}</span>
            </button>
          )}
        </div>

        <SettingCard>
          {devices.length === 0 ? (
            <div className="text-ink-muted flex items-center gap-2.5 px-4 py-4 text-[13px]">
              <Smartphone className="h-[15px] w-[15px] shrink-0 opacity-70" />
              <span>
                {t("settings.pushNoDevices", { host: activeHost?.display_name ?? t("settings.pushHostFallback") })}
              </span>
            </div>
          ) : (
            devices.map((dev) => (
              <SettingRow
                key={dev.device_id}
                title={
                  <span className="inline-flex items-center gap-2">
                    <span>{dev.device_name}</span>
                    <span className="border-border-default text-ink-muted rounded border px-1.5 py-0.5 text-[11px] font-medium">
                      {formatPlatformLabel(dev.platform)}
                    </span>
                    {dev.sandbox && (
                      <span className="border-signal-warning/30 bg-signal-warning-bg text-signal-warning rounded border px-1.5 py-0.5 text-[11px] font-medium">
                        {t("settings.pushSandbox")}
                      </span>
                    )}
                    {!dev.enabled && (
                      <span className="border-signal-danger/30 bg-signal-danger-bg text-signal-danger rounded border px-1.5 py-0.5 text-[11px] font-medium">
                        {t("settings.pushDisabled")}
                      </span>
                    )}
                  </span>
                }
                description={
                  dev.last_error
                    ? t("settings.pushDeliveryError", { token: dev.masked_token, error: dev.last_error })
                    : t("settings.pushLastDelivery", { token: dev.masked_token, time: formatTimestamp(dev.last_delivered_at_ms) })
                }
              >
                <div className="flex items-center gap-1.5">
                  <button
                    type="button"
                    disabled={!isPushReady || pushEntry?.testing}
                    onClick={() =>
                      void orbitActions.testPushNotification(activeHostId, dev.device_id)
                    }
                    className="border-border-default text-ink-primary hover:bg-surface-recessed inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11px] font-medium disabled:opacity-50"
                  >
                    <Send className="h-[11px] w-[11px]" />
                    <span>{t("settings.notifyTest")}</span>
                  </button>
                  <button
                    type="button"
                    aria-label={t("settings.pushRemove", { device: dev.device_name })}
                    disabled={!isPushReady}
                    onClick={() => {
                      if (!activeHostId) return;
                      setPendingRemoveDevice({
                        hostId: activeHostId,
                        hostName: activeHost?.display_name ?? t("settings.pushHostFallback"),
                        device_id: dev.device_id,
                        device_name: dev.device_name,
                        platform: dev.platform,
                      });
                    }}
                    className="text-ink-muted hover:text-signal-danger inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:opacity-50"
                  >
                    <Trash2 className="h-[14px] w-[14px]" />
                  </button>
                </div>
              </SettingRow>
            ))
          )}
        </SettingCard>
      </div>

      {pendingRemoveDevice && (
        <ConfirmDialog
          key={`${pendingRemoveDevice.hostId}:${pendingRemoveDevice.device_id}`}
          open
          onOpenChange={(open) => {
            if (!open) setPendingRemoveDevice(null);
          }}
          title={t("settings.pushRemoveTitle", { device: pendingRemoveDevice.device_name })}
          description={t("settings.pushRemoveDesc", { platform: formatPlatformLabel(pendingRemoveDevice.platform), host: pendingRemoveDevice.hostName })}
          confirmLabel={t("settings.pushRemoveButton")}
          tone="danger"
          onConfirm={async () => {
            const target = pendingRemoveDevice;
            if (!hosts.some((h) => h.host_id === target.hostId)) return;
            await orbitActions.unregisterPushDevice(target.hostId, target.device_id);
          }}
        />
      )}
    </div>
  );
}
