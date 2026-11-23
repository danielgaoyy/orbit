import { useEffect, useState } from "react";
import {
  AlertCircle,
  Check,
  Cloud,
  Copy,
  ExternalLink,
  KeyRound,
  Loader2,
  Lock,
  LogOut,
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
import { Checkbox } from "@/components/ui/checkbox";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { useImeCompositionGuard } from "@/lib/ime";
import { orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type LoginMethodTab = "device" | "token";

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
  const status = useOrbitSelector((s) => s.cloudSyncStatus);
  const busy = useOrbitSelector((s) => s.cloudSyncBusy);
  const storeError = useOrbitSelector((s) => s.cloudSyncError);
  const deviceFlow = useOrbitSelector((s) => s.cloudSyncDeviceFlow);
  const lastResult = useOrbitSelector((s) => s.cloudSyncLastResult);

  const [loginTab, setLoginTab] = useState<LoginMethodTab>("device");
  const [clientIdInput, setClientIdInput] = useState("");
  const [tokenInput, setTokenInput] = useState("");
  const [passphraseInput, setPassphraseInput] = useState("");
  const [confirmPassphraseInput, setConfirmPassphraseInput] = useState("");
  const [generatedKey, setGeneratedKey] = useState<string | null>(null);
  const [copiedFlag, setCopiedFlag] = useState<"code" | "key" | "cli" | null>(null);
  const [localError, setLocalError] = useState<string | null>(null);
  const [clearHostsOnLogout, setClearHostsOnLogout] = useState(false);
  const [confirmSignOutOpen, setConfirmSignOutOpen] = useState(false);

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
    if (status && !status.github_oauth_configured && !deviceFlow) {
      setLoginTab("token");
    }
  }, [status, deviceFlow]);

  const effectiveState = status?.state ?? "signed_out";
  const displayError = localError ?? storeError ?? status?.last_error ?? null;

  const markCopied = (which: "code" | "key" | "cli", text: string) => {
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
        title="Cloud Sync"
        description="Sync paired hosts, projects, and settings across all devices via your own GitHub Secret Gist — end-to-end encrypted, zero Orbit servers."
      />

      {/* Architecture / Zero-Server Trust Summary */}
      <div className="border-border/80 bg-background/55 flex items-start gap-3 rounded-[13px] border px-4 py-3.5">
        <ShieldCheck className="mt-0.5 h-[17px] w-[17px] shrink-0 text-emerald-500" />
        <div className="text-muted-foreground space-y-1 text-[12.5px] leading-relaxed">
          <div className="text-foreground text-[13px] font-medium">
            Zero-Server · Zero-Knowledge (BYOS)
          </div>
          <p>
            Orbit encrypts your fleet &amp; preferences locally with{" "}
            <span className="text-foreground/85 font-mono text-[12px]">Argon2id + AES-256-GCM</span>{" "}
            and stores only opaque ciphertext in a private{" "}
            <span className="text-foreground/85 font-medium">GitHub Secret Gist</span> (
            <code className="bg-muted rounded px-1 py-0.5 font-mono text-[11.5px]">gist</code>{" "}
            scope only — never touches your code repositories).
          </p>
        </div>
      </div>

      {/* 1. SIGNED OUT */}
      {effectiveState === "signed_out" && (
        <div className="space-y-3">
          {deviceFlow ? (
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
                    <Loader2 className="text-muted-foreground h-4 w-4 shrink-0 animate-spin" />
                  </div>

                  <div className="border-border bg-muted/40 flex items-center justify-between rounded-[11px] border px-4 py-3">
                    <span className="selectable-text text-foreground font-mono text-[22px] font-semibold tracking-[0.14em]">
                      {deviceFlow.user_code}
                    </span>
                    <button
                      type="button"
                      onClick={() => markCopied("code", deviceFlow.user_code)}
                      className="border-border bg-background hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
                    >
                      {copiedFlag === "code" ? (
                        <>
                          <Check className="h-3.5 w-3.5 text-emerald-500" />
                          <span>Copied</span>
                        </>
                      ) : (
                        <>
                          <Copy className="h-3.5 w-3.5" />
                          <span>Copy</span>
                        </>
                      )}
                    </button>
                  </div>

                  <div className="flex flex-wrap items-center gap-2">
                    <button
                      type="button"
                      onClick={() => {
                        markCopied("code", deviceFlow.user_code);
                        void orbitIpc.openExternalBrowserUrl(deviceFlow.verification_uri);
                      }}
                      className="bg-foreground text-background inline-flex h-[34px] cursor-pointer items-center gap-2 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90"
                    >
                      <ExternalLink className="h-3.5 w-3.5" />
                      <span>Copy Code &amp; Open GitHub</span>
                    </button>

                    <button
                      type="button"
                      onClick={() => orbitActions.cancelGithubCloudLogin()}
                      className="border-border text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[34px] cursor-pointer items-center rounded-lg border px-3 text-[12.5px] font-medium transition-colors"
                    >
                      Cancel
                    </button>
                  </div>
                </div>
              </SettingCard>
            </div>
          ) : (
            <div className="space-y-2">
              <SettingGroupLabel>Sign In with GitHub</SettingGroupLabel>
              <SettingCard>
                <div className="space-y-4 p-4">
                  {/* Method Toggle */}
                  <div className="bg-muted/60 grid grid-cols-2 gap-1 rounded-[10px] p-1">
                    <button
                      type="button"
                      onClick={() => {
                        setLoginTab("device");
                        setLocalError(null);
                      }}
                      className={cn(
                        "h-[30px] cursor-pointer rounded-[7px] text-[12.5px] font-medium transition-colors",
                        loginTab === "device"
                          ? "bg-background text-foreground shadow-xs"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      GitHub OAuth (Device Flow)
                    </button>
                    <button
                      type="button"
                      onClick={() => {
                        setLoginTab("token");
                        setLocalError(null);
                      }}
                      className={cn(
                        "h-[30px] cursor-pointer rounded-[7px] text-[12.5px] font-medium transition-colors",
                        loginTab === "token"
                          ? "bg-background text-foreground shadow-xs"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      Personal Access Token
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
                          <input
                            type="text"
                            value={clientIdInput}
                            onChange={(e) => setClientIdInput(e.target.value)}
                            {...ime.compositionProps}
                            onKeyDown={(e) => {
                              if (ime.shouldIgnoreKeyDown(e)) return;
                              if (e.key === "Enter") void handleStartDeviceLogin();
                            }}
                            placeholder="Ov23li..."
                            className="border-border bg-background text-foreground placeholder:text-muted-foreground h-[34px] w-full rounded-lg border px-3 font-mono text-[12.5px] outline-none focus:border-foreground/30"
                          />
                        </div>
                      )}

                      <button
                        type="button"
                        disabled={busy}
                        onClick={() => void handleStartDeviceLogin()}
                        className="bg-foreground text-background inline-flex h-[36px] cursor-pointer items-center gap-2 rounded-lg px-4 text-[13px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-50"
                      >
                        {busy ? (
                          <Loader2 className="h-4 w-4 animate-spin" />
                        ) : (
                          <Cloud className="h-4 w-4" />
                        )}
                        <span>Sign in with GitHub</span>
                      </button>
                    </div>
                  ) : (
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
                        <input
                          type="password"
                          value={tokenInput}
                          onChange={(e) => setTokenInput(e.target.value)}
                          {...ime.compositionProps}
                          onKeyDown={(e) => {
                            if (ime.shouldIgnoreKeyDown(e)) return;
                            if (e.key === "Enter") void handleTokenLogin();
                          }}
                          placeholder="ghp_... or github_pat_..."
                          className="border-border bg-background text-foreground placeholder:text-muted-foreground h-[34px] flex-1 rounded-lg border px-3 font-mono text-[12.5px] outline-none focus:border-foreground/30"
                        />
                        <button
                          type="button"
                          disabled={busy}
                          onClick={() => void handleTokenLogin()}
                          className="bg-foreground text-background inline-flex h-[34px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-50"
                        >
                          {busy ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : (
                            <KeyRound className="h-3.5 w-3.5" />
                          )}
                          <span>Continue</span>
                        </button>
                      </div>
                    </div>
                  )}
                </div>
              </SettingCard>
            </div>
          )}
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
                      <span className="rounded-full bg-amber-500/15 px-2 py-0.5 text-[11px] font-medium text-amber-600 dark:text-amber-400">
                        {status.remote_vault_exists ? "Vault Locked" : "Setup Required"}
                      </span>
                    </div>
                    <div className="text-muted-foreground truncate text-[12px]">
                      {status.remote_vault_exists
                        ? "Encrypted Orbit Vault found on GitHub — enter passphrase to unlock."
                        : "Create a Sync Passphrase to encrypt your first Orbit Vault."}
                    </div>
                  </div>
                </div>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => void orbitActions.logoutCloudSync(false)}
                  className="text-muted-foreground hover:text-foreground inline-flex h-[30px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-2.5 text-[12px] font-medium transition-colors"
                >
                  <LogOut className="h-3.5 w-3.5" />
                  <span>Sign Out</span>
                </button>
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
                  <div className="border-emerald-500/30 bg-emerald-500/8 space-y-2 rounded-[10px] border p-3">
                    <div className="flex items-center justify-between gap-2">
                      <span className="text-[12px] font-medium text-emerald-700 dark:text-emerald-300">
                        Generated Recovery Key — save this in your password manager
                      </span>
                      <button
                        type="button"
                        onClick={() => markCopied("key", generatedKey)}
                        className="border-border bg-background hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium"
                      >
                        {copiedFlag === "key" ? (
                          <>
                            <Check className="h-3 w-3 text-emerald-500" />
                            <span>Copied</span>
                          </>
                        ) : (
                          <>
                            <Copy className="h-3 w-3" />
                            <span>Copy</span>
                          </>
                        )}
                      </button>
                    </div>
                    <div className="selectable-text text-foreground font-mono text-[13px] font-semibold">
                      {generatedKey}
                    </div>
                  </div>
                )}

                <div className="space-y-2">
                  <input
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
                    className="border-border bg-background text-foreground placeholder:text-muted-foreground h-[36px] w-full rounded-lg border px-3 text-[13px] outline-none focus:border-foreground/30"
                  />

                  {!status.remote_vault_exists && !generatedKey && (
                    <input
                      type="password"
                      value={confirmPassphraseInput}
                      onChange={(e) => setConfirmPassphraseInput(e.target.value)}
                      {...ime.compositionProps}
                      onKeyDown={(e) => {
                        if (ime.shouldIgnoreKeyDown(e)) return;
                        if (e.key === "Enter") void handleUnlock();
                      }}
                      placeholder="Confirm Sync Passphrase"
                      className="border-border bg-background text-foreground placeholder:text-muted-foreground h-[36px] w-full rounded-lg border px-3 text-[13px] outline-none focus:border-foreground/30"
                    />
                  )}
                </div>

                <div className="flex items-center justify-between gap-3 pt-1">
                  <span className="text-muted-foreground text-[11.5px]">
                    Cached locally after unlock — you only enter this once per device.
                  </span>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void handleUnlock()}
                    className="bg-foreground text-background inline-flex h-[34px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-4 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-50"
                  >
                    {busy ? (
                      <Loader2 className="h-3.5 w-3.5 animate-spin" />
                    ) : (
                      <Lock className="h-3.5 w-3.5" />
                    )}
                    <span>
                      {status.remote_vault_exists ? "Unlock & Sync" : "Encrypt & Start Sync"}
                    </span>
                  </button>
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
                        <span
                          className={cn(
                            "rounded-full px-2 py-0.5 text-[11px] font-medium",
                            effectiveState === "error"
                              ? "bg-rose-500/15 text-rose-600 dark:text-rose-400"
                              : "bg-emerald-500/15 text-emerald-600 dark:text-emerald-400",
                          )}
                        >
                          {effectiveState === "error"
                            ? "Sync Attention"
                            : `E2EE Synced · rev #${status.last_vault_revision}`}
                        </span>
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

                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void orbitActions.syncCloudNow()}
                    className="bg-foreground text-background inline-flex h-[32px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-50"
                  >
                    <RefreshCw className={cn("h-3.5 w-3.5", busy && "animate-spin")} />
                    <span>{busy ? "Syncing…" : "Sync Now"}</span>
                  </button>
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
                    <button
                      type="button"
                      onClick={() =>
                        void orbitIpc.openExternalBrowserUrl(status.gist_html_url ?? "")
                      }
                      className="border-border bg-background hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
                    >
                      <span>Open on GitHub</span>
                      <ExternalLink className="h-3 w-3" />
                    </button>
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
                    <button
                      type="button"
                      disabled={busy}
                      onClick={() => setConfirmSignOutOpen(true)}
                      className="border-border hover:bg-rose-500/10 hover:text-rose-600 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12px] font-medium transition-colors"
                    >
                      <LogOut className="h-3.5 w-3.5" />
                      <span>Sign Out</span>
                    </button>
                  </div>
                </SettingRow>
              </SettingCard>
            </div>

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
                <button
                  type="button"
                  onClick={() => markCopied("cli", "orbit login")}
                  className="border-border bg-background hover:bg-accent inline-flex h-[30px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium"
                >
                  {copiedFlag === "cli" ? (
                    <>
                      <Check className="h-3.5 w-3.5 text-emerald-500" />
                      <span>Copied</span>
                    </>
                  ) : (
                    <>
                      <Copy className="h-3.5 w-3.5" />
                      <span>Copy</span>
                    </>
                  )}
                </button>
              </div>
            </div>
          </div>
        )}

      {displayError && (
        <div className="border-rose-500/30 bg-rose-500/8 flex items-start gap-2 rounded-[11px] border px-3.5 py-2.5 text-[12.5px] text-rose-600 dark:text-rose-400">
          <AlertCircle className="mt-0.5 h-4 w-4 shrink-0" />
          <span className="selectable-text break-words">{displayError}</span>
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
    </div>
  );
}
