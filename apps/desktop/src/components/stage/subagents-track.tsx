import { memo, useEffect, useMemo, useRef, useState, type ReactNode } from "react";
import {
  AlertTriangle,
  ChevronRight,
  HeartPulse,
  MessageSquareText,
  Target,
} from "lucide-react";
import {
  type RichBlockView,
  type TimelineItemView,
  orbitIpc,
} from "@/lib/orbit-ipc";
import {
  extractCleanAgentMessageBody,
  formatCleanAgentMessagePreview,
} from "@/lib/agent-message-clean";
import { useI18n } from "@/lib/i18n";
import type { ResolvedSubagentItem } from "@/lib/subagents";
import type { SubagentCharacterSpec } from "@/lib/subagent-avatar";
import { MarkdownBlocksView } from "@/components/markdown-blocks";
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from "@/components/ui/collapsible";
import {
  isTimelineAgentMessageHidden,
  peerFinishStatus,
  type CoalescedFinishStatus,
} from "@/lib/turn-delegation-split";
import { useOrbitSelector } from "@/state/orbit-store";
import { openSubagent, SubagentPanelAction } from "./subagent-crew-dock";
import { holdNearestViewport } from "@/components/conversation/conversation-viewport";
import type { ViewportAnchorHoldFn } from "./stage-helpers";
import { cn } from "@/lib/utils";

export { extractCleanAgentMessageBody };

const MAX_PARSED_MESSAGE_CACHE = 96;
const parsedMessageBlocksCache = new Map<string, RichBlockView[]>();

function cacheParsedMessageBlocks(text: string, blocks: RichBlockView[]) {
  if (parsedMessageBlocksCache.has(text)) {
    parsedMessageBlocksCache.delete(text);
  } else if (parsedMessageBlocksCache.size >= MAX_PARSED_MESSAGE_CACHE) {
    const oldest = parsedMessageBlocksCache.keys().next().value;
    if (oldest !== undefined) parsedMessageBlocksCache.delete(oldest);
  }
  parsedMessageBlocksCache.set(text, blocks);
}

/**
 * Renders cleaned peer/subagent message text as full Orbit Markdown blocks when expanded.
 * Reuses precomputed blocks when unchanged, otherwise lazily parses via Rust `parseMarkdownPreview`
 * with a small in-memory cache so collapsed rows pay zero parsing cost.
 */
export const LazyMarkdownMessageBody = memo(function LazyMarkdownMessageBody({
  text,
  precomputedBlocks,
}: {
  text: string;
  precomputedBlocks?: readonly RichBlockView[] | null;
}) {
  const cached = parsedMessageBlocksCache.get(text) ?? null;
  const [parsedBlocks, setParsedBlocks] = useState<readonly RichBlockView[] | null>(
    () => (precomputedBlocks && precomputedBlocks.length > 0 ? precomputedBlocks : cached),
  );

  useEffect(() => {
    if (precomputedBlocks && precomputedBlocks.length > 0) {
      setParsedBlocks(precomputedBlocks);
      return;
    }
    const hit = parsedMessageBlocksCache.get(text);
    if (hit) {
      setParsedBlocks(hit);
      return;
    }
    let cancelled = false;
    setParsedBlocks(null);
    void orbitIpc
      .parseMarkdownPreview(text, false)
      .then((blocks) => {
        if (cancelled) return;
        if (Array.isArray(blocks) && blocks.length > 0) {
          cacheParsedMessageBlocks(text, blocks);
          setParsedBlocks(blocks);
        }
      })
      .catch(() => {
        // Keep plain text fallback when running without native bridge
      });
    return () => {
      cancelled = true;
    };
  }, [text, precomputedBlocks]);

  if (parsedBlocks && parsedBlocks.length > 0) {
    return <MarkdownBlocksView blocks={parsedBlocks as RichBlockView[]} compact />;
  }
  return (
    <p className="selectable-text text-ink-secondary whitespace-pre-wrap break-words leading-[1.55]">
      {text}
    </p>
  );
});

