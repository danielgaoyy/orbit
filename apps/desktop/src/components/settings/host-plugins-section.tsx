import { memo, useCallback, useEffect, useState } from "react";
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
  SettingDrawer,
  SettingResourceRow,
  ToggleSwitch,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Empty, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldLabel } from "@/components/ui/field";
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

const InstallPluginDrawer = memo(function InstallPluginDrawer({
  open,
  onOpenChange,
  hostId,
  isOnline,
  onInstalled,
  onError,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  hostId: string;
  isOnline: boolean;
  onInstalled: (pluginId: string) => Promise<void>;
  onError: (msg: string | null) => void;
}) {
  const { t } = useI18n();
  const [sourcePath, setSourcePath] = useState("");
  const [enableOnInstall, setEnableOnInstall] = useState(false);
  const [installing, setInstalling] = useState(false);

  const handleInstall = async () => {
    const trimmed = sourcePath.trim();
    if (!trimmed || installing || !isOnline) return;
    setInstalling(true);
    onError(null);
    try {
      const installed = await orbitIpc.installPlugin(hostId, trimmed, enableOnInstall);
      setSourcePath("");
      onOpenChange(false);
      orbitActions.showToast(t("plugins.installedToast", { name: installed.manifest.name }));
      await onInstalled(installed.manifest.id);
      notifyPluginsChanged(hostId);
    } catch (err) {
      const msg = err instanceof Error ? err.message : String(err);
      onError(msg);
      orbitActions.showToast(msg);
    } finally {
      setInstalling(false);
    }
  };

  return (
    <SettingDrawer
      open={open}
      onOpenChange={(next) => {
        if (!installing) onOpenChange(next);
      }}
      title={t("plugins.installSection")}
    >
      <form
        className="flex flex-col gap-4"
        onSubmit={(e) => {
          e.preventDefault();
          void handleInstall();
        }}
      >
        <Field>
          <FieldLabel htmlFor="plugin-source">{t("managedSkills.pluginSource")}</FieldLabel>
          <Input
            id="plugin-source"
            value={sourcePath}
            onChange={(e) => setSourcePath(e.target.value)}
            placeholder={t("plugins.installPlaceholder")}
            disabled={installing || !isOnline}
            className="font-mono text-[13px]"
          />
        </Field>
        <SettingCard>
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
        <Button
          type="submit"
          size="sm"
          className="self-end"
          disabled={installing || !isOnline || !sourcePath.trim()}
        >
          {installing && <OrbitSpinner size={13} />}
          {t("plugins.installButton")}
        </Button>
      </form>
    </SettingDrawer>
  );
});

