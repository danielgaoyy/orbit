import { memo, useEffect, useState } from "react";
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
  SettingDisclosureCard,
  SettingGroup,
  SettingRow,
  hostConnectionLabel,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
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

export const HostConnectionSection = memo(function HostConnectionSection({
  hostId,
  onRemoved,
  inlinePairing,
}: {
  hostId: string;
  onRemoved: () => void;
  inlinePairing: boolean;
}) {
  const { t } = useI18n();
  const host = useOrbitSelector((s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null);
  const hostCount = useOrbitSelector((s) => s.fleet.hosts.length);
  const sshHosts = useOrbitSelector((s) => s.sshHosts);
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);
  const upgradeState = useOrbitSelector((s) => s.hostUpgradeStates[hostId] ?? null);

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
    <div className="flex flex-col gap-6">
      {/* Status, Service & Mobile Pairing */}
      <SettingGroup title={inlinePairing ? host.display_name : t("settings.groupStatus")}>
        <SettingCard>
          <SettingRow
            title={
              <span className="inline-flex items-center gap-2">
                <HostStatusIndicator state={host.connection_state} />
                <span>{hostConnectionLabel(host.connection_state)}</span>
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
              className="text-[13px]"
            >
              <RefreshCw className="size-[13px]" />
              <span>{t("banner.reconnect")}</span>
            </Button>
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
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={upgrading || isFleetBusy}
              onClick={() => void handleUpgradeDaemon()}
              className="text-[13px]"
            >
              {upgrading ? (
                <OrbitSpinner size={13} />
              ) : isDaemonOutdated ? (
                <ArrowUpCircle className="size-[13px]" />
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
          </SettingRow>

          {inlinePairing ? (
            <div data-setting-id="settings.mobilePairing">
              <PairingQrCard
                layout="inline"
                hostId={hostId}
                hostName={host.display_name}
                title={t("settings.mobilePairing")}
                description={t("settings.pairingQrDesc", { host: host.display_name })}
              />
            </div>
          ) : (
            <SettingRow
              settingId="settings.mobilePairing"
              title={t("settings.mobilePairing")}
              description={t("settings.pairingQrDesc", { host: host.display_name })}
            >
              <Button
                variant="outline"
                size="sm"
                onClick={() => setPairingQrOpen(true)}
                className="text-[13px]"
              >
                <QrCode className="size-[13px]" />
                <span>{t("settings.showQr")}</span>
              </Button>
            </SettingRow>
          )}
        </SettingCard>
      </SettingGroup>

      <ConnectedHostMesh hostId={hostId} />

      {/* Identity & Advanced Network Disclosures */}
      <div className="flex flex-col gap-3">
        <SettingDisclosureCard
          title={t("settings.fleetHostIdentity")}
          meta={
            <Badge variant="secondary" className="max-w-[180px] truncate font-mono tabular-nums">
              {host.host_id}
            </Badge>
          }
          contentClassName="divide-border-subtle divide-y"
        >
          <div className="flex items-center justify-between gap-4 px-4 py-2 text-[13px]">
            <span className="text-ink-muted shrink-0">{t("settings.fleetHostId")}</span>
            <button
              type="button"
              onClick={() => void copyValue("id", host.host_id)}
              className="selectable-text text-ink-primary hover:bg-surface-recessed inline-flex min-w-0 cursor-pointer items-center gap-2 rounded-md px-2 py-1 font-mono text-[13px] transition-colors"
            >
              <span className="truncate">{host.host_id}</span>
              {copiedField === "id" ? (
                <Check className="text-signal-success size-[13px] shrink-0" />
              ) : (
                <Copy className="text-ink-muted size-[13px] shrink-0 opacity-70" />
              )}
            </button>
          </div>

          <div className="flex items-center justify-between gap-4 px-4 py-2 text-[13px]">
            <span className="text-ink-muted shrink-0">{t("settings.fleetPublicKey")}</span>
            <button
              type="button"
              onClick={() => void copyValue("pubkey", host.daemon_pubkey_b64)}
              className="selectable-text text-ink-primary hover:bg-surface-recessed inline-flex min-w-0 cursor-pointer items-center gap-2 rounded-md px-2 py-1 font-mono text-[13px] transition-colors"
            >
              <span className="truncate">{host.daemon_pubkey_b64.slice(0, 26)}…</span>
              {copiedField === "pubkey" ? (
                <Check className="text-signal-success size-[13px] shrink-0" />
              ) : (
                <Copy className="text-ink-muted size-[13px] shrink-0 opacity-70" />
              )}
            </button>
          </div>

          <div className="flex items-center justify-between gap-4 px-4 py-2 text-[13px]">
            <span className="text-ink-muted shrink-0">{t("settings.fleetEndpoints")}</span>
            <span className="selectable-text text-ink-secondary truncate px-2 py-1 font-mono text-[13px]">
              {host.direct_endpoints.join(", ") || "—"}
            </span>
          </div>
        </SettingDisclosureCard>

        <SettingDisclosureCard
          settingId="settings.advancedRelayTitle"
          open={relayExpanded}
          onOpenChange={setRelayExpanded}
          title={t("settings.advancedRelayTitle")}
          meta={
            <Badge variant="secondary" className="max-w-[220px] truncate font-mono">
              {host.relay_url
                ? redactRelayForDisplay(host.relay_url)
                : t("settings.advancedRelayAutoBadge")}
            </Badge>
          }
          contentClassName="flex flex-col gap-3 px-4 py-3"
        >
          <p className="text-ink-secondary text-[13px] leading-[1.45]">
            {t("settings.advancedRelayDesc")}
          </p>

          <div className="flex flex-wrap items-center gap-2">
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
              className="h-[30px] min-w-[200px] flex-1 px-3 font-mono text-[13px]"
            />
            <Button
              type="button"
              size="sm"
              disabled={!relayDirty}
              onClick={handleRequestSaveRelay}
              className="text-[13px]"
            >
              {t("settings.fleetSave")}
            </Button>
            {hostCount > 1 && relayInput.trim() && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                disabled={sharingRelay}
                onClick={() => openRelayConfirm({ kind: "share_all", relayUrl: relayInput.trim() })}
                className="text-[13px]"
              >
                {sharingRelay && <OrbitSpinner size={13} />}
                <span>{t("settings.fleetShareRelay")}</span>
              </Button>
            )}
            {host.relay_url && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => openRelayConfirm({ kind: "clear" })}
                className="text-[13px]"
              >
                {t("settings.fleetClear")}
              </Button>
            )}
          </div>

          {sshHosts.length > 0 && (
            <div className="flex flex-col gap-2 pt-1">
              <button
                type="button"
                onClick={() => setShowSshDeploy((v) => !v)}
                className="text-ink-secondary hover:text-ink-primary inline-flex w-fit cursor-pointer items-center gap-2 text-[13px] font-medium transition-colors"
              >
                {showSshDeploy ? (
                  <ChevronDown className="size-[13px]" />
                ) : (
                  <ChevronRight className="size-[13px]" />
                )}
                <span>{t("settings.fleetDeploySsh")}</span>
              </button>

              {showSshDeploy && (
                <div className="flex flex-col gap-2 pt-1">
                  <div className="orbit-native-scroll border-border-subtle bg-surface-recessed/40 divide-border-subtle max-h-48 divide-y overflow-y-auto overscroll-contain rounded-[10px] border">
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
                        <div className="flex shrink-0 items-center gap-2">
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
                            className="shrink-0 gap-2 text-[11px]"
                          >
                            {deployingSsh === s.alias ? (
                              <OrbitSpinner size={12} />
                            ) : (
                              <Cloud className="size-[12px]" />
                            )}
                            <span>{t("settings.deployCloudflareShort")}</span>
                          </Button>
                          <Button
                            type="button"
                            variant="outline"
                            size="xs"
                            disabled={deployingSsh !== null}
                            onClick={() => openRelayConfirm({ kind: "deploy_ssh", ssh: s })}
                            className="shrink-0 gap-2 text-[11px]"
                          >
                            {deployingSsh === s.alias ? (
                              <OrbitSpinner size={12} />
                            ) : (
                              <Radio className="size-[12px]" />
                            )}
                            <span>{t("settings.fleetDeploy")}</span>
                          </Button>
                        </div>
                      </div>
                    ))}
                  </div>

                  <div className="flex flex-col gap-2">
                    <button
                      type="button"
                      onClick={() => setShowCfCustom((v) => !v)}
                      className="text-ink-muted hover:text-ink-primary inline-flex w-fit cursor-pointer items-center gap-1 text-[11px] font-medium transition-colors"
                    >
                      {showCfCustom ? (
                        <ChevronDown className="size-[12px]" />
                      ) : (
                        <ChevronRight className="size-[12px]" />
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
                          className="h-[30px] px-3 font-mono text-[13px]"
                        />
                        <Input
                          type="password"
                          value={cfTunnelToken}
                          onChange={(e) => setCfTunnelToken(e.target.value)}
                          {...ime.compositionProps}
                          placeholder={t("settings.deployCfTokenPlaceholder")}
                          className="h-[30px] px-3 font-mono text-[13px]"
                        />
                      </div>
                    )}
                  </div>
                </div>
              )}
            </div>
          )}

          {relayStatusMsg && (
            <div className="selectable-text border-border-subtle bg-surface-recessed/50 text-ink-secondary rounded-lg border px-3 py-2 text-[13px]">
              {relayStatusMsg}
            </div>
          )}
        </SettingDisclosureCard>
      </div>

      {/* Danger Zone: Remove host */}
      <SettingCard className="border-signal-danger-border/55">
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
            className="border-signal-danger-border text-signal-danger hover:bg-signal-danger-bg hover:text-signal-danger text-[13px]"
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
              ? t("settings.fleetOverwriteRelayConfirm", { count: hostCount })
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
                ? t("settings.fleetDeployRelayDesc", { count: hostCount })
                : pendingRelayConfirm?.kind === "deploy_cf"
                  ? t("settings.deployCfConfirmDesc", {
                      alias: pendingRelayConfirm.ssh.alias,
                      count: hostCount,
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
});
