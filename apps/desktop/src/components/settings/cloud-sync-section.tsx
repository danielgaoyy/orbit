import { useEffect, useMemo, useState } from "react";
import {
  AlertCircle,
  Check,
  ChevronRight,
  Cloud,
  Copy,
  ExternalLink,
  Globe,
  KeyRound,
  Link2,
  Lock,
  LogOut,
  QrCode,
  RefreshCw,
  ShieldCheck,
  Sparkles,
  Terminal,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Collapsible,
  CollapsibleContent,
  CollapsibleTrigger,
} from "@/components/ui/collapsible";
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

function formatRelativeSyncTime(tsMs: number | null): string {
  if (!tsMs) return "Never synced yet";
  const diffSec = Math.max(0, Math.floor((Date.now() - tsMs) / 1000));
  if (diffSec < 15) return "Just now";
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  return new Date(tsMs).toLocaleDateString();
}

export function CloudSyncSection() {
  const { t } = useI18n();
  const status = useOrbitSelector((s) => s.cloudSyncStatus);
  const busy = useOrbitSelector((s) => s.cloudSyncBusy);
  const storeError = useOrbitSelector((s) => s.cloudSyncError);
  const deviceFlow = useOrbitSelector((s) => s.cloudSyncDeviceFlow);
  const lastResult = useOrbitSelector((s) => s.cloudSyncLastResult);
  const hosts = useOrbitSelector((s) => s.fleet.hosts);

  const [loginTab, setLoginTab] = useState<LoginMethodTab>("device");
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
    if (status && !status.github_oauth_configured && !deviceFlow && loginTab === "device") {
      setLoginTab("token");
    }
  }, [status, deviceFlow, loginTab]);

  useEffect(() => {
    if (!linkQrOpen) return;
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

  const effectiveState = status?.state ?? "signed_out";
  const displayError = localError ?? storeError ?? status?.last_error ?? null;
  const isAuthExpiredError = Boolean(
    displayError &&
      /401|bad credentials|expired|revoked|unauthorized/i.test(displayError),
  );
  const hostsWithReachableFabric = hosts.filter(
    (h) => Boolean((h.relay_url ?? "").trim()) || Boolean((h.ssh_alias ?? "").trim()),
  );
  const allRemoteHostsLackReachability =
    hosts.length > 1 && hostsWithReachableFabric.length === 0;

  const markCopied = (which: "code" | "key" | "cli" | "link", text: string) => {
    if (typeof navigator !== "undefined" && navigator.clipboard) {
      void navigator.clipboard.writeText(text).catch(() => undefined);
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
      setLocalError(
        "Enter a GitHub OAuth App Client ID (with Device Flow enabled), or switch to Personal Access Token below.",
      );
      return;
    }
    await orbitActions.startGithubCloudLogin(trimmedCid || null);
  };

  const handleTokenLogin = async () => {
    setLocalError(null);
    if (!tokenInput.trim()) {
      setLocalError("Paste a GitHub Personal Access Token with the `gist` scope.");
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
      setLocalError("Paste an `orbit://sync-login?...` Device Link URI.");
      return;
    }
    const ok = await orbitActions.importCloudSyncDeviceLinkUri(deviceLinkInput);
    if (ok) {
      setDeviceLinkInput("");
    }
  };

  const handleGenerateKey = async () => {
    setLocalError(null);
    const rec = await orbitIpc.cloudSyncGeneratePassphrase();
    setGeneratedKey(rec);
    setPassphraseInput(rec);
    setConfirmPassphraseInput(rec);
  };

  const handleUnlock = async () => {
    setLocalError(null);
    const pass = passphraseInput.trim();
    if (!pass) {
      setLocalError("Sync Passphrase cannot be empty.");
      return;
    }
    if (!status?.remote_vault_exists && !generatedKey) {
      if (pass.length < 6) {
        setLocalError("Use at least 6 characters (or click Generate Recovery Key).");
        return;
      }
      if (pass !== confirmPassphraseInput.trim()) {
        setLocalError("Sync Passphrases do not match.");
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

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title={t("settings.cloudSyncTitle")}
        description={t("settings.cloudSyncDesc")}
      />

      {/* Architecture / Zero-Server Trust Summary */}
      <div className="border-border/80 bg-background/55 flex items-start gap-3 rounded-[13px] border px-4 py-3.5">
        <ShieldCheck className="text-signal-success mt-0.5 h-[17px] w-[17px] shrink-0" />
        <div className="text-muted-foreground space-y-1 text-[12.5px] leading-relaxed">
          <div className="text-foreground text-[13px] font-medium">
            Zero-Server · End-to-End Encrypted (GitHub Secret Gist)
          </div>
          <p>{t("settings.cloudSyncInstantSummary")}</p>
        </div>
      </div>

      {/* Active GitHub Device Flow Card (shown regardless of signed_out/error state for seamless re-auth) */}
      {deviceFlow && (
        <div className="space-y-2">
          <SettingGroupLabel>GitHub Verification</SettingGroupLabel>
          <SettingCard>
            <div className="space-y-4 p-4">
              <div className="flex items-center justify-between gap-3">
                <div>
                  <div className="text-foreground text-[13.5px] font-medium">
                    Enter this code on GitHub
                  </div>
                  <div className="text-muted-foreground text-[12px]">
                    Authorize Orbit to store your encrypted sync vault in a Secret Gist.
                  </div>
                </div>
                <OrbitSpinner size={16} className="text-muted-foreground" />
              </div>

              <div className="border-border bg-muted/40 flex items-center justify-between rounded-[11px] border px-4 py-3">
                <span className="selectable-text text-foreground font-mono text-[22px] font-semibold tracking-[0.14em]">
                  {deviceFlow.user_code}
                </span>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => markCopied("code", deviceFlow.user_code)}
                  className="px-2.5"
                >
                  {copiedFlag === "code" ? (
                    <>
                      <Check className="text-signal-success size-3.5" />
                      <span>{t("common.copied")}</span>
                    </>
                  ) : (
                    <>
                      <Copy className="size-3.5" />
                      <span>{t("common.copy")}</span>
                    </>
                  )}
                </Button>
              </div>

              <div className="flex flex-wrap items-center gap-2">
                <Button
                  type="button"
                  onClick={() => {
                    markCopied("code", deviceFlow.user_code);
                    void orbitIpc.openExternalBrowserUrl(deviceFlow.verification_uri);
                  }}
                  className="gap-2 text-[12.5px]"
                >
                  <ExternalLink className="size-3.5" />
                  <span>{t("settings.copyCodeOpenGithub")}</span>
                </Button>

                <Button
                  type="button"
                  variant="outline"
                  onClick={() => orbitActions.cancelGithubCloudLogin()}
                  className="text-muted-foreground hover:text-foreground px-3 text-[12.5px]"
                >
                  {t("common.cancel")}
                </Button>
              </div>
            </div>
          </SettingCard>
        </div>
      )}

      {/* 1. SIGNED OUT */}
      {effectiveState === "signed_out" && !deviceFlow && (
        <div className="space-y-3">
          <div className="space-y-2">
              <SettingGroupLabel>{t("settings.signInGithub")}</SettingGroupLabel>
              <SettingCard>
                <div className="space-y-4 p-4">
                  {/* Method Toggle */}
                  <div className="bg-muted/60 grid grid-cols-3 gap-1 rounded-[10px] p-1">
                    <button
                      type="button"
                      onClick={() => {
                        setLoginTab("device");
                        setLocalError(null);
                      }}
                      className={cn(
                        "h-[30px] cursor-pointer rounded-[7px] px-2 text-[12px] font-medium transition-colors",
                        loginTab === "device"
                          ? "bg-background text-foreground shadow-xs"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      {t("settings.oauthDeviceFlow")}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setLoginTab("token");
                        setLocalError(null);
                      }}
                      className={cn(
                        "h-[30px] cursor-pointer rounded-[7px] px-2 text-[12px] font-medium transition-colors",
                        loginTab === "token"
                          ? "bg-background text-foreground shadow-xs"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      {t("settings.personalAccessToken")}
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setLoginTab("link");
                        setLocalError(null);
                      }}
                      className={cn(
                        "h-[30px] cursor-pointer rounded-[7px] px-2 text-[12px] font-medium transition-colors",
                        loginTab === "link"
                          ? "bg-background text-foreground shadow-xs"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      {t("settings.deviceLinkTab")}
                    </button>
                  </div>

                  {loginTab === "device" ? (
                    <div className="space-y-3">
                      {!status?.github_oauth_configured && (
                        <div className="space-y-1.5">
                          <div className="text-muted-foreground text-[12px] leading-relaxed">
                            Provide a public GitHub OAuth App{" "}
                            <span className="text-foreground/90 font-medium">Client ID</span> (with{" "}
                            <span className="font-mono text-[11.5px]">Enable Device Flow</span>{" "}
                            checked on{" "}
                            <button
                              type="button"
                              onClick={() =>
                                void orbitIpc.openExternalBrowserUrl(
                                  "https://github.com/settings/developers",
                                )
                              }
                              className="text-foreground/90 cursor-pointer underline underline-offset-2"
                            >
                              GitHub Developer Settings
                            </button>
                            ). No client secret is needed.
                          </div>
                          <Input
                            type="text"
                            value={clientIdInput}
                            onChange={(e) => setClientIdInput(e.target.value)}
                            {...ime.compositionProps}
                            onKeyDown={(e) => {
                              if (ime.shouldIgnoreKeyDown(e)) return;
                              if (e.key === "Enter") void handleStartDeviceLogin();
                            }}
                            placeholder="Ov23li..."
                            className="h-[34px] px-3 font-mono"
                          />
                        </div>
                      )}

                      <Button
                        type="button"
                        disabled={busy}
                        onClick={() => void handleStartDeviceLogin()}
                        className="h-[36px] gap-2 px-4 text-[13px]"
                      >
                        {busy ? (
                          <OrbitSpinner size={16} className="size-4" />
                        ) : (
                          <Cloud className="size-4" />
                        )}
                        <span>Sign in with GitHub</span>
                      </Button>
                    </div>
                  ) : loginTab === "token" ? (
                    <div className="space-y-3">
                      <div className="text-muted-foreground text-[12px] leading-relaxed">
                        Create a classic token with only the{" "}
                        <code className="bg-muted text-foreground rounded px-1 py-0.5 font-mono text-[11.5px]">
                          gist
                        </code>{" "}
                        checkbox checked — zero repo access required.{" "}
                        <button
                          type="button"
                          onClick={() =>
                            void orbitIpc.openExternalBrowserUrl(
                              "https://github.com/settings/tokens/new?scopes=gist&description=Orbit%20Cloud%20Sync",
                            )
                          }
                          className="text-foreground inline-flex cursor-pointer items-center gap-1 underline underline-offset-2"
                        >
                          <span>Create GitHub token</span>
                          <ExternalLink className="h-3 w-3" />
                        </button>
                      </div>
                      <div className="flex gap-2">
                        <Input
                          type="password"
                          value={tokenInput}
                          onChange={(e) => setTokenInput(e.target.value)}
                          {...ime.compositionProps}
                          onKeyDown={(e) => {
                            if (ime.shouldIgnoreKeyDown(e)) return;
                            if (e.key === "Enter") void handleTokenLogin();
                          }}
                          placeholder="ghp_... or github_pat_..."
                          className="h-[34px] flex-1 px-3 font-mono"
                        />
                        <Button
                          type="button"
                          disabled={busy}
                          onClick={() => void handleTokenLogin()}
                          className="h-[34px] shrink-0 gap-1.5 px-3.5"
                        >
                          {busy ? (
                            <OrbitSpinner size={14} className="size-3.5" />
                          ) : (
                            <KeyRound className="size-3.5" />
                          )}
                          <span>Continue</span>
                        </Button>
                      </div>
                    </div>
                  ) : (
                    <div className="space-y-3">
                      <div className="text-muted-foreground text-[12px] leading-relaxed">
                        {t("settings.deviceLinkTabDesc")}
                      </div>
                      <div className="flex gap-2">
                        <Input
                          type="password"
                          value={deviceLinkInput}
                          onChange={(e) => setDeviceLinkInput(e.target.value)}
                          {...ime.compositionProps}
                          onKeyDown={(e) => {
                            if (ime.shouldIgnoreKeyDown(e)) return;
                            if (e.key === "Enter") void handleDeviceLinkLogin();
                          }}
                          placeholder={t("settings.deviceLinkPlaceholder")}
                          className="h-[34px] flex-1 px-3 font-mono"
                        />
                        <Button
                          type="button"
                          disabled={busy}
                          onClick={() => void handleDeviceLinkLogin()}
                          className="h-[34px] shrink-0 gap-1.5 px-3.5"
                        >
                          {busy ? (
                            <OrbitSpinner size={14} className="size-3.5" />
                          ) : (
                            <Link2 className="size-3.5" />
                          )}
                          <span>{t("settings.deviceLinkConnect")}</span>
                        </Button>
                      </div>
                    </div>
                  )}
                </div>
              </SettingCard>
            </div>
        </div>
      )}

      {/* 2. LOCKED (Signed into GitHub, waiting for E2EE Sync Passphrase) */}
      {effectiveState === "locked" && status && (
        <div className="space-y-4">
          <div className="space-y-2">
            <SettingGroupLabel>GitHub Account</SettingGroupLabel>
            <SettingCard>
              <div className="flex items-center justify-between gap-3 px-4 py-3">
                <div className="flex min-w-0 items-center gap-3">
                  {status.account_avatar_url ? (
                    <img
                      src={status.account_avatar_url}
                      alt={status.account_login ?? "GitHub"}
                      className="h-9 w-9 shrink-0 rounded-full border border-black/10 dark:border-white/10"
                    />
                  ) : (
                    <div className="bg-muted flex h-9 w-9 shrink-0 items-center justify-center rounded-full">
                      <Cloud className="h-4 w-4" />
                    </div>
                  )}
                  <div className="min-w-0">
                      <div className="text-foreground flex items-center gap-2 text-[13.5px] font-medium">
                        <span className="truncate">@{status.account_login}</span>
                        <Badge variant="warning" className="rounded-full">
                          {status.remote_vault_exists ? "Vault Locked" : "Setup Required"}
                        </Badge>
                      </div>
                    <div className="text-muted-foreground truncate text-[12px]">
                      {status.remote_vault_exists
                        ? "Encrypted Orbit Vault found on GitHub — enter passphrase to unlock."
                        : "Create a Sync Passphrase to encrypt your first Orbit Vault."}
                    </div>
                  </div>
                </div>
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onClick={() => void orbitActions.logoutCloudSync(false)}
                  className="shrink-0 px-2.5"
                >
                  <LogOut className="size-3.5" />
                  <span>Sign Out</span>
                </Button>
              </div>
            </SettingCard>
          </div>

          <div className="space-y-2">
            <SettingGroupLabel
              action={
                !status.remote_vault_exists && (
                  <button
                    type="button"
                    onClick={() => void handleGenerateKey()}
                    className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[12px] font-medium transition-colors"
                  >
                    <Sparkles className="h-3.5 w-3.5" />
                    <span>Generate Recovery Key</span>
                  </button>
                )
              }
            >
              {status.remote_vault_exists ? "Unlock Encrypted Vault" : "Create Sync Passphrase"}
            </SettingGroupLabel>
            <SettingCard>
              <div className="space-y-3 p-4">
                {generatedKey && (
                  <div className="border-signal-success-border bg-signal-success-bg space-y-2 rounded-[10px] border p-3">
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-signal-success text-[12px] font-medium">
                        Generated Recovery Key — save this in your password manager
                      </span>
                      <Button
                        type="button"
                        variant="outline"
                        size="xs"
                        onClick={() => markCopied("key", generatedKey)}
                        className="gap-1 px-2 text-[11.5px]"
                      >
                        {copiedFlag === "key" ? (
                          <>
                            <Check className="text-signal-success size-3" />
                            <span>Copied</span>
                          </>
                        ) : (
                          <>
                            <Copy className="size-3" />
                            <span>Copy</span>
                          </>
                        )}
                      </Button>
                    </div>
                    <div className="selectable-text text-foreground font-mono text-[13px] font-semibold">
                      {generatedKey}
                    </div>
                  </div>
                )}

                <div className="space-y-2">
                  <Input
                    type={generatedKey ? "text" : "password"}
                    value={passphraseInput}
                    onChange={(e) => {
                      setPassphraseInput(e.target.value);
                      if (generatedKey && e.target.value !== generatedKey) {
                        setGeneratedKey(null);
                      }
                    }}
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      if (ime.shouldIgnoreKeyDown(e)) return;
                      if (e.key === "Enter" && (status.remote_vault_exists || generatedKey)) {
                        void handleUnlock();
                      }
                    }}
                    placeholder={
                      status.remote_vault_exists
                        ? "Enter your Sync Passphrase or Recovery Key"
                        : "Choose a Sync Passphrase (or click Generate Recovery Key)"
                    }
                    className="h-[36px] px-3 text-[13px]"
                  />

                  {!status.remote_vault_exists && !generatedKey && (
                    <Input
                      type="password"
                      value={confirmPassphraseInput}
                      onChange={(e) => setConfirmPassphraseInput(e.target.value)}
                      {...ime.compositionProps}
                      onKeyDown={(e) => {
                        if (ime.shouldIgnoreKeyDown(e)) return;
                        if (e.key === "Enter") void handleUnlock();
                      }}
                      placeholder="Confirm Sync Passphrase"
                      className="h-[36px] px-3 text-[13px]"
                    />
                  )}
                </div>

                <div className="flex items-center justify-between gap-3 pt-1">
                  <span className="text-muted-foreground text-[11.5px]">
                    Cached locally after unlock — you only enter this once per device.
                  </span>
                  <Button
                    type="button"
                    disabled={busy}
                    onClick={() => void handleUnlock()}
                    className="h-[34px] shrink-0 gap-1.5 px-4"
                  >
                    {busy ? (
                      <OrbitSpinner size={14} className="size-3.5" />
                    ) : (
                      <Lock className="size-3.5" />
                    )}
                    <span>
                      {status.remote_vault_exists ? "Unlock & Sync" : "Encrypt & Start Sync"}
                    </span>
                  </Button>
                </div>
              </div>
            </SettingCard>
          </div>
        </div>
      )}

      {/* 3. READY / SYNCING / ERROR */}
      {(effectiveState === "ready" ||
        effectiveState === "syncing" ||
        effectiveState === "error") &&
        status && (
          <div className="space-y-4">
            <div className="space-y-2">
              <SettingGroupLabel>Connected Account</SettingGroupLabel>
              <SettingCard>
                <div className="flex items-center justify-between gap-3 px-4 py-3.5">
                  <div className="flex min-w-0 items-center gap-3">
                    {status.account_avatar_url ? (
                      <img
                        src={status.account_avatar_url}
                        alt={status.account_login ?? "GitHub"}
                        className="h-10 w-10 shrink-0 rounded-full border border-black/10 dark:border-white/10"
                      />
                    ) : (
                      <div className="bg-muted flex h-10 w-10 shrink-0 items-center justify-center rounded-full">
                        <Cloud className="h-4.5 w-4.5" />
                      </div>
                    )}
                    <div className="min-w-0">
                      <div className="text-foreground flex flex-wrap items-center gap-2 text-[13.5px] font-medium">
                        <span className="truncate">@{status.account_login}</span>
                        <Badge
                          variant={effectiveState === "error" ? "destructive" : "success"}
                          className="rounded-full"
                        >
                          {effectiveState === "error"
                            ? "Sync Attention"
                            : `E2EE Synced · rev #${status.last_vault_revision}`}
                        </Badge>
                      </div>
                      <div className="text-muted-foreground mt-0.5 flex flex-wrap items-center gap-x-2.5 gap-y-0.5 text-[12px]">
                        <span>{status.synced_host_count} hosts</span>
                        <span>·</span>
                        <span>{status.synced_project_count} projects</span>
                        <span>·</span>
                        <span>Last sync: {formatRelativeSyncTime(status.last_synced_at_ms)}</span>
                      </div>
                    </div>
                  </div>

                  <div className="flex shrink-0 items-center gap-2">
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() => setLinkQrOpen(true)}
                      className="h-[32px] gap-1.5 text-[12.5px]"
                    >
                      <QrCode className="size-3.5" />
                      <span>{t("settings.linkDeviceQr")}</span>
                    </Button>
                    <Button
                      type="button"
                      size="sm"
                      disabled={busy}
                      onClick={() => void orbitActions.syncCloudNow()}
                      className="h-[32px] shrink-0 text-[12.5px]"
                    >
                      <RefreshCw className={cn("size-3.5", busy && "animate-spin")} />
                      <span>{busy ? "Syncing…" : "Sync Now"}</span>
                    </Button>
                  </div>
                </div>

                {lastResult && (
                  <div className="border-border/60 text-muted-foreground flex flex-wrap items-center justify-between gap-2 border-t px-4 py-2 text-[11.5px]">
                    <span>
                      Last run: +{lastResult.hosts_added} hosts, {lastResult.hosts_updated} updated,{" "}
                      +{lastResult.projects_added} projects
                      {lastResult.pushed_remote ? " (uploaded)" : " (up to date)"}
                    </span>
                    {status.gist_html_url && (
                      <button
                        type="button"
                        onClick={() =>
                          void orbitIpc.openExternalBrowserUrl(status.gist_html_url ?? "")
                        }
                        className="text-foreground/80 hover:text-foreground inline-flex cursor-pointer items-center gap-1 font-medium"
                      >
                        <span>View Secret Gist</span>
                        <ExternalLink className="h-3 w-3" />
                      </button>
                    )}
                  </div>
                )}
              </SettingCard>
            </div>

            <div className="space-y-2">
              <SettingGroupLabel>Sync Preferences</SettingGroupLabel>
              <SettingCard>
                <SettingRow
                  title="Auto-sync changes"
                  description="Automatically sync newly paired hosts, pinned projects, and preferences in the background."
                >
                  <ToggleSwitch
                    checked={status.auto_sync_enabled}
                    onChange={(enabled) => void orbitActions.setCloudAutoSync(enabled)}
                  />
                </SettingRow>

                {status.gist_html_url && (
                  <SettingRow
                    title="Remote encrypted vault"
                    description="Stored as a private Secret Gist on your GitHub account."
                  >
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      onClick={() =>
                        void orbitIpc.openExternalBrowserUrl(status.gist_html_url ?? "")
                      }
                      className="px-2.5"
                    >
                      <span>Open on GitHub</span>
                      <ExternalLink className="size-3" />
                    </Button>
                  </SettingRow>
                )}

                <SettingRow
                  title="Sign out of this device"
                  description="Removes local credentials only — your encrypted GitHub Secret Gist remains safe for other devices."
                >
                  <div className="flex items-center gap-2.5">
                    <label className="text-muted-foreground flex cursor-pointer items-center gap-1.5 text-[12px] select-none">
                      <Checkbox
                        size="sm"
                        checked={clearHostsOnLogout}
                        onCheckedChange={setClearHostsOnLogout}
                      />
                      <span>Clear hosts</span>
                    </label>
                    <Button
                      type="button"
                      variant="outline"
                      size="sm"
                      disabled={busy}
                      onClick={() => setConfirmSignOutOpen(true)}
                      className="hover:border-signal-danger-border hover:bg-signal-danger-bg hover:text-signal-danger"
                    >
                      <LogOut className="size-3.5" />
                      <span>Sign Out</span>
                    </Button>
                  </div>
                </SettingRow>
              </SettingCard>
            </div>

            {/* Optional Manual Passphrase / Dual-Gist Security Toggle */}
            <Collapsible
              open={advancedSecOpen}
              onOpenChange={setAdvancedSecOpen}
              className="border-border bg-background/60 rounded-[13px] border px-4 py-3"
            >
              <CollapsibleTrigger className="group/sec flex w-full cursor-pointer items-center justify-between gap-2 text-left outline-none">
                <span className="flex min-w-0 items-center gap-2">
                  <ChevronRight className="text-muted-foreground size-3.5 shrink-0 transition-transform duration-150 group-data-[panel-open]/sec:rotate-90" />
                  <span className="text-foreground text-[12.5px] font-medium">
                    {t("settings.cloudSyncAdvancedTitle")}
                  </span>
                </span>
                <span className="bg-muted text-muted-foreground shrink-0 rounded-md px-2 py-0.5 text-[11px] font-medium">
                  {status.instant_unlock_enabled
                    ? t("settings.cloudSyncInstantBadge")
                    : t("settings.cloudSyncManualBadge")}
                </span>
              </CollapsibleTrigger>

              <CollapsibleContent className="pt-3">
                <SettingCard>
                  <SettingRow
                    title={t("settings.cloudSyncRequireManualTitle")}
                    description={t("settings.cloudSyncRequireManualDesc")}
                  >
                    <ToggleSwitch
                      checked={!status.instant_unlock_enabled}
                      onChange={(requireManual) =>
                        void orbitActions.setCloudSyncManualPassphraseMode(requireManual)
                      }
                    />
                  </SettingRow>
                </SettingCard>
              </CollapsibleContent>
            </Collapsible>

            {/* Auto-register remote Linux/macOS servers tip */}
            <div className="space-y-2">
              <SettingGroupLabel>Auto-Register Headless Servers</SettingGroupLabel>
              <div className="border-border bg-background/60 flex items-center justify-between gap-3 rounded-[13px] border px-4 py-3">
                <div className="min-w-0 space-y-0.5">
                  <div className="text-foreground flex items-center gap-1.5 text-[12.5px] font-medium">
                    <Terminal className="text-muted-foreground h-3.5 w-3.5" />
                    <span>Skip pairing codes on Linux &amp; macOS servers</span>
                  </div>
                  <div className="text-muted-foreground text-[12px]">
                    Run{" "}
                    <code className="selectable-text bg-muted text-foreground rounded px-1.5 py-0.5 font-mono text-[11.5px]">
                      orbit login
                    </code>{" "}
                    on any host — it encrypts &amp; registers itself into this vault automatically.
                  </div>
                </div>
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  onClick={() => markCopied("cli", "orbit login")}
                  className="shrink-0 px-2.5"
                >
                  {copiedFlag === "cli" ? (
                    <>
                      <Check className="text-signal-success size-3.5" />
                      <span>Copied</span>
                    </>
                  ) : (
                    <>
                      <Copy className="size-3.5" />
                      <span>Copy</span>
                    </>
                  )}
                </Button>
              </div>
            </div>
          </div>
        )}

      {effectiveState === "ready" && hosts.length > 0 && (
        <div className="space-y-2">
          <SettingGroupLabel>{t("settings.fleetNetworkTitle")}</SettingGroupLabel>
          <SettingCard>
            <div className="flex items-start gap-2.5 px-4 py-3 text-[12px]">
              <Globe className="text-muted-foreground mt-0.5 h-4 w-4 shrink-0" />
              <span className="text-muted-foreground leading-relaxed">
                {allRemoteHostsLackReachability
                  ? t("settings.fleetNetworkMissingReachability")
                  : t("settings.fleetNetworkAutoDesc")}
              </span>
            </div>
          </SettingCard>
        </div>
      )}

      {displayError && (
        <div className="border-signal-danger-border bg-signal-danger-bg text-signal-danger flex flex-wrap items-center justify-between gap-2.5 rounded-[11px] border px-3.5 py-2.5 text-[12.5px]">
          <div className="flex min-w-0 flex-1 items-start gap-2">
            <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
            <span className="selectable-text break-words">{displayError}</span>
          </div>
          {(effectiveState === "error" || isAuthExpiredError) && (
            <Button
              type="button"
              size="sm"
              disabled={busy}
              onClick={() => void handleStartDeviceLogin()}
              className="shrink-0"
            >
              {busy ? (
                <OrbitSpinner size={14} className="size-3.5" />
              ) : (
                <Cloud className="size-3.5" />
              )}
              <span>Sign In Again with GitHub</span>
            </Button>
          )}
        </div>
      )}

      <ConfirmDialog
        open={confirmSignOutOpen}
        onOpenChange={setConfirmSignOutOpen}
        title={
          clearHostsOnLogout
            ? "Sign out and clear paired hosts?"
            : "Sign out of Cloud Sync?"
        }
        description={
          clearHostsOnLogout
            ? "Removes local sync credentials and clears synced host pairings on this device. Your encrypted GitHub Secret Gist remains safe for other devices."
            : "Removes local Cloud Sync credentials from this device. Your paired hosts and encrypted GitHub Secret Gist remain intact."
        }
        confirmLabel={clearHostsOnLogout ? "Sign out & clear hosts" : "Sign out"}
        tone={clearHostsOnLogout ? "danger" : "default"}
        onConfirm={() => orbitActions.logoutCloudSync(clearHostsOnLogout)}
      />

      <Dialog open={linkQrOpen} onOpenChange={setLinkQrOpen}>
        <DialogContent className="max-w-[380px] gap-5 p-5">
          <DialogHeader className="gap-1 pr-6">
            <DialogTitle className="flex items-center gap-2 text-[15px]">
              <QrCode className="text-muted-foreground h-[16px] w-[16px] stroke-[1.75]" />
              <span>{t("settings.deviceLinkQrTitle")}</span>
            </DialogTitle>
            <DialogDescription className="text-[12.5px] leading-relaxed">
              {t("settings.deviceLinkQrDesc")}
            </DialogDescription>
          </DialogHeader>

          <div className="flex flex-col items-center gap-3.5">
            <div className="flex size-[208px] items-center justify-center rounded-[14px] border border-black/[0.08] bg-white p-3 shadow-2xs dark:border-white/[0.14]">
              {linkQrLoading ? (
                <OrbitSpinner size={20} className="size-5 text-zinc-500" />
              ) : linkQrDataUri ? (
                <img
                  src={linkQrDataUri}
                  alt={t("settings.deviceLinkQrTitle")}
                  draggable={false}
                  className="size-full object-contain select-none [image-rendering:pixelated]"
                />
              ) : (
                <AlertCircle className="size-5 text-rose-500" />
              )}
            </div>

            <div className="text-muted-foreground flex items-center gap-1.5 text-center text-[11.5px]">
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
                  <Copy className="text-muted-foreground size-3.5" />
                )}
                <span>
                  {copiedFlag === "link"
                    ? t("common.copied")
                    : t("settings.copyDeviceLinkUri")}
                </span>
              </Button>
            )}
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
}
