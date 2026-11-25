import { useCallback, useEffect, useMemo, useState } from "react";
import { Mic, RefreshCw } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingPageHeader,
  SettingRow,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import {
  type VoiceConfig,
  type VoiceConfigState,
  orbitIpc,
  parseVoiceRpcError,
} from "@/lib/orbit-ipc";
import {
  type AudioInputDeviceInfo,
  listAudioInputDevices,
  requestMicrophoneLabels,
} from "@/lib/voice-recorder";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const SYSTEM_DEFAULT_MIC = "default";

export function HostVoiceSection({ hostId }: { hostId: string }) {
  const { locale, t } = useI18n();
  const host = useOrbitSelector((s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null);
  const voiceInputDeviceId = useOrbitSelector((s) => s.voiceInputDeviceId);
  const isOnline =
    host?.connection_state === "online_direct" || host?.connection_state === "online_relay";

  const [state, setState] = useState<VoiceConfigState | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [audioInputs, setAudioInputs] = useState<AudioInputDeviceInfo[]>([]);
  const [needsMicPermission, setNeedsMicPermission] = useState(false);
  const [micSupported, setMicSupported] = useState(true);
  const [requestingMic, setRequestingMic] = useState(false);

  const refreshMicrophones = useCallback(async () => {
    try {
      const res = await listAudioInputDevices();
      setAudioInputs(res.devices);
      setNeedsMicPermission(res.needsPermission);
      setMicSupported(res.supported);
    } catch {
      setAudioInputs([]);
      setNeedsMicPermission(false);
    }
  }, []);

  useEffect(() => {
    void refreshMicrophones();
    if (typeof navigator === "undefined" || !navigator.mediaDevices?.addEventListener) return;
    const onChange = () => void refreshMicrophones();
    navigator.mediaDevices.addEventListener("devicechange", onChange);
    return () => navigator.mediaDevices.removeEventListener("devicechange", onChange);
  }, [refreshMicrophones]);

  const handleRequestMicLabels = useCallback(async () => {
    setRequestingMic(true);
    try {
      const res = await requestMicrophoneLabels();
      if (!res.granted) {
        orbitActions.showToast(t("voice.micAccessDenied"));
        return;
      }
      setAudioInputs(res.devices);
      setNeedsMicPermission(false);
    } finally {
      setRequestingMic(false);
    }
  }, [t]);

  const loadConfig = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const next = await orbitIpc.getVoiceConfig(hostId);
      setState(next);
    } catch (err) {
      setError(parseVoiceRpcError(err).message);
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  useEffect(() => {
    if (isOnline) void loadConfig();
    else setLoading(false);
  }, [loadConfig, isOnline]);

  const patchConfig = useCallback(
    async (patch: Partial<VoiceConfig>) => {
      if (!state) return;
      const nextConfig: VoiceConfig = { ...state.config, ...patch };
      setSaving(true);
      setError(null);
      try {
        const updated = await orbitIpc.updateVoiceConfig(hostId, nextConfig);
        setState(updated);
      } catch (err) {
        const msg = parseVoiceRpcError(err).message;
        setError(msg);
        orbitActions.showToast(msg);
      } finally {
        setSaving(false);
      }
    },
    [hostId, state],
  );

  const micOptions = useMemo<SettingSelectOption<string>[]>(() => {
    const opts: SettingSelectOption<string>[] = [
      { value: SYSTEM_DEFAULT_MIC, label: t("voice.micSystemDefault") },
      ...audioInputs.map((dev, idx) => ({
        value: dev.deviceId,
        label: dev.label || t("voice.micDeviceFallback", { index: idx + 1 }),
      })),
    ];
    if (voiceInputDeviceId && !opts.some((opt) => opt.value === voiceInputDeviceId)) {
      opts.push({
        value: voiceInputDeviceId,
        label: t("voice.micUnavailable"),
      });
    }
    return opts;
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [audioInputs, voiceInputDeviceId, locale]);

  const busy = saving || loading;
  const cfg = state?.config;
  const languageValue = cfg?.language ?? "auto";
  const selectedMicValue = voiceInputDeviceId ?? SYSTEM_DEFAULT_MIC;

  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
        title={t("voice.title")}
        description={t("voice.subtitle")}
        action={
          <Button
            variant="outline"
            size="sm"
            disabled={busy || !isOnline}
            onClick={() => {
              void refreshMicrophones();
              void loadConfig();
            }}
          >
            <RefreshCw data-icon="inline-start" className={loading ? "animate-spin" : undefined} />
            {t("common.refresh")}
          </Button>
        }
      />

      {!isOnline && <p className="text-ink-muted text-[13px]">{t("voice.offline")}</p>}
      {error && isOnline && (
        <div className="border-signal-danger/30 bg-signal-danger/8 text-signal-danger rounded-[10px] border px-3.5 py-2.5 text-[13px]">
          {error}
        </div>
      )}

      <div className="flex flex-col gap-2">
        <SettingGroupLabel>{t("voice.sectionControl")}</SettingGroupLabel>
        <SettingCard>
          {loading && !cfg ? (
            <div className="text-ink-muted flex items-center gap-2.5 px-4 py-6 text-[13px]">
              <OrbitSpinner size={14} />
              <span>{t("common.loading")}</span>
            </div>
          ) : cfg ? (
            <>
              <SettingRow
                settingId="voice.enableTitle"
                title={t("voice.enableTitle")}
                description={t("voice.enableDesc")}
              >
                <ToggleSwitch
                  checked={cfg.enabled}
                  disabled={busy || !isOnline}
                  ariaLabel={t("voice.enableTitle")}
                  onChange={(enabled) => void patchConfig({ enabled })}
                />
              </SettingRow>

              <SettingRow
                settingId="voice.microphoneTitle"
                title={t("voice.microphoneTitle")}
                description={t("voice.microphoneDesc")}
              >
                <div className="flex items-center gap-2">
                  {needsMicPermission && micSupported && (
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={requestingMic}
                      onClick={() => void handleRequestMicLabels()}
                    >
                      {requestingMic ? <OrbitSpinner size={12} /> : <Mic data-icon="inline-start" />}
                      {t("voice.micAllowAccess")}
                    </Button>
                  )}
                  <SettingSelect<string>
                    ariaLabel={t("voice.microphoneTitle")}
                    value={selectedMicValue}
                    disabled={!micSupported}
                    options={micOptions}
                    menuClassName="w-[260px]"
                    onChange={(val) =>
                      orbitActions.setVoiceInputDeviceId(val === SYSTEM_DEFAULT_MIC ? null : val)
                    }
                  />
                </div>
              </SettingRow>

              <SettingRow
                settingId="voice.languageTitle"
                title={t("voice.languageTitle")}
                description={t("voice.languageDesc")}
              >
                <SettingSelect<string>
                  ariaLabel={t("voice.languageTitle")}
                  value={languageValue}
                  disabled={busy || !isOnline}
                  onChange={(val) =>
                    void patchConfig({ language: val === "auto" ? null : val })
                  }
                  options={[
                    { value: "auto", label: t("voice.langAuto") },
                    { value: "en", label: "English" },
                    { value: "zh", label: "中文" },
                    { value: "ja", label: "日本語" },
                  ]}
                />
              </SettingRow>

              <SettingRow
                settingId="voice.autoSpeakTitle"
                title={t("voice.autoSpeakTitle")}
                description={t("voice.autoSpeakDesc")}
              >
                <ToggleSwitch
                  checked={cfg.auto_speak_responses}
                  disabled={busy || !isOnline}
                  ariaLabel={t("voice.autoSpeakTitle")}
                  onChange={(auto_speak_responses) =>
                    void patchConfig({ auto_speak_responses })
                  }
                />
              </SettingRow>
            </>
          ) : (
            <SettingRow
              settingId="voice.microphoneTitle"
              title={t("voice.microphoneTitle")}
              description={t("voice.microphoneDesc")}
            >
              <div className="flex items-center gap-2">
                {needsMicPermission && micSupported && (
                  <Button
                    type="button"
                    variant="outline"
                    size="sm"
                    disabled={requestingMic}
                    onClick={() => void handleRequestMicLabels()}
                  >
                    {requestingMic ? <OrbitSpinner size={12} /> : <Mic data-icon="inline-start" />}
                    {t("voice.micAllowAccess")}
                  </Button>
                )}
                <SettingSelect<string>
                  ariaLabel={t("voice.microphoneTitle")}
                  value={selectedMicValue}
                  disabled={!micSupported}
                  options={micOptions}
                  menuClassName="w-[260px]"
                  onChange={(val) =>
                    orbitActions.setVoiceInputDeviceId(val === SYSTEM_DEFAULT_MIC ? null : val)
                  }
                />
              </div>
            </SettingRow>
          )}
        </SettingCard>
      </div>
    </div>
  );
}
