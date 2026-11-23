import { memo, useMemo, useState } from "react";
import {
  AlertTriangle,
  Archive,
  ArrowLeft,
  ArrowUpRight,
  Check,
  ChevronRight,
  ChevronUp,
  HeartPulse,
  Layers,
  Server,
  Sparkles,
  Unlink,
  X,
} from "lucide-react";
import {
  type AgentCardView,
  type SessionSubagentEntryView,
  type TimelineItemView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  extractCleanAgentMessageBody,
  formatCleanAgentMessagePreview,
} from "@/lib/agent-message-clean";
import { filterVisibleSubagents, resolveSessionSubagents } from "@/lib/subagents";

export { extractCleanAgentMessageBody };
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_STAGE_SUBAGENTS: readonly SessionSubagentEntryView[] = Object.freeze([]);

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
    <div className="mt-2 mb-1 w-full px-[24px]">
      <div className="border-border bg-elevated/75 mx-auto flex w-full max-w-[var(--orbit-stage-max-w,768px)] items-center justify-between gap-2 rounded-xl border px-3 py-1.5 text-[12.5px]">
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
    <div className="mb-1.5 w-full px-[24px]">
      <div className="mx-auto flex w-full max-w-[var(--orbit-stage-max-w,768px)] items-center justify-between gap-3 rounded-[12px] border border-amber-500/25 bg-amber-500/[0.06] px-3 py-1.5 text-[12.5px] dark:border-amber-400/25 dark:bg-amber-400/[0.07]">
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
 * Compact docked Subagents pill above the composer (matching iOS `SubagentsTrackPill`).
 * Single unified hub for viewing, inspecting, jumping to, detaching, and archiving
 * both Orbit managed subagents and Provider native subagents.
 */
