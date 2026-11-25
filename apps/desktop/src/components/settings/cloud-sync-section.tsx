import { Fragment, memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  AlertTriangle,
  Check,
  ChevronRight,
  Cloud,
  Copy,
  ExternalLink,
  KeyRound,
  Link2,
  QrCode,
  RefreshCw,
  ShieldCheck,
} from "lucide-react";
import {
  SettingCard,
  SettingGroup,
  SettingRow,
  SettingSectionHeader,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Field, FieldDescription, FieldError, FieldGroup } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ShortcutKbd } from "@/components/ui/kbd";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Segmented } from "@/components/ui/segmented";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import { orbitIpc } from "@/lib/orbit-ipc";
import { cn } from "@/lib/utils";
import { formatDeviceFlowCountdown } from "@/state/cloud-sync-device-poll";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import type { CloudSyncDeviceFlowState } from "@/state/store-types";

const SHORTCUT_SENTINEL = "__ORBIT_PASTE_SHORTCUT__";

function GithubMark({ className, ...props }: React.ComponentProps<"svg">) {
  return (
    <svg
      viewBox="0 0 16 16"
      fill="currentColor"
      aria-hidden="true"
      className={cn("size-[15px] shrink-0", className)}
      {...props}
    >
      <path d="M8 0C3.58 0 0 3.58 0 8c0 3.54 2.29 6.53 5.47 7.59.4.07.55-.17.55-.38 0-.19-.01-.82-.01-1.49-2.01.37-2.53-.49-2.69-.94-.09-.23-.48-.94-.82-1.13-.28-.15-.68-.52-.01-.53.63-.01 1.08.58 1.23.82.72 1.21 1.87.87 2.33.66.07-.52.28-.87.51-1.07-1.78-.2-3.64-.89-3.64-3.95 0-.87.31-1.59.82-2.15-.08-.2-.36-1.02.08-2.12 0 0 .67-.21 2.2.82.64-.18 1.32-.27 2-.27s1.36.09 2 .27c1.53-1.04 2.2-.82 2.2-.82.44 1.1.16 1.92.08 2.12.51.56.82 1.27.82 2.15 0 3.07-1.87 3.75-3.65 3.95.29.25.54.73.54 1.48 0 1.07-.01 1.93-.01 2.2 0 .21.15.46.55.38A8.01 8.01 0 0 0 16 8c0-4.42-3.58-8-8-8Z" />
    </svg>
  );
}

function renderShortcutHint(template: string, shortcutKeys: string) {
  const parts = template.split(SHORTCUT_SENTINEL);
  if (parts.length === 1) return template;
  return parts.map((part, idx) => (
    <Fragment key={idx}>
      {part}
      {idx < parts.length - 1 && (
        <ShortcutKbd keys={shortcutKeys} size="xs" className="mx-1 align-middle" />
      )}
    </Fragment>
  ));
}

