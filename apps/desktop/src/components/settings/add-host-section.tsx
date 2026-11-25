import { useEffect, useRef, useState } from "react";
import { Cloud, Laptop, Server } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingResourceRow,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { type HostCardView, type LocalMachineInfo, isWebRuntime, orbitIpc } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export function AddHostSection({ onPaired }: { onPaired: (id: string) => void }) {
  const { t } = useI18n();
  const hosts = useOrbitSelector((s) => s.fleet.hosts);
  const sshHosts = useOrbitSelector((s) => s.sshHosts);
  const cloud = useOrbitSelector((s) => s.cloudSyncStatus);
  const [machine, setMachine] = useState<LocalMachineInfo | null>(null);
  const [uri, setUri] = useState("");
  const [pending, setPending] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mounted = useRef(true);
  const ime = useImeCompositionGuard();

  useEffect(() => {
    mounted.current = true;
    if (!isWebRuntime()) {
      void orbitIpc
        .getLocalMachineInfo()
        .then((info) => {
          if (mounted.current) setMachine(info);
        })
        .catch(() => undefined);
    }
    return () => {
      mounted.current = false;
    };
  }, []);

  const connect = async (key: string, pair: () => Promise<HostCardView>) => {
    if (pending !== null) return;
    setPending(key);
    setError(null);
    try {
      const card = await pair();
      await orbitActions.reloadFleetAndHosts();
      orbitActions.showToast(t("settings.fleetPaired", { host: card.display_name }));
      if (mounted.current) onPaired(card.host_id);
    } catch (err) {
      if (mounted.current) setError(String(err));
    } finally {
      if (mounted.current) setPending(null);
    }
  };
  const localPaired = hosts.some(
    (host) => isLocalHostCard(host) || machine?.host_id === host.host_id,
  );
  return (
    <div className="flex flex-col gap-[20px]">
      {machine && !localPaired && (
        <SettingCard>
          <SettingResourceRow
            icon={<Laptop />}
            title={machine.hostname}
            description={t("common.local")}
            action={
              <Button
                variant="outline"
                size="sm"
                disabled={pending !== null}
                onClick={() => void connect("local", () => orbitIpc.pairLocalHost())}
              >
                {t(pending === "local" ? "web.connecting" : "web.connect")}
              </Button>
            }
          />
        </SettingCard>
      )}
      <div className="flex flex-col gap-2">
        <SettingGroupLabel>{t("web.uri")}</SettingGroupLabel>
        <SettingCard>
          <form
            className="flex flex-col gap-3 p-4"
            onSubmit={(e) => {
              e.preventDefault();
              if (uri.trim()) void connect("uri", () => orbitIpc.pairHost(uri.trim()));
            }}
          >
            <p className="text-ink-secondary text-[13px] leading-[1.45]">{t("settings.fleetUriHint")}</p>
            <div className="flex items-center gap-2">
              <Input
                type="password"
                autoComplete="off"
                value={uri}
                onChange={(e) => setUri(e.target.value)}
                {...ime.compositionProps}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && ime.shouldIgnoreKeyDown(e)) e.preventDefault();
                }}
                aria-label={t("web.uri")}
                placeholder="orbit://pair/…"
                className="h-[30px] min-w-0 flex-1 font-mono text-[13px]"
              />
              <Button type="submit" size="sm" disabled={!uri.trim() || pending !== null} className="text-[13px]">
                {t(pending === "uri" ? "web.connecting" : "web.connect")}
              </Button>
            </div>
          </form>
        </SettingCard>
      </div>
      {sshHosts.length > 0 && (
        <div className="flex flex-col gap-2">
          <SettingGroupLabel>{t("settings.fleetSshConfig")}</SettingGroupLabel>
          <SettingCard>
            {sshHosts.map((ssh) => {
              const paired = hosts.some(
                (host) =>
                  host.ssh_alias?.toLowerCase() === ssh.alias.toLowerCase() ||
                  host.display_name.toLowerCase() === ssh.alias.toLowerCase(),
              );
              return (
                <SettingResourceRow
                  key={ssh.alias}
                  icon={<Server />}
                  title={ssh.alias}
                  description={ssh.display_target}
                  action={
                    paired ? (
                      <Badge variant="secondary">{t("settings.fleetPairedStatus")}</Badge>
                    ) : (
                      <Button
                        size="sm"
                        variant="outline"
                        disabled={pending !== null}
                        onClick={() =>
                          void connect(`ssh:${ssh.alias}`, () => orbitIpc.pairSshHost(ssh.alias))
                        }
                        className="text-[13px]"
                      >
                        {t(pending === `ssh:${ssh.alias}` ? "web.connecting" : "web.connect")}
                      </Button>
                    )
                  }
                />
              );
            })}
          </SettingCard>
        </div>
      )}
      {!isWebRuntime() && cloud?.state !== "ready" && (
        <SettingCard>
          <SettingResourceRow
            icon={<Cloud />}
            title={t("settings.cloudSyncTitle")}
            description={t("settings.fleetCloudImport")}
            onClick={() => orbitActions.setSettingsSection("cloud_sync")}
          />
        </SettingCard>
      )}
      {error && (
        <p role="alert" className="selectable-text text-signal-danger text-[13px]">
          {error}
        </p>
      )}
    </div>
  );
}