export const SubagentsTrack = memo(function SubagentsTrack({
  hostId,
  agentId,
  variant = "inline",
}: {
  hostId: string | null;
  agentId: string | null;
  variant?: "inline" | "floating";
}) {
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const fleetHosts = useOrbitSelector((s) => s.fleet.hosts);
  const archivedSubagents = useOrbitSelector((s) => s.archivedSubagents);
  const insightsSubagents = useOrbitSelector((s) =>
    hostId && agentId
      ? (s.stages[`${hostId}::${agentId}`]?.insights.subagents ?? EMPTY_STAGE_SUBAGENTS)
      : EMPTY_STAGE_SUBAGENTS,
  );
  const [open, setOpen] = useState(false);

  const orderedSubagents = useMemo(() => {
    if (!hostId || !agentId) return [];
    const resolved = resolveSessionSubagents({
      hostId,
      agentId,
      insightsSubagents,
      hostDetails,
      fleetHosts,
    });
    return filterVisibleSubagents(resolved, archivedSubagents, hostId, agentId);
  }, [hostId, agentId, insightsSubagents, hostDetails, fleetHosts, archivedSubagents]);

  const totalCount = orderedSubagents.length;
  if (!hostId || !agentId || totalCount === 0) return null;

  const awaitingPermCount = orderedSubagents.filter(
    (s) => s.status === "awaiting_permission",
  ).length;
  const runningCount = orderedSubagents.filter((s) => s.status === "running").length;
  const finishedSubagents = orderedSubagents.filter(
    (s) => s.status !== "running" && s.status !== "awaiting_permission",
  );

  const isFloating = variant === "floating";

  return (
    <div className={cn("w-full px-[24px]", isFloating ? "pointer-events-none" : "mb-1.5")}>
      <div className="mx-auto flex w-full max-w-[var(--orbit-stage-max-w,768px)] justify-start">
        <Popover open={open} onOpenChange={setOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              className={cn(
                "border-border/80 bg-elevated/90 hover:bg-accent/70 inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-full border px-2.5 text-[12px] shadow-xs backdrop-blur-sm transition-colors",
                isFloating && "pointer-events-auto",
                awaitingPermCount > 0 && "border-amber-500/40 bg-amber-500/[0.07]",
              )}
            >
              {awaitingPermCount > 0 ? (
                <span className="size-[6.5px] shrink-0 rounded-full bg-amber-500" />
              ) : runningCount > 0 ? (
                <OrbitSpinner size={12} className="text-foreground/75" />
              ) : (
                <Layers className="text-muted-foreground size-[12.5px] shrink-0" />
              )}

              <span className="text-foreground/90 font-medium">
                Subagents · {totalCount}
              </span>

              {awaitingPermCount > 0 ? (
                <span className="text-[11px] font-medium text-amber-600 dark:text-amber-400">
                  {awaitingPermCount} waiting
                </span>
              ) : runningCount > 0 ? (
                <span className="text-muted-foreground text-[11px]">
                  {runningCount} running
                </span>
              ) : null}

              <ChevronUp
                className={cn(
                  "text-muted-foreground/70 size-[12px] transition-transform",
                  open && "rotate-180",
                )}
              />
            </button>
          </PopoverTrigger>

          <PopoverContent
            align="start"
            side="top"
            sideOffset={6}
            className="border-border bg-popover w-[360px] max-w-[calc(100vw-32px)] overflow-hidden rounded-xl border p-0 shadow-lg"
          >
            <div className="flex items-center justify-between px-3 pt-2.5 pb-1.5">
              <span className="text-muted-foreground text-[11px] font-medium tracking-wide uppercase">
                Subagents ({totalCount})
              </span>
              {finishedSubagents.length > 0 && (
                <button
                  type="button"
                  onClick={() => {
                    const managed = finishedSubagents.filter(
                      (item) => item.isManaged && item.childAgentId,
                    );
                    for (const item of managed) {
                      if (item.childAgentId) {
                        orbitActions.archiveThread(item.childHostId, item.childAgentId);
                      }
                    }
                    const nativeItems = finishedSubagents.filter(
                      (item) => !(item.isManaged && item.childAgentId),
                    );
                    if (nativeItems.length > 0) {
                      orbitActions.archiveFinishedSubagents(
                        hostId,
                        agentId,
                        nativeItems.map((item) => ({
                          id: item.callId,
                          providerSubagentId: item.providerSubagentId,
                          childAgentId: item.childAgentId,
                        })),
                      );
                    }
                  }}
                  className="text-muted-foreground hover:text-foreground cursor-pointer text-[11px] transition-colors"
                >
                  Archive finished
                </button>
              )}
            </div>

            <div className="orbit-native-scroll max-h-[304px] space-y-0.5 overflow-y-auto overscroll-contain px-1.5 pt-0.5 pb-1.5">
              {orderedSubagents.map((sa) => {
                const isAwaiting = sa.status === "awaiting_permission";
                const isRunning = sa.status === "running";
                const isFailed = sa.status === "failed";
                const isCanceled = sa.status === "canceled";
                const canManage = Boolean(sa.isManaged && sa.childAgentId);

                return (
                  <div
                    key={sa.key}
                    className="hover:bg-accent/60 flex items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-[12.5px] transition-colors"
                  >
                    <button
                      type="button"
                      title={sa.promptPreview || sa.title}
                      onClick={() => {
                        setOpen(false);
                        if (isAwaiting && canManage && sa.childAgentId) {
                          orbitActions.selectAgent(sa.childHostId, sa.childAgentId);
                          return;
                        }
                        orbitActions.openSubagentTab({
                          callId: sa.callId,
                          role: sa.title,
                          promptPreview: sa.promptPreview,
                          childAgentId: sa.childAgentId,
                          childHostId: sa.childHostId,
                          providerSubagentId: sa.providerSubagentId,
                          parentAgentId: agentId,
                        });
                      }}
                      className="flex min-w-0 flex-1 cursor-pointer items-center gap-2 text-left"
                    >
                      {isAwaiting ? (
                        <span className="size-[6.5px] shrink-0 rounded-full bg-amber-500 shadow-[0_0_0_2px_rgba(245,158,11,0.18)]" />
                      ) : isRunning ? (
                        <OrbitSpinner size={12} className="text-foreground/75 shrink-0" />
                      ) : isFailed ? (
                        <AlertTriangle className="text-rose-500 size-[12px] shrink-0" />
                      ) : isCanceled ? (
                        <span className="bg-muted-foreground/45 size-[6.5px] shrink-0 rounded-full" />
                      ) : (
                        <Check className="text-emerald-500 size-[12px] shrink-0" />
                      )}
                      <div className="min-w-0 flex-1">
                        <div className="text-foreground truncate font-medium">{sa.title}</div>
                        <div className="text-muted-foreground flex items-center gap-1.5 truncate text-[11px]">
                          <span>
                            {isAwaiting
                              ? "Approval required"
                              : isRunning
                                ? "Running"
                                : isFailed
                                  ? "Failed"
                                  : isCanceled
                                    ? "Canceled"
                                    : "Completed"}
                          </span>
                          {sa.remoteHostName && <span>· {sa.remoteHostName}</span>}
                        </div>
                      </div>
                    </button>

                    <div className="flex shrink-0 items-center gap-0.5">
                      {canManage && sa.childAgentId && (
                        <>
                          <button
                            type="button"
                            title="Open thread"
                            onClick={() => {
                              setOpen(false);
                              orbitActions.selectAgent(sa.childHostId, sa.childAgentId!);
                            }}
                            className="text-muted-foreground hover:text-foreground hover:bg-accent cursor-pointer rounded p-1 transition-colors"
                          >
                            <ArrowUpRight className="size-[13px]" />
                          </button>
                          <button
                            type="button"
                            title="Detach to thread"
                            onClick={() => {
                              setOpen(false);
                              void orbitActions.detachAgent(sa.childHostId, sa.childAgentId!);
                            }}
                            className="text-muted-foreground hover:text-foreground hover:bg-accent cursor-pointer rounded p-1 transition-colors"
                          >
                            <Unlink className="size-[12.5px]" />
                          </button>
                        </>
                      )}
                      {!isRunning && !isAwaiting && (
                        <button
                          type="button"
                          title="Archive subagent"
                          onClick={() => {
                            if (canManage && sa.childAgentId) {
                              orbitActions.archiveThread(sa.childHostId, sa.childAgentId);
                            } else {
                              orbitActions.archiveSubagent(hostId, agentId, {
                                id: sa.callId,
                                providerSubagentId: sa.providerSubagentId,
                                childAgentId: sa.childAgentId,
                                role: sa.title,
                              });
                            }
                          }}
                          className="text-muted-foreground hover:text-foreground hover:bg-accent cursor-pointer rounded p-1 transition-colors"
                        >
                          <Archive className="size-[12.5px]" />
                        </button>
                      )}
                    </div>
                  </div>
                );
              })}
            </div>
          </PopoverContent>
        </Popover>
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
  agentId = null,
  item,
  inline = false,
}: {
  hostId: string;
  agentId?: string | null;
  item: Extract<TimelineItemView, { item_type: "agent_message" }>;
  inline?: boolean;
}) {
  const [expanded, setExpanded] = useState(false);
  const [resolvedLocal, setResolvedLocal] = useState<string | null>(null);

  const cleanBody = useMemo(() => extractCleanAgentMessageBody(item.text), [item.text]);
  const cleanPreview = useMemo(() => formatCleanAgentMessagePreview(cleanBody), [cleanBody]);

  // Heartbeat renders as a left-aligned collapsible activity row matching the stage timeline axis.
  if (item.kind === "heartbeat") {
    const trimmedBody = cleanBody || item.text.trim();
    const canExpand = trimmedBody.length > 0;
    const canRenderRawBlocks = item.blocks.length > 0 && trimmedBody === item.text.trim();
    return (
      <div className={cn("min-w-0", !inline && "px-[10px]")}>
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
            {canRenderRawBlocks ? (
              <MarkdownBlocksView blocks={item.blocks} compact />
            ) : (
              <p className="selectable-text text-muted-foreground whitespace-pre-wrap break-words leading-[1.58]">
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
  const hasBody = cleanBody.length > 0;
  const canRenderRawBlocks = item.blocks.length > 0 && cleanBody === item.text.trim();

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

  if (isPerm && perm) {
    return (
      <div className={cn("min-w-0", !inline && "px-[10px]")}>
        <div className="rounded-xl border border-amber-500/30 bg-amber-500/[0.05] px-3.5 py-2.5 text-[12.5px] dark:border-amber-400/28 dark:bg-amber-400/[0.06]">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <AlertTriangle className="size-[14px] shrink-0 text-amber-500" />
              <span className="text-foreground truncate font-medium">
                Approval requested by {senderLabel}
              </span>
            </div>

            {item.source_agent_id && (
              <button
                type="button"
                onClick={() =>
                  orbitActions.selectAgent(
                    perm.child_host_id || sourceHostId,
                    perm.child_agent_id || item.source_agent_id!,
                  )
                }
                className="text-muted-foreground hover:text-foreground inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-0.5 rounded-[6px] px-1.5 text-[11.5px] font-medium transition-colors hover:bg-black/[0.05] dark:hover:bg-white/[0.08]"
              >
                <span>Open</span>
                <ArrowUpRight className="size-3 shrink-0 opacity-75" />
              </button>
            )}
          </div>

          <div className="mt-2 flex flex-wrap items-center justify-between gap-2 rounded-lg bg-black/[0.03] px-2.5 py-1.5 dark:bg-white/[0.04]">
            <div className="text-foreground/90 min-w-0 text-[12.5px] font-medium">
              {perm.title}
            </div>
            {resolvedLocal ? (
              <span className="text-muted-foreground inline-flex items-center gap-1 text-[11.5px] font-medium">
                <Check className="size-3" />
                {resolvedLocal}
              </span>
            ) : (
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => void handleResolveChildPerm("allow")}
                  className="inline-flex cursor-pointer items-center gap-1 rounded-md bg-emerald-600 px-2.5 py-1 text-[11.5px] font-medium text-white transition-colors hover:bg-emerald-500"
                >
                  <Check className="size-3" />
                  Allow
                </button>
                <button
                  type="button"
                  onClick={() => void handleResolveChildPerm("deny")}
                  className="border-border hover:bg-accent text-foreground inline-flex cursor-pointer items-center gap-1 rounded-md border px-2.5 py-1 text-[11.5px] font-medium transition-colors"
                >
                  <X className="size-3" />
                  Deny
                </button>
              </div>
            )}
          </div>

          {hasBody && (
            <div className="border-border mt-2 ml-[5px] border-l pl-3 text-[12.5px]">
              {canRenderRawBlocks ? (
                <MarkdownBlocksView blocks={item.blocks} compact />
              ) : (
                <p className="selectable-text text-foreground/80 whitespace-pre-wrap break-words leading-[1.58]">
                  {cleanBody}
                </p>
              )}
            </div>
          )}
        </div>
      </div>
    );
  }

  const normalizedEvent = item.event?.trim().toLowerCase() ?? "";
  const isFailedFinish =
    isFinish && (normalizedEvent === "failed" || normalizedEvent === "error" || /^error:/i.test(cleanBody));
  const isCanceledFinish =
    isFinish && (normalizedEvent === "canceled" || normalizedEvent === "cancelled" || normalizedEvent === "stopped");
  const finishStatusLabel = isFailedFinish
    ? "failed"
    : isCanceledFinish
      ? "canceled"
      : "finished";

  return (
    <div className={cn("min-w-0", !inline && "px-[10px]")}>
      <div
        className={cn(
          "overflow-hidden rounded-xl border transition-colors",
          isFailedFinish
            ? "border-rose-500/20 bg-rose-500/[0.03] dark:border-rose-400/20 dark:bg-rose-400/[0.04]"
            : "border-black/[0.065] bg-black/[0.018] dark:border-white/[0.08] dark:bg-white/[0.025]",
        )}
      >
        <div className="flex h-[32px] min-w-0 items-center gap-1 pr-1.5">
          <button
            type="button"
            disabled={!hasBody}
            aria-expanded={hasBody ? expanded : undefined}
            title={cleanPreview || senderLabel}
            onClick={() => {
              if (!hasBody) return;
              setExpanded((v) => !v);
            }}
            className={cn(
              "flex h-full min-w-0 flex-1 items-center gap-2 pl-2.5 pr-1 text-left text-[12.5px] transition-colors",
              hasBody
                ? "cursor-pointer hover:bg-black/[0.02] dark:hover:bg-white/[0.025]"
                : "cursor-default",
            )}
          >
            <span className="flex size-3.5 shrink-0 items-center justify-center">
              {isFinish ? (
                isFailedFinish ? (
                  <AlertTriangle className="size-3.5 text-rose-500/85" />
                ) : isCanceledFinish ? (
                  <span className="bg-muted-foreground/45 size-[6px] rounded-full" />
                ) : (
                  <Check className="size-3.5 stroke-[2.1] text-emerald-600/85 dark:text-emerald-400/85" />
                )
              ) : (
                <Sparkles className="text-muted-foreground/75 size-3.5 stroke-[1.75]" />
              )}
            </span>

            <span className="text-foreground/88 max-w-[46%] shrink-0 truncate font-medium">
              {senderLabel}
            </span>

            {isFinish ? (
              <span
                className={cn(
                  "shrink-0 text-[12px]",
                  isFailedFinish
                    ? "text-rose-600/85 dark:text-rose-400/85"
                    : "text-muted-foreground/65",
                )}
              >
                {finishStatusLabel}
              </span>
            ) : (
              !cleanPreview && (
                <span className="text-muted-foreground/65 shrink-0 text-[12px]">message</span>
              )
            )}

            {!expanded && cleanPreview.length > 0 && (
              <>
                <span
                  aria-hidden="true"
                  className="text-muted-foreground/45 shrink-0 text-[12px]"
                >
                  ·
                </span>
                <span className="text-muted-foreground/75 min-w-0 flex-1 truncate text-[12px]">
                  {cleanPreview}
                </span>
              </>
            )}
          </button>

          <div className="flex shrink-0 items-center gap-0.5">
            {item.source_agent_id && (
              <button
                type="button"
                title={`Open ${senderLabel} in Side Panel`}
                onClick={(e) => {
                  e.stopPropagation();
                  orbitActions.openSubagentTab({
                    callId: item.message_id || `am_${item.seq}`,
                    role: senderLabel,
                    promptPreview: cleanPreview,
                    childAgentId: item.source_agent_id ?? null,
                    childHostId: sourceHostId,
                    parentAgentId: agentId ?? null,
                  });
                }}
                className="text-muted-foreground/80 hover:text-foreground inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-0.5 rounded-[6px] px-1.5 text-[11.5px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.07]"
              >
                <span>Open</span>
                <ArrowUpRight className="size-3 shrink-0 opacity-75" />
              </button>
            )}

            {hasBody && (
              <button
                type="button"
                aria-label={expanded ? "Collapse message" : "Expand message"}
                aria-expanded={expanded}
                onClick={() => setExpanded((v) => !v)}
                className="text-muted-foreground/60 hover:text-foreground inline-flex size-[22px] shrink-0 cursor-pointer items-center justify-center rounded-[6px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.07]"
              >
                <ChevronRight
                  className={cn(
                    "size-3.5 transition-transform duration-150",
                    expanded && "rotate-90",
                  )}
                />
              </button>
            )}
          </div>
        </div>

        {expanded && hasBody && (
          <div className="orbit-stage-enter border-t border-black/[0.05] px-3 py-2.5 dark:border-white/[0.06]">
            <div className="border-border ml-[5px] border-l pl-3 text-[12.5px]">
              {canRenderRawBlocks ? (
                <MarkdownBlocksView blocks={item.blocks} compact />
              ) : (
                <p className="selectable-text text-foreground/80 whitespace-pre-wrap break-words leading-[1.58]">
                  {cleanBody}
                </p>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
});
