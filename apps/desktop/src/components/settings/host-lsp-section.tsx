import { type ReactNode, useEffect, useState } from "react";
import { Loader2, RefreshCw, RotateCw } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingResourceRow,
} from "./settings-primitives";
import { IconButton } from "@/components/ui/icon-button";
import { type MessageKey, useI18n } from "@/lib/i18n";
import type { LspServerState } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const STATUS: Record<LspServerState, { label: MessageKey; dot: string }> = {
  ready: { label: "devTools.ready", dot: "bg-signal-success" },
  starting: { label: "devTools.starting", dot: "bg-signal-warning" },
  error: { label: "devTools.error", dot: "bg-signal-danger" },
  stopped: { label: "devTools.idle", dot: "bg-ink-muted" },
  unavailable: { label: "devTools.unavailable", dot: "bg-ink-faint" },
};

export function HostLspSection({
  hostId,
  workspaceCwd = null,
  workspacePicker,
}: {
  hostId: string;
  workspaceCwd?: string | null;
  workspacePicker?: ReactNode;
}) {
  const { t } = useI18n();
  const online = useOrbitSelector((s) => {
    const state = s.fleet.hosts.find(
      (h) => h.host_id === hostId,
    )?.connection_state;
    return state === "online_direct" || state === "online_relay";
  });
  const effectiveCwd = workspaceCwd && workspaceCwd !== "~" ? workspaceCwd : null;
  const entry = useOrbitSelector((s) =>
    effectiveCwd ? s.lspServersByWorkspace[`${hostId}::${effectiveCwd}`] : null,
  );
  const [restarting, setRestarting] = useState<string | null>(null);
  const [refreshing, setRefreshing] = useState(false);
  const loading =
    online &&
    Boolean(effectiveCwd) &&
    (!entry || entry.status === "idle" || entry.status === "loading");
  const isRefreshing = refreshing || loading;

  useEffect(() => {
    if (online && effectiveCwd)
      void orbitActions.loadLspServers(hostId, effectiveCwd);
  }, [online, hostId, effectiveCwd]);

  const handleRefresh = async () => {
    if (!online || !effectiveCwd) return;
    setRefreshing(true);
    try {
      await orbitActions.loadLspServers(hostId, effectiveCwd);
    } finally {
      setRefreshing(false);
    }
  };

  const restart = async (id: string) => {
    if (!effectiveCwd || restarting) return;
    setRestarting(id);
    try {
      await orbitActions.restartLspServer(hostId, effectiveCwd, id);
    } catch (error) {
      orbitActions.showToast(
        error instanceof Error ? error.message : t("common.actionFailed"),
      );
    } finally {
      setRestarting(null);
    }
  };

  return (
    <section data-setting-id="settings.subTabLsp" className="space-y-3">
      <SettingGroupLabel
        action={
          online &&
          effectiveCwd && (
            <div className="flex items-center gap-2">
              {workspacePicker}
              <IconButton
                tooltip={t("common.refresh")}
                disabled={isRefreshing}
                onClick={() => void handleRefresh()}
              >
                <RefreshCw className={isRefreshing ? "animate-spin" : undefined} />
              </IconButton>
            </div>
          )
        }
      >
        {t("devTools.lsp")}
      </SettingGroupLabel>
      <SettingCard>
        {online && effectiveCwd && entry?.status === "error" && (
          <p role="alert" className="text-signal-danger px-4 py-3 text-[13px]">
            {entry.error}
          </p>
        )}
        {online && effectiveCwd && Boolean(entry?.servers.length) ? (
          entry?.servers.map((server) => (
            <SettingResourceRow
              key={server.id}
              icon={
                <span
                  className={`block size-[7px] rounded-full ${STATUS[server.state].dot}`}
                />
              }
              title={server.name}
              description={[
                t(STATUS[server.state].label),
                server.diagnostics_count > 0 &&
                  t("devTools.diagnostics", {
                    count: server.diagnostics_count,
                  }),
                server.error,
                server.binary_path,
              ]
                .filter(Boolean)
                .join(" · ")}
              status={[
                (server.language_ids.length
                  ? server.language_ids
                  : server.file_extensions.map((ext) => `.${ext}`)
                )
                  .slice(0, 4)
                  .join(", "),
                server.source === "project_json" && ".lsp.json",
              ]
                .filter(Boolean)
                .join(" · ")}
              action={
                server.state !== "unavailable" && (
                  <IconButton
                    tooltip={t("devTools.restart", { name: server.name })}
                    disabled={Boolean(restarting)}
                    onClick={() => void restart(server.id)}
                  >
                    {restarting === server.id ? (
                      <Loader2 className="animate-spin" />
                    ) : (
                      <RotateCw />
                    )}
                  </IconButton>
                )
              }
            />
          ))
        ) : (
          <div className="text-ink-secondary flex items-center justify-between gap-3 px-4 py-4 text-[13px]">
            <span>
              {!online
                ? t("devTools.offline")
                : !effectiveCwd
                  ? t("devTools.needProject")
                  : loading
                    ? t("common.loading")
                    : t("devTools.emptyLsp")}
            </span>
          </div>
        )}
      </SettingCard>
      {online && effectiveCwd && (
        <p className="text-ink-muted text-[11px]">{t("devTools.lspPaths")}</p>
      )}
    </section>
  );
}
