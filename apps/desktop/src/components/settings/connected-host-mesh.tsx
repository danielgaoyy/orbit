import { useState } from "react";
import { Link2, Server, Trash2 } from "lucide-react";
import { SettingCard, SettingGroup, SettingResourceRow, ToggleSwitch } from "./settings-primitives";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Input } from "@/components/ui/input";
import { useI18n } from "@/lib/i18n";
import { formatProviderShortLabel, orbitIpc, type ConnectedHostSummary } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export function ConnectedHostMesh({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const peers = useOrbitSelector((s) => s.hostDetails[hostId]?.connected_hosts);
  const otherHosts = useOrbitSelector((s) => s.fleet.hosts.some((host) => host.host_id !== hostId));
  const [label, setLabel] = useState("");
  const [uri, setUri] = useState("");
  const [busy, setBusy] = useState(false);
  const [remove, setRemove] = useState<ConnectedHostSummary | null>(null);

  async function update(operation: () => Promise<unknown>) {
    if (busy) return;
    setBusy(true);
    try {
      await operation();
    } catch (error) {
      orbitActions.showToast(String(error));
    } finally {
      setBusy(false);
    }
  }
  return (
    <SettingGroup
      title={t("settings.agentMesh")}
      settingId="settings.agentMesh"
      action={
        otherHosts && (
          <Button
            size="sm"
            variant="outline"
            disabled={busy}
            onClick={() => void update(() => orbitIpc.syncFleetConnectedHosts(hostId))}
          >
            <Link2 />
            {t("settings.meshSync")}
          </Button>
        )
      }
    >
      <SettingCard>
        {peers?.map((peer) => (
          <SettingResourceRow
            key={peer.registration_id}
            icon={<Server />}
            title={peer.label}
            description={
              peer.last_error ||
              [peer.hostname ?? peer.host_id, ...peer.available_providers.map(formatProviderShortLabel)]
                .filter(Boolean)
                .join(" · ")
            }
            status={t(meshStatusLabels[peer.status])}
            action={
              <div className="flex items-center gap-2">
                <ToggleSwitch
                  checked={peer.enabled}
                  disabled={busy}
                  ariaLabel={peer.label}
                  onChange={(enabled) =>
                    void update(() =>
                      orbitIpc.updateConnectedHost(hostId, peer.registration_id, { enabled }),
                    )
                  }
                />
                <Button
                  variant="ghost"
                  size="icon-sm"
                  aria-label={t("settings.meshRemove", { name: peer.label })}
                  onClick={() => setRemove(peer)}
                >
                  <Trash2 />
                </Button>
              </div>
            }
          />
        ))}
        <form
          className="flex flex-wrap items-center gap-2 p-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (!uri.trim()) return;
            void update(async () => {
              await orbitIpc.addConnectedHost(hostId, uri.trim(), label.trim() || null);
              setUri("");
              setLabel("");
            });
          }}
        >
          <Input
            aria-label={t("settings.meshLabel")}
            placeholder={t("settings.meshLabel")}
            value={label}
            onChange={(event) => setLabel(event.target.value)}
            className="w-[140px]"
          />
          <Input
            aria-label={t("settings.meshUri")}
            placeholder="orbit://pair/…"
            type="password"
            autoComplete="off"
            value={uri}
            onChange={(event) => setUri(event.target.value)}
            className="min-w-[140px] flex-1 font-mono"
          />
          <Button size="sm" type="submit" disabled={busy || !uri.trim()}>
            {t("settings.meshAdd")}
          </Button>
        </form>
      </SettingCard>
      <ConfirmDialog
        open={!!remove}
        onOpenChange={(open) => {
          if (!open) setRemove(null);
        }}
        tone="danger"
        title={t("settings.meshRemove", { name: remove?.label ?? "" })}
        description={t("settings.meshRemoveDesc")}
        confirmLabel={t("common.delete")}
        onConfirm={async () => {
          if (remove) await orbitIpc.removeConnectedHost(hostId, remove.registration_id);
        }}
      />
    </SettingGroup>
  );
}

const meshStatusLabels = {
  online: "common.online",
  offline: "common.offline",
  connecting: "settings.statusConnecting",
  error: "settings.meshError",
  disabled: "settings.meshDisabled",
} as const;