export const HostPluginsSection = memo(function HostPluginsSection({
  hostId,
}: {
  hostId: string;
}) {
  const { t } = useI18n();
  const isOnline = useHostIsOnline(hostId);
  const selectedCwd = useOrbitSelector((s) =>
    s.selectedHostId === hostId ? s.selectedWorkspaceCwd : null,
  );
  const effectiveCwd = selectedCwd && selectedCwd !== "~" ? selectedCwd : null;

  const [plugins, setPlugins] = useState<InstalledPluginView[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [installOpen, setInstallOpen] = useState(false);
  const [detailOpen, setDetailOpen] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [uninstallTarget, setUninstallTarget] = useState<InstalledPluginView | null>(null);
  const [previewPluginIds, setPreviewPluginIds] = useState<Record<string, boolean>>({});
  const [opOutputByKey, setOpOutputByKey] = useState<Record<string, string>>({});
  const [runningOpKey, setRunningOpKey] = useState<string | null>(null);

  const loadPlugins = useCallback(async () => {
    if (!isOnline) {
      setLoading(false);
      return;
    }
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
  }, [hostId, isOnline]);

  useEffect(() => {
    void loadPlugins();
  }, [loadPlugins]);

  const handleInstalled = useCallback(
    async (pluginId: string) => {
      setSelectedId(pluginId);
      setDetailOpen(true);
      await loadPlugins();
    },
    [loadPlugins],
  );

  const handleToggleEnabled = useCallback(
    async (plugin: InstalledPluginView, nextEnabled: boolean) => {
      const id = plugin.manifest.id;
      const prevEnabled = plugin.enabled;
      setError(null);
      setPlugins((prev) =>
        prev.map((p) => (p.manifest.id === id ? { ...p, enabled: nextEnabled } : p)),
      );
      try {
        await orbitIpc.setPluginEnabled(hostId, id, nextEnabled);
        notifyPluginsChanged(hostId);
      } catch (err) {
        setPlugins((prev) =>
          prev.map((p) => (p.manifest.id === id ? { ...p, enabled: prevEnabled } : p)),
        );
        const msg = err instanceof Error ? err.message : String(err);
        setError(msg);
        orbitActions.showToast(msg);
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
    setSelectedId(null);
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

  const selected = plugins.find((plugin) => plugin.manifest.id === selectedId);
  const manifest = selected?.manifest;
  return (
    <section data-setting-id="plugins.title" className="flex flex-col gap-2">
      <SettingGroupLabel
        action={
          <div className="flex items-center gap-1">
            <Button
              variant="ghost"
              size="icon-sm"
              aria-label={t("common.refresh")}
              disabled={!isOnline || loading}
              onClick={() => void loadPlugins()}
            >
              <RefreshCw />
            </Button>
            <Button
              data-setting-id="plugins.installSection"
              variant="outline"
              size="sm"
              disabled={!isOnline}
              onClick={() => setInstallOpen(true)}
            >
              <Plus data-icon="inline-start" />
              {t("plugins.installSection")}
            </Button>
          </div>
        }
      >
        {t("plugins.title")} · {plugins.length}
      </SettingGroupLabel>

      {error && (
        <p role="alert" className="text-signal-danger text-[13px]">
          {error}
        </p>
      )}
      <SettingCard>
        {loading && plugins.length === 0 ? (
          <div className="text-ink-muted flex items-center gap-2 px-4 py-5 text-[13px]">
            <OrbitSpinner size={13} />
            {t("common.loading")}
          </div>
        ) : plugins.length === 0 ? (
          <Empty className="py-5">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Puzzle />
              </EmptyMedia>
              <EmptyTitle className="text-[13px]">
                {t(isOnline ? "plugins.emptyTitle" : "sidebar.noHostConnected")}
              </EmptyTitle>
            </EmptyHeader>
          </Empty>
        ) : (
          plugins.map((plugin) => (
            <SettingResourceRow
              key={plugin.manifest.id}
              icon={<Puzzle />}
              title={plugin.manifest.name}
              description={plugin.manifest.description}
              status={plugin.manifest.version}
              onClick={() => {
                setSelectedId(plugin.manifest.id);
                setDetailOpen(true);
              }}
              action={
                <ToggleSwitch
                  checked={plugin.enabled}
                  disabled={!isOnline}
                  ariaLabel={plugin.manifest.name}
                  onChange={(enabled) => void handleToggleEnabled(plugin, enabled)}
                />
              }
            />
          ))
        )}
      </SettingCard>

      <InstallPluginDrawer
        open={installOpen}
        onOpenChange={setInstallOpen}
        hostId={hostId}
        isOnline={isOnline}
        onInstalled={handleInstalled}
        onError={setError}
      />
      {manifest && selected && (
        <SettingDrawer
          open={detailOpen}
          onOpenChange={setDetailOpen}
          onOpenChangeComplete={(visible) => {
            if (!visible) setSelectedId(null);
          }}
          title={manifest.name}
          description={manifest.description}
        >
          <div className="flex flex-col gap-4">
            <p className="text-ink-muted font-mono text-[11px]">
              {manifest.id} · v{manifest.version}
            </p>
            {/* Declared Capabilities Audit Badges */}
            <div className="flex flex-wrap items-center gap-1.5">
              <Badge variant="outline" className="gap-1 text-[11px]">
                <Shield className="size-3" />
                {t("managedSkills.wasmSandbox")}
              </Badge>
              {manifest.capabilities.fs_read_globs.length > 0 ? (
                manifest.capabilities.fs_read_globs.map((glob) => (
                  <Badge
                    key={`fs:${glob}`}
                    variant="secondary"
                    className="gap-1 font-mono text-[11px]"
                  >
                    <FolderLock className="size-3" />
                    {glob}
                  </Badge>
                ))
              ) : (
                <Badge variant="outline" className="text-ink-muted text-[11px]">
                  {t("plugins.capNoFs")}
                </Badge>
              )}
              {manifest.capabilities.http_domains.length > 0 ? (
                manifest.capabilities.http_domains.map((domain) => (
                  <Badge
                    key={`http:${domain}`}
                    variant="secondary"
                    className="gap-1 font-mono text-[11px]"
                  >
                    <Globe className="size-3" />
                    {domain}
                  </Badge>
                ))
              ) : (
                <Badge variant="outline" className="text-ink-muted text-[11px]">
                  {t("plugins.capNoNet")}
                </Badge>
              )}
            </div>

            {/* Declared MCP Operations */}
            {manifest.mcp_ops.length > 0 && (
              <div className="flex flex-col gap-1.5 pt-1">
                <div className="text-ink-muted text-[11px] font-medium">
                  {t("plugins.opsHeading")}
                </div>
                <div className="flex flex-wrap gap-1.5">
                  {manifest.mcp_ops.map((op) => {
                    const opKey = `${manifest.id}:${op.name}`;
                    const running = runningOpKey === opKey;
                    return (
                      <Button
                        key={op.name}
                        variant="outline"
                        size="xs"
                        disabled={!selected.enabled || !isOnline || runningOpKey !== null}
                        title={op.description}
                        onClick={() => void handleInvokeOp(manifest.id, op.name)}
                      >
                        {running ? <OrbitSpinner size={11} /> : <Play data-icon="inline-start" />}
                        <span className="font-mono">{op.name}</span>
                      </Button>
                    );
                  })}
                </div>
                {manifest.mcp_ops.map((op) => {
                  const out = opOutputByKey[`${manifest.id}:${op.name}`];
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
            {manifest.ui_slots.length > 0 && selected.enabled && (
              <div className="border-border-subtle/70 flex flex-col gap-2.5 border-t pt-2.5">
                <button
                  type="button"
                  onClick={() =>
                    setPreviewPluginIds((prev) => ({ ...prev, [manifest.id]: !prev[manifest.id] }))
                  }
                  className="text-ink-secondary hover:text-ink-primary flex w-fit cursor-pointer items-center gap-1.5 text-[13px] font-medium transition-colors"
                >
                  {previewPluginIds[manifest.id] ? (
                    <ChevronDown className="size-3.5" />
                  ) : (
                    <ChevronRight className="size-3.5" />
                  )}
                  <span>{t("plugins.livePreviewToggle")}</span>
                </button>
                {previewPluginIds[manifest.id] && (
                  <div className="flex flex-col gap-2.5">
                    {manifest.ui_slots.map((slotDecl) => (
                      <PluginSlotCard
                        key={slotDecl.slot}
                        hostId={hostId}
                        pluginId={manifest.id}
                        title={slotDecl.title}
                        slot={slotDecl.slot}
                        cwd={effectiveCwd}
                        mcpOpNames={manifest.mcp_ops.map((op) => op.name)}
                      />
                    ))}
                  </div>
                )}
              </div>
            )}
            <Button
              variant="outline"
              size="sm"
              className="self-start text-signal-danger"
              disabled={!isOnline}
              onClick={() => setUninstallTarget(selected)}
            >
              <Trash2 data-icon="inline-start" />
              {t("plugins.uninstallButton")}
            </Button>
          </div>
        </SettingDrawer>
      )}
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
    </section>
  );
});
