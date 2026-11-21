import { useState } from "react";
import { ArrowRight, Check, Cloud, Loader2, Plus, Server } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
} from "@/components/settings/settings-primitives";
import { useImeCompositionGuard } from "@/lib/ime";
import { type SshHostItem, orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export function AddHostSection() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const sshHosts = useOrbitSelector((s) => s.sshHosts);
  const cloudSyncStatus = useOrbitSelector((s) => s.cloudSyncStatus);

  const [pairingUri, setPairingUri] = useState("");
  const [showManualUri, setShowManualUri] = useState(false);
  const [connectingSshAlias, setConnectingSshAlias] = useState<string | null>(null);
  const [errorMsg, setErrorMsg] = useState<string | null>(null);
  const ime = useImeCompositionGuard();

  const handlePairSsh = async (ssh: SshHostItem) => {
    setErrorMsg(null);
    setConnectingSshAlias(ssh.alias);
    try {
      const card = await orbitIpc.pairSshHost(ssh.alias);
      await orbitActions.reloadFleetAndHosts();
      orbitActions.selectHost(card.host_id);
      orbitActions.setSettingsSection(`host:${card.host_id}:agents`);
      orbitActions.showToast(`Connected ${card.display_name}`);
    } catch (err) {
      setErrorMsg(String(err));
    } finally {
      setConnectingSshAlias(null);
    }
  };

  const handlePairUri = async () => {
    if (!pairingUri.trim()) return;
    setErrorMsg(null);
    try {
      const card = await orbitIpc.pairHost(pairingUri.trim());
      await orbitActions.reloadFleetAndHosts();
      orbitActions.selectHost(card.host_id);
      setPairingUri("");
      setShowManualUri(false);
      orbitActions.setSettingsSection(`host:${card.host_id}:agents`);
      orbitActions.showToast(`Paired ${card.display_name}`);
    } catch (err) {
      setErrorMsg(String(err));
    }
  };

  return (
    <div className="space-y-6">
      <SettingSectionHeader
        title="Add Host"
        description="Connect a remote server or workstation running Orbit."
      />

      {cloudSyncStatus?.state !== "ready" && (
        <div className="border-border/80 bg-background/60 flex items-center justify-between gap-3 rounded-[13px] border px-4 py-3">
          <div className="flex min-w-0 items-center gap-3">
            <div className="bg-muted flex h-8 w-8 shrink-0 items-center justify-center rounded-lg">
              <Cloud className="text-foreground/80 h-4 w-4" />
            </div>
            <div className="min-w-0">
              <div className="text-foreground text-[13px] font-medium">
                {cloudSyncStatus?.state === "locked"
                  ? `Unlock Cloud Sync (@${cloudSyncStatus.account_login})`
                  : "Sync existing hosts via GitHub"}
              </div>
              <div className="text-muted-foreground truncate text-[12px]">
                {cloudSyncStatus?.state === "locked"
                  ? "Enter your Sync Passphrase to pull all paired hosts automatically."
                  : "Zero-server E2EE sync — skip entering pairing codes on every device."}
              </div>
            </div>
          </div>
          <button
            type="button"
            onClick={() => orbitActions.setSettingsSection("cloud_sync")}
            className="border-border bg-background hover:bg-accent inline-flex h-[30px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg border px-3 text-[12px] font-medium transition-colors"
          >
            <span>{cloudSyncStatus?.state === "locked" ? "Unlock" : "Sign in"}</span>
            <ArrowRight className="h-3.5 w-3.5" />
          </button>
        </div>
      )}

      {sshHosts.length > 0 && (
        <div className="space-y-2">
          <SettingGroupLabel
            action={
              <button
                type="button"
                onClick={() => setShowManualUri((v) => !v)}
                className="text-muted-foreground hover:text-foreground cursor-pointer text-[12px] font-medium transition-colors"
              >
                {showManualUri ? "Hide URI" : "Pair via URI…"}
              </button>
            }
          >
            SSH Config
          </SettingGroupLabel>

          <SettingCard>
            {sshHosts.map((ssh) => {
              const alreadyPaired = fleet.hosts.some(
                (h) =>
                  h.ssh_alias?.toLowerCase() === ssh.alias.toLowerCase() ||
                  h.display_name.toLowerCase() === ssh.alias.toLowerCase(),
              );
              const busy = connectingSshAlias === ssh.alias;

              return (
                <div
                  key={ssh.alias}
                  className="flex items-center justify-between gap-3 px-4 py-3"
                >
                  <div className="min-w-0">
                    <div className="text-foreground flex items-center gap-2 text-[13.5px] font-medium">
                      <Server className="text-muted-foreground h-[14.5px] w-[14.5px] shrink-0" />
                      <span className="truncate">{ssh.alias}</span>
                    </div>
                    <div className="selectable-text text-muted-foreground truncate pl-6 font-mono text-[12px]">
                      {ssh.display_target}
                    </div>
                  </div>

                  {alreadyPaired ? (
                    <span className="text-muted-foreground inline-flex h-[30px] shrink-0 items-center gap-1.5 px-2 text-[12px] font-medium">
                      <Check className="h-[13.5px] w-[13.5px] text-emerald-500" />
                      Connected
                    </span>
                  ) : (
                    <button
                      type="button"
                      disabled={connectingSshAlias !== null}
                      onClick={() => void handlePairSsh(ssh)}
                      className="bg-foreground text-background inline-flex h-[30px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-3 text-[12.5px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-45"
                    >
                      {busy ? (
                        <>
                          <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
                          <span>Connecting…</span>
                        </>
                      ) : (
                        <span>Connect</span>
                      )}
                    </button>
                  )}
                </div>
              );
            })}
          </SettingCard>
        </div>
      )}

      {(sshHosts.length === 0 || showManualUri) && (
        <div className="space-y-2">
          <SettingGroupLabel>Pairing URI</SettingGroupLabel>
          <div className="border-border bg-background/60 space-y-3 rounded-[13px] border p-4">
            <div className="text-muted-foreground text-[12.5px] leading-relaxed">
              Run{" "}
              <code className="selectable-text bg-muted text-foreground/90 rounded px-1.5 py-0.5 font-mono text-[12px]">
                orbit pair
              </code>{" "}
              on the target machine and paste the URI below.
            </div>
            <div className="flex gap-2">
              <input
                type="text"
                value={pairingUri}
                onChange={(e) => setPairingUri(e.target.value)}
                {...ime.compositionProps}
                onKeyDown={(e) => {
                  if (ime.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter") void handlePairUri();
                }}
                placeholder="orbit://pair/..."
                className="border-border bg-background text-foreground placeholder:text-muted-foreground h-[34px] flex-1 rounded-lg border px-3 font-mono text-[12.5px] outline-none focus:border-foreground/30"
              />
              <button
                type="button"
                onClick={() => void handlePairUri()}
                className="bg-foreground text-background inline-flex h-[34px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity hover:opacity-90"
              >
                <Plus className="h-[14.5px] w-[14.5px]" />
                <span>Pair</span>
              </button>
            </div>
          </div>
        </div>
      )}

      {errorMsg && (
        <div className="selectable-text px-0.5 text-[12.5px] text-rose-500">{errorMsg}</div>
      )}
    </div>
  );
}