type SubagentMaybeWithAvatar = ResolvedSubagentItem & {
  readonly character?: SubagentCharacterSpec;
};

/**
 * Single source of truth for timeline peer/subagent message row geometry.
 * Negative-bleed hover pill (`-mx-[6px]` / `-mx-[7px]` with 1px border) keeps every avatar
 * strictly aligned to `x = 0px` alongside Trace icons and assistant prose.
 */
const SUBAGENT_ROW_BASE = "flex min-h-[28px] min-w-0 items-center transition-colors";
const SUBAGENT_ROW_TONE = {
  normal: "-mx-[6px] rounded-[8px] pl-[6px] pr-[3px] hover:bg-accent/55",
  danger:
    "-mx-[7px] rounded-[8px] border border-signal-danger-border bg-signal-danger-bg/45 pl-[6px] pr-[3px]",
} as const;

const instantPanelClass = "h-auto duration-0 data-[starting-style]:h-auto data-[starting-style]:opacity-100";
const disclosureRevealInnerClass =
  "my-1 ml-[9px] border-l border-border-default pl-3 text-[13px] transition-[opacity,transform] duration-[130ms] ease-[var(--ease-out-quart)] starting:opacity-0 starting:-translate-y-[3px] motion-reduce:transition-none";

function useAnchoredOpenChange(onChange?: (open: boolean) => void, holdViewport = holdNearestViewport) {
  const triggerRef = useRef<HTMLButtonElement>(null);
  const onOpenChange = (open: boolean) => {
    if (!onChange) return;
    holdViewport(triggerRef.current, () => onChange(open), "top", { intent: open ? "expand" : "collapse" });
  };
  return { triggerRef, onOpenChange };
}