export const CloudSyncSection = memo(function CloudSyncSection() {
  const { t, locale } = useI18n();
  const status = useOrbitSelector((s) => s.cloudSyncStatus);
  const busy = useOrbitSelector((s) => s.cloudSyncBusy);
  const storeError = useOrbitSelector((s) => s.cloudSyncError);
  const deviceFlow = useOrbitSelector((s) => s.cloudSyncDeviceFlow);
  const lastResult = useOrbitSelector((s) => s.cloudSyncLastResult);
  const allRemoteHostsLackReachability = useOrbitSelector((s) => {
    const hosts = s.fleet.hosts;
    if (hosts.length <= 1) return false;
    return !hosts.some(
      (h) => Boolean((h.relay_url ?? "").trim()) || Boolean((h.ssh_alias ?? "").trim()),
    );
  });

  const [clientIdInput, setClientIdInput] = useState("");
  const syncedClientIdRef = useRef<string | null>(null);
  const [deviceLinkOpen, setDeviceLinkOpen] = useState(false);
  const [deviceLinkInput, setDeviceLinkInput] = useState("");
  const [passphraseInput, setPassphraseInput] = useState("");
  const [confirmPassphraseInput, setConfirmPassphraseInput] = useState("");
  const [generatedKey, setGeneratedKey] = useState<string | null>(null);
  const [copiedFlag, setCopiedFlag] = useState<"code" | "key" | "link" | "manual" | null>(null);
  const copiedTimerRef = useRef<number | null>(null);
  const [manualCodeCopiedFor, setManualCodeCopiedFor] = useState<string | null>(null);
  const [pendingAction, setPendingAction] = useState<"github" | "link" | "unlock" | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const [clearHostsOnLogout, setClearHostsOnLogout] = useState(false);
  const [confirmSignOutOpen, setConfirmSignOutOpen] = useState(false);
  const [manualDialogOpen, setManualDialogOpen] = useState(false);
  const [manualMode, setManualMode] = useState<"generate" | "custom">("generate");
  const [manualGeneratedPass, setManualGeneratedPass] = useState("");
  const [manualSavedAck, setManualSavedAck] = useState(false);
  const [manualCustomPass, setManualCustomPass] = useState("");
  const [manualCustomConfirm, setManualCustomConfirm] = useState("");
  const [manualDialogError, setManualDialogError] = useState<string | null>(null);
  const [manualSubmitting, setManualSubmitting] = useState(false);
  const [linkQrOpen, setLinkQrOpen] = useState(false);
  const [linkQrUri, setLinkQrUri] = useState<string | null>(null);
  const [linkQrSvg, setLinkQrSvg] = useState<string | null>(null);
  const [linkQrLoading, setLinkQrLoading] = useState(false);
  const [advancedSecOpen, setAdvancedSecOpen] = useState(false);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const ime = useImeCompositionGuard();
  const pasteShortcut = `${modSymbol()}V`;

  useEffect(() => {
    void orbitActions.refreshCloudSyncStatus();
  }, []);

  useEffect(() => {
    return () => {
      if (copiedTimerRef.current !== null) {
        window.clearTimeout(copiedTimerRef.current);
      }
    };
  }, []);

  useEffect(() => {
    const override = status?.github_client_id_override ?? null;
    if (override && syncedClientIdRef.current !== override) {
      syncedClientIdRef.current = override;
      setClientIdInput((prev) => prev || override);
    }
  }, [status?.github_client_id_override]);

  useEffect(() => {
    if (!deviceFlow) return;
    setNowMs(Date.now());
    const timer = window.setInterval(() => {
      setNowMs(Date.now());
    }, 1000);
    return () => window.clearInterval(timer);
  }, [deviceFlow?.device_code, deviceFlow?.expiresAtMs]);

  useEffect(() => {
    if (!busy) {
      setPendingAction(null);
    }
  }, [busy]);

  useEffect(() => {
    if (!linkQrOpen) {
      setLinkQrUri(null);
      setLinkQrSvg(null);
      return;
    }
    let active = true;
    setLinkQrLoading(true);
    setLocalError(null);
    void (async () => {
      try {
        const uri = await orbitIpc.cloudSyncExportDeviceLinkUri();
        const svg = await orbitIpc.renderQrSvg(uri);
        if (!active) return;
        setLinkQrUri(uri);
        setLinkQrSvg(svg);
      } catch (err) {
        if (!active) return;
        setLinkQrUri(null);
        setLinkQrSvg(null);
        setLocalError(String(err));
      } finally {
        if (active) setLinkQrLoading(false);
      }
    })();
    return () => {
      active = false;
    };
  }, [linkQrOpen]);

  const linkQrDataUri = useMemo(() => {
    if (!linkQrSvg) return null;
    return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(linkQrSvg)}`;
  }, [linkQrSvg]);

  const state = status?.state ?? "signed_out";
  const unlocked = state === "ready" || state === "syncing" || state === "error";
  const needsCustomClientId = status?.github_oauth_configured === false;
  const canStartGithubLogin = !needsCustomClientId || Boolean(clientIdInput.trim());
  const lastSynced = status?.last_synced_at_ms
    ? new Date(status.last_synced_at_ms).toLocaleString(locale)
    : t("settings.neverSynced");
  const displayError = localError ?? storeError ?? status?.last_error ?? null;
  const isAuthExpiredError = Boolean(
    displayError && /401|bad credentials|expired|revoked|unauthorized/i.test(displayError),
  );

  const trimmedPass = passphraseInput.trim();
  const trimmedConfirm = confirmPassphraseInput.trim();
  const isCreatingVault = !status?.remote_vault_exists;
  const usingCustomNewPassphrase = isCreatingVault && !generatedKey;
  const passTooShort = usingCustomNewPassphrase && trimmedPass.length > 0 && trimmedPass.length < 6;
  const passNeedsConfirm =
    usingCustomNewPassphrase && trimmedPass.length >= 6 && trimmedConfirm.length === 0;
  const passMismatch =
    usingCustomNewPassphrase &&
    trimmedPass.length >= 6 &&
    trimmedConfirm.length > 0 &&
    trimmedConfirm !== trimmedPass;
  const canSubmitVault = !status
    ? false
    : status.remote_vault_exists || Boolean(generatedKey)
      ? trimmedPass.length > 0
      : trimmedPass.length >= 6 && trimmedConfirm === trimmedPass;

  const markCopied = async (which: "code" | "key" | "link" | "manual", text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch (err) {
      if (which === "manual") {
        setManualDialogError(String(err));
      } else {
        setLocalError(String(err));
      }
      return false;
    }
    if (copiedTimerRef.current !== null) {
      window.clearTimeout(copiedTimerRef.current);
    }
    setCopiedFlag(which);
    copiedTimerRef.current = window.setTimeout(() => {
      copiedTimerRef.current = null;
      setCopiedFlag((cur) => (cur === which ? null : cur));
    }, 1800);
    return true;
  };

  const generateManualPassphrase = async () => {
    setManualDialogError(null);
    setManualSavedAck(false);
    try {
      const rec = await orbitIpc.cloudSyncGeneratePassphrase();
      setManualGeneratedPass(rec);
    } catch (err) {
      setManualDialogError(String(err));
    }
  };

  const openManualPassphraseDialog = () => {
    setManualMode("generate");
    setManualGeneratedPass("");
    setManualSavedAck(false);
    setManualCustomPass("");
    setManualCustomConfirm("");
    setManualDialogError(null);
    setManualDialogOpen(true);
    void generateManualPassphrase();
  };

  const trimmedManualGenerated = manualGeneratedPass.trim();
  const trimmedManualCustom = manualCustomPass.trim();
  const trimmedManualConfirm = manualCustomConfirm.trim();
  const manualCustomNeedsConfirm =
    trimmedManualCustom.length > 0 && trimmedManualConfirm.length === 0;
  const manualCustomMismatch =
    trimmedManualCustom.length > 0 &&
    trimmedManualConfirm.length > 0 &&
    trimmedManualConfirm !== trimmedManualCustom;
  const manualCanConfirm =
    !manualSubmitting &&
    !busy &&
    (manualMode === "generate"
      ? trimmedManualGenerated.length > 0 && manualSavedAck
      : trimmedManualCustom.length > 0 && trimmedManualConfirm === trimmedManualCustom);

  const handleConfirmEnableManual = async () => {
    if (!manualCanConfirm) return;
    const targetPass = manualMode === "generate" ? trimmedManualGenerated : trimmedManualCustom;
    setManualDialogError(null);
    setManualSubmitting(true);
    try {
      const res = await orbitActions.enableCloudSyncManualPassphrase(targetPass);
      if (res.ok) {
        setManualDialogOpen(false);
        setManualGeneratedPass("");
        setManualSavedAck(false);
        setManualCustomPass("");
        setManualCustomConfirm("");
      } else {
        setManualDialogError(res.error ?? t("settings.cloudSyncManualEnableFailed"));
      }
    } finally {
      setManualSubmitting(false);
    }
  };

  const handleCopyDeviceCode = async (flow: CloudSyncDeviceFlowState) => {
    const ok = await markCopied("code", flow.user_code);
    if (ok) {
      setManualCodeCopiedFor(flow.device_code);
    }
  };

  const handleStartDeviceLogin = async () => {
    setLocalError(null);
    const trimmedCid = clientIdInput.trim();
    if (needsCustomClientId && !trimmedCid) {
      setLocalError(t("settings.cloudClientHint"));
      return;
    }
    setPendingAction("github");
    await orbitActions.startGithubCloudLogin(needsCustomClientId ? trimmedCid : null);
  };

  const handleDeviceLinkLogin = async () => {
    setLocalError(null);
    if (!deviceLinkInput.trim()) return;
    setPendingAction("link");
    const ok = await orbitActions.importCloudSyncDeviceLinkUri(deviceLinkInput);
    if (ok) {
      setDeviceLinkInput("");
      setDeviceLinkOpen(false);
    }
  };

  const handleGenerateKey = async () => {
    setLocalError(null);
    try {
      const rec = await orbitIpc.cloudSyncGeneratePassphrase();
      setGeneratedKey(rec);
      setPassphraseInput(rec);
      setConfirmPassphraseInput(rec);
    } catch (err) {
      setLocalError(String(err));
    }
  };

  const handleUnlock = async () => {
    setLocalError(null);
    if (!canSubmitVault) return;
    setPendingAction("unlock");
    const ok = await orbitActions.unlockCloudSyncWithPassphrase(trimmedPass);
    if (ok) {
      setPassphraseInput("");
      setConfirmPassphraseInput("");
      setGeneratedKey(null);
    }
  };

  const stateLabel = {
    signed_out: t("settings.cloudSignedOut"),
    locked: t(status?.remote_vault_exists ? "settings.cloudLocked" : "settings.cloudCreateVault"),
    ready: t("settings.cloudReady"),
    syncing: t("common.syncing"),
    error: t("settings.cloudError"),
  }[state];
  const vaultUrl = status?.gist_html_url;
  const startingGithub = busy && pendingAction === "github";
  const linkingDevice = busy && pendingAction === "link";
  const unlockingVault = busy && pendingAction === "unlock";

  const remainingMs = deviceFlow ? Math.max(0, deviceFlow.expiresAtMs - nowMs) : 0;
  const countdownLabel = formatDeviceFlowCountdown(remainingMs);
  const codeCopiedReady = Boolean(
    deviceFlow &&
      (deviceFlow.clipboardCopied ||
        manualCodeCopiedFor === deviceFlow.device_code ||
        copiedFlag === "code"),
  );

  const renderDeviceFlowCard = (flow: CloudSyncDeviceFlowState, withPrimaryAnchors: boolean) => (
    <SettingCard settingId={withPrimaryAnchors ? "settings.cloudSyncTitle" : undefined}>
      <div
        data-setting-id={withPrimaryAnchors ? "settings.signInGithub" : undefined}
        className="flex flex-wrap items-center justify-between gap-2 px-4 py-3"
      >
        <div className="flex min-w-0 items-center gap-2">
          <OrbitSpinner size={14} className="text-ink-primary" />
          <span className="text-ink-primary text-[13px] font-medium">
            {t("settings.cloudWaitingAuth")}
          </span>
        </div>
        <span
          className={cn(
            "font-mono text-[11px] tabular-nums",
            remainingMs < 60_000 ? "text-signal-warning font-medium" : "text-ink-muted",
          )}
        >
          {t("settings.cloudCodeExpiresIn", { time: countdownLabel })}
        </span>
      </div>

      <div className="flex flex-col gap-3 p-4">
        <div className="border-border-subtle bg-surface-recessed flex flex-col items-center gap-2 rounded-[10px] border px-4 py-4 text-center">
          <code className="select-text text-ink-primary font-mono text-[24px] leading-none font-semibold tracking-[0.16em] tabular-nums">
            {flow.user_code}
          </code>
          <p className="text-ink-secondary flex flex-wrap items-center justify-center gap-2 text-[12px] leading-snug text-pretty">
            {codeCopiedReady ? (
              <>
                <Check className="text-signal-success size-[13px] shrink-0" />
                <span>
                  {renderShortcutHint(
                    t("settings.cloudCodeCopiedPasteHint", { shortcut: SHORTCUT_SENTINEL }),
                    pasteShortcut,
                  )}
                </span>
              </>
            ) : (
              <>
                <Copy className="text-ink-muted size-[13px] shrink-0" />
                <span>
                  {renderShortcutHint(
                    t("settings.cloudCodeCopyManualHint", { shortcut: SHORTCUT_SENTINEL }),
                    pasteShortcut,
                  )}
                </span>
              </>
            )}
          </p>
        </div>

        <div className="flex flex-wrap items-center justify-between gap-2">
          <div className="flex flex-wrap items-center gap-2">
            <Button
              type="button"
              size="sm"
              variant={codeCopiedReady ? "outline" : "default"}
              onClick={() => void handleCopyDeviceCode(flow)}
            >
              {copiedFlag === "code" ? (
                <Check data-icon="inline-start" className="text-signal-success" />
              ) : (
                <Copy data-icon="inline-start" />
              )}
              {copiedFlag === "code" ? t("common.copied") : t("settings.cloudCopyCode")}
            </Button>
            <Button
              type="button"
              size="sm"
              variant="outline"
              onClick={() =>
                void orbitIpc
                  .openExternalBrowserUrl(flow.verification_uri)
                  .catch((err) => setLocalError(String(err)))
              }
            >
              <ExternalLink data-icon="inline-start" />
              {t("settings.cloudReopenBrowser")}
            </Button>
          </div>
          <Button
            type="button"
            size="sm"
            variant="ghost"
            onClick={() => orbitActions.cancelGithubCloudLogin()}
          >
            {t("common.cancel")}
          </Button>
        </div>
      </div>
    </SettingCard>
  );

  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("settings.cloudSyncTitle")}
        description={t("settings.cloudSyncDesc")}
      />

      {state === "signed_out" ? (
        <div data-setting-fallback>
          {deviceFlow ? (
            renderDeviceFlowCard(deviceFlow, true)
          ) : (
            <SettingCard settingId="settings.cloudSyncTitle">
              <div
                data-setting-id="settings.signInGithub"
                className="flex flex-col items-start gap-4 p-4"
              >
                <div className="flex flex-col gap-2">
                  <div className="inline-flex flex-wrap items-center gap-2">
                    <Cloud className="text-ink-secondary size-[16px]" />
                    <span className="text-ink-primary text-[13px] font-medium">GitHub</span>
                    <Badge variant="secondary">{stateLabel}</Badge>
                  </div>
                  <p className="text-ink-secondary text-[13px] leading-[1.5] text-pretty">
                    {t("settings.cloudSignedOutSummary")}
                  </p>
                </div>

                {needsCustomClientId ? (
                  <form
                    className="flex w-full flex-col items-start gap-3"
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && ime.shouldIgnoreKeyDown(e)) e.preventDefault();
                    }}
                    onSubmit={(e) => {
                      e.preventDefault();
                      if (!busy && canStartGithubLogin) void handleStartDeviceLogin();
                    }}
                  >
                    <Field>
                      <div className="flex flex-wrap items-center justify-between gap-2">
                        <FieldDescription className="text-ink-secondary text-[12px]">
                          {t("settings.cloudClientHint")}
                        </FieldDescription>
                        <Button
                          type="button"
                          size="xs"
                          variant="link"
                          className="h-auto p-0"
                          onClick={() =>
                            void orbitIpc.openExternalBrowserUrl(
                              "https://github.com/settings/developers",
                            )
                          }
                        >
                          {t("settings.cloudDeveloperSettings")}
                          <ExternalLink data-icon="inline-end" />
                        </Button>
                      </div>
                      <Input
                        aria-label={t("settings.cloudGithubClientIdAria")}
                        placeholder="Ov23li…"
                        disabled={busy}
                        value={clientIdInput}
                        onChange={(e) => setClientIdInput(e.target.value)}
                      />
                    </Field>
                    <Button type="submit" disabled={busy || !canStartGithubLogin}>
                      {startingGithub ? (
                        <OrbitSpinner size={14} />
                      ) : (
                        <GithubMark data-icon="inline-start" />
                      )}
                      {startingGithub ? t("settings.cloudSigningIn") : t("settings.signInGithub")}
                    </Button>
                  </form>
                ) : (
                  <Button
                    type="button"
                    disabled={busy}
                    onClick={() => void handleStartDeviceLogin()}
                  >
                    {startingGithub ? (
                      <OrbitSpinner size={14} />
                    ) : (
                      <GithubMark data-icon="inline-start" />
                    )}
                    {startingGithub ? t("settings.cloudSigningIn") : t("settings.signInGithub")}
                  </Button>
                )}
              </div>

              <Collapsible
                open={deviceLinkOpen}
                onOpenChange={(next) => {
                  setDeviceLinkOpen(next);
                  if (next) setLocalError(null);
                }}
              >
                <CollapsibleTrigger className="group/link text-ink-secondary hover:bg-surface-recessed/50 hover:text-ink-primary focus-visible:bg-surface-recessed/60 flex w-full cursor-pointer items-center gap-2 px-4 py-3 text-left text-[13px] transition-colors outline-none">
                  <Link2 className="text-ink-muted size-[14px] shrink-0" />
                  <span className="flex-1 font-medium">
                    {t("settings.importFromSignedInDevice")}
                  </span>
                  <ChevronRight className="text-ink-muted size-[13px] shrink-0 transition-transform group-data-[panel-open]/link:rotate-90" />
                </CollapsibleTrigger>
                <CollapsibleContent>
                  <form
                    className="flex flex-col gap-3 px-4 pt-1 pb-4"
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      if (e.key === "Enter" && ime.shouldIgnoreKeyDown(e)) e.preventDefault();
                    }}
                    onSubmit={(e) => {
                      e.preventDefault();
                      if (!busy && deviceLinkInput.trim()) void handleDeviceLinkLogin();
                    }}
                  >
                    <Field>
                      <FieldDescription className="text-ink-secondary text-[12px] leading-[1.45]">
                        {t("settings.deviceLinkTabDesc")}
                      </FieldDescription>
                      <div className="flex items-center gap-2">
                        <Input
                          type="password"
                          autoComplete="off"
                          disabled={busy}
                          aria-label={t("settings.importFromSignedInDevice")}
                          value={deviceLinkInput}
                          onChange={(e) => setDeviceLinkInput(e.target.value)}
                          placeholder={t("settings.deviceLinkPlaceholder")}
                          className="min-w-0 flex-1 font-mono"
                        />
                        <Button
                          type="submit"
                          variant="outline"
                          size="sm"
                          disabled={busy || !deviceLinkInput.trim()}
                        >
                          {linkingDevice && <OrbitSpinner size={13} />}
                          {t("settings.deviceLinkConnect")}
                        </Button>
                      </div>
                    </Field>
                  </form>
                </CollapsibleContent>
              </Collapsible>
            </SettingCard>
          )}
        </div>
      ) : (
        <div data-setting-fallback>
          <SettingCard settingId="settings.cloudSyncTitle">
            <SettingRow
              title={
                <span className="inline-flex flex-wrap items-center gap-2">
                  <Cloud className="text-ink-secondary size-[16px]" />
                  {status?.account_login ? `@${status.account_login}` : "GitHub"}
                  <Badge
                    variant={
                      state === "ready" ? "success" : state === "error" ? "destructive" : "secondary"
                    }
                  >
                    {stateLabel}
                  </Badge>
                </span>
              }
              description={
                unlocked && status
                  ? t("settings.cloudAccountSummary", {
                      hosts: status.synced_host_count,
                      projects: status.synced_project_count,
                      time: lastSynced,
                    })
                  : t("settings.cloudLockedDesc")
              }
            >
              {unlocked && (
                <Button size="sm" disabled={busy} onClick={() => void orbitActions.syncCloudNow()}>
                  <RefreshCw
                    data-icon="inline-start"
                    className={cn(busy && "animate-spin motion-reduce:animate-none")}
                  />
                  {busy ? t("common.syncing") : t("settings.cloudSyncNow")}
                </Button>
              )}
            </SettingRow>
            {unlocked && lastResult && (
              <p role="status" className="text-ink-muted px-4 py-2 text-[11px]">
                {t("settings.cloudLastRun", {
                  added: lastResult.hosts_added,
                  updated: lastResult.hosts_updated,
                  projects: lastResult.projects_added,
                })}
              </p>
            )}
          </SettingCard>
        </div>
      )}

      {displayError && (
        <div
          role="alert"
          className="border-signal-danger-border bg-signal-danger-bg text-signal-danger flex flex-wrap items-center gap-2 rounded-xl border p-3 text-[13px]"
        >
          <AlertCircle className="size-[16px] shrink-0" />
          <span className="min-w-0 flex-1 break-words">{displayError}</span>
          {(state === "error" || isAuthExpiredError) && !deviceFlow && (
            <Button
              variant="outline"
              size="sm"
              disabled={busy || !canStartGithubLogin}
              onClick={() => void handleStartDeviceLogin()}
            >
              {t("settings.signInGithub")}
            </Button>
          )}
        </div>
      )}

      {deviceFlow && state !== "signed_out" && (
        <SettingGroup title={t("settings.cloudVerification")}>
          {renderDeviceFlowCard(deviceFlow, false)}
        </SettingGroup>
      )}

      {state === "locked" && status && (
        <SettingGroup
          title={t(
            status.remote_vault_exists ? "settings.cloudUnlock" : "settings.cloudCreateVault",
          )}
        >
          <SettingCard>
            <form
              className="flex flex-col gap-3 p-4"
              onSubmit={(e) => {
                e.preventDefault();
                if (!busy && canSubmitVault) void handleUnlock();
              }}
              {...ime.compositionProps}
              onKeyDown={(e) => {
                if (e.key === "Enter" && ime.shouldIgnoreKeyDown(e)) e.preventDefault();
              }}
            >
              {!status.remote_vault_exists && (
                <div className="flex flex-wrap items-center gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    type="button"
                    disabled={busy}
                    onClick={() => void handleGenerateKey()}
                  >
                    {t("settings.cloudGenerateKey")}
                  </Button>
                  {generatedKey && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      disabled={busy}
                      onClick={() => void markCopied("key", generatedKey)}
                    >
                      {t(copiedFlag === "key" ? "common.copied" : "common.copy")}
                    </Button>
                  )}
                </div>
              )}
              {generatedKey && (
                <p className="text-ink-secondary text-[13px]">{t("settings.cloudStoreKey")}</p>
              )}
              <FieldGroup className="gap-3">
                <Field>
                  <Input
                    type={generatedKey ? "text" : "password"}
                    autoComplete="off"
                    disabled={busy}
                    aria-label={t("settings.cloudEnterPass")}
                    placeholder={t("settings.cloudEnterPass")}
                    value={passphraseInput}
                    onChange={(e) => {
                      setPassphraseInput(e.target.value);
                      if (generatedKey && e.target.value !== generatedKey) setGeneratedKey(null);
                    }}
                  />
                  {passTooShort && <FieldDescription>{t("settings.cloudPassLength")}</FieldDescription>}
                </Field>
                {!status.remote_vault_exists && !generatedKey && (
                  <Field data-invalid={passMismatch || undefined}>
                    <Input
                      type="password"
                      autoComplete="new-password"
                      disabled={busy}
                      aria-invalid={passMismatch || undefined}
                      aria-label={t("settings.cloudConfirmPass")}
                      placeholder={t("settings.cloudConfirmPass")}
                      value={confirmPassphraseInput}
                      onChange={(e) => setConfirmPassphraseInput(e.target.value)}
                    />
                    {passNeedsConfirm && (
                      <FieldDescription>{t("settings.cloudConfirmPassHint")}</FieldDescription>
                    )}
                    {passMismatch && <FieldError>{t("settings.cloudPassMismatch")}</FieldError>}
                  </Field>
                )}
              </FieldGroup>
              <Button className="self-start" type="submit" disabled={busy || !canSubmitVault}>
                {unlockingVault && <OrbitSpinner size={14} />}
                {t(
                  status.remote_vault_exists ? "settings.cloudUnlock" : "settings.cloudCreateVault",
                )}
              </Button>
            </form>
          </SettingCard>
        </SettingGroup>
      )}

      {unlocked && status && (
        <>
          <SettingCard>
            <SettingRow
              settingId="settings.cloudAutoSync"
              title={t("settings.cloudAutoSync")}
              description={t("settings.cloudAutoSyncDesc")}
            >
              <ToggleSwitch
                checked={status.auto_sync_enabled}
                disabled={busy}
                ariaLabel={t("settings.cloudAutoSync")}
                onChange={(enabled) => void orbitActions.setCloudAutoSync(enabled)}
              />
            </SettingRow>
            <SettingRow
              settingId="settings.linkDeviceQr"
              title={t("settings.deviceLinkQrTitle")}
              description={t("settings.deviceLinkQrDesc")}
            >
              <Button variant="outline" size="sm" disabled={busy} onClick={() => setLinkQrOpen(true)}>
                <QrCode data-icon="inline-start" />
                {t("settings.linkDeviceQr")}
              </Button>
            </SettingRow>
            <SettingRow
              settingId="settings.fleetNetworkTitle"
              title={t("settings.fleetNetworkTitle")}
              description={t(
                allRemoteHostsLackReachability
                  ? "settings.fleetNetworkMissingReachability"
                  : "settings.fleetNetworkAutoDesc",
              )}
            >
              <Button
                size="sm"
                variant="ghost"
                onClick={() => orbitActions.setSettingsSection("hosts")}
              >
                {t("settings.nav4Hosts")}
              </Button>
            </SettingRow>
          </SettingCard>
          <Collapsible
            data-setting-id="settings.cloudSyncAdvancedTitle"
            open={advancedSecOpen}
            onOpenChange={setAdvancedSecOpen}
            className="border-border-subtle bg-surface-raised rounded-xl border"
          >
            <CollapsibleTrigger className="group/sec flex w-full cursor-pointer items-center gap-2 px-4 py-3 text-left text-[13px]">
              <ChevronRight className="text-ink-muted size-[13px] transition-transform group-data-[panel-open]/sec:rotate-90" />
              <span className="flex-1 font-medium">{t("settings.cloudSyncAdvancedTitle")}</span>
              <Badge variant="secondary">
                {t(
                  status.instant_unlock_enabled
                    ? "settings.cloudSyncInstantBadge"
                    : "settings.cloudSyncManualBadge",
                )}
              </Badge>
            </CollapsibleTrigger>
            <CollapsibleContent className="divide-border-subtle divide-y transition-opacity">
              <SettingRow
                title={t("settings.cloudSyncRequireManualTitle")}
                description={t("settings.cloudSyncRequireManualDesc")}
              >
                <ToggleSwitch
                  checked={!status.instant_unlock_enabled}
                  disabled={busy || manualSubmitting}
                  ariaLabel={t("settings.cloudSyncRequireManualTitle")}
                  onChange={(manual) => {
                    if (manual) {
                      openManualPassphraseDialog();
                    } else {
                      void orbitActions.setCloudSyncManualPassphraseMode(false);
                    }
                  }}
                />
              </SettingRow>
              {vaultUrl && (
                <SettingRow
                  title={t("settings.cloudRemoteVault")}
                  description={t("settings.cloudSyncDesc")}
                >
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={() => void orbitIpc.openExternalBrowserUrl(vaultUrl)}
                  >
                    GitHub <ExternalLink data-icon="inline-end" />
                  </Button>
                </SettingRow>
              )}
              <div
                data-setting-id="settings.cloudHeadless"
                className="flex flex-wrap items-center justify-between gap-x-4 gap-y-2 px-4 py-3"
              >
                <div className="min-w-[140px] flex-1">
                  <div className="text-ink-primary text-[13px] font-medium">
                    {t("settings.cloudHeadless")}
                  </div>
                  <div className="text-ink-secondary mt-1 text-[11px] leading-[1.45]">
                    {t("settings.cloudHeadlessDesc")}
                  </div>
                </div>
                <code className="border-border-subtle bg-surface-recessed text-ink-primary rounded-md border px-2 py-1 font-mono text-[11px] select-text">
                  orbit login
                </code>
              </div>
            </CollapsibleContent>
          </Collapsible>
        </>
      )}

      {(unlocked || state === "locked") && (
        <div className="flex justify-end">
          <Button
            size="sm"
            variant="ghost"
            disabled={busy}
            onClick={() => {
              setClearHostsOnLogout(false);
              setConfirmSignOutOpen(true);
            }}
          >
            {t("settings.cloudSignOut")}
          </Button>
        </div>
      )}
      <ConfirmDialog
        open={confirmSignOutOpen}
        onOpenChange={setConfirmSignOutOpen}
        title={t("settings.cloudSignOutTitle")}
        description={t("settings.cloudSignOutDesc")}
        detail={
          <label
            className={cn(
              "text-ink-secondary flex items-center gap-2 text-[13px]",
              busy ? "cursor-not-allowed opacity-60" : "cursor-pointer",
            )}
          >
            <Checkbox
              size="sm"
              checked={clearHostsOnLogout}
              disabled={busy}
              onCheckedChange={setClearHostsOnLogout}
            />
            {t("settings.cloudClearHosts")}
          </label>
        }
        defaultAction="cancel"
        confirmLabel={t(
          clearHostsOnLogout ? "settings.cloudSignOutAndClear" : "settings.cloudSignOut",
        )}
        tone={clearHostsOnLogout ? "danger" : "default"}
        onConfirm={() => orbitActions.logoutCloudSync(clearHostsOnLogout)}
      />
      <Dialog open={linkQrOpen} onOpenChange={setLinkQrOpen}>
        <DialogContent className="max-w-[380px] gap-5 p-5">
          <DialogHeader className="gap-1 pr-6">
            <DialogTitle className="flex items-center gap-2 text-[17px]">
              <QrCode className="text-ink-muted h-[16px] w-[16px] stroke-[1.75]" />
              <span>{t("settings.deviceLinkQrTitle")}</span>
            </DialogTitle>
            <DialogDescription className="text-[13px] leading-relaxed">
              {t("settings.deviceLinkQrDesc")}
            </DialogDescription>
          </DialogHeader>

          <div className="flex flex-col items-center gap-3">
            <div className="border-border-subtle bg-surface-recessed flex size-[208px] items-center justify-center rounded-[14px] border p-3">
              {linkQrLoading ? (
                <OrbitSpinner size={20} className="text-ink-muted size-5" />
              ) : linkQrDataUri ? (
                <img
                  src={linkQrDataUri}
                  alt={t("settings.deviceLinkQrTitle")}
                  draggable={false}
                  className="size-full object-contain select-none [image-rendering:pixelated]"
                />
              ) : (
                <AlertCircle className="text-signal-danger size-5" />
              )}
            </div>

            <div className="text-ink-muted flex items-center gap-2 text-center text-[11px]">
              <ShieldCheck className="h-[13px] w-[13px] shrink-0 opacity-75" />
              <span>{t("settings.deviceLinkQrHint")}</span>
            </div>

            {linkQrUri && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => void markCopied("link", linkQrUri)}
                className="gap-2"
              >
                {copiedFlag === "link" ? (
                  <Check className="text-signal-success size-3.5" />
                ) : (
                  <Copy className="text-ink-muted size-3.5" />
                )}
                <span>
                  {copiedFlag === "link" ? t("common.copied") : t("settings.copyDeviceLinkUri")}
                </span>
              </Button>
            )}
          </div>
        </DialogContent>
      </Dialog>
      <Dialog
        open={manualDialogOpen}
        onOpenChange={(next) => {
          if (!manualSubmitting) {
            setManualDialogOpen(next);
            if (!next) setManualDialogError(null);
          }
        }}
      >
        <DialogContent className="max-w-[440px] gap-4 p-5" closeDisabled={manualSubmitting}>
          <DialogHeader className="gap-1 pr-6">
            <DialogTitle className="flex items-center gap-2 text-[17px]">
              <KeyRound className="text-ink-muted size-[16px] shrink-0 stroke-[1.75]" />
              <span>{t("settings.cloudSyncManualDialogTitle")}</span>
            </DialogTitle>
            <DialogDescription className="text-[13px] leading-[1.45]">
              {t("settings.cloudSyncManualDialogDesc")}
            </DialogDescription>
          </DialogHeader>

          <div
            role="note"
            className="border-signal-warning-border bg-signal-warning-bg text-signal-warning flex items-start gap-2 rounded-xl border p-3 text-[13px] leading-[1.45]"
          >
            <AlertTriangle className="mt-0.5 size-[15px] shrink-0" />
            <span className="min-w-0 flex-1 text-pretty">
              {t("settings.cloudSyncManualWarning")}
            </span>
          </div>

          <Segmented
            fullWidth
            disabled={manualSubmitting}
            ariaLabel={t("settings.cloudSyncManualDialogTitle")}
            value={manualMode}
            onValueChange={(next) => {
              setManualMode(next);
              setManualDialogError(null);
              if (next === "generate" && !manualGeneratedPass) {
                void generateManualPassphrase();
              }
            }}
            options={[
              { value: "generate", label: t("settings.cloudSyncManualModeGenerate") },
              { value: "custom", label: t("settings.cloudSyncManualModeCustom") },
            ]}
          />

          <form
            className="flex flex-col gap-4"
            {...ime.compositionProps}
            onKeyDown={(e) => {
              if (e.key === "Enter" && ime.shouldIgnoreKeyDown(e)) e.preventDefault();
            }}
            onSubmit={(e) => {
              e.preventDefault();
              if (manualCanConfirm) void handleConfirmEnableManual();
            }}
          >
            {manualMode === "generate" ? (
              <div className="flex flex-col gap-3">
                <div className="border-border-subtle bg-surface-recessed flex flex-col gap-2 rounded-xl border p-3">
                  <code className="text-ink-primary font-mono text-[13px] leading-[1.5] font-medium break-all tabular-nums select-text">
                    {manualGeneratedPass || "…"}
                  </code>
                  <div className="flex flex-wrap items-center gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={manualSubmitting || !trimmedManualGenerated}
                      onClick={() => void markCopied("manual", trimmedManualGenerated)}
                    >
                      {copiedFlag === "manual" ? (
                        <Check data-icon="inline-start" className="text-signal-success" />
                      ) : (
                        <Copy data-icon="inline-start" />
                      )}
                      {t(copiedFlag === "manual" ? "common.copied" : "common.copy")}
                    </Button>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      disabled={manualSubmitting}
                      onClick={() => void generateManualPassphrase()}
                    >
                      <RefreshCw data-icon="inline-start" />
                      {t("settings.cloudSyncManualRegenerate")}
                    </Button>
                  </div>
                </div>
                <label
                  className={cn(
                    "text-ink-secondary flex items-start gap-2 text-[13px] leading-[1.45]",
                    manualSubmitting ? "cursor-not-allowed opacity-60" : "cursor-pointer",
                  )}
                >
                  <Checkbox
                    size="sm"
                    className="mt-0.5"
                    checked={manualSavedAck}
                    disabled={manualSubmitting || !trimmedManualGenerated}
                    onCheckedChange={setManualSavedAck}
                  />
                  <span>{t("settings.cloudSyncManualSavedAck")}</span>
                </label>
              </div>
            ) : (
              <FieldGroup className="gap-3">
                <p className="text-ink-secondary text-[13px] leading-[1.45] text-pretty">
                  {t("settings.cloudSyncManualCustomHint")}
                </p>
                <Field>
                  <Input
                    type="password"
                    autoComplete="new-password"
                    disabled={manualSubmitting}
                    aria-label={t("settings.cloudEnterPass")}
                    placeholder={t("settings.cloudEnterPass")}
                    value={manualCustomPass}
                    onChange={(e) => setManualCustomPass(e.target.value)}
                  />
                </Field>
                <Field data-invalid={manualCustomMismatch || undefined}>
                  <Input
                    type="password"
                    autoComplete="new-password"
                    disabled={manualSubmitting}
                    aria-invalid={manualCustomMismatch || undefined}
                    aria-label={t("settings.cloudConfirmPass")}
                    placeholder={t("settings.cloudConfirmPass")}
                    value={manualCustomConfirm}
                    onChange={(e) => setManualCustomConfirm(e.target.value)}
                  />
                  {manualCustomNeedsConfirm && (
                    <FieldDescription className="text-[11px]">
                      {t("settings.cloudConfirmPassHint")}
                    </FieldDescription>
                  )}
                  {manualCustomMismatch && (
                    <FieldError className="text-[11px]">
                      {t("settings.cloudPassMismatch")}
                    </FieldError>
                  )}
                </Field>
              </FieldGroup>
            )}

            {manualDialogError && (
              <div
                role="alert"
                className="border-signal-danger-border bg-signal-danger-bg text-signal-danger flex items-start gap-2 rounded-xl border p-3 text-[13px] leading-[1.45]"
              >
                <AlertCircle className="mt-0.5 size-[15px] shrink-0" />
                <span className="min-w-0 flex-1 break-words">{manualDialogError}</span>
              </div>
            )}

            <DialogFooter>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                disabled={manualSubmitting}
                onClick={() => {
                  setManualDialogOpen(false);
                  setManualDialogError(null);
                }}
              >
                {t("common.cancel")}
              </Button>
              <Button type="submit" size="sm" disabled={!manualCanConfirm}>
                {manualSubmitting && <OrbitSpinner size={14} />}
                {t(
                  manualSubmitting
                    ? "settings.cloudSyncManualEnabling"
                    : "settings.cloudSyncManualConfirmButton",
                )}
              </Button>
            </DialogFooter>
          </form>
        </DialogContent>
      </Dialog>
    </div>
  );
});
