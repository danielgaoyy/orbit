import { type ReactNode, useEffect, useMemo, useState } from "react";
import {
  Activity,
  Bell,
  BookOpen,
  Check,
  ChevronsUpDown,
  Cloud,
  Cpu,
  Keyboard,
  MessageSquare,
  Palette,
  Plug,
  Plus,
  Search,
  Server,
  Settings2,
  ShieldCheck,
  X,
} from "lucide-react";
import { AddHostSection } from "@/components/settings/add-host-section";
import { AppearanceSection } from "@/components/settings/appearance-section";
import { ArchivedSection } from "@/components/settings/archived-section";
import { CloudSyncSection } from "@/components/settings/cloud-sync-section";
import { GeneralSection } from "@/components/settings/general-section";
import { InstructionsSection } from "@/components/settings/instructions-section";
import { HostAcpSection } from "@/components/settings/host-acp-section";
import { HostAgentsSection } from "@/components/settings/host-agents-section";
import { HostConnectionSection } from "@/components/settings/host-connection-section";
import { HostGuardianSection } from "@/components/settings/host-guardian-section";
import { HostHooksSection } from "@/components/settings/host-hooks-section";
import { HostImChannelsSection } from "@/components/settings/host-im-channels-section";
import { HostLspSection } from "@/components/settings/host-lsp-section";
import { HostMcpSection } from "@/components/settings/host-mcp-section";
import { HostRulesSection } from "@/components/settings/host-rules-section";
import { HostSkillsSection } from "@/components/settings/host-skills-section";
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

type RailNavId =
  | "general"
  | "appearance"
  | "notifications"
  | "shortcuts"
  | "sync_data"
  | "models_agents"
  | "context"
  | "extensions"
  | "guardian"
  | "usage"
  | "channels"
  | "hosts";

interface RailItemSpec {
  id: RailNavId;
  group: "app" | "agents" | "fleet";
  label: string;
  icon: React.ComponentType<{ className?: string }>;
  keywords: string[];
  requiresHost?: boolean;
}

const RAIL_ITEMS: RailItemSpec[] = [
  // App (5)
  {
    id: "general",
    group: "app",
    label: "General",
    icon: Settings2,
    keywords: ["general", "enter", "editor", "worktree", "branch", "update", "version"],
  },
  {
    id: "appearance",
    group: "app",
    label: "Appearance",
    icon: Palette,
    keywords: ["appearance", "theme", "dark", "light", "font", "diff", "terminal", "cursor"],
  },
  {
    id: "notifications",
    group: "app",
    label: "Notifications",
    icon: Bell,
    keywords: ["notifications", "push", "apns", "fcm", "harmony", "quiet hours", "sound", "alert"],
  },
  {
    id: "shortcuts",
    group: "app",
    label: "Shortcuts",
    icon: Keyboard,
    keywords: ["shortcuts", "keybindings", "hotkeys", "keyboard"],
  },
  {
    id: "sync_data",
    group: "app",
    label: "Sync & Data",
    icon: Cloud,
    keywords: ["cloud", "sync", "gist", "backup", "archived", "archive", "history", "data"],
  },
  // Agents (4)
  {
    id: "models_agents",
    group: "agents",
    label: "Models & Agents",
    icon: Cpu,
    requiresHost: true,
    keywords: [
      "models",
      "agents",
      "providers",
      "claude",
      "codex",
      "antigravity",
      "acp",
      "registry",
      "profiles",
      "fast",
      "keep awake",
      "orchestrator",
    ],
  },
  {
    id: "context",
    group: "agents",
    label: "Context",
    icon: BookOpen,
    keywords: ["context", "instructions", "agents.md", "claude.md", "rules", "glob", "skills"],
  },
  {
    id: "extensions",
    group: "agents",
    label: "Extensions",
    icon: Plug,
    requiresHost: true,
    keywords: ["extensions", "mcp", "servers", "hooks", "lifecycle", "lsp", "language servers"],
  },
  {
    id: "guardian",
    group: "agents",
    label: "Guardian",
    icon: ShieldCheck,
    requiresHost: true,
    keywords: ["guardian", "auto", "approval", "safety", "risk", "policy", "security", "blocked"],
  },
  // Operations & Fleet (3)
  {
    id: "usage",
    group: "fleet",
    label: "Usage",
    icon: Activity,
    requiresHost: true,
    keywords: ["usage", "tokens", "cost", "spend", "quotas", "insights", "analytics"],
  },
  {
    id: "channels",
    group: "fleet",
    label: "Channels",
    icon: MessageSquare,
    requiresHost: true,
    keywords: ["channels", "im", "lark", "feishu", "slack", "telegram", "discord", "bot"],
  },
  {
    id: "hosts",
    group: "fleet",
    label: "Hosts",
    icon: Server,
    keywords: ["hosts", "connection", "remote", "ssh", "relay", "pair", "pairing", "mobile", "add host"],
  },
];