/** Shared collapsible disclosure for incoming reports and outgoing messages. Opening the peer is a separate action. */
export function PeerMessageRow({
  open = false, onOpenChange, holdViewport, avatar, label, statusLabel, badges, preview, failed = false, action, children,
}: {
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  holdViewport?: ViewportAnchorHoldFn;
  avatar: ReactNode;
  label: string;
  statusLabel?: string;
  badges?: ReactNode;
  preview: string;
  failed?: boolean;
  action?: ReactNode;
  children?: ReactNode;
}) {
  const anchored = useAnchoredOpenChange(onOpenChange, holdViewport);
  const canExpand = Boolean(children && onOpenChange);
  return (
    <Collapsible open={open} onOpenChange={anchored.onOpenChange} className="min-w-0 w-full">
      <div
        className={cn(
          "group/msgrow gap-1",
          SUBAGENT_ROW_BASE,
          failed ? SUBAGENT_ROW_TONE.danger : SUBAGENT_ROW_TONE.normal,
        )}
      >
        <CollapsibleTrigger
          ref={anchored.triggerRef}
          disabled={!canExpand}
          title={preview ? `${label} · ${preview}` : label}
          className="flex min-h-[28px] min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-[6px] px-0 pr-1 text-left text-[13px] leading-[18px] outline-none focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-default"
        >
          {avatar}
          <span className="shrink-0 max-w-[62%] truncate font-medium text-ink-primary">{label}</span>
          {statusLabel && (
            <span className={cn("shrink-0 text-[11px]", failed ? "text-signal-danger" : "text-ink-tertiary")}>
              {statusLabel}
            </span>
          )}
          {badges}
          {!open && preview && (
            <>
              <span aria-hidden="true" className="shrink-0 text-ink-muted">·</span>
              <span className="min-w-0 flex-1 truncate text-ink-tertiary">{preview}</span>
            </>
          )}
          {canExpand && (
            <ChevronRight aria-hidden="true" className={cn(
              "ml-auto size-3 shrink-0 text-ink-muted transition-transform duration-[150ms] ease-[var(--ease-panel)] motion-reduce:transition-none",
              open && "rotate-90",
            )} />
          )}
        </CollapsibleTrigger>
        {action}
      </div>
      {canExpand && (
        <CollapsibleContent className={instantPanelClass}>
          <div className={disclosureRevealInnerClass}>{children}</div>
        </CollapsibleContent>
      )}
    </Collapsible>
  );
}

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
  coalescedFinishStatus,
  holdViewport,
  sessionSubagents,
}: {
  hostId: string;
  agentId?: string | null;
  item: Extract<TimelineItemView, { item_type: "agent_message" }>;
  inline?: boolean;
  coalescedFinishStatus?: CoalescedFinishStatus;
  holdViewport?: ViewportAnchorHoldFn;
  sessionSubagents?: readonly SubagentMaybeWithAvatar[];
}) {
  const { t } = useI18n();
  const [expanded, setExpanded] = useState(false);
  const heartbeat = useAnchoredOpenChange(setExpanded, holdViewport);

  const cleanBody = useMemo(() => extractCleanAgentMessageBody(item.text), [item.text]);
  const cleanPreview = useMemo(() => formatCleanAgentMessagePreview(cleanBody), [cleanBody]);

  const perm = item.permission_context;
  const senderId = perm?.child_agent_id || item.source_agent_id;
  const senderHostId = perm?.child_host_id || item.source_host_id;
  const sender = useMemo(() => {
    if (senderId) {
      return sessionSubagents?.find((sa) =>
        (!senderHostId || sa.childHostId === senderHostId) &&
        (sa.childAgentId === senderId || sa.providerSubagentId === senderId || sa.callId === senderId),
      );
    }
    const title = item.source_title?.trim().toLowerCase();
    if (!title) return undefined;
    const matches = sessionSubagents?.filter((sa) =>
      (!senderHostId || sa.childHostId === senderHostId) && sa.title.trim().toLowerCase() === title,
    );
    return matches?.length === 1 ? matches[0] : undefined;
  }, [sessionSubagents, senderId, senderHostId, item.source_title]);
  const senderLabel = item.source_title || sender?.title || senderId || t("approval.subagent");
  const targetHost = perm?.child_host_id || sender?.childHostId || item.source_host_id || hostId;
  const isStillPending = useOrbitSelector((s) => {
    if (!perm) return false;
    const pending = s.hostDetails[targetHost]?.pending_permissions;
    if (pending) {
      return pending.some((p) => p.agent_id === perm.child_agent_id && p.request_id === perm.permission_request_id);
    }
    return s.stages[`${targetHost}::${perm.child_agent_id}`]?.pending_permissions.some((p) =>
      p.item_type === "permission_request" && p.request_id === perm.permission_request_id && p.resolved_decision === null,
    ) ?? false;
  });

  const openSender = () => openSubagent(agentId ?? undefined, {
    callId: sender?.callId || item.message_id || `am_${item.seq}`,
    title: sender?.title || senderLabel,
    promptPreview: perm?.title || sender?.promptPreview || cleanPreview,
    childAgentId: perm?.child_agent_id || (sender ? sender.childAgentId : senderId ?? null),
    childHostId: targetHost,
    providerSubagentId: sender?.providerSubagentId ?? null,
  });

  const wrapperClass = cn("min-w-0", !inline && "px-[10px]");
  const body = item.kind === "heartbeat" ? cleanBody || item.text.trim() : cleanBody;
  const precomputedBlocks =
    body && item.blocks.length > 0 && body === item.text.trim() ? item.blocks : null;
  const detail = body ? (
    <LazyMarkdownMessageBody text={body} precomputedBlocks={precomputedBlocks} />
  ) : undefined;

  if (isTimelineAgentMessageHidden(item)) {
    return null;
  }

  if (item.kind === "heartbeat" && !perm) {
    const isGoalWakeup = Boolean(item.source_title && /^Goal\b/i.test(item.source_title));
    return (
      <Collapsible open={expanded} onOpenChange={heartbeat.onOpenChange} className={wrapperClass}>
        <CollapsibleTrigger
          ref={heartbeat.triggerRef}
          disabled={!body}
          className="-mx-[6px] inline-flex min-h-[28px] max-w-[calc(100%+12px)] cursor-pointer items-center gap-1.5 rounded-[8px] px-[6px] text-left text-[13px] leading-[18px] text-ink-tertiary outline-none transition-colors hover:bg-accent/55 focus-visible:ring-2 focus-visible:ring-ring/50 disabled:cursor-default"
        >
          {isGoalWakeup ? (
            <Target className="text-ink-tertiary size-3.5 shrink-0" />
          ) : (
            <HeartPulse className="text-signal-danger/80 size-3.5 shrink-0" />
          )}
          <span className="truncate">{item.source_title && item.source_title !== "Heartbeat wakeup" ? item.source_title : t("stage.heartbeat")}</span>
          {body && <ChevronRight aria-hidden="true" className={cn("size-3 shrink-0 transition-transform duration-[150ms] ease-[var(--ease-panel)] motion-reduce:transition-none", expanded && "rotate-90")} />}
        </CollapsibleTrigger>
        {body && (
          <CollapsibleContent className={instantPanelClass}>
            <div className={disclosureRevealInnerClass}>{detail}</div>
          </CollapsibleContent>
        )}
      </Collapsible>
    );
  }

  if (perm) {
    if (!isStillPending) {
      return null;
    }
    return (
      <div className={wrapperClass}>
        <div
          data-status="awaiting_permission"
          className="group/msgrow -mx-[7px] flex min-h-[32px] flex-wrap items-center gap-x-2 gap-y-1 rounded-[8px] border border-signal-warning-border bg-signal-warning-bg/55 px-[6px] py-1.5 text-[13px] leading-[18px]"
        >
          <span aria-hidden="true" className="inline-flex size-4 shrink-0 items-center justify-center">
            <AlertTriangle className="size-3.5 text-signal-warning" />
          </span>
          <div className="min-w-0 flex-1 basis-32">
            <div className="text-ink-primary truncate font-medium">
              {t("stage.approvalRequested", { name: senderLabel })}
            </div>
            <div className="text-ink-secondary break-words">{perm.title}</div>
          </div>
          <span className="text-signal-warning text-[11px]">{t("stage.awaitingDecision")}</span>
          <SubagentPanelAction name={senderLabel} inspect alwaysVisible onClick={openSender} />
        </div>
      </div>
    );
  }

  const finish = coalescedFinishStatus ?? peerFinishStatus(item, cleanBody);
  const preview = cleanPreview;
  const statusLabel = finish === "failed" ? t("stage.failed")
    : finish === "canceled" ? t("stage.stopped")
    : finish === "completed" ? t("stage.finished")
    : !preview ? t("stage.message") : undefined;

  const rowAction =
    sender || senderId ? (
      <div className="flex shrink-0 items-center gap-1">
        <SubagentPanelAction name={senderLabel} onClick={openSender} />
      </div>
    ) : undefined;

  const leadingIcon = (
    <span
      aria-hidden="true"
      data-status={finish ?? "completed"}
      className="inline-flex size-4 shrink-0 items-center justify-center"
    >
      {finish === "failed" ? (
        <AlertTriangle className="size-3.5 text-signal-danger" />
      ) : (
        <MessageSquareText className="size-3.5 text-ink-tertiary" />
      )}
    </span>
  );

  return (
    <div className={wrapperClass}>
      <PeerMessageRow
        open={expanded}
        onOpenChange={setExpanded}
        holdViewport={holdViewport}
        label={senderLabel}
        statusLabel={statusLabel}
        failed={finish === "failed"}
        preview={preview}
        avatar={leadingIcon}
        action={rowAction}
      >
        {detail}
      </PeerMessageRow>
    </div>
  );
});
