import { useEffect, useState } from "react";
import {
  ArrowUpCircle,
  Check,
  ChevronDown,
  ChevronRight,
  Cloud,
  Copy,
  QrCode,
  Radio,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { PairingQrCard, PairingQrDialog } from "@/components/modals/pairing-qr-dialog";
import { ConnectedHostMesh } from "@/components/settings/connected-host-mesh";
import {
  HostStatusIndicator,
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  hostConnectionLabel,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { type SshHostItem, compareSemver, normalizeSemver, orbitIpc } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, resolveFleetTargetVersion, useOrbitSelector } from "@/state/orbit-store";

type PendingRelayConfirm =
  | { kind: "clear" }
  | { kind: "share_all"; relayUrl: string }
  | { kind: "deploy_ssh"; ssh: SshHostItem }
  | { kind: "deploy_cf"; ssh: SshHostItem; publicHostname?: string; tunnelToken?: string };

function redactRelayForDisplay(url: string): string {
  return url
    .replace(/([?&])ft=[^&#]*/gi, "$1ft=***")
    .replace(/([?&])token=[^&#]*/gi, "$1token=***");
}

export function HostConnectionSection({
  hostId,
  onRemoved,
  inlinePairing,
}: {
  hostId: string;
  onRemoved: () => void;
  inlinePairing: boolean;
}) {
  const { t } = useI18n();
  const fleet = useOrbitSelector((s) => s.fleet);
  const sshHosts = useOrbitSelector((s) => s.sshHosts);
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);
  const upgradeState = useOrbitSelector((s) => s.hostUpgradeStates[hostId] ?? null);
  const host = fleet.hosts.find((h) => h.host_id === hostId) ?? null;

  const [relayInput, setRelayInput] = useState(host?.relay_url ?? "");
  const [relayStatusMsg, setRelayStatusMsg] = useState<string | null>(null);
  const [sharingRelay, setSharingRelay] = useState(false);
  const [showSshDeploy, setShowSshDeploy] = useState(false);
  const [deployingSsh, setDeployingSsh] = useState<string | null>(null);
  const [showCfCustom, setShowCfCustom] = useState(false);
  const [cfHostname, setCfHostname] = useState("");
  const [cfTunnelToken, setCfTunnelToken] = useState("");
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const [confirmRemoveOpen, setConfirmRemoveOpen] = useState(false);
  const [relayConfirmOpen, setRelayConfirmOpen] = useState(false);
  const [pendingRelayConfirm, setPendingRelayConfirm] = useState<PendingRelayConfirm | null>(null);
  const [pairingQrOpen, setPairingQrOpen] = useState(false);
  const [relayExpanded, setRelayExpanded] = useState(false);
  const ime = useImeCompositionGuard();

  useEffect(() => {
    setRelayInput(host?.relay_url ?? "");
    setRelayStatusMsg(null);
    setShowSshDeploy(false);
    setRelayExpanded(false);
    setPairingQrOpen(false);
  }, [host?.host_id, host?.relay_url]);

  useEffect(() => {
    setRelayConfirmOpen(false);
    setPendingRelayConfirm(null);
  }, [host?.host_id]);

  if (!host) return null;

  const isLocal = isLocalHostCard(host);
  const relayDirty = relayInput.trim() !== (host.relay_url ?? "").trim();

  const openRelayConfirm = (action: PendingRelayConfirm) => {
    setPendingRelayConfirm(action);
    setRelayConfirmOpen(true);
  };

  const copyValue = async (field: string, value: string) => {
    try {
      await navigator.clipboard.writeText(value);
    } catch (err) {
      orbitActions.showToast(String(err));
      return;
    }
    setCopiedField(field);
    window.setTimeout(() => setCopiedField((prev) => (prev === field ? null : prev)), 1500);
  };

  const handleSaveRelay = async (clear = false) => {
    const val = clear || !relayInput.trim() ? null : relayInput.trim();
    await orbitIpc.updateHostRelay(host.host_id, val);
    if (clear) setRelayInput("");
    orbitActions.showToast(t(val ? "settings.fleetRelaySaved" : "settings.fleetRelayCleared"));
  };

  const handleRequestSaveRelay = () => {
    if (!relayInput.trim() && host.relay_url) {
      openRelayConfirm({ kind: "clear" });
      return;
    }
    void handleSaveRelay(false).catch((err) => setRelayStatusMsg(String(err)));
  };

  const handleShareRelayAllHosts = async (relayUrl?: string) => {
    const val = (relayUrl ?? relayInput).trim();
    if (!val) return;
    setSharingRelay(true);
    try {
      const count = await orbitActions.applyRelayToAllHosts(val);
      setRelayStatusMsg(t("settings.fleetRelayShared", { count }));
      orbitActions.showToast(t("settings.fleetRelayShared", { count }));
    } catch (err) {
      setRelayStatusMsg(String(err));
      throw err;
    } finally {
      setSharingRelay(false);
    }
  };

  const handleDeploySshRelay = async (ssh: SshHostItem) => {
    setDeployingSsh(ssh.alias);
    setRelayStatusMsg(t("settings.fleetRelayDeploying", { host: ssh.alias }));
    try {
      const url = await orbitIpc.provisionSshRelay({
        targetHostId: host.host_id,
        sshAlias: ssh.alias,
        fallbackHostname: ssh.host_name,
        relayPort: 7878,
        applyToAllHosts: true,
      });
      setRelayInput(url);
      await orbitActions.reloadFleetAndHosts();
      setRelayStatusMsg(t("settings.fleetRelayActive", { url: redactRelayForDisplay(url) }));
    } catch (err) {
      setRelayStatusMsg(String(err));
      throw err;
    } finally {
      setDeployingSsh(null);
    }
  };

  const handleDeployCfRelay = async (
    ssh: SshHostItem,
    publicHostname?: string,
    tunnelToken?: string,
  ) => {
    setDeployingSsh(ssh.alias);
    setRelayStatusMsg(t("settings.fleetCfDeploying", { host: ssh.alias }));
    try {
      const url = await orbitIpc.provisionCloudflareTunnelRelay({
        targetHostId: host.host_id,
        sshAlias: ssh.alias,
        relayPort: 7878,
        publicHostname: publicHostname?.trim() || null,
        tunnelToken: tunnelToken?.trim() || null,
        applyToAllHosts: true,
      });
      setRelayInput(url);
      await orbitActions.reloadFleetAndHosts();
      setRelayStatusMsg(t("settings.fleetRelayActive", { url: redactRelayForDisplay(url) }));
    } catch (err) {
      setRelayStatusMsg(String(err));
      throw err;
    } finally {
      setDeployingSsh(null);
    }
  };

  const upgrading = upgradeState?.status === "upgrading";
  const isFleetBusy =
    fleetUpdate.phase === "checking" ||
    fleetUpdate.phase === "upgrading_hosts" ||
    fleetUpdate.phase === "upgrading_desktop";
  const targetVersion = resolveFleetTargetVersion(appVersion, releaseUpdateInfo);
  const normDaemon = normalizeSemver(host.daemon_version);
  const isDaemonOutdated = Boolean(
    targetVersion && normDaemon && compareSemver(normDaemon, targetVersion) < 0,
  );

  const handleUpgradeDaemon = async () => {
    await orbitActions.upgradeHostDaemon(host.host_id, {
      force: true,
      targetVersion,
    });
  };

  const handleRemoveHost = async () => {
    await orbitIpc.removeHost(host.host_id);
    await orbitActions.reloadFleetAndHosts();
    onRemoved();
    orbitActions.showToast(t("settings.fleetRemoved", { host: host.display_name }));
  };

  return (
    <div className="flex flex-col gap-[20px]">
      {/* Status & Actions */}
      <div className="space-y-2">
        <SettingGroupLabel>
          {inlinePairing ? host.display_name : t("settings.groupStatus")}
        </SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={
              <span className="inline-flex items-center gap-2">
                <HostStatusIndicator state={host.connection_state} />
                {hostConnectionLabel(host.connection_state)}
              </span>
            }
            description={
              isLocal
                ? t("settings.runningLocally")
                : host.ssh_alias
                  ? t("settings.connectedViaSsh", { alias: host.ssh_alias })
                  : t("settings.remoteOrbitDaemon")
            }
          >
            <Button
              type="button"
              variant="outline"
              size="sm"
              onClick={() => {
                void orbitIpc
                  .connectHost(host.host_id)
                  .then(() => orbitIpc.refreshHost(host.host_id))
                  .catch((err) => orbitActions.showToast(String(err)));
                orbitActions.showToast(t("settings.reconnectingHost", { host: host.display_name }));
              }}
              className="px-2.5"
            >
              <RefreshCw className="size-[13px]" />
              <span>{t("banner.reconnect")}</span>
            </Button>
          </SettingRow>

          <SettingRow
            settingId="settings.mobilePairing"
            title={t("settings.mobilePairing")}
            description={t("settings.pairingQrDesc", { host: host.display_name })}
          >
            {inlinePairing ? (
              <PairingQrCard
                compact
                hostId={hostId}
                hostName={host.display_name}
                className="max-w-[224px]"
              />
            ) : (
              <Button variant="outline" size="sm" onClick={() => setPairingQrOpen(true)}>
                <QrCode data-icon="inline-start" />
                {t("settings.showQr")}
              </Button>
            )}
          </SettingRow>

          <SettingRow
            settingId="settings.hostService"
            title={isLocal ? t("settings.localService") : t("settings.hostService")}
            description={
              normDaemon
                ? isDaemonOutdated && targetVersion
                  ? t("settings.runningVersionOutdated", {
                      current: normDaemon,
                      target: targetVersion,
                    })
                  : t("settings.runningVersionCurrent", { current: normDaemon })
                : isLocal
                  ? t("settings.localServiceDesc")
                  : t("settings.hostServiceDesc")
            }
          >
            <div className="flex items-center gap-1.5">
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={upgrading || isFleetBusy}
                onClick={() => void handleUpgradeDaemon()}
                className="px-2.5"
              >
                {upgrading ? (
                  <OrbitSpinner size={13} className="size-[13px]" />
                ) : isDaemonOutdated ? (
                  <ArrowUpCircle className="size-[13.5px]" />
                ) : (
                  <RefreshCw className="size-[13px]" />
                )}
                <span>
                  {upgrading
                    ? t("common.updating")
                    : isDaemonOutdated
                      ? t("common.update")
                      : isLocal
                        ? t("settings.restart")
                        : t("common.update")}
                </span>
              </Button>
            </div>
          </SettingRow>
        </SettingCard>
      </div>

      <ConnectedHostMesh hostId={hostId} />

      {/* Identity */}
      <details className="group border-border-subtle rounded-xl border bg-surface-raised">
        <summary className="flex cursor-pointer list-none items-center gap-2 px-4 py-3 text-[13px] font-medium">
          <ChevronRight className="size-[13px] text-ink-muted transition-transform group-open:rotate-90" />
          {t("settings.fleetHostIdentity")}
        </summary>

        <div className="space-y-2.5 px-4 py-3.5 text-[13px]">
          <div className="flex items-center justify-between gap-4">
            <span className="text-ink-muted shrink-0">Host ID</span>
            <button
              type="button"
              onClick={() => void copyValue("id", host.host_id)}
              className="selectable-text text-ink-primary/90 hover:text-ink-primary inline-flex min-w-0 cursor-pointer items-center gap-1.5 font-mono text-[13px]"
            >
              <span className="truncate">{host.host_id}</span>
              {copiedField === "id" ? (
                <Check className="text-signal-success size-[12.5px] shrink-0" />
              ) : (
                <Copy className="text-ink-muted size-[12.5px] shrink-0 opacity-70" />
              )}
            </button>
          </div>

          <div className="flex items-center justify-between gap-4">
            <span className="text-ink-muted shrink-0">{t("settings.fleetPublicKey")}</span>
            <button
              type="button"
              onClick={() => void copyValue("pubkey", host.daemon_pubkey_b64)}
              className="selectable-text text-ink-primary/90 hover:text-ink-primary inline-flex min-w-0 cursor-pointer items-center gap-1.5 font-mono text-[13px]"
            >
              <span className="truncate">{host.daemon_pubkey_b64.slice(0, 26)}…</span>
              {copiedField === "pubkey" ? (
                <Check className="text-signal-success size-[12.5px] shrink-0" />
              ) : (
                <Copy className="text-ink-muted size-[12.5px] shrink-0 opacity-70" />
              )}
            </button>
          </div>

          <div className="flex items-center justify-between gap-4">
            <span className="text-ink-muted shrink-0">{t("settings.fleetEndpoints")}</span>
            <span className="selectable-text text-ink-primary/85 truncate font-mono text-[13px]">
              {host.direct_endpoints.join(", ") || "—"}
            </span>
          </div>
        </div>
      </details>

      {/* Advanced Network & Custom Relay (Collapsed by default; auto-managed across Fleet) */}
      <Collapsible
        data-setting-id="settings.advancedRelayTitle"
        open={relayExpanded}
        onOpenChange={setRelayExpanded}
        className="border-border-subtle bg-surface-raised/60 rounded-[13px] border px-4 py-3"
      >
        <CollapsibleTrigger className="group/relay flex w-full cursor-pointer items-center justify-between gap-2 text-left outline-none">
          <span className="flex min-w-0 items-center gap-2">
            <ChevronRight className="text-ink-muted size-3.5 shrink-0 transition-transform duration-150 group-data-[panel-open]/relay:rotate-90" />
            <span className="text-ink-primary text-[13px] font-medium">
              {t("settings.advancedRelayTitle")}
            </span>
          </span>
          <span className="bg-surface-recessed text-ink-muted max-w-[220px] shrink-0 truncate rounded-md px-2 py-0.5 font-mono text-[11px]">
            {host.relay_url
              ? redactRelayForDisplay(host.relay_url)
              : t("settings.advancedRelayAutoBadge")}
          </span>
        </CollapsibleTrigger>

        <CollapsibleContent className="space-y-3 pt-3 transition-opacity">
          <p className="text-ink-muted text-[13px] leading-relaxed">
            {t("settings.advancedRelayDesc")}
          </p>

          <div className="flex flex-wrap gap-2">
            <Input
              type="text"
              value={relayInput}
              onChange={(e) => setRelayInput(e.target.value)}
              {...ime.compositionProps}
              onKeyDown={(e) => {
                if (ime.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter" && relayDirty) handleRequestSaveRelay();
              }}
              placeholder="wss://relay.example.com"
              className="h-[34px] min-w-[200px] flex-1 px-3 font-mono"
            />
            <Button
              type="button"
              disabled={!relayDirty}
              onClick={handleRequestSaveRelay}
              className="h-[34px] px-3.5"
            >
              {t("settings.fleetSave")}
            </Button>
            {fleet.hosts.length > 1 && relayInput.trim() && (
              <Button
                type="button"
                variant="outline"
                disabled={sharingRelay}
                onClick={() => openRelayConfirm({ kind: "share_all", relayUrl: relayInput.trim() })}
                className="h-[34px] gap-1.5 px-3 text-[13px]"
              >
                {sharingRelay && <OrbitSpinner size={13.5} className="size-[13.5px]" />}
                <span>{t("settings.fleetShareRelay")}</span>
              </Button>
            )}
            {host.relay_url && (
              <Button
                type="button"
                variant="outline"
                onClick={() => openRelayConfirm({ kind: "clear" })}
                className="h-[34px] px-3"
              >
                {t("settings.fleetClear")}
              </Button>
            )}
          </div>

          {sshHosts.length > 0 && (
            <div className="pt-0.5">
              <button
                type="button"
                onClick={() => setShowSshDeploy((v) => !v)}
                className="text-ink-muted hover:text-ink-primary inline-flex cursor-pointer items-center gap-1 text-[13px] font-medium transition-colors"
              >
                {showSshDeploy ? (
                  <ChevronDown className="h-[13.5px] w-[13.5px]" />
                ) : (
                  <ChevronRight className="h-[13.5px] w-[13.5px]" />
                )}
                <span>{t("settings.fleetDeploySsh")}</span>
              </button>

              {showSshDeploy && (
                <div className="space-y-2.5 pt-2">
                  <div className="orbit-native-scroll border-border-subtle divide-border-subtle max-h-48 divide-y overflow-y-auto overscroll-contain rounded-[10px] border">
                    {sshHosts.map((s) => (
                      <div
                        key={s.alias}
                        className="flex items-center justify-between gap-2 px-3 py-2 text-[13px]"
                      >
                        <div className="min-w-0 truncate">
                          <span className="text-ink-primary font-medium">{s.alias}</span>
                          <span className="selectable-text text-ink-muted ml-2 font-mono text-[11px]">
                            {s.display_target}
                          </span>
                        </div>
                        <div className="flex shrink-0 items-center gap-1.5">
                          <Button
                            type="button"
                            variant="outline"
                            size="xs"
                            disabled={deployingSsh !== null}
                            onClick={() =>
                              openRelayConfirm({
                                kind: "deploy_cf",
                                ssh: s,
                                publicHostname: cfHostname.trim() || undefined,
                                tunnelToken: cfTunnelToken.trim() || undefined,
                              })
                            }
                            className="shrink-0 gap-1.5 text-[11px]"
                          >
                            {deployingSsh === s.alias ? (
                              <OrbitSpinner size={12} className="size-3" />
                            ) : (
                              <Cloud className="size-3" />
                            )}
                            <span>{t("settings.deployCloudflareShort")}</span>
                          </Button>
                          <Button
                            type="button"
                            variant="outline"
                            size="xs"
                            disabled={deployingSsh !== null}
                            onClick={() => openRelayConfirm({ kind: "deploy_ssh", ssh: s })}
                            className="shrink-0 gap-1.5 text-[11px]"
                          >
                            {deployingSsh === s.alias ? (
                              <OrbitSpinner size={12} className="size-3" />
                            ) : (
                              <Radio className="size-3" />
                            )}
                            <span>{t("settings.fleetDeploy")}</span>
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>

                  <div className="space-y-2">
                    <button
                      type="button"
                      onClick={() => setShowCfCustom((v) => !v)}
                      className="text-ink-muted hover:text-ink-primary inline-flex cursor-pointer items-center gap-1 text-[11px] font-medium transition-colors"
                    >
                      {showCfCustom ? (
                        <ChevronDown className="size-3" />
                      ) : (
                        <ChevronRight className="size-3" />
                      )}
                      <span>{t("settings.deployCfCustomToggle")}</span>
                    </button>

                    {showCfCustom && (
                      <div className="grid gap-2 sm:grid-cols-2">
                        <Input
                          type="text"
                          value={cfHostname}
                          onChange={(e) => setCfHostname(e.target.value)}
                          {...ime.compositionProps}
                          placeholder={t("settings.deployCfHostnamePlaceholder")}
                          className="h-[32px] px-2.5 font-mono text-[13px]"
                        />
                        <Input
                          type="password"
                          value={cfTunnelToken}
                          onChange={(e) => setCfTunnelToken(e.target.value)}
                          {...ime.compositionProps}
                          placeholder={t("settings.deployCfTokenPlaceholder")}
                          className="h-[32px] px-2.5 font-mono text-[13px]"
                        />
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          )}

          {relayStatusMsg && (
            <div className="selectable-text text-ink-primary/90 text-[13px]">{relayStatusMsg}</div>
          )}
        </CollapsibleContent>
      </Collapsible>

      {/* Danger Zone: Remove host */}
      <SettingCard>
        <SettingRow
          settingId="settings.fleetRemoveHost"
          title={t("settings.fleetRemoveHost")}
          description={t("settings.fleetRemoveDesc")}
        >
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setConfirmRemoveOpen(true)}
            className="border-signal-danger-border text-signal-danger hover:bg-signal-danger-bg hover:text-signal-danger"
          >
            <Trash2 className="size-[13px]" />
            <span>{t("settings.fleetRemoveHost")}</span>
          </Button>
        </SettingRow>
      </SettingCard>

      <ConfirmDialog
        open={confirmRemoveOpen}
        onOpenChange={setConfirmRemoveOpen}
        title={t("settings.fleetRemoveConfirm", { host: host.display_name })}
        description={t("settings.fleetRemoveExplanation")}
        confirmLabel={t("settings.fleetRemoveHost")}
        tone="danger"
        defaultAction="cancel"
        onConfirm={handleRemoveHost}
      />

      <ConfirmDialog
        open={relayConfirmOpen && pendingRelayConfirm !== null}
        onOpenChange={setRelayConfirmOpen}
        title={
          pendingRelayConfirm?.kind === "clear"
            ? t("settings.fleetClearRelayConfirm", { host: host.display_name })
            : pendingRelayConfirm?.kind === "share_all"
              ? t("settings.fleetOverwriteRelayConfirm", { count: fleet.hosts.length })
              : pendingRelayConfirm?.kind === "deploy_ssh"
                ? t("settings.fleetDeployRelayConfirm", { host: pendingRelayConfirm.ssh.alias })
                : pendingRelayConfirm?.kind === "deploy_cf"
                  ? t("settings.deployCfConfirmTitle", { alias: pendingRelayConfirm.ssh.alias })
                  : ""
        }
        description={
          pendingRelayConfirm?.kind === "clear"
            ? t("settings.fleetClearRelayDesc")
            : pendingRelayConfirm?.kind === "share_all"
              ? t("settings.fleetOverwriteRelayDesc")
              : pendingRelayConfirm?.kind === "deploy_ssh"
                ? t("settings.fleetDeployRelayDesc", { count: fleet.hosts.length })
                : pendingRelayConfirm?.kind === "deploy_cf"
                  ? t("settings.deployCfConfirmDesc", {
                      alias: pendingRelayConfirm.ssh.alias,
                      count: fleet.hosts.length,
                    })
                  : undefined
        }
        detail={
          pendingRelayConfirm?.kind === "clear" && host.relay_url ? (
            <span className="selectable-text font-mono text-[11px] break-all">
              {redactRelayForDisplay(host.relay_url)}
            </span>
          ) : pendingRelayConfirm?.kind === "share_all" ? (
            <span className="selectable-text font-mono text-[11px] break-all">
              {redactRelayForDisplay(pendingRelayConfirm.relayUrl)}
            </span>
          ) : pendingRelayConfirm?.kind === "deploy_ssh" ||
            pendingRelayConfirm?.kind === "deploy_cf" ? (
            <span className="selectable-text font-mono text-[11px] break-all">
              {pendingRelayConfirm.ssh.alias} ({pendingRelayConfirm.ssh.display_target})
            </span>
          ) : undefined
        }
        confirmLabel={
          pendingRelayConfirm?.kind === "clear"
            ? t("settings.fleetClearRelay")
            : pendingRelayConfirm?.kind === "share_all"
              ? t("settings.fleetOverwriteRelay")
              : t("settings.fleetDeployReplace")
        }
        tone="danger"
        defaultAction="cancel"
        onConfirm={async () => {
          if (!pendingRelayConfirm) return;
          if (pendingRelayConfirm.kind === "clear") {
            await handleSaveRelay(true);
          } else if (pendingRelayConfirm.kind === "share_all") {
            await handleShareRelayAllHosts(pendingRelayConfirm.relayUrl);
          } else if (pendingRelayConfirm.kind === "deploy_cf") {
            await handleDeployCfRelay(
              pendingRelayConfirm.ssh,
              pendingRelayConfirm.publicHostname,
              pendingRelayConfirm.tunnelToken,
            );
          } else {
            await handleDeploySshRelay(pendingRelayConfirm.ssh);
          }
        }}
      />

      <PairingQrDialog
        open={pairingQrOpen}
        onOpenChange={setPairingQrOpen}
        hostId={host.host_id}
        hostName={host.display_name}
      />
    </div>
  );
}