function parseHostSection(
  section: SettingsSection,
): { hostId: string; tab: HostSettingsTab } | null {
  if (!section.startsWith("host:")) return null;
  const lastColon = section.lastIndexOf(":");
  if (lastColon <= 5) return null;
  const hostId = section.slice(5, lastColon);
  const tab = section.slice(lastColon + 1) as HostSettingsTab;
  if (
    tab !== "agents" &&
    tab !== "guardian" &&
    tab !== "im_channels" &&
    tab !== "skills" &&
    tab !== "acp" &&
    tab !== "mcp" &&
    tab !== "rules" &&
    tab !== "hooks" &&
    tab !== "lsp" &&
    tab !== "usage" &&
    tab !== "connection"
  ) {
    return null;
  }
  return { hostId, tab };
}

function resolveActiveRailId(
  section: SettingsSection,
  parsedHost: { hostId: string; tab: HostSettingsTab } | null,
): RailNavId {
  if (section === "general") return "general";
  if (section === "appearance") return "appearance";
  if (section === "notifications") return "notifications";
  if (section === "shortcuts") return "shortcuts";
  if (section === "cloud_sync" || section === "archived") return "sync_data";
  if (section === "instructions") return "context";
  if (section === "add_host") return "hosts";
  if (parsedHost) {
    switch (parsedHost.tab) {
      case "agents":
      case "acp":
        return "models_agents";
      case "rules":
      case "skills":
        return "context";
      case "mcp":
      case "hooks":
      case "lsp":
        return "extensions";
      case "guardian":
        return "guardian";
      case "usage":
        return "usage";
      case "im_channels":
        return "channels";
      case "connection":
        return "hosts";
    }
  }
  return "general";
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
        "flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2.5 text-left text-[13px] transition-colors",
        active
          ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.085]"
          : "text-foreground/76 hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.05]",
      )}
    >
      <span className="flex min-w-0 items-center gap-2.5">
        <Icon className="h-[14.5px] w-[14.5px] shrink-0 stroke-[1.75] opacity-80" />
        <span className="truncate">{label}</span>
      </span>
      {badge}
    </button>
  );
}

