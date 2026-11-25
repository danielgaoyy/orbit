import { useState } from "react";
import { Link2, Server, Trash2 } from "lucide-react";
import { SettingCard, SettingGroup, SettingResourceRow, ToggleSwitch } from "./settings-primitives";
import { Badge } from "@/components/ui/badge";
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
            size="xs"
            variant="outline"
            disabled={busy}
            onClick={() => void update(() => orbitIpc.syncFleetConnectedHosts(hostId))}
            className="text-[11px]"
          >
            <Link2 className="size-[12px]" />
            <span>{t("settings.meshSync")}</span>
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
            status={
              <Badge variant={meshStatusVariants[peer.status]}>
                {t(meshStatusLabels[peer.status])}
              </Badge>
            }
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
                  className="text-ink-muted hover:bg-signal-danger-bg hover:text-signal-danger"
                >
                  <Trash2 className="size-[13px]" />
                </Button>
              </div>
            }
          />
        ))}
        <form
          className="bg-surface-recessed/35 flex flex-wrap items-center gap-2 px-4 py-3"
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
            className="h-[30px] w-[148px] text-[13px]"
          />
          <Input
            aria-label={t("settings.meshUri")}
            placeholder="orbit://pair/…"
            type="password"
            autoComplete="off"
            value={uri}
            onChange={(event) => setUri(event.target.value)}
            className="h-[30px] min-w-[160px] flex-1 font-mono text-[13px]"
          />
          <Button size="sm" type="submit" disabled={busy || !uri.trim()} className="text-[13px]">
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

const meshStatusVariants = {
  online: "success",
  offline: "secondary",
  connecting: "warning",
  error: "destructive",
  disabled: "secondary",
} as const;
