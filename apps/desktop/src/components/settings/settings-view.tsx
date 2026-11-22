import { type ReactNode, useEffect, useMemo, useState } from "react";
import {
  Activity,
  Archive,
  Bell,
  Check,
  ChevronsUpDown,
  Cloud,
  Cpu,
  Keyboard,
  Palette,
  Plus,
  Settings2,
  Wifi,
  X,
} from "lucide-react";
import { AddHostSection } from "@/components/settings/add-host-section";
import { AppearanceSection } from "@/components/settings/appearance-section";
import { ArchivedSection } from "@/components/settings/archived-section";
import { CloudSyncSection } from "@/components/settings/cloud-sync-section";
import { GeneralSection } from "@/components/settings/general-section";
import { HostAgentsSection } from "@/components/settings/host-agents-section";
import { HostConnectionSection } from "@/components/settings/host-connection-section";
import { HostUsageSection } from "@/components/settings/host-usage-section";
import { NotificationsSection } from "@/components/settings/notifications-section";
import {
  HostStatusIndicator,
  hostConnectionLabel,
} from "@/components/settings/settings-primitives";
import { ShortcutsSection } from "@/components/settings/shortcuts-section";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import type { HostCardView } from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import {
  type HostSettingsTab,
  type SettingsSection,
  orbitActions,
  threadScopeKey,
  useOrbitSelector,
} from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const APP_SECTIONS: {
  id: Extract<
    SettingsSection,
    "general" | "cloud_sync" | "appearance" | "shortcuts" | "notifications" | "archived"
  >;
  label: string;
  icon: React.ComponentType<{ className?: string }>;
}[] = [
  { id: "general", label: "General", icon: Settings2 },
  { id: "cloud_sync", label: "Cloud Sync", icon: Cloud },
  { id: "appearance", label: "Appearance", icon: Palette },
  { id: "shortcuts", label: "Shortcuts", icon: Keyboard },
  { id: "notifications", label: "Notifications", icon: Bell },
  { id: "archived", label: "Archived", icon: Archive },
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

function orderHostsLocalFirst(hosts: HostCardView[]): HostCardView[] {
  const local: HostCardView[] = [];
  const remote: HostCardView[] = [];
  for (const host of hosts) {
    if (isLocalHostCard(host)) {
      local.push(host);
    } else {
      remote.push(host);
    }
  }
  return [...local, ...remote];
}

function formatHostTransportSubtitle(host: HostCardView): string {
  if (isLocalHostCard(host)) return "Local";
  if (host.ssh_alias) return `SSH · ${host.ssh_alias}`;
  if (host.connection_state === "online_relay") return "Relay";
  return hostConnectionLabel(host.connection_state);
}

function SettingsRailButton({
  label,
  icon: Icon,
  active,
  badge,
  onClick,
}: {
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  active: boolean;
  badge?: ReactNode;
  onClick: () => void;
}) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex h-[32px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2.5 text-left text-[13.5px] transition-colors",
        active
          ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.085]"
          : "text-foreground/76 hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
      )}
    >
      <span className="flex min-w-0 items-center gap-2.5">
        <Icon className="h-[15px] w-[15px] shrink-0 stroke-[1.75] opacity-80" />
        <span className="truncate">{label}</span>
      </span>
      {badge}
    </button>
  );
}

function SettingsHostTabList({
  hostId,
  activeSection,
}: {
  hostId: string;
  activeSection: SettingsSection;
}) {
  return (
    <div className="space-y-0.5">
      {HOST_TABS.map((tab) => {
        const targetSection: SettingsSection = `host:${hostId}:${tab.id}`;
        return (
          <SettingsRailButton
            key={tab.id}
            label={tab.label}
            icon={tab.icon}
            active={activeSection === targetSection}
            onClick={() => orbitActions.setSettingsSection(targetSection)}
          />
        );
      })}
    </div>
  );
}

