import { useEffect, useState } from "react";
import {
  ArrowUpCircle,
  Check,
  ChevronDown,
  ChevronRight,
  Copy,
  Loader2,
  Radio,
  RefreshCw,
  Trash2,
} from "lucide-react";
import {
  HostStatusIndicator,
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  encodeHostPairingUri,
  hostConnectionLabel,
} from "@/components/settings/settings-primitives";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
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
  const fleet = useOrbitSelector((s) => s.fleet);
  const sshHosts = useOrbitSelector((s) => s.sshHosts);
  const appVersion = useOrbitSelector((s) => s.appVersion);
  const releaseUpdateInfo = useOrbitSelector((s) => s.releaseUpdateInfo);
  const fleetUpdate = useOrbitSelector((s) => s.fleetUpdate);
  const upgradeState = useOrbitSelector((s) => s.hostUpgradeStates[hostId] ?? null);
  const host = fleet.hosts.find((h) => h.host_id === hostId) ?? null;

  const [relayInput, setRelayInput] = useState(host?.relay_url ?? "");
  const [relayStatusMsg, setRelayStatusMsg] = useState<string | null>(null);
  const [showSshDeploy, setShowSshDeploy] = useState(false);
  const [deployingSsh, setDeployingSsh] = useState<string | null>(null);
  const [copiedField, setCopiedField] = useState<string | null>(null);
  const [confirmRemoveOpen, setConfirmRemoveOpen] = useState(false);
  const ime = useImeCompositionGuard();

  useEffect(() => {
    setRelayInput(host?.relay_url ?? "");
    setRelayStatusMsg(null);
    setShowSshDeploy(false);
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

  const handleDeploySshRelay = async (ssh: SshHostItem) => {
    setDeployingSsh(ssh.alias);
    setRelayStatusMsg(`Deploying relay to ${ssh.alias}…`);
    try {
      const url = await orbitIpc.provisionSshRelay({
        targetHostId: host.host_id,
        sshAlias: ssh.alias,
        fallbackHostname: ssh.host_name,
        relayPort: 7878,
      });
      setRelayInput(url);
      setRelayStatusMsg(`Active on ${url}`);
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
        title="Connection"
        description="Network status, identity, mobile pairing, and relay."
        badge={
          <span className="bg-muted text-muted-foreground inline-flex items-center gap-1.5 rounded-md px-2 py-0.5 text-[11.5px] font-medium">
            <HostStatusIndicator state={host.connection_state} />
            <span>{host.display_name}</span>
            {isLocal && <span className="text-muted-foreground/70">· Local</span>}
          </span>
        }
      />

      {/* Status & Actions */}
      <div className="space-y-2">
        <SettingGroupLabel>Status</SettingGroupLabel>
        <SettingCard>
          <SettingRow
            title={hostConnectionLabel(host.connection_state)}
            description={
              isLocal
                ? "Running locally on this device."
                : host.ssh_alias
                  ? `Connected via SSH host ${host.ssh_alias}.`
                  : "Remote Orbit daemon."
            }
          >
            <button
              type="button"
              onClick={() => {
                void orbitIpc.connectHost(host.host_id);
                void orbitIpc.refreshHost(host.host_id);
                orbitActions.showToast(`Reconnecting ${host.display_name}…`);
              }}
              className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
            >
              <RefreshCw className="h-[13px] w-[13px]" />
              <span>Reconnect</span>
            </button>
          </SettingRow>

          <SettingRow
            title="Mobile pairing"
            description="Copy pairing URI for Orbit mobile apps."
          >
            <button
              type="button"
              onClick={() => void copyValue("uri", encodeHostPairingUri(host))}
              className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
            >
              {copiedField === "uri" ? (
                <Check className="h-[13px] w-[13px] text-emerald-500" />
              ) : (
                <Copy className="text-muted-foreground h-[13px] w-[13px]" />
              )}
              <span>{copiedField === "uri" ? "Copied" : "Copy URI"}</span>
            </button>
          </SettingRow>

          <SettingRow
            title={isLocal ? "Local service" : "Host service"}
            description={
              normDaemon
                ? isDaemonOutdated && targetVersion
                  ? `Running v${normDaemon} · v${targetVersion} available (active sessions stay running).`
                  : `Running v${normDaemon} · Up to date.`
                : isLocal
                  ? "Keep the local Orbit background service synced with Desktop."
                  : "Orbit background service on this host."
            }
          >
            <div className="flex items-center gap-1.5">
              {fleet.hosts.length > 1 && (
                <button
                  type="button"
                  disabled={isFleetBusy}
                  onClick={() => void orbitActions.upgradeAllConnectedHosts()}
                  className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
                >
                  {isFleetBusy ? (
                    <Loader2 className="h-[13px] w-[13px] animate-spin" />
                  ) : (
                    <ArrowUpCircle className="h-[13.5px] w-[13.5px]" />
                  )}
                  <span>{isFleetBusy ? "更新全部中…" : "更新全部机器"}</span>
                </button>
              )}
              <button
                type="button"
                disabled={upgrading || isFleetBusy}
                onClick={() => void handleUpgradeDaemon()}
                className="border-border hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
              >
                {upgrading ? (
                  <Loader2 className="h-[13px] w-[13px] animate-spin" />
                ) : isDaemonOutdated ? (
                  <ArrowUpCircle className="h-[13.5px] w-[13.5px]" />
                ) : (
                  <RefreshCw className="h-[13px] w-[13px]" />
                )}
                <span>
                  {upgrading
                    ? "Updating…"
                    : isDaemonOutdated
                      ? "Update"
                      : isLocal
                        ? "Restart"
                        : "Update"}
                </span>
              </button>
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
                  <Check className="h-[12.5px] w-[12.5px] shrink-0 text-emerald-500" />
                ) : (
                  <Copy className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0 opacity-70" />
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
                  <Check className="h-[12.5px] w-[12.5px] shrink-0 text-emerald-500" />
                ) : (
                  <Copy className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0 opacity-70" />
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
            Route remote traffic through an end-to-end encrypted Noise relay.
          </p>

          <div className="flex gap-2">
            <input
              type="text"
              value={relayInput}
              onChange={(e) => setRelayInput(e.target.value)}
              {...ime.compositionProps}
              onKeyDown={(e) => {
                if (ime.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter" && relayDirty) void handleSaveRelay(false);
              }}
              placeholder="wss://relay.example.com"
              className="border-border bg-background text-foreground placeholder:text-muted-foreground h-[34px] flex-1 rounded-lg border px-3 font-mono text-[12.5px] outline-none focus:border-foreground/30"
            />
            <button
              type="button"
              disabled={!relayDirty}
              onClick={() => void handleSaveRelay(false)}
              className="bg-foreground text-background inline-flex h-[34px] cursor-pointer items-center rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-40"
            >
              Save
            </button>
            {host.relay_url && (
              <button
                type="button"
                onClick={() => void handleSaveRelay(true)}
                className="border-border hover:bg-accent inline-flex h-[34px] cursor-pointer items-center rounded-lg border px-3 text-[12.5px] font-medium transition-colors"
              >
                Clear
              </button>
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
                <div className="border-border divide-border mt-2 max-h-40 divide-y overflow-y-auto rounded-[10px] border">
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
                      <button
                        type="button"
                        disabled={deployingSsh !== null}
                        onClick={() => void handleDeploySshRelay(s)}
                        className="border-border hover:bg-accent inline-flex h-[26px] shrink-0 cursor-pointer items-center gap-1.5 rounded-md border px-2.5 text-[11.5px] font-medium transition-colors disabled:opacity-50"
                      >
                        {deployingSsh === s.alias ? (
                          <Loader2 className="h-[12px] w-[12px] animate-spin" />
                        ) : (
                          <Radio className="h-[12px] w-[12px]" />
                        )}
                        <span>Deploy</span>
                      </button>
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

      {/* Danger Zone: Remove non-sole host */}
      {fleet.hosts.length > 1 && (
        <SettingCard>
          <SettingRow
            title="Remove host"
            description="Unpair this machine from Orbit Desktop."
          >
            <button
              type="button"
              onClick={() => setConfirmRemoveOpen(true)}
              className="inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg border border-rose-500/25 px-3 text-[12px] font-medium text-rose-600 transition-colors hover:bg-rose-500/10 dark:text-rose-400"
            >
              <Trash2 className="h-[13px] w-[13px]" />
              <span>Remove</span>
            </button>
          </SettingRow>
        </SettingCard>
      )}

      <ConfirmDialog
        open={confirmRemoveOpen}
        onOpenChange={setConfirmRemoveOpen}
        title={`Remove ${host.display_name}?`}
        description="Unpairs this machine from Orbit Desktop. Running agents and shells on the host keep running, and you can pair it again later."
        confirmLabel="Remove host"
        tone="danger"
        onConfirm={handleRemoveHost}
      />
    </div>
  );
}
