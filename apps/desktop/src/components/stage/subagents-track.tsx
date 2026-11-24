import { memo, useEffect, useMemo, useState } from "react";
import {
  ArrowLeft,
  ArrowUpRight,
  Check,
  ChevronRight,
  HeartPulse,
  Server,
  Unlink,
  X,
} from "lucide-react";
import {
  type AgentCardView,
  type TimelineItemView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  extractCleanAgentMessageBody,
  formatCleanAgentMessagePreview,
} from "@/lib/agent-message-clean";
import { useI18n } from "@/lib/i18n";
import type { ResolvedSubagentItem } from "@/lib/subagents";
import {
  type SubagentCharacterSpec,
  resolveSubagentCharacter,
} from "@/lib/subagent-avatar";
import { formatDurationCompact } from "@/lib/timeline-coalesce";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { ContextBanner } from "@/components/ui/context-banner";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SubagentAvatar } from "@/components/ui/subagent-avatar";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export { extractCleanAgentMessageBody };

type SubagentMaybeWithAvatar = ResolvedSubagentItem & {
  readonly character?: SubagentCharacterSpec;
};

/**
 * Unified inline provenance banner displayed when inspecting a managed subagent thread:
 * lets the user jump back to the parent thread (even across hosts) or detach into a standalone thread.
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
    <div className="mt-2 mb-1 w-full">
      <div className="mx-auto w-full max-w-[var(--orbit-stage-max-w,740px)]">
        <ContextBanner
          variant="inline"
          tone="neutral"
          actions={
            <button
              type="button"
              onClick={() => void orbitActions.detachAgent(hostId, agent.agent_id)}
              className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-1 rounded-[6px] px-2 text-[11px] font-medium transition-colors"
            >
              <Unlink className="size-3" />
              <span>Detach</span>
            </button>
          }
        >
          <div className="flex min-w-0 items-center gap-2">
            <button
              type="button"
              onClick={() => {
                if (agent.parent_agent_id) {
                  orbitActions.selectAgent(parentHostId, agent.parent_agent_id);
                }
              }}
              className="text-ink-primary hover:bg-accent inline-flex cursor-pointer items-center gap-1.5 rounded-[6px] px-1.5 py-0.5 font-medium transition-colors"
            >
              <ArrowLeft className="size-3.5 shrink-0" />
              <span className="truncate">
                Parent: {parentAgent?.title ?? agent.parent_agent_id}
              </span>
            </button>
            {parentHostName && (
              <span className="bg-surface-raised text-ink-tertiary border-border-subtle inline-flex items-center gap-1 rounded-[6px] border px-1.5 py-0.5 text-[11px]">
                <Server className="size-3" />
                {parentHostName}
              </span>
            )}
          </div>
        </ContextBanner>
      </div>
    </div>
  );
});

/**
 * Unified timeline row for `agent_message` events (subagent finish reports, inline permission
 * checkpoints, direct agent-to-agent messages, and heartbeat dividers).
 * Shares the exact same visual language and animal avatar as `DelegationMilestoneRow`.
 */
