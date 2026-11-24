import { memo, useCallback, useEffect, useMemo, useState } from "react";
import { Puzzle } from "lucide-react";
import { PluginSlotCard } from "@/components/plugins/plugin-ui-renderer";
import { ORBIT_PLUGINS_CHANGED_EVENT } from "@/components/settings/host-plugins-section";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { type InstalledPluginView, orbitIpc } from "@/lib/orbit-ipc";
import { useOrbitSelector } from "@/state/orbit-store";

/**
 * Renders enabled WASM plugins' `side_panel_tab` / `stage_card` UI cards inside a
 * quiet hairline popover in the workspace side panel.
 * Emits ZERO visual chrome (`null`) when 0 plugins with UI slots are enabled.
 */
export const WorkspacePluginsPopover = memo(function WorkspacePluginsPopover({
  hostId,
}: {
  hostId: string;
}) {
  const { t } = useI18n();
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const [plugins, setPlugins] = useState<InstalledPluginView[]>([]);
  const [open, setOpen] = useState(false);

  const load = useCallback(async () => {
    try {
      const list = await orbitIpc.listPlugins(hostId);
      setPlugins(list);
    } catch {
      setPlugins([]);
    }
  }, [hostId]);

  useEffect(() => {
    void load();
    const onChanged = (ev: Event) => {
      const detail = (ev as CustomEvent<{ hostId?: string }>).detail;
      if (!detail?.hostId || detail.hostId === hostId) {
        void load();
      }
    };
    window.addEventListener(ORBIT_PLUGINS_CHANGED_EVENT, onChanged);
    return () => window.removeEventListener(ORBIT_PLUGINS_CHANGED_EVENT, onChanged);
  }, [hostId, load]);

  const panelSlots = useMemo(() => {
    const items: Array<{
      pluginId: string;
      title: string;
      slot: "side_panel_tab" | "stage_card";
      opNames: string[];
    }> = [];
    for (const plugin of plugins) {
      if (!plugin.enabled) continue;
      for (const decl of plugin.manifest.ui_slots) {
        if (decl.slot === "side_panel_tab" || decl.slot === "stage_card") {
          items.push({
            pluginId: plugin.manifest.id,
            title: decl.title || plugin.manifest.name,
            slot: decl.slot,
            opNames: plugin.manifest.mcp_ops.map((o) => o.name),
          });
        }
      }
    }
    return items;
  }, [plugins]);

  if (panelSlots.length === 0) return null;

  const effectiveCwd = selectedCwd && selectedCwd !== "~" ? selectedCwd : null;

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <SimpleTooltip label={t("plugins.sidePanelButton")}>
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label={t("plugins.sidePanelButton")}
            className="text-muted-foreground hover:text-foreground inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-[7px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
          >
            <Puzzle className="h-[14px] w-[14px] stroke-[1.75]" />
          </button>
        </PopoverTrigger>
      </SimpleTooltip>
      <PopoverContent align="end" sideOffset={6} className="w-[360px] max-h-[520px] overflow-y-auto p-3">
        <div className="flex flex-col gap-2.5">
          <div className="text-ink-muted px-0.5 text-[11px] font-semibold tracking-[0.04em] uppercase">
            {t("plugins.title")}
          </div>
          {open &&
            panelSlots.map((item) => (
              <PluginSlotCard
                key={`${item.pluginId}:${item.slot}`}
                hostId={hostId}
                pluginId={item.pluginId}
                title={item.title}
                slot={item.slot}
                cwd={effectiveCwd}
                mcpOpNames={item.opNames}
              />
            ))}
        </div>
      </PopoverContent>
    </Popover>
  );
});
