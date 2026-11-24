import { memo, useCallback, useEffect, useRef, useState } from "react";
import { AlertCircle, Check, ChevronDown, ChevronRight } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Field,
  FieldContent,
  FieldDescription,
  FieldGroup,
  FieldLabel,
  FieldTitle,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Popover, PopoverContent, PopoverTrigger, SimpleTooltip } from "@/components/ui/overlays";
import { Switch } from "@/components/ui/switch";
import {
  DEFAULT_FIM_CONFIG,
  validateFimConfigDraft,
  type FimConfigDraft,
  type FimConfigValidationErrorCode,
  type FimRuntimeStatus,
} from "@/lib/codemirror-fim";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import type { FimConfig, FimMode } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";

export interface FimSettingsPopoverProps {
  config: FimConfig;
  loading: boolean;
  runtimeStatus: FimRuntimeStatus;
  onSaveConfig: (next: FimConfig) => Promise<FimConfig>;
  onReturnFocusToEditor?: () => void;
}

function configToDraft(config: FimConfig): FimConfigDraft {
  return {
    mode: config.mode,
    endpointUrl: config.endpoint_url ?? "",
    model: config.model ?? "",
    apiKeyEnv: config.api_key_env ?? "",
    maxTokens: String(config.max_tokens ?? DEFAULT_FIM_CONFIG.max_tokens),
    timeoutMs: String(config.timeout_ms ?? DEFAULT_FIM_CONFIG.timeout_ms),
    allowFastCliFallback: Boolean(config.allow_fast_cli_fallback),
  };
}

function modeLabelKey(mode: FimMode):
  | "sidePanel.editorFimModeOff"
  | "sidePanel.editorFimModeSubtle"
  | "sidePanel.editorFimModeEager" {
  switch (mode) {
    case "eager":
      return "sidePanel.editorFimModeEager";
    case "subtle":
      return "sidePanel.editorFimModeSubtle";
    default:
      return "sidePanel.editorFimModeOff";
  }
}

function validationErrorKey(code: FimConfigValidationErrorCode):
  | "sidePanel.editorFimErrEndpointUrl"
  | "sidePanel.editorFimErrEndpointPath"
  | "sidePanel.editorFimErrApiKeyEnv"
  | "sidePanel.editorFimErrInsecureTransport"
  | "sidePanel.editorFimErrOllamaModel"
  | "sidePanel.editorFimErrModel" {
  switch (code) {
    case "invalid_endpoint_url":
      return "sidePanel.editorFimErrEndpointUrl";
    case "invalid_endpoint_path":
      return "sidePanel.editorFimErrEndpointPath";
    case "invalid_api_key_env":
      return "sidePanel.editorFimErrApiKeyEnv";
    case "insecure_api_key_transport":
      return "sidePanel.editorFimErrInsecureTransport";
    case "ollama_model_required":
      return "sidePanel.editorFimErrOllamaModel";
    case "invalid_model":
      return "sidePanel.editorFimErrModel";
  }
}

const FIM_MODES: readonly FimMode[] = ["off", "subtle", "eager"];

