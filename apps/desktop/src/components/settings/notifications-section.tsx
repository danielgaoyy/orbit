import { useEffect, useMemo, useRef, useState } from "react";
import { Bell, Check, RefreshCw, Send, Shield, Smartphone, Trash2 } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useI18n } from "@/lib/i18n";
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
  if (!ms) return "Never";
  try {
    return new Date(ms).toLocaleString();
  } catch {
    return "Unknown";
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
    if (res) {
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
        <SettingGroupLabel>Desktop Permission</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Desktop notifications"
            description={
              permission === "granted"
                ? "Clicking an alert brings Orbit to front and opens that session."
                : permission === "denied"
                  ? "Enable notifications for Orbit in System Settings."
                  : "Allow Orbit to deliver system notifications."
            }
          >
            {permission === "granted" ? (
              <div className="flex items-center gap-2.5">
                <button
                  type="button"
                  onClick={() => orbitActions.sendTestDesktopNotification()}
                  className="border-border text-foreground hover:bg-black/[0.04] dark:hover:bg-white/[0.06] inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
                >
                  <Send className="h-[12px] w-[12px]" />
                  <span>Test</span>
                </button>
                <span className="inline-flex items-center gap-1.5 text-[12.5px] font-medium text-emerald-600 dark:text-emerald-400">
                  <Check className="h-[14px] w-[14px]" />
                  Enabled
                </span>
              </div>
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

      {/* Desktop Alert Categories & Sound */}
      <div className={cn("space-y-2", isBlocked && "opacity-55")}>
        <SettingGroupLabel>Desktop Alerts</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title="Approval required"
            description="Notify when an agent waits for permission or input."
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
            title="Turn completed"
            description="Notify when a background agent finishes its turn."
          >
            <ToggleSwitch
              checked={notifyOnTurnComplete}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnTurnComplete: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow
            title="Agent error"
            description="Notify when an agent stops with an error."
          >
            <ToggleSwitch
              checked={notifyOnAgentError}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnAgentError: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow
            title="Goal finished"
            description="Notify when an autonomous /goal run finishes."
          >
            <ToggleSwitch
              checked={notifyOnGoalFinished}
              onChange={(v) => {
                orbitActions.setNotificationPrefs({ notifyOnGoalFinished: v });
                if (v) void ensurePermissionIfDefault();
              }}
            />
          </SettingRow>

          <SettingRow title="Sound" description="Play the system alert sound with notifications.">
            <ToggleSwitch
              checked={notifySoundEnabled}
              onChange={(v) => orbitActions.setNotificationPrefs({ notifySoundEnabled: v })}
            />
          </SettingRow>

          <SettingRow
            title="Quiet hours"
            description="Silence desktop alerts during a daily schedule (local time)."
          >
            <div className="flex items-center gap-2">
              {notifyQuietHoursEnabled && (
                <div className="flex items-center gap-1.5 text-[12px]">
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel="Desktop quiet start hour"
                    value={notifyQuietStartHour}
                    options={HOUR_OPTIONS}
                    triggerClassName="min-w-[82px]"
                    menuClassName="min-w-[104px]"
                    onChange={(hour) =>
                      orbitActions.setNotificationPrefs({ notifyQuietStartHour: hour })
                    }
                  />
                  <span className="text-muted-foreground">to</span>
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel="Desktop quiet end hour"
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
          <SettingGroupLabel>Mobile Push & Gateway</SettingGroupLabel>
          {hosts.length > 1 && (
            <SettingSelect<string>
              size="compact"
              ariaLabel="Target host for mobile push"
              value={activeHostId}
              options={hostSelectOptions}
              triggerClassName="h-[26px] min-w-[132px] max-w-[200px]"
              onChange={(hostId) => setTargetHostId(hostId)}
            />
          )}
        </div>

        <SettingCard>
          <SettingRow
            title="Background push"
            description="Deliver alerts to registered iOS, Android, and HarmonyOS devices."
          >
            <ToggleSwitch
              checked={draftPrefs.enabled}
              disabled={!isPushReady}
              onChange={(v) => updateDraftPrefs((p) => ({ ...p, enabled: v }))}
            />
          </SettingRow>

          <SettingRow
            title="Include message preview"
            description="Off by default — push relays receive metadata only."
          >
            <div className="flex items-center gap-2">
              {!draftPrefs.include_message_preview && (
                <span className="text-muted-foreground inline-flex items-center gap-1 text-[11.5px]">
                  <Shield className="h-[12px] w-[12px]" />
                  Zero-knowledge
                </span>
              )}
              <ToggleSwitch
                checked={draftPrefs.include_message_preview}
                disabled={!isPushReady}
                onChange={(v) => updateDraftPrefs((p) => ({ ...p, include_message_preview: v }))}
              />
            </div>
          </SettingRow>

          <SettingRow
            title="Event triggers"
            description="Choose which daemon events dispatch mobile push."
          >
            <div className="flex flex-wrap justify-end gap-1.5">
              {(
                [
                  ["notify_permission_required", "Approvals"],
                  ["notify_turn_completed", "Turns"],
                  ["notify_agent_error", "Errors"],
                  ["notify_goal_finished", "Goals"],
                  ["notify_timer_fired", "Timers"],
                ] as const
              ).map(([field, label]) => {
                const active = draftPrefs[field];
                return (
                  <button
                    key={field}
                    type="button"
                    disabled={!isPushReady}
                    onClick={() => updateDraftPrefs((p) => ({ ...p, [field]: !p[field] }))}
                    className={cn(
                      "h-[26px] cursor-pointer rounded-md border px-2 text-[11.5px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-50",
                      active
                        ? "border-foreground/25 bg-foreground/10 text-foreground"
                        : "border-border text-muted-foreground hover:text-foreground",
                    )}
                  >
                    {label}
                  </button>
                );
              })}
            </div>
          </SettingRow>

          <SettingRow
            title="Mobile quiet hours"
            description="Host local time — pause mobile push on this host during quiet hours."
          >
            <div className="flex items-center gap-2">
              {draftPrefs.quiet_hours_enabled && (
                <div className="flex items-center gap-1.5 text-[12px]">
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel="Mobile quiet start hour"
                    value={draftPrefs.quiet_start_hour}
                    options={HOUR_OPTIONS}
                    disabled={!isPushReady}
                    triggerClassName="min-w-[82px]"
                    menuClassName="min-w-[104px]"
                    onChange={(hour) =>
                      updateDraftPrefs((p) => ({ ...p, quiet_start_hour: hour }))
                    }
                  />
                  <span className="text-muted-foreground">to</span>
                  <SettingSelect<number>
                    size="sm"
                    ariaLabel="Mobile quiet end hour"
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
                checked={draftPrefs.quiet_hours_enabled}
                disabled={!isPushReady}
                onChange={(v) => updateDraftPrefs((p) => ({ ...p, quiet_hours_enabled: v }))}
              />
            </div>
          </SettingRow>

          <div className="space-y-3 px-4 py-3.5">
            <div className="space-y-1.5">
              <label
                htmlFor="push-gateway-url"
                className="text-foreground block text-[13px] font-medium"
              >
                Push gateway URL
              </label>
              <input
                id="push-gateway-url"
                type="url"
                value={draftPrefs.push_gateway_url ?? ""}
                disabled={!isPushReady}
                onChange={(e) =>
                  updateDraftPrefs((p) => ({ ...p, push_gateway_url: e.target.value }))
                }
                placeholder="https://push.example.com/v1/dispatch"
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 h-[32px] w-full rounded-lg border px-2.5 text-[12.5px] outline-none focus:border-foreground/40 disabled:opacity-55"
              />
            </div>

            <div className="space-y-1.5">
              <div className="flex items-center justify-between">
                <label
                  htmlFor="push-gateway-token"
                  className="text-foreground block text-[13px] font-medium"
                >
                  Gateway Bearer token
                </label>
                {draftPrefs.has_gateway_auth_token && !clearStoredToken && isPushReady && (
                  <button
                    type="button"
                    onClick={() => {
                      setIsDirty(true);
                      setClearStoredToken(true);
                    }}
                    className="text-muted-foreground hover:text-foreground cursor-pointer text-[11.5px]"
                  >
                    Clear stored token
                  </button>
                )}
              </div>
              <input
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
                    ? "Stored token kept unless replaced"
                    : "Optional Bearer token"
                }
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 h-[32px] w-full rounded-lg border px-2.5 text-[12.5px] outline-none focus:border-foreground/40 disabled:opacity-55"
              />
            </div>

            <div className="flex items-center justify-between pt-1">
              <span className="text-muted-foreground text-[12px]">
                {pushEntry?.error
                  ? pushEntry.error
                  : !isHostOnline
                    ? "Host offline — reconnect to update push settings."
                    : pushEntry?.status !== "ready"
                      ? "Loading host push settings…"
                      : "HTTPS only with DNS-pinned SSRF protection."}
              </span>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  disabled={!isPushReady || pushEntry?.testing || devices.length === 0}
                  onClick={() => void orbitActions.testPushNotification(activeHostId, null)}
                  className="border-border text-foreground hover:bg-black/[0.04] dark:hover:bg-white/[0.06] inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <Send className="h-[12.5px] w-[12.5px]" />
                  <span>{pushEntry?.testing ? "Sending…" : "Test Notification"}</span>
                </button>
                <button
                  type="button"
                  disabled={!isPushReady || pushEntry?.saving}
                  onClick={() => void handleSaveMobilePrefs()}
                  className="bg-foreground text-background inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12px] font-medium transition-opacity hover:opacity-90 disabled:cursor-not-allowed disabled:opacity-50"
                >
                  <span>{pushEntry?.saving ? "Saving…" : "Save"}</span>
                </button>
              </div>
            </div>
          </div>
        </SettingCard>
      </div>

      {/* Registered Mobile Devices */}
      <div className="space-y-2">
        <div className="flex items-center justify-between">
          <SettingGroupLabel>Registered Mobile Devices</SettingGroupLabel>
          {activeHostId && (
            <button
              type="button"
              onClick={() => void orbitActions.loadPushDevices(activeHostId)}
              className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[11.5px]"
            >
              <RefreshCw
                className={cn(
                  "h-[11.5px] w-[11.5px]",
                  pushEntry?.status === "loading" && "animate-spin",
                )}
              />
              <span>Refresh</span>
            </button>
          )}
        </div>

        <SettingCard>
          {devices.length === 0 ? (
            <div className="text-muted-foreground flex items-center gap-2.5 px-4 py-4 text-[12.5px]">
              <Smartphone className="h-[15px] w-[15px] shrink-0 opacity-70" />
              <span>
                No mobile devices registered on {activeHost?.display_name ?? "this host"} yet. Pair
                Orbit on iOS, Android, or HarmonyOS to receive background push.
              </span>
            </div>
          ) : (
            devices.map((dev) => (
              <SettingRow
                key={dev.device_id}
                title={
                  <span className="inline-flex items-center gap-2">
                    <span>{dev.device_name}</span>
                    <span className="border-border text-muted-foreground rounded border px-1.5 py-0.5 text-[10.5px] font-medium">
                      {formatPlatformLabel(dev.platform)}
                    </span>
                    {dev.sandbox && (
                      <span className="border-amber-500/30 bg-amber-500/10 text-amber-600 dark:text-amber-400 rounded border px-1.5 py-0.5 text-[10.5px] font-medium">
                        Sandbox
                      </span>
                    )}
                    {!dev.enabled && (
                      <span className="border-destructive/30 bg-destructive/10 text-destructive rounded border px-1.5 py-0.5 text-[10.5px] font-medium">
                        Disabled
                      </span>
                    )}
                  </span>
                }
                description={
                  dev.last_error
                    ? `Token ${dev.masked_token} · Error: ${dev.last_error}`
                    : `Token ${dev.masked_token} · Last delivered: ${formatTimestamp(dev.last_delivered_at_ms)}`
                }
              >
                <div className="flex items-center gap-1.5">
                  <button
                    type="button"
                    disabled={!isPushReady || pushEntry?.testing}
                    onClick={() =>
                      void orbitActions.testPushNotification(activeHostId, dev.device_id)
                    }
                    className="border-border text-foreground hover:bg-black/[0.04] dark:hover:bg-white/[0.06] inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium disabled:opacity-50"
                  >
                    <Send className="h-[11px] w-[11px]" />
                    <span>Test</span>
                  </button>
                  <button
                    type="button"
                    aria-label={`Remove ${dev.device_name}`}
                    disabled={!isPushReady}
                    onClick={() => {
                      if (!activeHostId) return;
                      setPendingRemoveDevice({
                        hostId: activeHostId,
                        hostName: activeHost?.display_name ?? "this host",
                        device_id: dev.device_id,
                        device_name: dev.device_name,
                        platform: dev.platform,
                      });
                    }}
                    className="text-muted-foreground hover:text-destructive inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:opacity-50"
                  >
                    <Trash2 className="h-[13.5px] w-[13.5px]" />
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
          title={`Remove "${pendingRemoveDevice.device_name}"?`}
          description={`This ${formatPlatformLabel(pendingRemoveDevice.platform)} device will stop receiving background push alerts from ${pendingRemoveDevice.hostName} until paired again.`}
          confirmLabel="Remove device"
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
