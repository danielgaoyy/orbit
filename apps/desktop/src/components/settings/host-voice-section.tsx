import { useCallback, useEffect, useState } from "react";
import { Check, Download, RefreshCw, ShieldCheck, Trash2, Volume2 } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingPageHeader,
  SettingSelect,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { isBrowserSpeechRecognitionAvailable } from "@/lib/voice-recorder";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import {
  type VoiceConfig,
  type VoiceConfigState,
  type VoiceSttBackend,
  type VoiceTtsBackend,
  orbitIpc,
  parseVoiceRpcError,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

function formatBytes(bytes: number): string {
  if (!Number.isFinite(bytes) || bytes <= 0) return "0 B";
  if (bytes >= 1024 * 1024 * 1024) return `${(bytes / (1024 * 1024 * 1024)).toFixed(2)} GB`;
  if (bytes >= 1024 * 1024) return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  if (bytes >= 1024) return `${Math.round(bytes / 1024)} KB`;
  return `${bytes} B`;
}

export function HostVoiceSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const host = useOrbitSelector((s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null);
  const isOnline =
    host?.connection_state === "online_direct" || host?.connection_state === "online_relay";

  const [state, setState] = useState<VoiceConfigState | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [busyModelId, setBusyModelId] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);

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

  const handleDownloadModel = useCallback(
    async (modelId: string) => {
      setBusyModelId(modelId);
      setError(null);
      try {
        const updated = await orbitIpc.downloadVoiceModel(hostId, modelId);
        setState(updated);
        orbitActions.showToast(t("voice.modelDownloadedToast", { id: modelId }));
      } catch (err) {
        const msg = parseVoiceRpcError(err).message;
        setError(msg);
        orbitActions.showToast(msg);
      } finally {
        setBusyModelId(null);
      }
    },
    [hostId, t],
  );

  const handleDeleteModel = useCallback(
    async (modelId: string) => {
      setBusyModelId(modelId);
      setError(null);
      try {
        const updated = await orbitIpc.deleteVoiceModel(hostId, modelId);
        setState(updated);
        orbitActions.showToast(t("voice.modelDeletedToast", { id: modelId }));
      } catch (err) {
        const msg = parseVoiceRpcError(err).message;
        setError(msg);
        orbitActions.showToast(msg);
      } finally {
        setBusyModelId(null);
      }
    },
    [hostId, t],
  );

  const busy = saving || loading || busyModelId !== null;
  const cfg = state?.config;
  const languageValue = cfg?.language ?? "auto";

  return (
    <div className="flex flex-col gap-6">
      <SettingPageHeader
        title={t("voice.title")}
        description={t("voice.subtitle")}
        badge={cfg?.stt_backend === "system_client_fallback" && (
          <Badge variant="outline">{t("voice.sttBrowserFallback")}</Badge>
        )}
        action={
          <Button
            variant="outline"
            size="sm"
            disabled={busy || !isOnline}
            onClick={() => void loadConfig()}
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

      {loading && !cfg ? (
        <SettingCard>
          <div className="text-ink-muted flex items-center gap-2.5 px-4 py-6 text-[13px]">
            <OrbitSpinner size={14} />
            <span>{t("common.loading")}</span>
          </div>
        </SettingCard>
      ) : cfg ? (
        <>
          <div className="flex flex-col gap-2">
            <SettingGroupLabel>{t("voice.sectionControl")}</SettingGroupLabel>
            <SettingCard>
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

              <SettingRow
                settingId="voice.sttBackendTitle"
                title={t("voice.sttBackendTitle")}
                description={t("voice.sttBackendDesc")}
              >
                <SettingSelect<VoiceSttBackend>
                  ariaLabel={t("voice.sttBackendTitle")}
                  value={cfg.stt_backend}
                  disabled={busy || !isOnline}
                  onChange={(stt_backend) => void patchConfig({ stt_backend })}
                  options={[
                    { value: "local_whisper", label: t("voice.sttLocalWhisper") },
                    { value: "local_sense_voice", label: t("voice.sttLocalSenseVoice") },
                    { value: "open_ai_compatible", label: t("voice.backendOpenAi") },
                    { value: "system_client_fallback", label: t("voice.sttBrowserFallback") },
                  ]}
                />
              </SettingRow>

              <SettingRow
                settingId="voice.ttsBackendTitle"
                title={t("voice.ttsBackendTitle")}
                description={t("voice.ttsBackendDesc")}
              >
                <SettingSelect<VoiceTtsBackend>
                  ariaLabel={t("voice.ttsBackendTitle")}
                  value={cfg.tts_backend}
                  disabled={busy || !isOnline}
                  onChange={(tts_backend) => void patchConfig({ tts_backend })}
                  options={[
                    { value: "system_native", label: t("voice.ttsSystemNative") },
                    { value: "local_kokoro", label: t("voice.ttsLocalKokoro") },
                    { value: "open_ai_compatible", label: t("voice.backendOpenAi") },
                    { value: "disabled", label: t("voice.ttsDisabled") },
                  ]}
                />
              </SettingRow>
            </SettingCard>
          </div>

          {state?.capabilities && (
            <div className="flex flex-col gap-2">
              <SettingGroupLabel>{t("voice.sectionCapabilities")}</SettingGroupLabel>
              <SettingCard>
                <div className="flex flex-wrap items-center gap-2 px-4 py-3">
                  <Badge variant={state.capabilities.local_whisper ? "secondary" : "outline"}>
                    {state.capabilities.local_whisper ? (
                      <Check className="text-signal-success size-3" />
                    ) : null}
                    Whisper
                  </Badge>
                  <Badge variant={state.capabilities.local_sense_voice ? "secondary" : "outline"}>
                    {state.capabilities.local_sense_voice ? (
                      <Check className="text-signal-success size-3" />
                    ) : null}
                    SenseVoice
                  </Badge>
                  <Badge variant={state.capabilities.local_kokoro ? "secondary" : "outline"}>
                    {state.capabilities.local_kokoro ? (
                      <Check className="text-signal-success size-3" />
                    ) : null}
                    Kokoro TTS
                  </Badge>
                  <Badge variant={state.capabilities.system_native_tts ? "secondary" : "outline"}>
                    <Volume2 className="size-3" />
                    {t("voice.ttsSystemNative")}
                  </Badge>
                  {state.capabilities.system_client_fallback && isBrowserSpeechRecognitionAvailable() && cfg.stt_backend !== "system_client_fallback" && (
                    <Badge variant="outline">{t("voice.sttBrowserFallback")}</Badge>
                  )}
                </div>
              </SettingCard>
            </div>
          )}

          <div data-setting-id="voice.sectionModels" className="flex flex-col gap-2">
            <SettingGroupLabel>{t("voice.sectionModels")}</SettingGroupLabel>
            <SettingCard>
              {state && state.models.length > 0 ? (
                state.models.map((model) => {
                  const busy = busyModelId === model.id;
                  return (
                    <div
                      key={model.id}
                      className="flex flex-col gap-3 px-4 py-3.5 sm:flex-row sm:items-center sm:justify-between"
                    >
                      <div className="min-w-0 flex-1 space-y-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-ink-primary text-[13px] font-medium">
                            {model.name}
                          </span>
                          <Badge variant="outline" className="font-mono text-[11px] uppercase">
                            {model.kind}
                          </Badge>
                          <span className="text-ink-muted font-mono text-[11px] tabular-nums">
                            {formatBytes(model.size_bytes)}
                          </span>
                          {model.installed && (
                            <Badge variant="secondary" className="gap-1 text-[11px]">
                              <ShieldCheck className="text-signal-success size-3" />
                              {t("voice.shaVerified")}
                            </Badge>
                          )}
                        </div>
                        <div className="text-ink-faint truncate font-mono text-[11px]">
                          SHA-256: {model.sha256.slice(0, 16)}…
                        </div>
                      </div>

                      <div className="flex shrink-0 items-center gap-2">
                        {model.installed ? (
                          <Button
                            variant="outline"
                            size="sm"
                            disabled={busyModelId !== null || saving || loading || !isOnline}
                            onClick={() => void handleDeleteModel(model.id)}
                          >
                            {busy ? (
                              <OrbitSpinner size={12} />
                            ) : (
                              <Trash2 data-icon="inline-start" />
                            )}
                            {t("common.delete")}
                          </Button>
                        ) : (
                          <Button
                            variant="outline"
                            size="sm"
                            disabled={busyModelId !== null || saving || loading || !isOnline}
                            onClick={() => void handleDownloadModel(model.id)}
                          >
                            {busy ? (
                              <OrbitSpinner size={12} />
                            ) : (
                              <Download data-icon="inline-start" />
                            )}
                            {t("voice.downloadModel")}
                          </Button>
                        )}
                      </div>
                    </div>
                  );
                })
              ) : (
                <div className="text-ink-muted px-4 py-5 text-[13px]">
                  {t("voice.noModels")}
                </div>
              )}
            </SettingCard>
          </div>
        </>
      ) : null}
    </div>
  );
}
