import { useMemo } from "react";
import {
  Archive,
  ChevronDown,
  ChevronRight,
  GitBranch,
  Loader2,
  Plus,
  RefreshCw,
  Settings,
  ShieldCheck,
} from "lucide-react";
import {
  type AgentCardView,
  type HostCardView,
  type HostConnectionState,
  isMacDesktop,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { SimpleTooltip } from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function basename(path: string): string {
  if (!path || path === "~") return "~";
  const clean = path.replace(/\/+$/, "");
  const i = clean.lastIndexOf("/");
  return i >= 0 ? clean.slice(i + 1) || "/" : clean;
}

function HostStateDot({ state }: { state: HostConnectionState }) {
  switch (state) {
    case "online_direct":
      return <span className="size-1.5 shrink-0 rounded-full bg-emerald-500" title="Online" />;
    case "online_relay":
      return (
        <ShieldCheck
          className="size-3 shrink-0 text-indigo-400"
          aria-label="Noise E2EE Relay"
        />
      );
    case "connecting_direct":
    case "connecting_relay":
      return <Loader2 className="size-2.5 shrink-0 animate-spin text-amber-500" />;
    default:
      return <span className="size-1.5 shrink-0 rounded-full bg-zinc-400/70" title="Offline" />;
  }
}

function AgentStatusGlyph({ agent }: { agent: AgentCardView }) {
  if (agent.is_awaiting_permission) {
    return <span className="size-2 shrink-0 rounded-full bg-amber-500" />;
  }
  if (agent.is_running) {
    return <Loader2 className="text-muted-foreground size-3 shrink-0 animate-spin" />;
  }
  if (agent.status_label === "Error") {
    return <span className="size-1.5 shrink-0 rounded-full bg-rose-500" />;
  }
  return <span className="bg-muted-foreground/30 size-1.5 shrink-0 rounded-full" />;
}

interface WorkspaceGroup {
  cwd: string;
  label: string;
  isWorktree: boolean;
  agents: AgentCardView[];
}

function groupAgentsByWorkspace(
  agents: AgentCardView[],
  workspacesCwdMap: Map<string, boolean>,
): WorkspaceGroup[] {
  const groups = new Map<string, WorkspaceGroup>();
  for (const ag of agents) {
    const existing = groups.get(ag.cwd);
    if (existing) {
      existing.agents.push(ag);
    } else {
      groups.set(ag.cwd, {
        cwd: ag.cwd,
        label: basename(ag.cwd),
        isWorktree: workspacesCwdMap.get(ag.cwd) ?? ag.cwd.includes("/worktrees/"),
        agents: [ag],
      });
    }
  }
  return Array.from(groups.values());
}

export function FleetSidebar() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const collapsedHostIds = useOrbitSelector((s) => s.collapsedHostIds);

  const isMac = isMacDesktop();

  // Collect all cross-host blocked sessions for the conditional Attention Inbox
  const attentionItems = useMemo(() => {
    const items: { host: HostCardView; agent: AgentCardView }[] = [];
    for (const host of fleet.hosts) {
      const detail = hostDetails[host.host_id];
      if (!detail) continue;
      for (const ag of detail.agents) {
        if (ag.is_awaiting_permission) {
          items.push({ host, agent: ag });
        }
      }
    }
    return items;
  }, [fleet.hosts, hostDetails]);

  const singleHostMode = fleet.hosts.length === 1;

  return (
    <aside className="bg-sidebar border-border flex h-full w-full flex-col border-r select-none">
      {/* Top Header (aligned with macOS traffic lights — `38px` titlebar rhythm, `86px` left clearance) */}
      <div
        data-tauri-drag-region
        className={cn(
          "flex h-[38px] shrink-0 items-center justify-between px-3.5",
          isMac && "pl-[86px]",
        )}
      >
        <div className="flex items-center gap-2" data-tauri-drag-region>
          <span className="text-foreground/88 text-[13.5px] font-medium tracking-[-0.01em]">
            Orbit
          </span>
          {singleHostMode && fleet.hosts[0] && (
            <HostStateDot state={fleet.hosts[0].connection_state} />
          )}
        </div>

        <SimpleTooltip label="New session">
          <button
            type="button"
            onClick={() => orbitActions.startNewThreadDraft()}
            className={cn(
              "text-foreground/65 hover:text-foreground inline-flex size-8 cursor-pointer items-center justify-center rounded-lg transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
              isDraftNewSession && "text-foreground bg-black/[0.06] dark:bg-white/[0.08]",
            )}
          >
            <Plus className="size-[17px] stroke-[1.75]" />
          </button>
        </SimpleTooltip>
      </div>

      {/* Conditional Attention Inbox — 0px invisible when all agents are clear */}
      {attentionItems.length > 0 && (
        <div className="border-border mx-3 mb-2 rounded-xl border bg-amber-500/[0.06] p-2">
          <div className="mb-1 px-1.5 text-[11.5px] font-medium text-amber-700 dark:text-amber-400">
            Waiting for approval · {attentionItems.length}
          </div>
          <div className="space-y-0.5">
            {attentionItems.map(({ host, agent }) => {
              const active =
                !isDraftNewSession &&
                selectedHostId === host.host_id &&
                selectedAgentId === agent.agent_id;
              return (
                <button
                  key={`${host.host_id}-${agent.agent_id}`}
                  type="button"
                  onClick={() => orbitActions.selectAgent(host.host_id, agent.agent_id)}
                  className={cn(
                    "flex w-full cursor-pointer items-center justify-between gap-2 rounded-[9px] px-2.5 py-1.5 text-left text-[13px] transition-colors",
                    active
                      ? "bg-amber-500/18 text-foreground font-medium"
                      : "text-foreground/90 hover:bg-amber-500/10",
                  )}
                >
                  <span className="truncate">{agent.title}</span>
                  {!singleHostMode && (
                    <span className="text-muted-foreground shrink-0 font-mono text-[10.5px]">
                      {host.display_name}
                    </span>
                  )}
                </button>
              );
            })}
          </div>
        </div>
      )}

      {/* Main Unified Tree: Host -> Workspace -> Session */}
      <div className="flex-1 space-y-4 overflow-y-auto px-3 py-1.5">
        {fleet.hosts.length === 0 ? (
          <div className="text-muted-foreground space-y-2.5 rounded-xl border border-dashed p-4 text-center text-[13px]">
            <p>No host connected</p>
            <button
              type="button"
              onClick={() => orbitActions.openSettings("hosts")}
              className="bg-foreground text-background inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[13px] font-medium"
            >
              <Plus className="size-4" /> Connect Host
            </button>
          </div>
        ) : (
          fleet.hosts.map((host) => {
            const detail = hostDetails[host.host_id];
            const collapsed = Boolean(collapsedHostIds[host.host_id]);
            const wsMap = new Map<string, boolean>();
            detail?.workspaces.forEach((w) => wsMap.set(w.cwd, w.is_worktree));
            const groups = groupAgentsByWorkspace(detail?.agents ?? [], wsMap);
            const isHostSelected = host.host_id === selectedHostId;

            return (
              <div key={host.host_id} className="space-y-1.5">
                {!singleHostMode && (
                  <div
                    className={cn(
                      "group flex h-7.5 items-center justify-between rounded-lg px-2.5 text-[12.5px]",
                      isHostSelected ? "text-foreground" : "text-muted-foreground",
                    )}
                  >
                    <button
                      type="button"
                      onClick={() => {
                        orbitActions.selectHost(host.host_id);
                        orbitActions.toggleHostCollapsed(host.host_id);
                      }}
                      className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
                    >
                      {collapsed ? (
                        <ChevronRight className="size-3.5 shrink-0 opacity-60" />
                      ) : (
                        <ChevronDown className="size-3.5 shrink-0 opacity-60" />
                      )}
                      <HostStateDot state={host.connection_state} />
                      <span className="truncate font-medium">{host.display_name}</span>
                    </button>

                    <div className="flex items-center gap-0.5 opacity-0 transition-opacity group-hover:opacity-100">
                      <button
                        type="button"
                        title="Reconnect host"
                        onClick={() => {
                          void orbitIpc.connectHost(host.host_id);
                          void orbitIpc.refreshHost(host.host_id);
                        }}
                        className="hover:bg-accent inline-flex size-6 cursor-pointer items-center justify-center rounded-md"
                      >
                        <RefreshCw className="size-3" />
                      </button>
                      <button
                        type="button"
                        title={`New session on ${host.display_name}`}
                        onClick={() => orbitActions.startNewThreadDraft(host.host_id)}
                        className="hover:bg-accent inline-flex size-6 cursor-pointer items-center justify-center rounded-md"
                      >
                        <Plus className="size-3.5" />
                      </button>
                    </div>
                  </div>
                )}

                {!collapsed && (
                  <div className="space-y-3">
                    {groups.length === 0 ? (
                      <div className="text-foreground/45 px-2.5 py-2 text-[13px]">
                        No active sessions
                      </div>
                    ) : (
                      groups.map((grp) => (
                        <div key={grp.cwd} className="space-y-0.5">
                          <div className="group/ws flex items-center justify-between px-2.5 py-0.5">
                            <span
                              title={grp.cwd}
                              className="text-foreground/48 flex min-w-0 items-center gap-1 truncate text-[12px] font-medium"
                            >
                              <span className="truncate">{grp.label}</span>
                              {grp.isWorktree && (
                                <GitBranch className="size-3 shrink-0 opacity-70" />
                              )}
                            </span>
                            <button
                              type="button"
                              title={`New session in ${grp.label}`}
                              onClick={() => orbitActions.startNewThreadDraft(host.host_id, grp.cwd)}
                              className="text-foreground/50 hover:text-foreground inline-flex size-5 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover/ws:opacity-100"
                            >
                              <Plus className="size-3.5" />
                            </button>
                          </div>

                          {grp.agents.map((ag) => {
                            const selected =
                              !isDraftNewSession &&
                              selectedHostId === host.host_id &&
                              selectedAgentId === ag.agent_id;
                            const isChild = Boolean(ag.parent_agent_id);

                            return (
                              <div
                                key={ag.agent_id}
                                onClick={() => orbitActions.selectAgent(host.host_id, ag.agent_id)}
                                className={cn(
                                  "group/item flex h-9 cursor-pointer items-center justify-between gap-2 rounded-[10px] px-3 transition-colors",
                                  isChild && "pl-5.5",
                                  selected
                                    ? "text-foreground bg-black/[0.06] font-medium dark:bg-white/[0.085]"
                                    : "text-foreground/78 hover:text-foreground hover:bg-black/[0.035] dark:hover:bg-white/[0.045]",
                                )}
                              >
                                <div className="flex min-w-0 items-center gap-2.5">
                                  <AgentStatusGlyph agent={ag} />
                                  <span className="truncate text-[13.5px]">{ag.title}</span>
                                </div>

                                <button
                                  type="button"
                                  title="Archive session"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    void orbitIpc.archiveAgent(host.host_id, ag.agent_id);
                                  }}
                                  className="text-foreground/50 hover:text-foreground hidden size-6 shrink-0 cursor-pointer items-center justify-center rounded-md group-hover/item:inline-flex hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
                                >
                                  <Archive className="size-3.5" />
                                </button>
                              </div>
                            );
                          })}
                        </div>
                      ))
                    )}
                  </div>
                )}
              </div>
            );
          })
        )}
      </div>

      {/* Antigravity-Style Sidebar Footer: Clean Borderless Settings Entry */}
      <div className="px-3 pt-1 pb-3">
        <button
          type="button"
          onClick={() => orbitActions.openSettings("general")}
          className="text-foreground/72 hover:text-foreground flex h-9 w-full cursor-pointer items-center gap-2.5 rounded-[10px] px-3 text-[13.5px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.055]"
        >
          <Settings className="size-4 stroke-[1.75] opacity-80" />
          <span>Settings</span>
        </button>
      </div>
    </aside>
  );
}
