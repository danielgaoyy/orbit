import { useCallback, useEffect, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  FolderLock,
  Globe,
  Play,
  Plus,
  Puzzle,
  RefreshCw,
  Shield,
  Trash2,
} from "lucide-react";
import { PluginSlotCard } from "@/components/plugins/plugin-ui-renderer";
import {
  SettingCard,
  SettingGroupLabel,
  SettingRow,
  SettingSectionHeader,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { useI18n } from "@/lib/i18n";
import { type InstalledPluginView, orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

export const ORBIT_PLUGINS_CHANGED_EVENT = "orbit:plugins-changed";

function notifyPluginsChanged(hostId: string): void {
  if (typeof window !== "undefined") {
    window.dispatchEvent(new CustomEvent(ORBIT_PLUGINS_CHANGED_EVENT, { detail: { hostId } }));
  }
}

export function HostPluginsSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const host = useOrbitSelector((s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const isOnline =
    host?.connection_state === "online_direct" || host?.connection_state === "online_relay";
  const effectiveCwd = selectedCwd && selectedCwd !== "~" ? selectedCwd : null;

  const [plugins, setPlugins] = useState<InstalledPluginView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [sourcePath, setSourcePath] = useState("");
  const [enableOnInstall, setEnableOnInstall] = useState(false);
  const [installing, setInstalling] = useState(false);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [uninstallTarget, setUninstallTarget] = useState<InstalledPluginView | null>(null);
  const [previewPluginIds, setPreviewPluginIds] = useState<Record<string, boolean>>({});
  const [opOutputByKey, setOpOutputByKey] = useState<Record<string, string>>({});
  const [runningOpKey, setRunningOpKey] = useState<string | null>(null);

  const loadPlugins = useCallback(async () => {
    setLoading(true);
    setError(null);
    try {
      const list = await orbitIpc.listPlugins(hostId);
      setPlugins(list);
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setLoading(false);
    }
  }, [hostId]);

  useEffect(() => {
    void loadPlugins();
  }, [loadPlugins]);

  const handleInstall = useCallback(async () => {
    const trimmed = sourcePath.trim();
    if (!trimmed) return;
    setInstalling(true);
    setError(null);
    try {
      const installed = await orbitIpc.installPlugin(hostId, trimmed, enableOnInstall);
      setSourcePath("");
      orbitActions.showToast(t("plugins.installedToast", { name: installed.manifest.name }));
      await loadPlugins();
      notifyPluginsChanged(hostId);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      setError(msg);
      orbitActions.showToast(msg);
    } finally {
      setInstalling(false);
    }
  }, [enableOnInstall, hostId, loadPlugins, sourcePath, t]);

  const handleToggleEnabled = useCallback(
    async (plugin: InstalledPluginView, nextEnabled: boolean) => {
      const id = plugin.manifest.id;
      setTogglingId(id);
      setError(null);
      try {
        await orbitIpc.setPluginEnabled(hostId, id, nextEnabled);
        setPlugins((prev) =>
          prev.map((p) => (p.manifest.id === id ? { ...p, enabled: nextEnabled } : p)),
        );
        notifyPluginsChanged(hostId);
      } catch (err) {
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg);
        orbitActions.showToast(msg);
      } finally {
        setTogglingId(null);
      }
    },
    [hostId],
  );

  const handleConfirmUninstall = useCallback(async () => {
    if (!uninstallTarget) return;
    const id = uninstallTarget.manifest.id;
    setError(null);
    await orbitIpc.uninstallPlugin(hostId, id);
    setPlugins((prev) => prev.filter((p) => p.manifest.id !== id));
    orbitActions.showToast(t("plugins.uninstalledToast", { name: uninstallTarget.manifest.name }));
    setUninstallTarget(null);
    notifyPluginsChanged(hostId);
  }, [hostId, t, uninstallTarget]);

  const handleInvokeOp = useCallback(
    async (pluginId: string, opName: string) => {
      const key = `${pluginId}:${opName}`;
      setRunningOpKey(key);
      try {
        const res = await orbitIpc.invokePluginOp(hostId, pluginId, opName, {}, effectiveCwd);
        setOpOutputByKey((prev) => ({
          ...prev,
          [key]: typeof res === "string" ? res : JSON.stringify(res, null, 2),
        }));
      } catch (err) {
        orbitActions.showToast(err instanceof Error ? err.message : String(err));
      } finally {
        setRunningOpKey(null);
      }
    },
    [effectiveCwd, hostId],
  );

  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("plugins.title")}
        description={t("plugins.subtitle")}
        action={
          <Button
            variant="outline"
            size="sm"
            disabled={loading || !isOnline}
            onClick={() => void loadPlugins()}
          >
            <RefreshCw data-icon="inline-start" className={loading ? "animate-spin" : undefined} />
            {t("common.refresh")}
          </Button>
        }
      />

      {error && (
        <div className="border-signal-danger/30 bg-signal-danger/8 text-signal-danger rounded-[10px] border px-3.5 py-2.5 text-[12px]">
          {error}
        </div>
      )}

      {/* Install Local / GitHub Plugin */}
      <div className="flex flex-col gap-2">
        <SettingGroupLabel>{t("plugins.installSection")}</SettingGroupLabel>
        <SettingCard>
          <div className="flex flex-col gap-3 px-4 py-3.5">
            <div className="flex flex-col gap-2 sm:flex-row">
              <Input
                value={sourcePath}
                onChange={(e) => setSourcePath(e.target.value)}
                placeholder={t("plugins.installPlaceholder")}
                disabled={installing || !isOnline}
                onKeyDown={(e) => {
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void handleInstall();
                  }
                }}
                className="flex-1 font-mono text-[12.5px]"
              />
              <Button
                size="sm"
                disabled={installing || !isOnline || !sourcePath.trim()}
                onClick={() => void handleInstall()}
              >
                {installing ? <OrbitSpinner size={12} /> : <Plus data-icon="inline-start" />}
                {t("plugins.installButton")}
              </Button>
            </div>
          </div>
          <SettingRow
            title={t("plugins.enableAfterInstallTitle")}
            description={t("plugins.enableAfterInstallDesc")}
          >
            <ToggleSwitch
              checked={enableOnInstall}
              disabled={installing || !isOnline}
              ariaLabel={t("plugins.enableAfterInstallTitle")}
              onChange={setEnableOnInstall}
            />
          </SettingRow>
        </SettingCard>
      </div>

      {/* Installed Plugins */}
      <div className="flex flex-col gap-2">
        <SettingGroupLabel>
          {t("plugins.installedSection", { count: plugins.length })}
        </SettingGroupLabel>

        {loading && plugins.length === 0 ? (
          <SettingCard>
            <div className="text-ink-muted flex items-center gap-2.5 px-4 py-6 text-[13px]">
              <OrbitSpinner size={14} />
              <span>{t("common.loading")}</span>
            </div>
          </SettingCard>
        ) : plugins.length === 0 ? (
          <SettingCard>
            <Empty className="border-0 py-8">
              <EmptyHeader>
                <EmptyMedia variant="icon">
                  <Puzzle />
                </EmptyMedia>
                <EmptyTitle>{t("plugins.emptyTitle")}</EmptyTitle>
                <EmptyDescription>{t("plugins.emptyDesc")}</EmptyDescription>
              </EmptyHeader>
            </Empty>
          </SettingCard>
        ) : (
          <div className="flex flex-col gap-3">
            {plugins.map((plugin) => {
              const m = plugin.manifest;
              const isPreviewOpen = Boolean(previewPluginIds[m.id]);
              const hasUiSlots = m.ui_slots.length > 0;
              const opNames = m.mcp_ops.map((o) => o.name);
              const fsCount = m.capabilities.fs_read_globs.length;
              const httpCount = m.capabilities.http_domains.length;

              return (
                <SettingCard key={m.id}>
                  <div className="flex flex-col gap-3 px-4 py-3.5">
                    <div className="flex items-start justify-between gap-3">
                      <div className="min-w-0 flex-1 space-y-1">
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="text-ink-primary text-[13.5px] font-semibold">
                            {m.name}
                          </span>
                          <Badge variant="outline" className="font-mono text-[10.5px]">
                            v{m.version}
                          </Badge>
                          <span className="text-ink-faint font-mono text-[11px]">{m.id}</span>
                        </div>
                        {m.description && (
                          <p className="text-ink-secondary text-[12.5px] leading-[1.45]">
                            {m.description}
                          </p>
                        )}
                      </div>

                      <div className="flex shrink-0 items-center gap-2">
                        <ToggleSwitch
                          checked={plugin.enabled}
                          disabled={togglingId === m.id || !isOnline}
                          ariaLabel={m.name}
                          onChange={(next) => void handleToggleEnabled(plugin, next)}
                        />
                        <Button
                          variant="ghost"
                          size="icon-sm"
                          aria-label={t("plugins.uninstallButton")}
                          disabled={!isOnline}
                          onClick={() => setUninstallTarget(plugin)}
                        >
                          <Trash2 className="text-ink-muted hover:text-signal-danger" />
                        </Button>
                      </div>
                    </div>

                    {/* Declared Capabilities Audit Badges */}
                    <div className="flex flex-wrap items-center gap-1.5">
                      <Badge variant="outline" className="gap-1 text-[11px]">
                        <Shield className="size-3" />
                        WASM Sandbox
                      </Badge>
                      {fsCount > 0 ? (
                        m.capabilities.fs_read_globs.map((glob) => (
                          <Badge
                            key={`fs:${glob}`}
                            variant="secondary"
                            className="gap-1 font-mono text-[10.5px]"
                          >
                            <FolderLock className="size-3" />
                            {glob}
                          </Badge>
                        ))
                      ) : (
                        <Badge variant="outline" className="text-ink-muted text-[10.5px]">
                          {t("plugins.capNoFs")}
                        </Badge>
                      )}
                      {httpCount > 0 ? (
                        m.capabilities.http_domains.map((domain) => (
                          <Badge
                            key={`http:${domain}`}
                            variant="secondary"
                            className="gap-1 font-mono text-[10.5px]"
                          >
                            <Globe className="size-3" />
                            {domain}
                          </Badge>
                        ))
                      ) : (
                        <Badge variant="outline" className="text-ink-muted text-[10.5px]">
                          {t("plugins.capNoNet")}
                        </Badge>
                      )}
                    </div>

                    {/* Declared MCP Operations */}
                    {m.mcp_ops.length > 0 && (
                      <div className="flex flex-col gap-1.5 pt-1">
                        <div className="text-ink-muted text-[11px] font-medium">
                          {t("plugins.opsHeading")}
                        </div>
                        <div className="flex flex-wrap gap-1.5">
                          {m.mcp_ops.map((op) => {
                            const opKey = `${m.id}:${op.name}`;
                            const running = runningOpKey === opKey;
                            return (
                              <Button
                                key={op.name}
                                variant="outline"
                                size="xs"
                                disabled={!plugin.enabled || !isOnline || running}
                                title={op.description}
                                onClick={() => void handleInvokeOp(m.id, op.name)}
                              >
                                {running ? (
                                  <OrbitSpinner size={11} />
                                ) : (
                                  <Play data-icon="inline-start" />
                                )}
                                <span className="font-mono">{op.name}</span>
                              </Button>
                            );
                          })}
                        </div>
                        {m.mcp_ops.map((op) => {
                          const out = opOutputByKey[`${m.id}:${op.name}`];
                          if (!out) return null;
                          return (
                            <pre
                              key={`out:${op.name}`}
                              className="border-border-subtle bg-surface-recessed text-ink-secondary max-h-36 overflow-auto rounded-[8px] border p-2.5 font-mono text-[11px]"
                            >
                              {out}
                            </pre>
                          );
                        })}
                      </div>
                    )}

                    {/* Inline Live UI Preview */}
                    {hasUiSlots && plugin.enabled && (
                      <div className="border-border-subtle/70 flex flex-col gap-2.5 border-t pt-2.5">
                        <button
                          type="button"
                          onClick={() =>
                            setPreviewPluginIds((prev) => ({ ...prev, [m.id]: !prev[m.id] }))
                          }
                          className="text-ink-secondary hover:text-ink-primary flex w-fit cursor-pointer items-center gap-1.5 text-[12px] font-medium transition-colors"
                        >
                          {isPreviewOpen ? (
                            <ChevronDown className="size-3.5" />
                          ) : (
                            <ChevronRight className="size-3.5" />
                          )}
                          <span>{t("plugins.livePreviewToggle")}</span>
                        </button>
                        {isPreviewOpen && (
                          <div className="flex flex-col gap-2.5">
                            {m.ui_slots.map((slotDecl) => (
                              <PluginSlotCard
                                key={slotDecl.slot}
                                hostId={hostId}
                                pluginId={m.id}
                                title={slotDecl.title}
                                slot={slotDecl.slot}
                                cwd={effectiveCwd}
                                mcpOpNames={opNames}
                              />
                            ))}
                          </div>
                        )}
                      </div>
                    )}
                  </div>
                </SettingCard>
              );
            })}
          </div>
        )}
      </div>

      <ConfirmDialog
        open={Boolean(uninstallTarget)}
        onOpenChange={(open) => {
          if (!open) setUninstallTarget(null);
        }}
        title={t("plugins.uninstallConfirmTitle")}
        description={
          uninstallTarget
            ? t("plugins.uninstallConfirmDesc", { name: uninstallTarget.manifest.name })
            : ""
        }
        confirmLabel={t("plugins.uninstallButton")}
        tone="danger"
        defaultAction="cancel"
        onConfirm={handleConfirmUninstall}
      />
    </div>
  );
}
