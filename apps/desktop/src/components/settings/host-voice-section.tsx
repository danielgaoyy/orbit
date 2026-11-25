import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Mic, RefreshCw } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingPageHeader,
  SettingRow,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import {
  type SystemAudioInputDevice,
  type VoiceConfig,
  type VoiceConfigState,
  orbitIpc,
  parseVoiceRpcError,
} from "@/lib/orbit-ipc";
import {
  type AudioInputDeviceInfo,
  findMatchingWebDeviceId,
  listAudioInputDevices,
  mergeAudioInputDevices,
  normalizeAudioDeviceName,
  parseLabelPrefixedDeviceId,
  requestMicrophoneLabels,
} from "@/lib/voice-recorder";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const SYSTEM_DEFAULT_MIC = "default";
const MIC_POLL_INTERVAL_MS = 2500;

function sameWebDevices(
  a: readonly AudioInputDeviceInfo[],
  b: readonly AudioInputDeviceInfo[],
): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (a[i].deviceId !== b[i].deviceId || a[i].label !== b[i].label) return false;
  }
  return true;
}

function sameSystemDevices(
  a: readonly SystemAudioInputDevice[],
  b: readonly SystemAudioInputDevice[],
): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i += 1) {
    if (
      a[i].name !== b[i].name ||
      a[i].is_default !== b[i].is_default ||
      a[i].transport !== b[i].transport
    ) {
      return false;
    }
  }
  return true;
}

