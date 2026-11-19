import { useEffect, useMemo, useState } from "react";
import {
  Activity,
  Bell,
  ChevronDown,
  ChevronRight,
  Cpu,
  Keyboard,
  Laptop,
  Palette,
  Plus,
  Server,
  Settings2,
  Wifi,
  X,
} from "lucide-react";
import { AddHostSection } from "@/components/settings/add-host-section";
import { AppearanceSection } from "@/components/settings/appearance-section";
import { GeneralSection } from "@/components/settings/general-section";
import { HostAgentsSection } from "@/components/settings/host-agents-section";
import { HostConnectionSection } from "@/components/settings/host-connection-section";
import { HostUsageSection } from "@/components/settings/host-usage-section";
import { NotificationsSection } from "@/components/settings/notifications-section";
import { HostStatusIndicator } from "@/components/settings/settings-primitives";
import { ShortcutsSection } from "@/components/settings/shortcuts-section";
import { SimpleTooltip } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { isLocalHostCard } from "@/lib/projects";
import {
  type HostSettingsTab,
  type SettingsSection,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const APP_SECTIONS: {
  id: Extract<SettingsSection, "general" | "appearance" | "shortcuts" | "notifications">;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}[] = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "shortcuts", label: "Shortcuts", icon: Keyboard },
  { id: "notifications", label: "Notifications", icon: Bell },
];

const HOST_TABS: {
  id: HostSettingsTab;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}[] = [
  { id: "agents", label: "Agents", icon: Cpu },
  { id: "usage", label: "Usage", icon: Activity },
  { id: "connection", label: "Connection", icon: Wifi },
];

function parseHostSection(
  section: SettingsSection,
): { hostId: string; tab: HostSettingsTab } | null {
  if (!section.startsWith("host:")) return null;
  const lastColon = section.lastIndexOf(":");
  if (lastColon <= 5) return null;
  const hostId = section.slice(5, lastColon);
  const tab = section.slice(lastColon + 1) as HostSettingsTab;
  if (tab !== "agents" && tab !== "usage" && tab !== "connection") return null;
  return { hostId, tab };
}