export const FimSettingsPopover = memo(function FimSettingsPopover({
  config,
  loading,
  runtimeStatus,
  onSaveConfig,
  onReturnFocusToEditor,
}: FimSettingsPopoverProps) {
  const { t } = useI18n();
  const mod = modSymbol();
  const [open, setOpen] = useState(false);
  const [draft, setDraft] = useState<FimConfigDraft>(() => configToDraft(config));
  const [saving, setSaving] = useState(false);
  const [savedNotice, setSavedNotice] = useState(false);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const [showAdvanced, setShowAdvanced] = useState(false);
  const savedTimerRef = useRef<number | null>(null);

  useEffect(() => {
    return () => {
      if (savedTimerRef.current !== null) {
        window.clearTimeout(savedTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    setDraft(configToDraft(config));
    setErrorMessage(null);
  }, [config]);

  const handleOpenChange = useCallback(
    (nextOpen: boolean) => {
      setOpen(nextOpen);
      if (nextOpen) {
        setDraft(configToDraft(config));
        setErrorMessage(null);
        setSavedNotice(false);
      } else {
        onReturnFocusToEditor?.();
      }
    },
    [config, onReturnFocusToEditor],
  );

  const persistDraft = useCallback(
    async (candidate: FimConfigDraft) => {
      const checked = validateFimConfigDraft(candidate);
      if (!checked.valid || !checked.normalized) {
        if (checked.errorCode) {
          setErrorMessage(t(validationErrorKey(checked.errorCode)));
        }
        return false;
      }

      setSaving(true);
      setErrorMessage(null);
      try {
        const saved = await onSaveConfig(checked.normalized);
        setDraft(configToDraft(saved));
        setSavedNotice(true);
        if (savedTimerRef.current !== null) {
          window.clearTimeout(savedTimerRef.current);
        }
        savedTimerRef.current = window.setTimeout(() => {
          savedTimerRef.current = null;
          setSavedNotice(false);
        }, 1400);
        return true;
      } catch (err) {
        setErrorMessage(err instanceof Error ? err.message : String(err));
        return false;
      } finally {
        setSaving(false);
      }
    },
    [onSaveConfig, t],
  );

  const handleModeSelect = useCallback(
    async (nextMode: FimMode) => {
      const nextDraft: FimConfigDraft = { ...draft, mode: nextMode };
      setDraft(nextDraft);
      setErrorMessage(null);

      // Persist immediately when turning Off or when a backend is already configured
      const hasBackend =
        nextDraft.endpointUrl.trim().length > 0 || nextDraft.allowFastCliFallback;
      if (nextMode === "off" || hasBackend) {
        await persistDraft(nextDraft);
      }
    },
    [draft, persistDraft],
  );

  const modeLabel = t(modeLabelKey(config.mode));
  const isUnconfiguredActive =
    config.mode !== "off" && !config.endpoint_url && !config.allow_fast_cli_fallback;
  const isDraftUnconfiguredActive =
    draft.mode !== "off" && !draft.endpointUrl.trim() && !draft.allowFastCliFallback;

  const statusDotClass =
    config.mode === "off"
      ? "bg-ink-faint/55"
      : runtimeStatus.phase === "error"
        ? "bg-signal-danger"
        : isUnconfiguredActive
          ? "bg-signal-warning"
          : runtimeStatus.phase === "active"
            ? "bg-signal-success"
            : "bg-signal-info";

  return (
    <Popover open={open} onOpenChange={handleOpenChange}>
      <SimpleTooltip label={t("sidePanel.editorFimTooltip", { mode: modeLabel })}>
        <PopoverTrigger
          render={
            <button
              type="button"
              aria-label={t("sidePanel.editorFimTitle")}
              className={cn(
                "inline-flex h-[19px] cursor-pointer items-center gap-1.5 rounded px-1.5 font-sans text-[11px] font-medium transition-colors",
                open
                  ? "bg-accent text-ink-primary"
                  : config.mode === "off"
                    ? "text-ink-tertiary hover:bg-accent hover:text-ink-primary"
                    : "text-ink-secondary hover:bg-accent hover:text-ink-primary",
              )}
            />
          }
        >
          {runtimeStatus.phase === "pending" || loading ? (
            <OrbitSpinner size={10} />
          ) : (
            <span className={cn("size-[5.5px] shrink-0 rounded-full", statusDotClass)} />
          )}
          <span className="tabular-nums">
            {t("sidePanel.editorFimPill", { mode: modeLabel })}
          </span>
        </PopoverTrigger>
      </SimpleTooltip>

      <PopoverContent
        side="top"
        align="end"
        sideOffset={8}
        className="w-[336px] p-3 font-sans"
      >
        <div className="flex flex-col gap-3">
          {/* Header & Runtime Telemetry */}
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 flex-col gap-0.5">
              <span className="text-ink-primary text-[12.5px] font-medium leading-tight">
                {t("sidePanel.editorFimTitle")}
              </span>
              <span className="text-ink-tertiary text-[11px] leading-snug">
                {t("sidePanel.editorFimSubtitle")}
              </span>
            </div>
            {runtimeStatus.source && runtimeStatus.source !== "off" && (
              <span className="border-border-subtle bg-surface-recessed text-ink-tertiary tabular-nums shrink-0 rounded-[5px] border px-1.5 py-0.5 font-mono text-[10px]">
                {runtimeStatus.source}
                {typeof runtimeStatus.latencyMs === "number" && runtimeStatus.latencyMs > 0
                  ? ` · ${runtimeStatus.latencyMs}ms`
                  : ""}
              </span>
            )}
          </div>

          {/* 3-Way Segmented Mode Control (Off / Subtle / Eager) */}
          <div
            role="radiogroup"
            aria-label={t("sidePanel.editorFimModeLabel")}
            className="border-border-subtle bg-surface-recessed grid grid-cols-3 gap-0.5 rounded-lg border p-0.5"
          >
            {FIM_MODES.map((mode) => {
              const selected = draft.mode === mode;
              const timingHint =
                mode === "eager" ? "250ms" : mode === "subtle" ? "650ms" : null;
              return (
                <button
                  key={mode}
                  type="button"
                  role="radio"
                  aria-checked={selected}
                  disabled={saving}
                  onClick={() => void handleModeSelect(mode)}
                  className={cn(
                    "inline-flex h-[24px] cursor-pointer items-center justify-center gap-1 rounded-[6px] px-2 text-[11.5px] font-medium transition-colors disabled:opacity-50",
                    selected
                      ? "bg-surface-overlay text-ink-primary shadow-2xs"
                      : "text-ink-tertiary hover:text-ink-primary",
                  )}
                >
                  <span>{t(modeLabelKey(mode))}</span>
                  {timingHint && (
                    <span className="text-ink-faint tabular-nums font-mono text-[10px]">
                      {timingHint}
                    </span>
                  )}
                </button>
              );
            })}
          </div>

          {isDraftUnconfiguredActive && (
            <div className="border-signal-warning-border bg-signal-warning-bg text-signal-warning flex items-start gap-1.5 rounded-lg border px-2.5 py-1.5 text-[11px] leading-snug">
              <AlertCircle className="mt-0.5 size-[12px] shrink-0" />
              <span>{t("sidePanel.editorFimUnconfiguredHint")}</span>
            </div>
          )}

          {runtimeStatus.phase === "error" && runtimeStatus.error && (
            <div className="border-signal-danger-border bg-signal-danger-bg text-signal-danger flex items-start gap-1.5 rounded-lg border px-2.5 py-1.5 text-[11px] leading-snug">
              <AlertCircle className="mt-0.5 size-[12px] shrink-0" />
              <span className="break-words">{runtimeStatus.error}</span>
            </div>
          )}

          {/* Host-Scoped Provider Configuration */}
          <FieldGroup className="gap-2.5">
            <Field className="gap-1">
              <FieldLabel htmlFor="orbit-fim-endpoint" className="text-[11px]">
                {t("sidePanel.editorFimEndpointLabel")}
              </FieldLabel>
              <Input
                id="orbit-fim-endpoint"
                type="url"
                value={draft.endpointUrl}
                disabled={saving}
                onChange={(e) => {
                  setDraft((prev) => ({ ...prev, endpointUrl: e.target.value }));
                  setErrorMessage(null);
                }}
                placeholder="http://127.0.0.1:11434/api/generate"
                className="h-[28px] px-2 font-mono text-[11.5px]"
              />
              <FieldDescription className="text-[10.5px]">
                {t("sidePanel.editorFimEndpointHint")}
              </FieldDescription>
            </Field>

            <div className="grid grid-cols-2 gap-2">
              <Field className="gap-1">
                <FieldLabel htmlFor="orbit-fim-model" className="text-[11px]">
                  {t("sidePanel.editorFimModelLabel")}
                </FieldLabel>
                <Input
                  id="orbit-fim-model"
                  type="text"
                  value={draft.model}
                  disabled={saving}
                  onChange={(e) => {
                    setDraft((prev) => ({ ...prev, model: e.target.value }));
                    setErrorMessage(null);
                  }}
                  placeholder="qwen2.5-coder:1.5b"
                  className="h-[28px] px-2 font-mono text-[11.5px]"
                />
              </Field>

              <Field className="gap-1">
                <FieldLabel htmlFor="orbit-fim-api-key-env" className="text-[11px]">
                  {t("sidePanel.editorFimApiKeyEnvLabel")}
                </FieldLabel>
                <Input
                  id="orbit-fim-api-key-env"
                  type="text"
                  value={draft.apiKeyEnv}
                  disabled={saving}
                  onChange={(e) => {
                    setDraft((prev) => ({ ...prev, apiKeyEnv: e.target.value.toUpperCase() }));
                    setErrorMessage(null);
                  }}
                  placeholder="ORBIT_FIM_API_KEY"
                  className="h-[28px] px-2 font-mono text-[11.5px]"
                />
              </Field>
            </div>
            <p className="text-ink-tertiary -mt-1.5 text-[10.5px] leading-snug">
              {t("sidePanel.editorFimApiKeyEnvHint")}
            </p>

            {/* Collapsible Advanced Tuning (Max Tokens, Timeout, Fast CLI Fallback) */}
            <div className="flex flex-col gap-2">
              <button
                type="button"
                onClick={() => setShowAdvanced((v) => !v)}
                className="text-ink-tertiary hover:text-ink-primary inline-flex w-fit cursor-pointer items-center gap-1 text-[11px] font-medium transition-colors"
              >
                {showAdvanced ? (
                  <ChevronDown className="size-[11px]" />
                ) : (
                  <ChevronRight className="size-[11px]" />
                )}
                <span>{t("sidePanel.editorFimAdvancedToggle")}</span>
              </button>

              {showAdvanced && (
                <div className="border-border-subtle bg-surface-recessed/70 flex flex-col gap-2.5 rounded-lg border p-2.5">
                  <div className="grid grid-cols-2 gap-2">
                    <Field className="gap-1">
                      <FieldLabel htmlFor="orbit-fim-max-tokens" className="text-[11px]">
                        {t("sidePanel.editorFimMaxTokensLabel")}
                      </FieldLabel>
                      <Input
                        id="orbit-fim-max-tokens"
                        type="number"
                        min={1}
                        max={2048}
                        value={draft.maxTokens}
                        disabled={saving}
                        onChange={(e) =>
                          setDraft((prev) => ({ ...prev, maxTokens: e.target.value }))
                        }
                        className="tabular-nums h-[26px] px-2 font-mono text-[11.5px]"
                      />
                    </Field>
                    <Field className="gap-1">
                      <FieldLabel htmlFor="orbit-fim-timeout" className="text-[11px]">
                        {t("sidePanel.editorFimTimeoutLabel")}
                      </FieldLabel>
                      <Input
                        id="orbit-fim-timeout"
                        type="number"
                        min={200}
                        max={5000}
                        step={100}
                        value={draft.timeoutMs}
                        disabled={saving}
                        onChange={(e) =>
                          setDraft((prev) => ({ ...prev, timeoutMs: e.target.value }))
                        }
                        className="tabular-nums h-[26px] px-2 font-mono text-[11.5px]"
                      />
                    </Field>
                  </div>

                  <Field orientation="horizontal" className="items-center gap-2">
                    <FieldContent>
                      <FieldTitle className="text-[11.5px]">
                        {t("sidePanel.editorFimFallbackLabel")}
                      </FieldTitle>
                      <FieldDescription className="text-[10.5px]">
                        {t("sidePanel.editorFimFallbackHint")}
                      </FieldDescription>
                    </FieldContent>
                    <Switch
                      checked={draft.allowFastCliFallback}
                      disabled={saving}
                      onCheckedChange={(checked) => {
                        setDraft((prev) => ({ ...prev, allowFastCliFallback: checked }));
                        setErrorMessage(null);
                      }}
                    />
                  </Field>
                </div>
              )}
            </div>
          </FieldGroup>

          {errorMessage && (
            <div className="text-signal-danger text-[11px] leading-snug">{errorMessage}</div>
          )}

          {/* Footer: Shortcut Legend + Save */}
          <div className="border-border-subtle flex items-center justify-between gap-2 border-t pt-2.5">
            <div className="text-ink-faint flex flex-wrap items-center gap-x-2 gap-y-1 text-[10.5px]">
              <span className="inline-flex items-center gap-1">
                <ShortcutKbd keys="Tab" size="xs" variant="subtle" />
                <span>{t("sidePanel.editorFimKeyAccept")}</span>
              </span>
              <span className="inline-flex items-center gap-1">
                <ShortcutKbd keys={`${mod}→`} size="xs" variant="subtle" />
                <span>{t("sidePanel.editorFimKeyWord")}</span>
              </span>
              <span className="inline-flex items-center gap-1">
                <ShortcutKbd keys="Esc" size="xs" variant="subtle" />
                <span>{t("sidePanel.editorFimKeyDismiss")}</span>
              </span>
            </div>

            <Button
              type="button"
              size="xs"
              disabled={saving}
              onClick={() => void persistDraft(draft)}
              className="h-[24px] shrink-0 gap-1 px-2.5 text-[11.5px]"
            >
              {saving ? (
                <OrbitSpinner size={11} />
              ) : savedNotice ? (
                <Check className="size-[11px]" />
              ) : null}
              <span>{savedNotice ? t("sidePanel.editorFimSaved") : t("sidePanel.editorFimSave")}</span>
            </Button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
});
