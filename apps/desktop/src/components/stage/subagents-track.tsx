import { memo, useEffect, useMemo, useState } from "react";
import {
  AlertTriangle,
  Archive,
  ArrowLeft,
  ArrowUpRight,
  Bot,
  Check,
  ChevronDown,
  ChevronRight,
  HeartPulse,
  Layers,
  Loader2,
  Server,
  ShieldAlert,
  Sparkles,
  Unlink,
  X,
} from "lucide-react";
import {
  type AgentCardView,
  type ProviderSubagentDescriptor,
  type ProviderSubagentEvent,
  type TimelineItemView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/overlays";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

/**
 * Top banner displayed when the user opens a managed subagent thread:
 * lets them jump back to the parent thread (even across hosts!) or detach into a standalone thread.
 */
export const ParentSubagentBanner = memo(function ParentSubagentBanner({
  hostId,
  agent,
}: {
  hostId: string;
  agent: AgentCardView | null;
}) {
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const hosts = useOrbitSelector((s) => s.fleet.hosts);

  if (!agent?.parent_agent_id) return null;

  const parentHostId = agent.parent_host_id ?? hostId;
  const parentAgent =
    hostDetails[parentHostId]?.agents.find((a) => a.agent_id === agent.parent_agent_id) ?? null;
  const parentHostName =
    parentHostId !== hostId
      ? (hosts.find((h) => h.host_id === parentHostId)?.display_name ?? parentHostId)
      : null;

  return (
    <div className="border-border bg-elevated/75 mx-auto mt-2 mb-1 flex w-full max-w-[780px] items-center justify-between gap-2 rounded-xl border px-3 py-1.5 text-[12.5px]">
      <div className="flex min-w-0 items-center gap-2">
        <button
          type="button"
          onClick={() => {
            if (agent.parent_agent_id) {
              orbitActions.selectAgent(parentHostId, agent.parent_agent_id);
            }
          }}
          className="text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1.5 rounded-md px-2 py-0.5 font-medium transition-colors"
        >
          <ArrowLeft className="h-[13.5px] w-[13.5px]" />
          <span className="truncate">
            Parent: {parentAgent?.title ?? agent.parent_agent_id}
          </span>
        </button>
        {parentHostName && (
          <span className="bg-muted text-muted-foreground inline-flex items-center gap-1 rounded px-1.5 py-0.5 text-[11px]">
            <Server className="h-[10.5px] w-[10.5px]" />
            {parentHostName}
          </span>
        )}
      </div>

      <button
        type="button"
        onClick={() => void orbitActions.detachAgent(hostId, agent.agent_id)}
        className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex shrink-0 cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[12px] transition-colors"
      >
        <Unlink className="h-[12.5px] w-[12.5px]" />
        <span>Detach</span>
      </button>
    </div>
  );
});

/**
 * Floating Subagents Track pill + popover combining:
 * 1. Orbit Managed Subagents (same-host & cross-host children pointing to this parent)
 * 2. Provider Native Subagents (Claude Code Tasks, Codex collab agents, Antigravity invoke_subagent)
 */
export const SubagentsTrack = memo(function SubagentsTrack({
  hostId,
  agentId,
}: {
  hostId: string | null;
  agentId: string | null;
}) {
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleetHosts = useOrbitSelector((s) => s.fleet.hosts);
  const [open, setOpen] = useState(false);
  const [inspectedProviderSub, setInspectedProviderSub] =
    useState<ProviderSubagentDescriptor | null>(null);
  const [providerEvents, setProviderEvents] = useState<ProviderSubagentEvent[]>([]);

  // Collect all Orbit managed children across ALL hosts whose parent points to `(hostId, agentId)`
  const managedSubagents = useMemo(() => {
    if (!hostId || !agentId) return [];
    const out: { hostId: string; hostName: string; agent: AgentCardView }[] = [];
    for (const [hid, detail] of Object.entries(hostDetails)) {
      const hostName =
        fleetHosts.find((h) => h.host_id === hid)?.display_name ?? detail.host.display_name;
      for (const ag of detail.agents) {
        if (ag.archived_at_rfc3339) continue;
        const matchesParent =
          ag.parent_agent_id === agentId &&
          (!ag.parent_host_id || ag.parent_host_id === hostId);
        if (matchesParent) {
          out.push({ hostId: hid, hostName, agent: ag });
        }
      }
    }
    return out;
  }, [hostId, agentId, hostDetails, fleetHosts]);

  const providerSubagents: ProviderSubagentDescriptor[] = useMemo(() => {
    if (!hostId || !agentId) return [];
    return hostDetails[hostId]?.provider_subagents?.[agentId] ?? [];
  }, [hostId, agentId, hostDetails]);

  useEffect(() => {
    if (!hostId || !agentId || !inspectedProviderSub) {
      setProviderEvents([]);
      return;
    }
    let cancelled = false;
    void orbitIpc.openProviderSubagentTimeline(hostId, agentId, inspectedProviderSub.subagent_id);
    const poll = async () => {
      const evs = await orbitIpc.getProviderSubagentTimeline(
        hostId,
        agentId,
        inspectedProviderSub.subagent_id,
      );
      if (!cancelled) setProviderEvents(evs);
    };
    void poll();
    const timer = window.setInterval(() => void poll(), 1500);
    return () => {
      cancelled = true;
      window.clearInterval(timer);
    };
  }, [hostId, agentId, inspectedProviderSub]);

  const totalCount = managedSubagents.length + providerSubagents.length;
  if (!hostId || !agentId || totalCount === 0) return null;

  const runningCount =
    managedSubagents.filter((m) => m.agent.is_running).length +
    providerSubagents.filter((p) => p.status === "running").length;
  const awaitingPermCount = managedSubagents.filter(
    (m) => m.agent.is_awaiting_permission,
  ).length;
  const finishedManaged = managedSubagents.filter(
    (m) => !m.agent.is_running && !m.agent.is_awaiting_permission,
  );

  return (
    <div className="mx-auto mb-1.5 flex w-full max-w-[780px] justify-end px-4">
      <Popover open={open} onOpenChange={setOpen}>
        <PopoverTrigger asChild>
          <button
            type="button"
            className={cn(
              "border-border bg-elevated hover:bg-accent inline-flex h-[30px] cursor-pointer items-center gap-2 rounded-full border px-3 text-[12px] font-medium shadow-xs transition-colors",
              awaitingPermCount > 0 && "border-amber-500/40 bg-amber-500/10 text-amber-700 dark:text-amber-300",
            )}
          >
            {runningCount > 0 ? (
              <Loader2 className="h-[13px] w-[13px] animate-spin text-blue-500" />
            ) : awaitingPermCount > 0 ? (
              <ShieldAlert className="h-[13px] w-[13px] text-amber-500" />
            ) : (
              <Layers className="text-muted-foreground h-[13px] w-[13px]" />
            )}
            <span>
              Subagents · {totalCount}
              {runningCount > 0 ? ` (${runningCount} running)` : ""}
              {awaitingPermCount > 0 ? ` · ${awaitingPermCount} approval` : ""}
            </span>
            <ChevronDown className="text-muted-foreground h-[12.5px] w-[12.5px]" />
          </button>
        </PopoverTrigger>

        <PopoverContent
          align="end"
          side="top"
          sideOffset={8}
          className="w-[420px] max-w-[92vw] p-0"
        >
          <div className="border-border flex items-center justify-between border-b px-3.5 py-2.5">
            <div className="flex items-center gap-2 text-[13px] font-medium">
              <Layers className="text-muted-foreground h-[14px] w-[14px]" />
              <span>Subagents ({totalCount})</span>
            </div>
            {finishedManaged.length > 0 && (
              <button
                type="button"
                onClick={() => {
                  for (const item of finishedManaged) {
                    orbitActions.archiveThread(item.hostId, item.agent.agent_id);
                  }
                }}
                className="text-muted-foreground hover:text-foreground hover:bg-accent cursor-pointer rounded-md px-2 py-0.5 text-[11.5px] transition-colors"
              >
                Archive finished ({finishedManaged.length})
              </button>
            )}
          </div>

          <ScrollArea className="max-h-[360px]" viewportClassName="p-2 space-y-3">
            {managedSubagents.length > 0 && (
              <div className="space-y-1">
                <div className="text-muted-foreground px-2 text-[11px] font-medium tracking-wide uppercase">
                  Orbit Managed Subagents
                </div>
                {managedSubagents.map(({ hostId: childHostId, hostName, agent }) => (
                  <div
                    key={`${childHostId}:${agent.agent_id}`}
                    className="hover:bg-accent/60 group flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 transition-colors"
                  >
                    <button
                      type="button"
                      onClick={() => {
                        setOpen(false);
                        orbitActions.selectAgent(childHostId, agent.agent_id);
                      }}
                      className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
                    >
                      <ProviderIcon provider={agent.provider} className="size-3.5 shrink-0" />
                      <div className="min-w-0 flex-1">
                        <div className="flex items-center gap-1.5">
                          <span className="text-foreground truncate text-[12.5px] font-medium">
                            {agent.title}
                          </span>
                          {childHostId !== hostId && (
                            <span className="bg-muted text-muted-foreground rounded px-1 py-0.5 text-[10px]">
                              {hostName}
                            </span>
                          )}
                        </div>
                        <div className="text-muted-foreground flex items-center gap-1.5 text-[11px]">
                          <span
                            className={cn(
                              "inline-block h-1.5 w-1.5 rounded-full",
                              agent.is_awaiting_permission
                                ? "bg-amber-500"
                                : agent.is_running
                                  ? "bg-blue-500"
                                  : "bg-emerald-500",
                            )}
                          />
                          <span>{agent.status_label}</span>
                        </div>
                      </div>
                    </button>

                    <div className="flex shrink-0 items-center gap-1">
                      <button
                        type="button"
                        title="Detach to standalone thread"
                        onClick={() => void orbitActions.detachAgent(childHostId, agent.agent_id)}
                        className="text-muted-foreground hover:text-foreground hover:bg-muted cursor-pointer rounded p-1"
                      >
                        <Unlink className="h-[13px] w-[13px]" />
                      </button>
                      <button
                        type="button"
                        title="Archive subagent"
                        onClick={() => orbitActions.archiveThread(childHostId, agent.agent_id)}
                        className="text-muted-foreground hover:text-foreground hover:bg-muted cursor-pointer rounded p-1"
                      >
                        <Archive className="h-[13px] w-[13px]" />
                      </button>
                    </div>
                  </div>
                ))}
              </div>
            )}

            {providerSubagents.length > 0 && (
              <div className="space-y-1">
                <div className="text-muted-foreground px-2 text-[11px] font-medium tracking-wide uppercase">
                  Provider Native Subagents (Read-Only)
                </div>
                {providerSubagents.map((sub) => {
                  const isSelected = inspectedProviderSub?.subagent_id === sub.subagent_id;
                  return (
                    <div key={sub.subagent_id} className="rounded-lg border border-transparent">
                      <div className="hover:bg-accent/60 flex w-full items-center justify-between gap-1 rounded-lg px-2 py-1.5">
                        <button
                          type="button"
                          onClick={() =>
                            setInspectedProviderSub(isSelected ? null : sub)
                          }
                          className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
                        >
                          <Bot className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
                          <div className="min-w-0 flex-1">
                            <div className="text-foreground truncate text-[12.5px] font-medium">
                              {sub.title}
                            </div>
                            <div className="text-muted-foreground flex items-center gap-1.5 truncate text-[11px]">
                              <span
                                className={cn(
                                  "inline-block h-1.5 w-1.5 shrink-0 rounded-full",
                                  sub.status === "running"
                                    ? "bg-blue-500"
                                    : sub.status === "completed"
                                      ? "bg-emerald-500"
                                      : "bg-rose-500",
                                )}
                              />
                              <span className="truncate">
                                {sub.status}
                                {sub.subtitle ? ` · ${sub.subtitle}` : ""}
                              </span>
                            </div>
                          </div>
                          <ChevronRight
                            className={cn(
                              "text-muted-foreground h-[13px] w-[13px] shrink-0 transition-transform",
                              isSelected && "rotate-90",
                            )}
                          />
                        </button>
                        <button
                          type="button"
                          title="Inspect in Side Panel"
                          onClick={() => {
                            setOpen(false);
                            orbitActions.openSubagentTab({
                              callId: `provider:${sub.subagent_id}`,
                              role: sub.title || "Subagent",
                              promptPreview: sub.subtitle || "",
                              childAgentId: null,
                              providerSubagentId: sub.subagent_id,
                              parentAgentId: agentId,
                            });
                          }}
                          className="text-muted-foreground hover:text-foreground hover:bg-muted shrink-0 cursor-pointer rounded p-1"
                        >
                          <ArrowUpRight className="h-[13px] w-[13px]" />
                        </button>
                      </div>

                      {isSelected && (
                        <div className="border-border bg-muted/40 mx-2 mt-1 mb-2 max-h-48 overflow-auto rounded-lg border p-2 font-mono text-[11px]">
                          {providerEvents.length === 0 ? (
                            <div className="text-muted-foreground">Waiting for subagent events…</div>
                          ) : (
                            providerEvents.map((ev) => (
                              <div key={ev.seq} className="border-border/50 border-b py-1 last:border-b-0">
                                <span className="text-muted-foreground mr-1.5">#{ev.seq}</span>
                                <span className="text-foreground break-words">
                                  {formatProviderSubEvent(ev.payload)}
                                </span>
                              </div>
                            ))
                          )}
                        </div>
                      )}
                    </div>
                  );
                })}
              </div>
            )}
          </ScrollArea>
        </PopoverContent>
      </Popover>
    </div>
  );
});

function formatProviderSubEvent(payload: unknown): string {
  if (!payload || typeof payload !== "object") return String(payload ?? "");
  const obj = payload as Record<string, unknown>;
  if (typeof obj.text === "string" && obj.text.trim()) return obj.text.trim().slice(0, 300);
  if (typeof obj.message === "string" && obj.message.trim()) return obj.message.trim().slice(0, 300);
  if (obj.detail && typeof obj.detail === "object") {
    const d = obj.detail as Record<string, unknown>;
    if (typeof d.command === "string") return `$ ${d.command}`;
    if (typeof d.path === "string") return `file: ${d.path}`;
    if (typeof d.tool_name === "string") return `tool: ${d.tool_name}`;
  }
  return JSON.stringify(payload).slice(0, 240);
}

/**
 * Structured timeline card for `agent_message` events (finish reports, inline permission
 * checkpoints with direct Allow/Deny, direct agent-to-agent messages, and heartbeat dividers).
 */
export const AgentMessageCard = memo(function AgentMessageCard({
  hostId,
  item,
}: {
  hostId: string;
  item: Extract<TimelineItemView, { item_type: "agent_message" }>;
}) {
  const [expanded, setExpanded] = useState(
    item.kind !== "heartbeat" && item.text.length <= 600,
  );
  const [resolvedLocal, setResolvedLocal] = useState<string | null>(null);

  // Heartbeat renders as a left-aligned collapsible activity row matching the stage timeline axis.
  if (item.kind === "heartbeat") {
    const trimmedBody = item.text.trim();
    const canExpand = trimmedBody.length > 0;
    return (
      <div className="min-w-0 px-[10px]">
        <button
          type="button"
          disabled={!canExpand}
          aria-expanded={canExpand ? expanded : undefined}
          onClick={() => {
            if (!canExpand) return;
            setExpanded((v) => !v);
          }}
          className={cn(
            "group/hb text-muted-foreground/85 -ml-1.5 inline-flex max-w-full items-center gap-1.5 rounded-md px-1.5 py-0.5 text-left text-[12.5px] font-normal transition-colors",
            canExpand
              ? "hover:text-foreground hover:bg-accent/55 cursor-pointer"
              : "cursor-default",
          )}
        >
          <HeartPulse className="size-3.5 shrink-0 text-rose-500/75" />
          <span className="truncate">{item.source_title || "Heartbeat wakeup"}</span>
          {canExpand && (
            <ChevronRight
              className={cn(
                "size-3 shrink-0 opacity-65 transition-transform duration-150 group-hover/hb:opacity-100",
                expanded && "rotate-90",
              )}
            />
          )}
        </button>

        {expanded && canExpand && (
          <div className="orbit-stage-enter border-border my-1.5 ml-[7px] border-l pl-3 text-[12.5px]">
            {item.blocks.length > 0 ? (
              <MarkdownBlocksView blocks={item.blocks} compact />
            ) : (
              <p className="selectable-text text-muted-foreground whitespace-pre-wrap break-words">
                {trimmedBody}
              </p>
            )}
          </div>
        )}
      </div>
    );
  }

  const perm = item.permission_context ?? null;
  const sourceHostId = item.source_host_id ?? hostId;
  const senderLabel = item.source_title || item.source_agent_id || "Subagent";
  const isPerm = Boolean(perm);
  const isFinish = item.kind === "finish_notification" && !isPerm;

  const handleResolveChildPerm = async (decision: string) => {
    if (!perm) return;
    setResolvedLocal(decision);
    const targetHost = perm.child_host_id || sourceHostId;
    await orbitIpc.resolvePermission(
      targetHost,
      perm.child_agent_id,
      perm.permission_request_id,
      decision,
    );
  };

  return (
    <div
      className={cn(
        "rounded-2xl border px-4 py-3 text-[13px]",
        isPerm
          ? "border-amber-500/35 bg-amber-500/[0.06]"
          : "border-border bg-elevated/65",
      )}
    >
      <div className="flex items-center justify-between gap-2">
        <div className="flex min-w-0 items-center gap-2">
          {isPerm ? (
            <AlertTriangle className="h-[15px] w-[15px] shrink-0 text-amber-500" />
          ) : (
            <Sparkles className="text-muted-foreground h-[14px] w-[14px] shrink-0" />
          )}
          <span className="text-foreground truncate font-medium">
            {isPerm
              ? `Approval requested by ${senderLabel}`
              : isFinish
                ? `Subagent finished: ${senderLabel}`
                : `Message from ${senderLabel}`}
          </span>
          {item.event && (
            <span className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[10.5px]">
              {item.event}
            </span>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1.5">
          {item.source_agent_id && (
            <button
              type="button"
              onClick={() =>
                orbitActions.selectAgent(sourceHostId, item.source_agent_id!)
              }
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex cursor-pointer items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium transition-colors"
            >
              <span>Open</span>
              <ArrowUpRight className="h-[12px] w-[12px]" />
            </button>
          )}
          {item.text.length > 260 && (
            <button
              type="button"
              onClick={() => setExpanded((v) => !v)}
              className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-1.5 py-0.5 text-[11.5px]"
            >
              {expanded ? "Collapse" : "Expand"}
            </button>
          )}
        </div>
      </div>

      {perm && (
        <div className="mt-2.5 flex flex-wrap items-center justify-between gap-2 rounded-xl bg-black/[0.03] px-3 py-2 dark:bg-white/[0.04]">
          <div className="text-foreground/90 min-w-0 text-[12.5px] font-medium">
            {perm.title}
          </div>
          {resolvedLocal ? (
            <span className="text-muted-foreground inline-flex items-center gap-1 text-[11.5px] font-medium">
              <Check className="h-[12.5px] w-[12.5px]" />
              {resolvedLocal}
            </span>
          ) : (
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                onClick={() => void handleResolveChildPerm("allow")}
                className="inline-flex cursor-pointer items-center gap-1 rounded-lg bg-emerald-600 px-2.5 py-1 text-[11.5px] font-medium text-white hover:bg-emerald-500"
              >
                <Check className="h-[12px] w-[12px]" />
                Allow
              </button>
              <button
                type="button"
                onClick={() => void handleResolveChildPerm("deny")}
                className="border-border hover:bg-accent text-foreground inline-flex cursor-pointer items-center gap-1 rounded-lg border px-2.5 py-1 text-[11.5px] font-medium"
              >
                <X className="h-[12px] w-[12px]" />
                Deny
              </button>
            </div>
          )}
        </div>
      )}

      {expanded && item.blocks.length > 0 && (
        <div className="text-foreground/85 mt-2 text-[13px] leading-relaxed">
          <MarkdownBlocksView blocks={item.blocks} />
        </div>
      )}
      {!expanded && (
        <div className="text-muted-foreground mt-1.5 line-clamp-2 text-[12.5px]">
          {item.text}
        </div>
      )}
    </div>
  );
});