export const AgentMessageCard = memo(function AgentMessageCard({
  hostId,
  agentId = null,
  item,
  inline = false,
  sessionSubagents,
  isParentTurnRunning = false,
  onInterruptAndContinue = null,
}: {
  hostId: string;
  agentId?: string | null;
  item: Extract<TimelineItemView, { item_type: "agent_message" }>;
  inline?: boolean;
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
  isParentTurnRunning?: boolean;
  onInterruptAndContinue?: (() => void) | null;
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const [resolvedLocal, setResolvedLocal] = useState<string | null>(null);
  const [nowMs, setNowMs] = useState(() => Date.now());

  const deliveryStatus = item.delivery_status ?? null;
  useEffect(() => {
    if (deliveryStatus !== "queued") return;
    setNowMs(Date.now());
    const id = window.setInterval(() => setNowMs(Date.now()), 1000);
    return () => window.clearInterval(id);
  }, [deliveryStatus]);

  const queuedWaitedMs =
    deliveryStatus === "queued" && typeof item.queued_at_ms === "number"
      ? Math.max(0, nowMs - item.queued_at_ms)
      : 0;
  const settledWaitedMs =
    deliveryStatus === "settled" &&
    typeof item.queued_at_ms === "number" &&
    typeof item.dispatched_at_ms === "number"
      ? item.dispatched_at_ms - item.queued_at_ms
      : 0;

  const cleanBody = useMemo(() => extractCleanAgentMessageBody(item.text), [item.text]);
  const cleanPreview = useMemo(() => formatCleanAgentMessagePreview(cleanBody), [cleanBody]);

  // Resolve the sender's assigned animal avatar from sessionSubagents (or stable fallback)
  const senderCharacter = useMemo(() => {
    const matched = sessionSubagents?.find(
      (sa) =>
        (item.source_agent_id &&
          (sa.childAgentId === item.source_agent_id ||
            sa.providerSubagentId === item.source_agent_id ||
            sa.callId === item.source_agent_id)) ||
        (item.source_title &&
          sa.title.trim().toLowerCase() === item.source_title.trim().toLowerCase()),
    );
    if (matched?.character) return matched.character;
    const seed = item.source_agent_id || item.source_title || `agent-${item.seq}`;
    return resolveSubagentCharacter(seed);
  }, [sessionSubagents, item.source_agent_id, item.source_title, item.seq]);

  // Heartbeat renders as a quiet left-aligned collapsible activity line
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
            "group/hb text-ink-tertiary -ml-1.5 inline-flex max-w-full items-center gap-1.5 rounded-[6px] px-1.5 py-0.5 text-left text-[12px] font-normal transition-colors",
            canExpand
              ? "hover:text-ink-primary hover:bg-accent/65 cursor-pointer"
              : "cursor-default",
          )}
        >
          <HeartPulse className="text-signal-danger/80 size-3.5 shrink-0" />
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
          <div className="orbit-stage-enter border-border-default my-1.5 ml-[7px] border-l-2 pl-3 text-[12px]">
            {canRenderRawBlocks ? (
              <MarkdownBlocksView blocks={item.blocks} compact />
            ) : (
              <p className="selectable-text text-ink-secondary whitespace-pre-wrap break-words leading-[1.6]">
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
        <div className="border-signal-warning-border bg-signal-warning-bg/65 rounded-[12px] border px-3.5 py-2.5 text-[12px]">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <SubagentAvatar
                character={senderCharacter}
                status={resolvedLocal ? "completed" : "awaiting_permission"}
                size="sm"
              />
              <span className="text-ink-primary truncate font-medium">
                {senderLabel} requested approval
              </span>
            </div>

            {item.source_agent_id && (
              <button
                type="button"
                onClick={() =>
                  orbitActions.openSubagentTab({
                    callId: item.message_id || `perm_${item.seq}`,
                    role: senderLabel,
                    promptPreview: perm.title,
                    childAgentId: perm.child_agent_id || item.source_agent_id!,
                    childHostId: perm.child_host_id || sourceHostId,
                    parentAgentId: agentId ?? null,
                  })
                }
                className="text-ink-tertiary hover:text-ink-primary hover:bg-accent inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-0.5 rounded-[6px] px-1.5 text-[11px] font-medium transition-colors"
              >
                <span>Inspect</span>
                <ArrowUpRight className="size-3 shrink-0" />
              </button>
            )}
          </div>

          <div className="bg-surface-raised/85 border-border-subtle mt-2 flex flex-wrap items-center justify-between gap-2 rounded-[8px] border px-2.5 py-1.5">
            <div className="text-ink-primary min-w-0 text-[12px] font-medium">
              {perm.title}
            </div>
            {resolvedLocal ? (
              <span className="text-ink-tertiary inline-flex items-center gap-1 text-[11px] font-medium">
                <Check className="text-signal-success size-3" />
                {resolvedLocal}
              </span>
            ) : (
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => void handleResolveChildPerm("allow")}
                  className="bg-action-primary text-action-primary-fg inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-full px-2.5 text-[11px] font-medium transition-opacity hover:opacity-90"
                >
                  <Check className="size-3" />
                  Allow
                </button>
                <button
                  type="button"
                  onClick={() => void handleResolveChildPerm("deny")}
                  className="border-border-default text-ink-secondary hover:bg-signal-danger-bg hover:text-signal-danger inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-full border px-2.5 text-[11px] font-medium transition-colors"
                >
                  <X className="size-3" />
                  Deny
                </button>
              </div>
            )}
          </div>

          {hasBody && (
            <div className="border-border-default mt-2 ml-2 border-l-2 pl-3 text-[12px]">
              {canRenderRawBlocks ? (
                <MarkdownBlocksView blocks={item.blocks} compact />
              ) : (
                <p className="selectable-text text-ink-secondary whitespace-pre-wrap break-words leading-[1.6]">
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
      ? "stopped"
      : "finished";
  const avatarStatus = isFailedFinish ? "failed" : isCanceledFinish ? "canceled" : "completed";

  return (
    <div className={cn("min-w-0", !inline && "px-[10px]")}>
      <div
        className={cn(
          "group/amrow flex min-h-[34px] min-w-0 items-center gap-1.5 rounded-[10px] border pr-1.5 transition-colors",
          isFailedFinish
            ? "border-signal-danger-border bg-signal-danger-bg/45"
            : "border-border-subtle bg-surface-recessed/55 hover:border-border-default",
        )}
      >
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
            "flex min-h-[34px] min-w-0 flex-1 items-center gap-2 py-1 pl-2 pr-1 text-left text-[12px]",
            hasBody ? "cursor-pointer" : "cursor-default",
          )}
        >
          <SubagentAvatar
            character={senderCharacter}
            status={avatarStatus}
            size="sm"
            showStatusBadge={isFinish}
          />

          <span className="text-ink-primary max-w-[44%] shrink-0 truncate font-medium">
            {senderLabel}
          </span>

          {isFinish ? (
            <span
              className={cn(
                "shrink-0 text-[11px]",
                isFailedFinish ? "text-signal-danger" : "text-ink-tertiary",
              )}
            >
              {finishStatusLabel}
            </span>
          ) : (
            !cleanPreview && (
              <span className="text-ink-tertiary shrink-0 text-[11px]">message</span>
            )
          )}

          {deliveryStatus === "queued" && (
            <span className="border-signal-warning-border bg-signal-warning-bg text-signal-warning inline-flex shrink-0 items-center rounded-full border px-2 py-0.5 font-mono text-[10.5px] font-medium tabular-nums">
              {queuedWaitedMs >= 1000
                ? t("stage.deliveryQueuedWaiting", {
                    duration: formatDurationCompact(queuedWaitedMs),
                  })
                : t("stage.deliveryQueued")}
            </span>
          )}

          {deliveryStatus === "dispatched" && (
            <span className="border-signal-info-border bg-signal-info-bg text-signal-info inline-flex shrink-0 items-center gap-1 rounded-full border px-2 py-0.5 text-[10.5px] font-medium">
              <OrbitSpinner size={10} />
              <span>{t("stage.deliveryProcessing")}</span>
            </span>
          )}

          {deliveryStatus === "settled" && settledWaitedMs >= 2000 && (
            <span className="text-ink-tertiary shrink-0 font-mono text-[11px] tabular-nums">
              {t("stage.deliveryDeliveredWaited", {
                duration: formatDurationCompact(settledWaitedMs),
              })}
            </span>
          )}

          {deliveryStatus === "dropped" && (
            <span className="border-signal-danger-border bg-signal-danger-bg text-signal-danger inline-flex shrink-0 items-center rounded-full border px-2 py-0.5 text-[10.5px] font-medium">
              {t("stage.deliveryDropped")}
            </span>
          )}

          {!expanded && cleanPreview.length > 0 && (
            <>
              <span aria-hidden="true" className="text-ink-muted shrink-0">
                ·
              </span>
              <span className="text-ink-tertiary min-w-0 flex-1 truncate">
                {cleanPreview}
              </span>
            </>
          )}
        </button>

        <div className="flex shrink-0 items-center gap-1">
          {deliveryStatus === "queued" &&
            isParentTurnRunning &&
            (onInterruptAndContinue || agentId) && (
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  if (onInterruptAndContinue) {
                    onInterruptAndContinue();
                  } else if (agentId) {
                    void orbitIpc.interruptAgentAndContinue(hostId, agentId);
                  }
                }}
                className="border-signal-warning-border bg-signal-warning-bg text-signal-warning hover:bg-signal-warning-bg/80 inline-flex h-[22px] shrink-0 cursor-pointer items-center rounded-[6px] border px-2 text-[11px] font-medium transition-colors"
              >
                {t("stage.interruptAndDeliver")}
              </button>
            )}
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
              className="text-ink-tertiary hover:text-ink-primary hover:bg-accent hidden h-[22px] shrink-0 cursor-pointer items-center gap-0.5 rounded-[6px] px-1.5 text-[11px] font-medium transition-colors group-focus-within/amrow:inline-flex group-hover/amrow:inline-flex"
            >
              <span>Open</span>
              <ArrowUpRight className="size-3 shrink-0" />
            </button>
          )}

          {hasBody && (
            <IconButton
              size="xs"
              aria-label={expanded ? "Collapse message" : "Expand message"}
              aria-expanded={expanded}
              onClick={() => setExpanded((v) => !v)}
            >
              <ChevronRight
                className={cn(
                  "size-3.5 transition-transform duration-150",
                  expanded && "rotate-90",
                )}
              />
            </IconButton>
          )}
        </div>
      </div>

      {/* Expanded body uses clean left-rail prose directly below — never nested card-in-card */}
      {expanded && hasBody && (
        <div className="orbit-stage-enter border-border-default mt-1.5 ml-3 border-l-2 pl-3 text-[12px]">
          {canRenderRawBlocks ? (
            <MarkdownBlocksView blocks={item.blocks} compact />
          ) : (
            <p className="selectable-text text-ink-secondary whitespace-pre-wrap break-words leading-[1.6]">
              {cleanBody}
            </p>
          )}
        </div>
      )}
    </div>
  );
});
