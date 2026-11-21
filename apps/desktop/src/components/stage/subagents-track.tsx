import { memo, useMemo, useState } from "react";
import {
  AlertTriangle,
  ArrowLeft,
  ArrowUpRight,
  Check,
  ChevronRight,
  HeartPulse,
  Server,
  Sparkles,
  Unlink,
  X,
} from "lucide-react";
import { type AgentCardView, type TimelineItemView, orbitIpc } from "@/lib/orbit-ipc";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
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
 * Minimal fallback banner displayed above the composer ONLY when a managed subagent is
 * waiting for user approval AND the Overview panel is currently hidden.
 * All routine subagent listing and management lives in the right Overview panel.
 */
export const SubagentApprovalBanner = memo(function SubagentApprovalBanner({
  hostId,
  agentId,
}: {
  hostId: string | null;
  agentId: string | null;
}) {
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleetHosts = useOrbitSelector((s) => s.fleet.hosts);
  const overviewVisible = useOrbitSelector((s) =>
    s.stageCanDockOverview ? s.overviewDesktopOpenPref : s.overviewPopoverOpen,
  );

  const awaitingSubagents = useMemo(() => {
    if (!hostId || !agentId || overviewVisible) return [];
    const out: { hostId: string; hostName: string | null; agent: AgentCardView }[] = [];
    for (const [hid, detail] of Object.entries(hostDetails)) {
      const hostName =
        hid !== hostId
          ? (fleetHosts.find((h) => h.host_id === hid)?.display_name ??
            detail.host.display_name ??
            hid)
          : null;
      for (const ag of detail.agents) {
        if (ag.archived_at_rfc3339 || !ag.is_awaiting_permission) continue;
        const matchesParent =
          ag.parent_agent_id === agentId &&
          (!ag.parent_host_id || ag.parent_host_id === hostId);
        if (matchesParent) {
          out.push({ hostId: hid, hostName, agent: ag });
        }
      }
    }
    return out;
  }, [hostId, agentId, overviewVisible, hostDetails, fleetHosts]);

  if (awaitingSubagents.length === 0) return null;

  const primary = awaitingSubagents[0];
  const extraCount = awaitingSubagents.length - 1;

  return (
    <div className="mx-auto mb-1.5 w-full max-w-[780px] px-4">
      <div className="flex items-center justify-between gap-3 rounded-[12px] border border-amber-500/25 bg-amber-500/[0.06] px-3 py-1.5 text-[12.5px] dark:border-amber-400/25 dark:bg-amber-400/[0.07]">
        <div className="flex min-w-0 items-center gap-2">
          <span className="size-[6.5px] shrink-0 rounded-full bg-amber-500 shadow-[0_0_0_2.5px_rgba(245,158,11,0.18)]" />
          <span className="text-foreground/90 truncate">
            {extraCount > 0
              ? `${awaitingSubagents.length} subagents awaiting approval`
              : `${primary.agent.title || "Subagent"} needs approval`}
          </span>
          {extraCount === 0 && primary.hostName && (
            <span className="text-muted-foreground/75 shrink-0 text-[11px]">
              {primary.hostName}
            </span>
          )}
        </div>

        <button
          type="button"
          onClick={() => orbitActions.selectAgent(primary.hostId, primary.agent.agent_id)}
          className="text-amber-700 hover:bg-amber-500/12 dark:text-amber-300 inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1 rounded-[7px] px-2 text-[12px] font-medium transition-colors"
        >
          <span>Review</span>
          <ArrowUpRight className="size-[12.5px]" />
        </button>
      </div>
    </div>
  );
});

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