function SettingsSubTabBar({
  tabs,
  activeId,
  onSelect,
}: {
  tabs: { id: string; label: string; count?: number }[];
  activeId: string;
  onSelect: (id: string) => void;
}) {
  return (
    <div className="mb-5 flex items-center gap-1 border-b border-black/[0.06] pb-2.5 dark:border-white/[0.07]">
      {tabs.map((tab) => {
        const selected = tab.id === activeId;
        return (
          <button
            key={tab.id}
            type="button"
            onClick={() => onSelect(tab.id)}
            className={cn(
              "inline-flex h-[26px] cursor-pointer items-center gap-1.5 rounded-[7px] px-2.5 text-[12.5px] transition-colors",
              selected
                ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.09]"
                : "text-muted-foreground hover:bg-black/[0.035] hover:text-foreground dark:hover:bg-white/[0.05]",
            )}
          >
            <span>{tab.label}</span>
            {tab.count !== undefined && tab.count > 0 && (
              <span className="text-muted-foreground/75 tabular-nums text-[11px] font-normal">
                {tab.count}
              </span>
            )}
          </button>
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

  const [filterQuery, setFilterQuery] = useState("");

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

  const parsedHost = useMemo(() => parseHostSection(section), [section]);
  const orderedHosts = useMemo(() => orderHostsLocalFirst(fleet.hosts), [fleet.hosts]);
  const hasRemoteHosts = useMemo(
    () => orderedHosts.some((h) => !isLocalHostCard(h)),
    [orderedHosts],
  );
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

  const activeRailId = useMemo(
    () => resolveActiveRailId(section, parsedHost),
    [section, parsedHost],
  );

  const handleSelectHost = (hostId: string) => {
    setPickedHostId(hostId);
    if (parsedHost) {
      orbitActions.setSettingsSection(`host:${hostId}:${parsedHost.tab}`);
    } else if (section === "add_host") {
      orbitActions.setSettingsSection(`host:${hostId}:connection`);
    }
  };

  const navigateRailItem = (id: RailNavId) => {
    const hId = activeHost?.host_id ?? null;
    switch (id) {
      case "general":
        orbitActions.setSettingsSection("general");
        return;
      case "appearance":
        orbitActions.setSettingsSection("appearance");
        return;
      case "notifications":
        orbitActions.setSettingsSection("notifications");
        return;
      case "shortcuts":
        orbitActions.setSettingsSection("shortcuts");
        return;
      case "sync_data":
        orbitActions.setSettingsSection("cloud_sync");
        return;
      case "models_agents":
        orbitActions.setSettingsSection(hId ? `host:${hId}:agents` : "add_host");
        return;
      case "context":
        orbitActions.setSettingsSection("instructions");
        return;
      case "extensions":
        orbitActions.setSettingsSection(hId ? `host:${hId}:mcp` : "add_host");
        return;
      case "guardian":
        orbitActions.setSettingsSection(hId ? `host:${hId}:guardian` : "add_host");
        return;
      case "usage":
        orbitActions.setSettingsSection(hId ? `host:${hId}:usage` : "add_host");
        return;
      case "channels":
        orbitActions.setSettingsSection(hId ? `host:${hId}:im_channels` : "add_host");
        return;
      case "hosts":
        orbitActions.setSettingsSection(hId ? `host:${hId}:connection` : "add_host");
        return;
    }
  };

  const normalizedFilter = filterQuery.trim().toLowerCase();
  const visibleRailItems = useMemo(() => {
    return RAIL_ITEMS.filter((item) => {
      if (item.requiresHost && !activeHost) return false;
      if (!normalizedFilter) return true;
      if (item.label.toLowerCase().includes(normalizedFilter)) return true;
      return item.keywords.some((k) => k.includes(normalizedFilter));
    });
  }, [activeHost, normalizedFilter]);

  const appItems = visibleRailItems.filter((i) => i.group === "app");
  const agentItems = visibleRailItems.filter((i) => i.group === "agents");
  const fleetItems = visibleRailItems.filter((i) => i.group === "fleet");

  const renderRailBadge = (id: RailNavId) => {
    if (id === "sync_data" && archivedCount > 0) {
      return (
        <span className="text-muted-foreground/70 shrink-0 text-[11px] tabular-nums">
          {archivedCount}
        </span>
      );
    }
    return undefined;
  };

  const effectiveHostId = resolvedHost?.hostId ?? activeHost?.host_id ?? null;

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

        {/* Left Settings Rail (232px) — Full-bleed ScrollArea so scrollbar hugs border without overlapping items */}
        <aside className="border-border bg-sidebar/75 flex w-[232px] shrink-0 flex-col border-r py-4">
          <div className="px-5 pb-2.5 text-[14.5px] font-semibold tracking-[-0.01em]">
            Settings
          </div>

          <div className="mb-3 px-3">
            <div className="relative px-0.5">
              <Search className="text-muted-foreground/65 pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2 stroke-[1.8]" />
              <input
                type="text"
                value={filterQuery}
                onChange={(e) => setFilterQuery(e.target.value)}
                placeholder="Search settings…"
                aria-label="Search settings"
                className="border-border/75 bg-background/65 text-foreground placeholder:text-muted-foreground/55 focus:border-foreground/25 h-[28px] w-full rounded-[8px] border pr-6 pl-7 text-[12px] outline-none transition-colors"
              />
              {filterQuery && (
                <button
                  type="button"
                  aria-label="Clear settings search"
                  onClick={() => setFilterQuery("")}
                  className="text-muted-foreground hover:text-foreground absolute top-1/2 right-2 -translate-y-1/2 cursor-pointer rounded p-0.5"
                >
                  <X className="size-3" />
                </button>
              )}
            </div>
          </div>

          <ScrollArea className="min-h-0 flex-1" viewportClassName="px-3 pb-1">
            <nav className="space-y-3.5">
              {appItems.length > 0 && (
                <div className="space-y-0.5">
                  <div className="text-muted-foreground/75 px-2.5 pb-0.5 text-[11px] font-medium">
                    App
                  </div>
                  {appItems.map((item) => (
                    <SettingsRailButton
                      key={item.id}
                      label={item.label}
                      icon={item.icon}
                      active={activeRailId === item.id}
                      badge={renderRailBadge(item.id)}
                      onClick={() => navigateRailItem(item.id)}
                    />
                  ))}
                </div>
              )}

              {/* Multi-host selector appears cleanly above Host-scoped groups */}
              {isMultiOrRemoteMode && activeHost && (agentItems.length > 0 || fleetItems.length > 0) && (
                <div className="border-border/60 border-t pt-3">
                  <DropdownMenu>
                    <DropdownMenuTrigger asChild>
                      <button
                        type="button"
                        aria-label="Switch host"
                        className="border-border/80 bg-background/70 hover:bg-accent/70 focus-visible:ring-foreground/20 flex h-[30px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] border px-2.5 text-left transition-colors outline-none focus-visible:ring-1"
                      >
                        <span className="flex min-w-0 flex-1 items-center gap-2">
                          <HostStatusIndicator state={activeHost.connection_state} />
                          <span className="text-foreground truncate text-[12.5px] font-medium">
                            {activeHost.display_name}
                          </span>
                          {isLocalHostCard(activeHost) && (
                            <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10px] font-medium">
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
                          <span className="text-[12.5px]">Connect Host</span>
                        </span>
                      </DropdownMenuItem>
                    </DropdownMenuContent>
                  </DropdownMenu>
                </div>
              )}

              {agentItems.length > 0 && (
                <div className={cn("space-y-0.5", !isMultiOrRemoteMode && "border-border/60 border-t pt-3")}>
                  <div className="text-muted-foreground/75 px-2.5 pb-0.5 text-[11px] font-medium">
                    Agents
                  </div>
                  {agentItems.map((item) => (
                    <SettingsRailButton
                      key={item.id}
                      label={item.label}
                      icon={item.icon}
                      active={activeRailId === item.id}
                      onClick={() => navigateRailItem(item.id)}
                    />
                  ))}
                </div>
              )}

              {fleetItems.length > 0 && (
                <div className="border-border/60 space-y-0.5 border-t pt-3">
                  <div className="text-muted-foreground/75 px-2.5 pb-0.5 text-[11px] font-medium">
                    Operations & Fleet
                  </div>
                  {fleetItems.map((item) => (
                    <SettingsRailButton
                      key={item.id}
                      label={item.label}
                      icon={item.icon}
                      active={activeRailId === item.id}
                      onClick={() => navigateRailItem(item.id)}
                    />
                  ))}
                </div>
              )}

              {visibleRailItems.length === 0 && (
                <div className="text-muted-foreground/75 px-2.5 py-6 text-center text-[12px]">
                  No matching settings
                </div>
              )}
            </nav>
          </ScrollArea>

          <div className="text-muted-foreground/65 px-5 pt-2 text-[11.5px]">Orbit</div>
        </aside>

        {/* Right Settings Content Area */}
        {section === "instructions" ? (
          <div className="flex min-h-0 flex-1 flex-col overflow-hidden px-7 pt-6 pb-5">
            {effectiveHostId && (
              <SettingsSubTabBar
                activeId="instructions"
                tabs={[
                  { id: "instructions", label: "Instructions" },
                  { id: "rules", label: "Rules" },
                  { id: "skills", label: "Skills" },
                ]}
                onSelect={(tabId) => {
                  if (tabId === "instructions") orbitActions.setSettingsSection("instructions");
                  else if (tabId === "rules")
                    orbitActions.setSettingsSection(`host:${effectiveHostId}:rules`);
                  else if (tabId === "skills")
                    orbitActions.setSettingsSection(`host:${effectiveHostId}:skills`);
                }}
              />
            )}
            <div key={section} className="orbit-stage-enter min-h-0 flex-1">
              <InstructionsSection />
            </div>
          </div>
        ) : (
          <ScrollArea className="min-h-0 flex-1" viewportClassName="px-10 py-7">
            <div key={section} className="orbit-stage-enter mx-auto w-full max-w-[640px] pb-8">
              {/* In-page sub-tabs for consolidated sections */}
              {activeRailId === "sync_data" && (
                <SettingsSubTabBar
                  activeId={section === "archived" ? "archived" : "cloud_sync"}
                  tabs={[
                    { id: "cloud_sync", label: "Cloud Sync" },
                    { id: "archived", label: "Archived", count: archivedCount },
                  ]}
                  onSelect={(tabId) =>
                    orbitActions.setSettingsSection(
                      tabId === "archived" ? "archived" : "cloud_sync",
                    )
                  }
                />
              )}

              {activeRailId === "models_agents" && effectiveHostId && (
                <SettingsSubTabBar
                  activeId={resolvedHost?.tab === "acp" ? "acp" : "agents"}
                  tabs={[
                    { id: "agents", label: "Providers & Runtime" },
                    { id: "acp", label: "ACP Registry" },
                  ]}
                  onSelect={(tabId) =>
                    orbitActions.setSettingsSection(
                      `host:${effectiveHostId}:${tabId === "acp" ? "acp" : "agents"}`,
                    )
                  }
                />
              )}

              {activeRailId === "context" && effectiveHostId && (
                <SettingsSubTabBar
                  activeId={resolvedHost?.tab === "skills" ? "skills" : "rules"}
                  tabs={[
                    { id: "instructions", label: "Instructions" },
                    { id: "rules", label: "Rules" },
                    { id: "skills", label: "Skills" },
                  ]}
                  onSelect={(tabId) => {
                    if (tabId === "instructions") orbitActions.setSettingsSection("instructions");
                    else if (tabId === "rules")
                      orbitActions.setSettingsSection(`host:${effectiveHostId}:rules`);
                    else if (tabId === "skills")
                      orbitActions.setSettingsSection(`host:${effectiveHostId}:skills`);
                  }}
                />
              )}

              {activeRailId === "extensions" && effectiveHostId && (
                <SettingsSubTabBar
                  activeId={
                    resolvedHost?.tab === "hooks"
                      ? "hooks"
                      : resolvedHost?.tab === "lsp"
                        ? "lsp"
                        : "mcp"
                  }
                  tabs={[
                    { id: "mcp", label: "MCP Servers" },
                    { id: "hooks", label: "Hooks" },
                    { id: "lsp", label: "Language Servers" },
                  ]}
                  onSelect={(tabId) =>
                    orbitActions.setSettingsSection(
                      `host:${effectiveHostId}:${tabId as "mcp" | "hooks" | "lsp"}`,
                    )
                  }
                />
              )}

              {activeRailId === "hosts" && effectiveHostId && (
                <SettingsSubTabBar
                  activeId={section === "add_host" ? "add_host" : "connection"}
                  tabs={[
                    { id: "connection", label: "Host & Pairing" },
                    { id: "add_host", label: "Connect Remote Host" },
                  ]}
                  onSelect={(tabId) =>
                    orbitActions.setSettingsSection(
                      tabId === "add_host" ? "add_host" : `host:${effectiveHostId}:connection`,
                    )
                  }
                />
              )}

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
              {resolvedHost?.tab === "guardian" && (
                <HostGuardianSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "im_channels" && (
                <HostImChannelsSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "skills" && (
                <HostSkillsSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "acp" && (
                <HostAcpSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "mcp" && (
                <HostMcpSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "rules" && (
                <HostRulesSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "hooks" && (
                <HostHooksSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "lsp" && (
                <HostLspSection key={resolvedHost.hostId} hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "usage" && (
                <HostUsageSection hostId={resolvedHost.hostId} />
              )}
              {resolvedHost?.tab === "connection" && (
                <HostConnectionSection hostId={resolvedHost.hostId} />
              )}
            </div>
          </ScrollArea>
        )}
      </div>
    </div>
  );
}