export function SettingsView({ closing = false }: { closing?: boolean }) {
  const section = useOrbitSelector((s) => s.settingsSection);
  const fleet = useOrbitSelector((s) => s.fleet);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);

  const parsedHost = useMemo(() => parseHostSection(section), [section]);

  const [expandedHostIds, setExpandedHostIds] = useState<Record<string, boolean>>(() => {
    const initial: Record<string, boolean> = {};
    const target = parsedHost?.hostId ?? selectedHostId ?? fleet.hosts[0]?.host_id;
    if (target) initial[target] = true;
    return initial;
  });

  // Auto-expand host whenever navigated directly into one of its tabs
  useEffect(() => {
    if (parsedHost?.hostId) {
      setExpandedHostIds((prev) =>
        prev[parsedHost.hostId] ? prev : { ...prev, [parsedHost.hostId]: true },
      );
    }
  }, [parsedHost?.hostId]);

  const resolvedHost = useMemo(() => {
    if (!parsedHost) return null;
    const exists = fleet.hosts.some((h) => h.host_id === parsedHost.hostId);
    if (exists) return parsedHost;
    const fallback = fleet.hosts[0];
    return fallback ? { hostId: fallback.host_id, tab: parsedHost.tab } : null;
  }, [parsedHost, fleet.hosts]);

  const toggleHostExpanded = (hostId: string) => {
    const currentlyExpanded = Boolean(expandedHostIds[hostId]);
    setExpandedHostIds((prev) => ({ ...prev, [hostId]: !currentlyExpanded }));
    if (!currentlyExpanded && parsedHost?.hostId !== hostId) {
      orbitActions.setSettingsSection(`host:${hostId}:agents`);
    }
  };

  return (
    <div
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) {
          orbitActions.closeSettings();
        }
      }}
      style={{
        animation: closing
          ? "orbit-backdrop-out 115ms var(--ease-exit) forwards"
          : "orbit-backdrop-in 160ms var(--ease-out-expo)",
      }}
      className={cn(
        "fixed inset-0 z-50 flex items-center justify-center bg-black/40 p-6 backdrop-blur-[2px] select-none",
        closing && "pointer-events-none",
      )}
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label="Settings"
        style={{
          animation: closing
            ? "orbit-modal-card-out 115ms var(--ease-exit) forwards"
            : "orbit-modal-card-in 180ms var(--ease-out-expo)",
        }}
        className="border-border bg-elevated text-foreground floating-surface relative flex h-[min(720px,86vh)] w-[min(1020px,92vw)] overflow-hidden rounded-[18px] border"
      >
        <button
          type="button"
          aria-label="Close settings"
          onClick={() => orbitActions.closeSettings()}
          className="text-muted-foreground hover:text-foreground hover:bg-accent absolute top-4 right-4 z-10 inline-flex h-[30px] w-[30px] cursor-pointer items-center justify-center rounded-[9px] transition-colors"
        >
          <X className="h-[15.5px] w-[15.5px]" />
        </button>

        {/* Left Settings Rail (232px) — App Preferences + Per-Host Fleet */}
        <aside className="border-border bg-sidebar/75 flex w-[232px] shrink-0 flex-col border-r px-3 py-4">
          <div className="px-2.5 pb-3 text-[14.5px] font-semibold tracking-[-0.01em]">
            Settings
          </div>

          <ScrollArea className="min-h-0 flex-1" viewportClassName="pr-0.5">
            <nav className="space-y-4">
            {/* Tier 1: App Global Preferences */}
            <div className="space-y-0.5">
              {APP_SECTIONS.map((item) => {
                const Icon = item.icon;
                const active = section === item.id;
                return (
                  <button
                    key={item.id}
                    type="button"
                    onClick={() => orbitActions.setSettingsSection(item.id)}
                    className={cn(
                      "flex h-[32px] w-full cursor-pointer items-center gap-2.5 rounded-[8px] px-2.5 text-left text-[13.5px] transition-colors",
                      active
                        ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.085]"
                        : "text-foreground/76 hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
                    )}
                  >
                    <Icon className="h-[15px] w-[15px] shrink-0 stroke-[1.75] opacity-80" />
                    <span className="truncate">{item.label}</span>
                  </button>
                );
              })}
            </div>

            {/* Tier 2: Per-Host Fleet Sections */}
            <div className="space-y-1">
              <div className="flex h-[24px] items-center justify-between px-2.5">
                <span className="text-muted-foreground text-[11.5px] font-medium">Hosts</span>
                <SimpleTooltip label="Add host">
                  <button
                    type="button"
                    aria-label="Add host"
                    onClick={() => orbitActions.setSettingsSection("add_host")}
                    className={cn(
                      "inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[6px] transition-colors",
                      section === "add_host"
                        ? "bg-black/[0.07] text-foreground dark:bg-white/[0.10]"
                        : "text-muted-foreground hover:bg-black/[0.045] hover:text-foreground dark:hover:bg-white/[0.06]",
                    )}
                  >
                    <Plus className="h-[13.5px] w-[13.5px] stroke-[1.9]" />
                  </button>
                </SimpleTooltip>
              </div>

              {fleet.hosts.length === 0 ? (
                <button
                  type="button"
                  onClick={() => orbitActions.setSettingsSection("add_host")}
                  className={cn(
                    "flex h-[32px] w-full cursor-pointer items-center gap-2.5 rounded-[8px] px-2.5 text-left text-[13px] transition-colors",
                    section === "add_host"
                      ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.085]"
                      : "text-muted-foreground hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
                  )}
                >
                  <Plus className="h-[14.5px] w-[14.5px] shrink-0" />
                  <span>Connect Host</span>
                </button>
              ) : (
                <div className="space-y-0.5">
                  {fleet.hosts.map((host) => {
                    const expanded = Boolean(expandedHostIds[host.host_id]);
                    const isLocal = isLocalHostCard(host);
                    const HostIcon = isLocal ? Laptop : Server;

                    return (
                      <div key={host.host_id} className="space-y-0.5">
                        <button
                          type="button"
                          onClick={() => toggleHostExpanded(host.host_id)}
                          className="group/host text-foreground/82 hover:text-foreground flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2.5 text-left text-[13px] transition-colors hover:bg-black/[0.035] dark:hover:bg-white/[0.045]"
                        >
                          <span className="flex min-w-0 items-center gap-2">
                            <span className="text-muted-foreground relative inline-flex h-[15px] w-[15px] shrink-0 items-center justify-center">
                              <HostIcon className="h-[14.5px] w-[14.5px] stroke-[1.75] transition-opacity group-hover/host:opacity-0" />
                              {expanded ? (
                                <ChevronDown className="absolute h-[13px] w-[13px] opacity-0 transition-opacity group-hover/host:opacity-90" />
                              ) : (
                                <ChevronRight className="absolute h-[13px] w-[13px] opacity-0 transition-opacity group-hover/host:opacity-90" />
                              )}
                            </span>
                            <span className="truncate font-medium">{host.display_name}</span>
                          </span>
                          <HostStatusIndicator state={host.connection_state} />
                        </button>

                        <div
                          data-open={expanded ? "true" : "false"}
                          className="orbit-collapse-grid"
                        >
                          <div className="orbit-collapse-inner">
                            <div className="space-y-0.5 pt-0.5 pl-4">
                              {HOST_TABS.map((tab) => {
                                const TabIcon = tab.icon;
                                const targetSection: SettingsSection = `host:${host.host_id}:${tab.id}`;
                                const active = section === targetSection;
                                return (
                                  <button
                                    key={tab.id}
                                    type="button"
                                    onClick={() =>
                                      orbitActions.setSettingsSection(targetSection)
                                    }
                                    className={cn(
                                      "flex h-[30px] w-full cursor-pointer items-center gap-2 rounded-[7px] px-2.5 text-left text-[13px] transition-colors",
                                      active
                                        ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.085]"
                                        : "text-foreground/72 hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
                                    )}
                                  >
                                    <TabIcon className="h-[14px] w-[14px] shrink-0 stroke-[1.75] opacity-75" />
                                    <span className="truncate">{tab.label}</span>
                                  </button>
                                );
                              })}
                            </div>
                          </div>
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </div>
            </nav>
          </ScrollArea>

          <div className="text-muted-foreground/65 px-2.5 pt-2 text-[11.5px]">Orbit</div>
        </aside>

        {/* Right Scrollable Settings Content */}
        <ScrollArea className="min-h-0 flex-1" viewportClassName="px-10 py-8">
          <div key={section} className="orbit-stage-enter mx-auto w-full max-w-[640px] pb-8">
            {section === "general" && <GeneralSection />}
            {section === "appearance" && <AppearanceSection />}
            {section === "shortcuts" && <ShortcutsSection />}
            {section === "notifications" && <NotificationsSection />}
            {section === "add_host" && <AddHostSection />}
            {parsedHost && !resolvedHost && <AddHostSection />}
            {resolvedHost?.tab === "agents" && (
              <HostAgentsSection hostId={resolvedHost.hostId} />
            )}
            {resolvedHost?.tab === "usage" && (
              <HostUsageSection hostId={resolvedHost.hostId} />
            )}
            {resolvedHost?.tab === "connection" && (
              <HostConnectionSection hostId={resolvedHost.hostId} />
            )}
          </div>
        </ScrollArea>
      </div>
    </div>
  );
}
