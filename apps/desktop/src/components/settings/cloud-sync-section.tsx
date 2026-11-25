import { useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronRight,
  Cloud,
  Copy,
  ExternalLink,
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
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type LoginMethodTab = "device" | "token" | "link";

export function CloudSyncSection() {
  const { t, locale } = useI18n();
  const status = useOrbitSelector((s) => s.cloudSyncStatus);
  const busy = useOrbitSelector((s) => s.cloudSyncBusy);
  const storeError = useOrbitSelector((s) => s.cloudSyncError);
  const deviceFlow = useOrbitSelector((s) => s.cloudSyncDeviceFlow);
  const lastResult = useOrbitSelector((s) => s.cloudSyncLastResult);
  const hosts = useOrbitSelector((s) => s.fleet.hosts);

  const [selectedMethod, setLoginTab] = useState<LoginMethodTab | null>(null);
  const loginTab =
    selectedMethod ?? (status?.github_oauth_configured === false ? "token" : "device");
  const [clientIdInput, setClientIdInput] = useState("");
  const [tokenInput, setTokenInput] = useState("");
  const [deviceLinkInput, setDeviceLinkInput] = useState("");
  const [passphraseInput, setPassphraseInput] = useState("");
  const [confirmPassphraseInput, setConfirmPassphraseInput] = useState("");
  const [generatedKey, setGeneratedKey] = useState<string | null>(null);
  const [copiedFlag, setCopiedFlag] = useState<"code" | "key" | "cli" | "link" | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const [clearHostsOnLogout, setClearHostsOnLogout] = useState(false);
  const [confirmSignOutOpen, setConfirmSignOutOpen] = useState(false);
  const [linkQrOpen, setLinkQrOpen] = useState(false);
  const [linkQrUri, setLinkQrUri] = useState<string | null>(null);
  const [linkQrSvg, setLinkQrSvg] = useState<string | null>(null);
  const [linkQrLoading, setLinkQrLoading] = useState(false);
  const [advancedSecOpen, setAdvancedSecOpen] = useState(false);

  const ime = useImeCompositionGuard();

  useEffect(() => {
    void orbitActions.refreshCloudSyncStatus();
  }, []);

  useEffect(() => {
    if (status?.github_client_id_override && !clientIdInput) {
      setClientIdInput(status.github_client_id_override);
    }
  }, [status?.github_client_id_override, clientIdInput]);

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
  const lastSynced = status?.last_synced_at_ms
    ? new Date(status.last_synced_at_ms).toLocaleString(locale)
    : t("settings.neverSynced");
  const displayError = localError ?? storeError ?? status?.last_error ?? null;
  const isAuthExpiredError = Boolean(
    displayError && /401|bad credentials|expired|revoked|unauthorized/i.test(displayError),
  );
  const hostsWithReachableFabric = hosts.filter(
    (h) => Boolean((h.relay_url ?? "").trim()) || Boolean((h.ssh_alias ?? "").trim()),
  );
  const allRemoteHostsLackReachability = hosts.length > 1 && hostsWithReachableFabric.length === 0;

  const markCopied = async (which: "code" | "key" | "cli" | "link", text: string) => {
    try {
      await navigator.clipboard.writeText(text);
    } catch (err) {
      setLocalError(String(err));
      return;
    }
    setCopiedFlag(which);
    window.setTimeout(() => {
      setCopiedFlag((cur) => (cur === which ? null : cur));
    }, 1800);
  };

  const handleStartDeviceLogin = async () => {
    setLocalError(null);
    const trimmedCid = clientIdInput.trim();
    if (!status?.github_oauth_configured && !trimmedCid) {
      setLocalError(t("settings.cloudClientHint"));
      return;
    }
    await orbitActions.startGithubCloudLogin(trimmedCid || null);
  };

  const handleTokenLogin = async () => {
    setLocalError(null);
    if (!tokenInput.trim()) {
      setLocalError(t("settings.cloudTokenHint"));
      return;
    }
    const ok = await orbitActions.loginCloudSyncWithToken(tokenInput);
    if (ok) {
      setTokenInput("");
    }
  };

  const handleDeviceLinkLogin = async () => {
    setLocalError(null);
    if (!deviceLinkInput.trim()) {
      setLocalError(t("settings.deviceLinkTabDesc"));
      return;
    }
    const ok = await orbitActions.importCloudSyncDeviceLinkUri(deviceLinkInput);
    if (ok) {
      setDeviceLinkInput("");
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
    const pass = passphraseInput.trim();
    if (!pass) {
      setLocalError(t("settings.cloudEnterPass"));
      return;
    }
    if (!status?.remote_vault_exists && !generatedKey) {
      if (pass.length < 6) {
        setLocalError(t("settings.cloudPassLength"));
        return;
      }
      if (pass !== confirmPassphraseInput.trim()) {
        setLocalError(t("settings.cloudPassMismatch"));
        return;
      }
    }
    const ok = await orbitActions.unlockCloudSyncWithPassphrase(pass);
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
  const loginMethods = [
    ["device", t("settings.oauthDeviceFlow")],
    ["token", t("settings.personalAccessToken")],
    ["link", t("settings.deviceLinkTab")],
  ] as const;

  return (
    <div className="flex flex-col gap-[24px]">
      <SettingSectionHeader
        title={t("settings.cloudSyncTitle")}
        description={t("settings.cloudSyncDesc")}
      />
      <div data-setting-fallback>
        <SettingCard settingId="settings.cloudSyncTitle">
          <SettingRow
            title={
              <span className="inline-flex flex-wrap items-center gap-2">
                <Cloud className="size-[16px] text-ink-secondary" />
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
                : t(state === "locked" ? "settings.cloudLockedDesc" : "settings.cloudSignedOutDesc")
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

      {displayError && (
        <div
          role="alert"
          className="border-signal-danger-border bg-signal-danger-bg flex flex-wrap items-center gap-2 rounded-xl border p-3 text-[13px] text-signal-danger"
        >
          <AlertCircle className="size-[16px]" />
          <span className="min-w-0 flex-1 break-words">{displayError}</span>
          {(state === "error" || isAuthExpiredError) && !deviceFlow && (
            <Button
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={() => void handleStartDeviceLogin()}
            >
              {t("settings.signInGithub")}
            </Button>
          )}
        </div>
      )}

      {deviceFlow ? (
        <SettingGroup title={t("settings.cloudVerification")}>
          <SettingCard>
            <SettingRow
              title={
                <code className="select-text font-mono text-[17px]">{deviceFlow.user_code}</code>
              }
              description={t("settings.cloudVerifyDesc")}
            >
              <Button
                size="sm"
                onClick={() => {
                  void markCopied("code", deviceFlow.user_code);
                  void orbitIpc.openExternalBrowserUrl(deviceFlow.verification_uri);
                }}
              >
                <ExternalLink data-icon="inline-start" />
                {t("settings.copyCodeOpenGithub")}
              </Button>
            </SettingRow>
            <div className="flex items-center justify-end gap-2 px-3 py-2">
              <Button
                size="sm"
                variant="ghost"
                onClick={() => void markCopied("code", deviceFlow.user_code)}
              >
                {copiedFlag === "code" ? t("common.copied") : t("common.copy")}
              </Button>
              <Button
                size="sm"
                variant="ghost"
                onClick={() => orbitActions.cancelGithubCloudLogin()}
              >
                {t("common.cancel")}
              </Button>
            </div>
          </SettingCard>
        </SettingGroup>
      ) : (
        state === "signed_out" && (
          <SettingGroup settingId="settings.signInGithub" title={t("settings.signInGithub")}>
            <SettingCard>
              <div
                className="flex flex-wrap gap-1 p-2"
                role="group"
                aria-label={t("settings.signInGithub")}
              >
                {loginMethods.map(([method, label]) => (
                  <Button
                    variant={method === loginTab ? "secondary" : "ghost"}
                    size="sm"
                    key={method}
                    aria-pressed={method === loginTab}
                    onClick={() => {
                      setLoginTab(method);
                      setLocalError(null);
                    }}
                  >
                    {label}
                  </Button>
                ))}
              </div>
              <form
                className="flex flex-col items-start gap-3 p-4"
                {...ime.compositionProps}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && ime.shouldIgnoreKeyDown(e)) e.preventDefault();
                }}
                onSubmit={(e) => {
                  e.preventDefault();
                  if (busy) return;
                  if (loginTab === "device") void handleStartDeviceLogin();
                  else if (loginTab === "token") void handleTokenLogin();
                  else void handleDeviceLinkLogin();
                }}
              >
                {loginTab === "device" && !status?.github_oauth_configured && (
                  <>
                    <p className="text-ink-secondary text-[13px]">
                      {t("settings.cloudClientHint")}
                    </p>
                    <Button
                      type="button"
                      size="sm"
                      variant="link"
                      className="h-auto p-0"
                      onClick={() =>
                        void orbitIpc.openExternalBrowserUrl(
                          "https://github.com/settings/developers",
                        )
                      }
                    >
                      {t("settings.cloudDeveloperSettings")} <ExternalLink data-icon="inline-end" />
                    </Button>
                    <Input
                      aria-label="GitHub OAuth Client ID"
                      placeholder="Ov23li…"
                      value={clientIdInput}
                      onChange={(e) => setClientIdInput(e.target.value)}
                    />
                  </>
                )}
                {loginTab === "token" && (
                  <>
                    <p className="text-ink-secondary text-[13px]">{t("settings.cloudTokenHint")}</p>
                    <Button
                      type="button"
                      size="sm"
                      variant="link"
                      className="h-auto p-0"
                      onClick={() =>
                        void orbitIpc.openExternalBrowserUrl(
                          "https://github.com/settings/tokens/new?scopes=gist&description=Orbit%20Cloud%20Sync",
                        )
                      }
                    >
                      {t("settings.cloudCreateToken")} <ExternalLink data-icon="inline-end" />
                    </Button>
                    <Input
                      type="password"
                      autoComplete="off"
                      aria-label={t("settings.personalAccessToken")}
                      value={tokenInput}
                      onChange={(e) => setTokenInput(e.target.value)}
                      placeholder="ghp_…"
                    />
                  </>
                )}
                {loginTab === "link" && (
                  <>
                    <p className="text-ink-secondary text-[13px]">
                      {t("settings.deviceLinkTabDesc")}
                    </p>
                    <Input
                      type="password"
                      autoComplete="off"
                      aria-label={t("settings.deviceLinkTab")}
                      value={deviceLinkInput}
                      onChange={(e) => setDeviceLinkInput(e.target.value)}
                      placeholder={t("settings.deviceLinkPlaceholder")}
                    />
                  </>
                )}
                <Button type="submit" disabled={busy}>
                  {t(loginTab === "link" ? "settings.deviceLinkConnect" : "settings.signInGithub")}
                </Button>
              </form>
            </SettingCard>
          </SettingGroup>
        )
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
                if (!busy) void handleUnlock();
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
                    onClick={() => void handleGenerateKey()}
                  >
                    {t("settings.cloudGenerateKey")}
                  </Button>
                  {generatedKey && (
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
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
              <Input
                type={generatedKey ? "text" : "password"}
                autoComplete="off"
                aria-label={t("settings.cloudEnterPass")}
                placeholder={t("settings.cloudEnterPass")}
                value={passphraseInput}
                onChange={(e) => {
                  setPassphraseInput(e.target.value);
                  if (generatedKey && e.target.value !== generatedKey) setGeneratedKey(null);
                }}
              />
              {!status.remote_vault_exists && !generatedKey && (
                <Input
                  type="password"
                  autoComplete="new-password"
                  aria-label={t("settings.cloudConfirmPass")}
                  placeholder={t("settings.cloudConfirmPass")}
                  value={confirmPassphraseInput}
                  onChange={(e) => setConfirmPassphraseInput(e.target.value)}
                />
              )}
              <Button className="self-start" type="submit" disabled={busy}>
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
              <Button variant="outline" size="sm" onClick={() => setLinkQrOpen(true)}>
                <QrCode data-icon="inline-start" />
                {t("settings.linkDeviceQr")}
              </Button>
            </SettingRow>
            <SettingRow
              settingId="settings.cloudHeadless"
              title={t("settings.cloudHeadless")}
              description={t("settings.cloudHeadlessDesc")}
            >
              <Button
                variant="outline"
                size="sm"
                onClick={() => void markCopied("cli", "orbit login")}
              >
                {copiedFlag === "cli" ? (
                  <Check data-icon="inline-start" />
                ) : (
                  <Copy data-icon="inline-start" />
                )}
                <code>orbit login</code>
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
              <ChevronRight className="size-[13px] text-ink-muted transition-transform group-data-[panel-open]/sec:rotate-90" />
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
                  disabled={busy}
                  ariaLabel={t("settings.cloudSyncRequireManualTitle")}
                  onChange={(manual) => void orbitActions.setCloudSyncManualPassphraseMode(manual)}
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
          <label className="flex cursor-pointer items-center gap-2 text-ink-secondary text-[13px]">
            <Checkbox
              size="sm"
              checked={clearHostsOnLogout}
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

          <div className="flex flex-col items-center gap-3.5">
            <div className="flex size-[208px] items-center justify-center rounded-[14px] border border-border-subtle bg-surface-recessed p-3">
              {linkQrLoading ? (
                <OrbitSpinner size={20} className="size-5 text-ink-muted" />
              ) : linkQrDataUri ? (
                <img
                  src={linkQrDataUri}
                  alt={t("settings.deviceLinkQrTitle")}
                  draggable={false}
                  className="size-full object-contain select-none [image-rendering:pixelated]"
                />
              ) : (
                <AlertCircle className="size-5 text-signal-danger" />
              )}
            </div>

            <div className="text-ink-muted flex items-center gap-1.5 text-center text-[11px]">
              <ShieldCheck className="h-[13px] w-[13px] shrink-0 opacity-75" />
              <span>{t("settings.deviceLinkQrHint")}</span>
            </div>

            {linkQrUri && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => markCopied("link", linkQrUri)}
                className="gap-1.5"
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
    </div>
  );
}