export const HostVoiceSection = memo(function HostVoiceSection({ hostId }: { hostId: string }) {
  const { locale, t } = useI18n();
  const isOnline = useHostIsOnline(hostId);
  const voiceInputDeviceId = useOrbitSelector((s) => s.voiceInputDeviceId);

  const [state, setState] = useState<VoiceConfigState | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const [webAudioInputs, setWebAudioInputs] = useState<AudioInputDeviceInfo[]>([]);
  const [systemAudioInputs, setSystemAudioInputs] = useState<SystemAudioInputDevice[]>([]);
  const [needsMicPermission, setNeedsMicPermission] = useState(false);
  const [micSupported, setMicSupported] = useState(true);
  const [requestingMic, setRequestingMic] = useState(false);
  const lastAutoProbedSignatureRef = useRef<string | null>(null);

  const reconcileWebSelection = useCallback((devices: readonly AudioInputDeviceInfo[]) => {
    const current = orbitActions.getSnapshot().voiceInputDeviceId;
    if (!parseLabelPrefixedDeviceId(current)) return;
    const matchedWebId = findMatchingWebDeviceId(devices, current);
    if (matchedWebId && matchedWebId !== current) {
      orbitActions.setVoiceInputDeviceId(matchedWebId);
    }
  }, []);

  const refreshMicrophones = useCallback(
    async (options?: { probe?: boolean; autoProbeOnMissing?: boolean; notifyDenied?: boolean }) => {
      try {
        const [webRes, sysDevices] = await Promise.all([
          listAudioInputDevices(),
          orbitIpc.listSystemAudioInputs(),
        ]);

        let webDevices = webRes.devices;
        let needsPerm = webRes.needsPermission;
        setMicSupported((prev) => (prev === webRes.supported ? prev : webRes.supported));
        setSystemAudioInputs((prev) => (sameSystemDevices(prev, sysDevices) ? prev : sysDevices));

        const initialMerged = mergeAudioInputDevices(webDevices, sysDevices);
        const osSignature =
          sysDevices.length > 0
            ? sysDevices
                .map((d) => normalizeAudioDeviceName(d.name))
                .sort()
                .join("|")
            : null;
        const shouldAutoProbe =
          Boolean(options?.autoProbeOnMissing) &&
          webRes.supported &&
          Boolean(osSignature) &&
          initialMerged.hasMissingSystemDevice &&
          lastAutoProbedSignatureRef.current !== osSignature;

        if ((options?.probe || shouldAutoProbe) && webRes.supported) {
          if (osSignature) lastAutoProbedSignatureRef.current = osSignature;
          const probed = await requestMicrophoneLabels();
          if (probed.granted) {
            webDevices = probed.devices;
            needsPerm = false;
          } else if (options?.notifyDenied) {
            orbitActions.showToast(t("voice.micAccessDenied"));
          }
        }

        setWebAudioInputs((prev) => (sameWebDevices(prev, webDevices) ? prev : webDevices));
        setNeedsMicPermission((prev) => (prev === needsPerm ? prev : needsPerm));
        reconcileWebSelection(webDevices);
      } catch {
        setWebAudioInputs([]);
        setNeedsMicPermission(false);
      }
    },
    [reconcileWebSelection, t],
  );

  useEffect(() => {
    void refreshMicrophones({ autoProbeOnMissing: true });

    const onDeviceChange = () => void refreshMicrophones({ autoProbeOnMissing: true });
    const onWindowFocus = () => void refreshMicrophones({ autoProbeOnMissing: true });
    const onVisibilityChange = () => {
      if (document.visibilityState === "visible") {
        void refreshMicrophones({ autoProbeOnMissing: true });
      }
    };

    const mediaDevices = typeof navigator !== "undefined" ? navigator.mediaDevices : undefined;
    mediaDevices?.addEventListener?.("devicechange", onDeviceChange);
    window.addEventListener("focus", onWindowFocus);
    document.addEventListener("visibilitychange", onVisibilityChange);

    // macOS WKWebView often skips `devicechange` for Bluetooth headsets; poll while Voice is open.
    const pollId = window.setInterval(() => {
      if (document.visibilityState === "visible") {
        void refreshMicrophones({ autoProbeOnMissing: true });
      }
    }, MIC_POLL_INTERVAL_MS);

    return () => {
      mediaDevices?.removeEventListener?.("devicechange", onDeviceChange);
      window.removeEventListener("focus", onWindowFocus);
      document.removeEventListener("visibilitychange", onVisibilityChange);
      window.clearInterval(pollId);
    };
  }, [refreshMicrophones]);

  const handleRequestMicLabels = useCallback(async () => {
    setRequestingMic(true);
    try {
      await refreshMicrophones({ probe: true, notifyDenied: true });
    } finally {
      setRequestingMic(false);
    }
  }, [refreshMicrophones]);

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

  const mergedMicrophones = useMemo(
    () => mergeAudioInputDevices(webAudioInputs, systemAudioInputs),
    [webAudioInputs, systemAudioInputs],
  );

  const micOptions = useMemo<SettingSelectOption<string>[]>(() => {
    const defaultLabel = mergedMicrophones.defaultSystemDeviceName
      ? t("voice.micSystemDefaultNamed", { name: mergedMicrophones.defaultSystemDeviceName })
      : t("voice.micSystemDefault");

    const opts: SettingSelectOption<string>[] = [
      { value: SYSTEM_DEFAULT_MIC, label: defaultLabel },
      ...mergedMicrophones.devices.map((dev, idx) => ({
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
  }, [mergedMicrophones, voiceInputDeviceId, locale]);

  const languageOptions = useMemo<SettingSelectOption<string>[]>(
    () => [
      { value: "auto", label: t("voice.langAuto") },
      { value: "en", label: "English" },
      { value: "zh", label: "中文" },
      { value: "ja", label: "日本語" },
    ],
    [t],
  );

  const busy = saving || loading;
  const cfg = state?.config;
  const languageValue = cfg?.language ?? "auto";
  const selectedMicValue = voiceInputDeviceId ?? SYSTEM_DEFAULT_MIC;
  const showManualDetectButton =
    needsMicPermission && micSupported && systemAudioInputs.length === 0;

  const microphoneRow = (
    <SettingRow
      settingId="voice.microphoneTitle"
      title={t("voice.microphoneTitle")}
      description={t("voice.microphoneDesc")}
    >
      <div className="flex items-center gap-2">
        {showManualDetectButton && (
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
          disabled={!micSupported && systemAudioInputs.length === 0}
          options={micOptions}
          menuClassName="w-[280px]"
          onOpenChange={(open) => {
            if (open) {
              void refreshMicrophones({
                probe: needsMicPermission && systemAudioInputs.length === 0,
                autoProbeOnMissing: true,
              });
            }
          }}
          onChange={(val) =>
            orbitActions.setVoiceInputDeviceId(val === SYSTEM_DEFAULT_MIC ? null : val)
          }
        />
      </div>
    </SettingRow>
  );

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
              lastAutoProbedSignatureRef.current = null;
              void refreshMicrophones({ probe: true });
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

              {microphoneRow}

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
                  options={languageOptions}
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
            microphoneRow
          )}
        </SettingCard>
      </div>
    </div>
  );
});