export function SettingsView({ closing = false }: { closing?: boolean }) {
  const section = useOrbitSelector((s) => s.settingsSection);
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const archivedThreads = useOrbitSelector((s) => s.archivedThreads);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);

  const archivedCount = useMemo(() => {
    const seen = new Set<string>();
    for (const [hostId, detail] of Object.entries(hostDetails)) {
      for (const ag of detail.agents) {
        if (ag.background) continue;
        const tKey = threadScopeKey(hostId, ag.agent_id);
        if (!ag.archived_at_rfc3339 && !archivedThreads[tKey]) continue;
        seen.add(tKey);
      }
    }
    return seen.size;
  }, [hostDetails, archivedThreads]);

  const renderAppSectionBadge = (id: (typeof APP_SECTIONS)[number]["id"]) => {
    if (id !== "archived" || archivedCount === 0) return undefined;
    return (
      <span className="text-muted-foreground/75 shrink-0 text-[11.5px] tabular-nums">
        {archivedCount}
      </span>
    );
  };

  const parsedHost = useMemo(() => parseHostSection(section), [section]);
  const orderedHosts = useMemo(() => orderHostsLocalFirst(fleet.hosts), [fleet.hosts]);
  const hasRemoteHosts = useMemo(
    () => orderedHosts.some((h) => !isLocalHostCard(h)),
    [orderedHosts],
  );
  const isLocalOnlyMode = orderedHosts.length === 1 && !hasRemoteHosts;
  const isMultiOrRemoteMode = hasRemoteHosts || orderedHosts.length > 1;

  const [pickedHostId, setPickedHostId] = useState<string | null>(
    () => parsedHost?.hostId ?? selectedHostId ?? orderedHosts[0]?.host_id ?? null,
  );

  // Sync picked host whenever navigated directly into a host tab
  useEffect(() => {
    if (parsedHost?.hostId && orderedHosts.some((h) => h.host_id === parsedHost.hostId)) {
      setPickedHostId(parsedHost.hostId);
    }
  }, [parsedHost?.hostId, orderedHosts]);

  const activeHost = useMemo(() => {
    if (parsedHost) {
      const matched = orderedHosts.find((h) => h.host_id === parsedHost.hostId);
      if (matched) return matched;
    }
    if (pickedHostId) {
      const matched = orderedHosts.find((h) => h.host_id === pickedHostId);
      if (matched) return matched;
    }
    if (selectedHostId) {
      const matched = orderedHosts.find((h) => h.host_id === selectedHostId);
      if (matched) return matched;
    }
    return orderedHosts[0] ?? null;
  }, [parsedHost, pickedHostId, selectedHostId, orderedHosts]);

  const resolvedHost = useMemo(() => {
    if (!parsedHost) return null;
    const exists = orderedHosts.some((h) => h.host_id === parsedHost.hostId);
    if (exists) return parsedHost;
    const fallback = orderedHosts[0];
    return fallback ? { hostId: fallback.host_id, tab: parsedHost.tab } : null;
  }, [parsedHost, orderedHosts]);

  const handleSelectHost = (hostId: string) => {
    setPickedHostId(hostId);
    if (parsedHost) {
      orbitActions.setSettingsSection(`host:${hostId}:${parsedHost.tab}`);
    } else if (section === "add_host") {
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
          ? "orbit-backdrop-out 155ms var(--ease-exit) forwards"
          : "orbit-backdrop-in 195ms var(--ease-out-quart)",
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
            ? "orbit-modal-card-out 155ms var(--ease-exit) forwards"
            : "orbit-modal-card-in 210ms var(--ease-out-expo)",
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

        {/* Left Settings Rail (232px) — Flat on Local-Only; Host Picker on Remote/Multi-Host */}
        <aside className="border-border bg-sidebar/75 flex w-[232px] shrink-0 flex-col border-r px-3 py-4">
          <div className="px-2.5 pb-3 text-[14.5px] font-semibold tracking-[-0.01em]">
            Settings
          </div>

          <ScrollArea className="min-h-0 flex-1" viewportClassName="pr-0.5">
            {orderedHosts.length === 0 ? (
              <nav className="space-y-3">
                <div className="space-y-0.5">
                  {APP_SECTIONS.map((item) => (
                    <SettingsRailButton
                      key={item.id}
                      label={item.label}
                      icon={item.icon}
                      active={section === item.id}
                      badge={renderAppSectionBadge(item.id)}
                      onClick={() => orbitActions.setSettingsSection(item.id)}
                    />
                  ))}
                </div>

                <div className="border-border/60 border-t pt-3">
                  <SettingsRailButton
                    label="Connect Remote Host"
                    icon={Plus}
                    active={section === "add_host"}
                    onClick={() => orbitActions.setSettingsSection("add_host")}
                  />
                </div>
              </nav>
            ) : isLocalOnlyMode && activeHost ? (
              <nav className="space-y-3">
                <div className="space-y-0.5">
                  {APP_SECTIONS.map((item) => (
                    <SettingsRailButton
                      key={item.id}
                      label={item.label}
                      icon={item.icon}
                      active={section === item.id}
                      badge={renderAppSectionBadge(item.id)}
                      onClick={() => orbitActions.setSettingsSection(item.id)}
                    />
                  ))}
                </div>

                <div className="border-border/60 border-t pt-3">
                  <SettingsHostTabList hostId={activeHost.host_id} activeSection={section} />
                </div>

                <div className="border-border/60 border-t pt-3">
                  <SettingsRailButton
                    label="Connect Remote Host"
                    icon={Plus}
                    active={section === "add_host"}
                    onClick={() => orbitActions.setSettingsSection("add_host")}
                  />
                </div>
              </nav>
            ) : isMultiOrRemoteMode && activeHost ? (
              <nav className="space-y-4">
                {/* Group 1: App Preferences */}
                <div className="space-y-1">
                  <div className="px-2.5 pb-0.5 text-[11.5px] font-medium text-muted-foreground">
                    App
                  </div>
                  <div className="space-y-0.5">
                    {APP_SECTIONS.map((item) => (
                      <SettingsRailButton
                        key={item.id}
                        label={item.label}
                        icon={item.icon}
                        active={section === item.id}
                        badge={renderAppSectionBadge(item.id)}
                        onClick={() => orbitActions.setSettingsSection(item.id)}
                      />
                    ))}
                  </div>
                </div>

                {/* Group 2: Host Picker + Flat Host Sections */}
                <div className="space-y-1.5">
                  <div className="flex h-[22px] items-center justify-between px-2.5">
                    <span className="text-muted-foreground text-[11.5px] font-medium">Host</span>
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

                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        aria-label="Switch host"
                        className="border-border/80 bg-background/70 hover:bg-accent/70 focus-visible:ring-foreground/20 flex h-[32px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] border px-2.5 text-left transition-colors outline-none focus-visible:ring-1"
                      >
                        <span className="flex min-w-0 flex-1 items-center gap-2">
                          <HostStatusIndicator state={activeHost.connection_state} />
                          <span className="text-foreground truncate text-[13px] font-medium">
                            {activeHost.display_name}
                          </span>
                          {isLocalHostCard(activeHost) && (
                            <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium">
                              Local
                            </span>
                          )}
                        </span>
                        <ChevronsUpDown className="text-muted-foreground/75 h-[13px] w-[13px] shrink-0" />
                      </button>
                    </DropdownMenuTrigger>

                    <DropdownMenuContent
                      align="start"
                      sideOffset={5}
                      finalFocus={false}
                      className="w-[212px] p-1.5"
                    >
                      {orderedHosts.map((host) => {
                        const selected = host.host_id === activeHost.host_id;
                        return (
                          <DropdownMenuItem
                            key={host.host_id}
                            onSelect={() => handleSelectHost(host.host_id)}
                            className="items-center py-1.5"
                          >
                            <span className="flex min-w-0 flex-1 items-center gap-2.5">
                              <HostStatusIndicator state={host.connection_state} />
                              <span className="min-w-0 flex-1">
                                <span
                                  className={cn(
                                    "block truncate text-[12.5px]",
                                    selected
                                      ? "text-foreground font-medium"
                                      : "text-foreground/90 font-normal",
                                  )}
                                >
                                  {host.display_name}
                                </span>
                                <span className="text-muted-foreground block truncate text-[11px]">
                                  {formatHostTransportSubtitle(host)}
                                </span>
                              </span>
                            </span>
                            {selected && (
                              <Check className="text-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[2]" />
                            )}
                          </DropdownMenuItem>
                        );
                      })}

                      <DropdownMenuSeparator />

                      <DropdownMenuItem
                        onSelect={() => orbitActions.setSettingsSection("add_host")}
                      >
                        <span className="flex items-center gap-2">
                          <Plus className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.85]" />
                          <span className="text-[12.5px]">Add Host</span>
                        </span>
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>

                  <SettingsHostTabList hostId={activeHost.host_id} activeSection={section} />
                </div>
              </nav>
            ) : null}
          </ScrollArea>

          <div className="text-muted-foreground/65 px-2.5 pt-2 text-[11.5px]">Orbit</div>
        </aside>

        {/* Right Scrollable Settings Content */}
        <ScrollArea className="min-h-0 flex-1" viewportClassName="px-10 py-8">
          <div key={section} className="orbit-stage-enter mx-auto w-full max-w-[640px] pb-8">
            {section === "general" && <GeneralSection />}
            {section === "cloud_sync" && <CloudSyncSection />}
            {section === "appearance" && <AppearanceSection />}
            {section === "shortcuts" && <ShortcutsSection />}
            {section === "notifications" && <NotificationsSection />}
            {section === "archived" && <ArchivedSection />}
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
