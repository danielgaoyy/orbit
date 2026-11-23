import { useEffect, useState } from "react";
import {
  ArrowUpCircle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  QrCode,
  Radio,
  RefreshCw,
  Trash2,
} from "lucide-react";
import { PairingQrDialog } from "@/components/modals/pairing-qr-dialog";
import {
  HostStatusIndicator,
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  hostConnectionLabel,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  type SshHostItem,
  compareSemver,
  normalizeSemver,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import {
  orbitActions,
  resolveFleetTargetVersion,
  useOrbitSelector,
} from "@/state/orbit-store";

export function HostConnectionSection({ hostId }: { hostId: string }) {
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
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const [confirmRemoveOpen, setConfirmRemoveOpen] = useState(false);
  const [pairingQrOpen, setPairingQrOpen] = useState(false);
  const ime = useImeCompositionGuard();

  useEffect(() => {
    setRelayInput(host?.relay_url ?? "");
    setRelayStatusMsg(null);
    setShowSshDeploy(false);
    setPairingQrOpen(false);
  }, [host?.host_id, host?.relay_url]);

  if (!host) return null;

  const isLocal = isLocalHostCard(host);
  const relayDirty = relayInput.trim() !== (host.relay_url ?? "").trim();

  const copyValue = async (field: string, value: string) => {
    await navigator.clipboard.writeText(value);
    setCopiedField(field);
    window.setTimeout(() => setCopiedField((prev) => (prev === field ? null : prev)), 1500);
  };

  const handleSaveRelay = async (clear = false) => {
    const val = clear || !relayInput.trim() ? null : relayInput.trim();
    await orbitIpc.updateHostRelay(host.host_id, val);
    if (clear) setRelayInput("");
    orbitActions.showToast(val ? "Saved relay endpoint" : "Cleared relay endpoint");
  };

  const handleShareRelayAllHosts = async () => {
    const val = relayInput.trim();
    if (!val) return;
    setSharingRelay(true);
    try {
      const count = await orbitActions.applyRelayToAllHosts(val);
      setRelayStatusMsg(`Shared relay across ${count} host${count === 1 ? "" : "s"}`);
      orbitActions.showToast(`Shared relay with ${count} host${count === 1 ? "" : "s"}`);
    } catch (err) {
      setRelayStatusMsg(String(err));
    } finally {
      setSharingRelay(false);
    }
  };

  const handleDeploySshRelay = async (ssh: SshHostItem) => {
    setDeployingSsh(ssh.alias);
    setRelayStatusMsg(`Deploying relay to ${ssh.alias}…`);
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
      setRelayStatusMsg(`Active across fleet on ${url}`);
    } catch (err) {
      setRelayStatusMsg(String(err));
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
    orbitActions.setSettingsSection("general");
    orbitActions.showToast(`Removed ${host.display_name}`);
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title={t("settings.connectionTitle")}
        description={t("settings.connectionDesc")}
        badge={
          <span className="bg-muted text-muted-foreground inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-[11.5px] font-medium">
            <HostStatusIndicator state={host.connection_state} />
            <span>{host.display_name}</span>
            {isLocal && (
              <span className="text-muted-foreground/70">· {t("common.local")}</span>
            )}
          </span>
        }
      />

      {/* Status & Actions */}
      <div className="space-y-2">
        <SettingGroupLabel>{t("settings.groupStatus")}</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={hostConnectionLabel(host.connection_state)}
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
                void orbitIpc.connectHost(host.host_id);
                void orbitIpc.refreshHost(host.host_id);
                orbitActions.showToast(
                  t("settings.reconnectingHost", { host: host.display_name }),
                );
              }}
              className="px-2.5"
            >
              <RefreshCw className="size-[13px]" />
              <span>{t("banner.reconnect")}</span>
            </Button>
          </SettingRow>

          <SettingRow
            title={t("settings.mobilePairing")}
            description={t("settings.mobilePairingDesc")}
          >
            <div className="flex items-center gap-1.5">
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => setPairingQrOpen(true)}
                className="px-2.5"
              >
                <QrCode className="text-muted-foreground size-[13px]" />
                <span>{t("settings.showQr")}</span>
              </Button>
              <Button
                type="button"
                variant="outline"
                size="sm"
                onClick={() => {
                  void orbitIpc
                    .exportHostPairingUri(host.host_id)
                    .then((uri) => copyValue("uri", uri))
                    .catch((err) => orbitActions.showToast(String(err)));
                }}
                className="px-2.5"
              >
                {copiedField === "uri" ? (
                  <Check className="text-signal-success size-[13px]" />
                ) : (
                  <Copy className="text-muted-foreground size-[13px]" />
                )}
                <span>{copiedField === "uri" ? t("common.copied") : t("settings.copyUri")}</span>
              </Button>
            </div>
          </SettingRow>

          <SettingRow
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
              {fleet.hosts.length > 1 && (
                <Button
                  type="button"
                  variant="outline"
                  size="sm"
                  disabled={isFleetBusy}
                  onClick={() => void orbitActions.upgradeAllConnectedHosts()}
                  className="px-2.5"
                >
                  {isFleetBusy ? (
                    <OrbitSpinner size={13} className="size-[13px]" />
                  ) : (
                    <ArrowUpCircle className="size-[13.5px]" />
                  )}
                  <span>
                    {isFleetBusy
                      ? t("settings.updatingAllHosts")
                      : t("settings.updateAllHosts")}
                  </span>
                </Button>
              )}
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

      {/* Identity */}
      <div className="space-y-2">
        <SettingGroupLabel>Identity</SettingGroupLabel>
        <SettingCard>
          <div className="space-y-2.5 px-4 py-3.5 text-[12.5px]">
            <div className="flex items-center justify-between gap-4">
              <span className="text-muted-foreground shrink-0">Host ID</span>
              <button
                type="button"
                onClick={() => void copyValue("id", host.host_id)}
                className="selectable-text text-foreground/90 hover:text-foreground inline-flex min-w-0 cursor-pointer items-center gap-1.5 font-mono text-[12px]"
              >
                <span className="truncate">{host.host_id}</span>
                {copiedField === "id" ? (
                  <Check className="text-signal-success size-[12.5px] shrink-0" />
                ) : (
                  <Copy className="text-muted-foreground size-[12.5px] shrink-0 opacity-70" />
                )}
              </button>
            </div>

            <div className="flex items-center justify-between gap-4">
              <span className="text-muted-foreground shrink-0">Public Key</span>
              <button
                type="button"
                onClick={() => void copyValue("pubkey", host.daemon_pubkey_b64)}
                className="selectable-text text-foreground/90 hover:text-foreground inline-flex min-w-0 cursor-pointer items-center gap-1.5 font-mono text-[12px]"
              >
                <span className="truncate">{host.daemon_pubkey_b64.slice(0, 26)}…</span>
                {copiedField === "pubkey" ? (
                  <Check className="text-signal-success size-[12.5px] shrink-0" />
                ) : (
                  <Copy className="text-muted-foreground size-[12.5px] shrink-0 opacity-70" />
                )}
              </button>
            </div>

            <div className="flex items-center justify-between gap-4">
              <span className="text-muted-foreground shrink-0">Endpoints</span>
              <span className="selectable-text text-foreground/85 truncate font-mono text-[12px]">
                {host.direct_endpoints.join(", ") || "None"}
              </span>
            </div>
          </div>
        </SettingCard>
      </div>

      {/* Private E2EE Relay */}
      <div className="space-y-2">
        <SettingGroupLabel>Private Relay</SettingGroupLabel>
        <div className="border-border bg-background/60 space-y-3 rounded-[13px] border p-4">
          <p className="text-muted-foreground text-[12.5px] leading-relaxed">
            Route remote traffic through an end-to-end encrypted Noise relay (needed for mobile access off your local network; SSH relay deployment applies across all hosts automatically).
          </p>

          <div className="flex flex-wrap gap-2">
            <Input
              type="text"
              value={relayInput}
              onChange={(e) => setRelayInput(e.target.value)}
              {...ime.compositionProps}
              onKeyDown={(e) => {
                if (ime.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter" && relayDirty) void handleSaveRelay(false);
              }}
              placeholder="wss://relay.example.com"
              className="h-[34px] min-w-[200px] flex-1 px-3 font-mono"
            />
            <Button
              type="button"
              disabled={!relayDirty}
              onClick={() => void handleSaveRelay(false)}
              className="h-[34px] px-3.5"
            >
              Save
            </Button>
            {fleet.hosts.length > 1 && relayInput.trim() && (
              <Button
                type="button"
                variant="outline"
                disabled={sharingRelay}
                onClick={() => void handleShareRelayAllHosts()}
                className="h-[34px] gap-1.5 px-3 text-[12.5px]"
              >
                {sharingRelay && <OrbitSpinner size={13.5} className="size-[13.5px]" />}
                <span>Share with All Hosts</span>
              </Button>
            )}
            {host.relay_url && (
              <Button
                type="button"
                variant="outline"
                onClick={() => void handleSaveRelay(true)}
                className="h-[34px] px-3"
              >
                Clear
              </Button>
            )}
          </div>

          {sshHosts.length > 0 && (
            <div className="pt-0.5">
              <button
                type="button"
                onClick={() => setShowSshDeploy((v) => !v)}
                className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[12px] font-medium transition-colors"
              >
                {showSshDeploy ? (
                  <ChevronDown className="h-[13.5px] w-[13.5px]" />
                ) : (
                  <ChevronRight className="h-[13.5px] w-[13.5px]" />
                )}
                <span>Deploy relay to an SSH server</span>
              </button>

              {showSshDeploy && (
                <div className="orbit-native-scroll border-border divide-border mt-2 max-h-40 divide-y overflow-y-auto overscroll-contain rounded-[10px] border">
                  {sshHosts.map((s) => (
                    <div
                      key={s.alias}
                      className="flex items-center justify-between gap-2 px-3 py-2 text-[12.5px]"
                    >
                      <div className="min-w-0 truncate">
                        <span className="text-foreground font-medium">{s.alias}</span>
                        <span className="selectable-text text-muted-foreground ml-2 font-mono text-[11.5px]">
                          {s.display_target}
                        </span>
                      </div>
                      <Button
                        type="button"
                        variant="outline"
                        size="xs"
                        disabled={deployingSsh !== null}
                        onClick={() => void handleDeploySshRelay(s)}
                        className="shrink-0 gap-1.5 text-[11.5px]"
                      >
                        {deployingSsh === s.alias ? (
                          <OrbitSpinner size={12} className="size-3" />
                        ) : (
                          <Radio className="size-3" />
                        )}
                        <span>Deploy</span>
                      </Button>
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}

          {relayStatusMsg && (
            <div className="selectable-text text-foreground/90 text-[12px]">
              {relayStatusMsg}
            </div>
          )}
        </div>
      </div>

      {/* Danger Zone: Remove host */}
      <SettingCard>
        <SettingRow
          title="Remove host"
          description="Unpair this machine from Orbit Desktop."
        >
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => setConfirmRemoveOpen(true)}
            className="border-signal-danger-border text-signal-danger hover:bg-signal-danger-bg hover:text-signal-danger"
          >
            <Trash2 className="size-[13px]" />
            <span>Remove</span>
          </Button>
        </SettingRow>
      </SettingCard>

      <ConfirmDialog
        open={confirmRemoveOpen}
        onOpenChange={setConfirmRemoveOpen}
        title={`Remove ${host.display_name}?`}
        description="Unpairs this machine from Orbit Desktop. Running agents and shells on the host keep running, and you can pair it again later."
        confirmLabel="Remove"
        tone="danger"
        onConfirm={handleRemoveHost}
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
