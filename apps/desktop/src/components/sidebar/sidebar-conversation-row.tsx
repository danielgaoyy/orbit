import { memo, useCallback, useEffect, useRef, useState } from "react";
import { Archive, MoreHorizontal } from "lucide-react";
import {
  type AgentCardView,
  type HostCardView,
  isAgentForegroundRunning,
  isGenericHarnessHandle,
} from "@/lib/orbit-ipc";
import { useImeCompositionGuard } from "@/lib/ime";
import { useI18n } from "@/lib/i18n";
import { splitHighlightSegments } from "@/lib/fuzzy-match";
import type { MenuAnchorBox } from "@/components/sidebar/sidebar-menus";
import {
  ConversationStatusGlyph,
  SidebarRelativeTime,
} from "@/components/sidebar/sidebar-primitives";
import type { ThreadDragSource } from "@/components/sidebar/use-sidebar-drag";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { IconButton } from "@/components/ui/icon-button";
import { orbitActions, useHasThreadDraft } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const DRAFT_BADGE_CLASS = "text-ink-muted text-[11px] font-medium leading-none";

export interface SidebarConversationRowProps {
  threadKey: string;
  navId: string;
  host: HostCardView;
  agent: AgentCardView;
  projectName: string;
  effectiveTitle: string;
  /** Optional subtle project context tag (used in Attention & multi-project Pinned rows) */
  contextLabel?: string;
  isWorktree: boolean;
  isPinned: boolean;
  isUnread: boolean;
  hasRunningSubagents?: boolean;
  selected: boolean;
  keyboardFocused?: boolean;
  highlightIndices?: readonly number[];
  isChild: boolean;
  /** True when rendered inside a Group's own vertical-rail container (compact gutter, no duplicate border-l) */
  inGroupRail?: boolean;
  peerHandle?: string | null;
  menuOpen: boolean;
  isRenaming: boolean;
  /** Drag metadata primitives (kept flat so React.memo stays intact across renders) */
  dragProjectId?: string;
  dragScopeKey?: string;
  dragWorkspaceId?: string | null;
  isDragging?: boolean;
  dropIndicator?: "before" | "after" | null;
  onRegisterNavRow?: (navId: string, el: HTMLElement | null) => void;
  onRegisterThreadRow?: (
    refKey: string,
    entry: {
      hostId: string;
      projectId: string;
      agentId: string;
      scopeKey: string;
      workspaceId: string | null;
    } | null,
    el: HTMLDivElement | null,
  ) => void;
  onStartThreadDrag?: (e: React.PointerEvent<HTMLDivElement>, source: ThreadDragSource) => void;
  shouldSuppressClick?: () => boolean;
  onOpenMenu: (
    threadKey: string,
    host: HostCardView,
    agent: AgentCardView,
    isPinned: boolean,
    isUnread: boolean,
    anchor: MenuAnchorBox,
  ) => void;
  onStartRename: (threadKey: string) => void;
  onFinishRename: () => void;
}

export const SidebarConversationRow = memo(function SidebarConversationRow({
  threadKey,
  navId,
  host,
  agent,
  projectName,
  effectiveTitle,
  contextLabel,
  isWorktree,
  isPinned,
  isUnread,
  hasRunningSubagents = false,
  selected,
  keyboardFocused = false,
  highlightIndices,
  isChild,
  inGroupRail = false,
  peerHandle,
  menuOpen,
  isRenaming,
  dragProjectId,
  dragScopeKey,
  dragWorkspaceId = null,
  isDragging = false,
  dropIndicator = null,
  onRegisterNavRow,
  onRegisterThreadRow,
  onStartThreadDrag,
  shouldSuppressClick,
  onOpenMenu,
  onStartRename,
  onFinishRename,
}: SidebarConversationRowProps) {
  const [renameDraft, setRenameDraft] = useState(effectiveTitle);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameCancelledRef = useRef(false);
  const renameCommittedRef = useRef(false);
  const ime = useImeCompositionGuard();
  const { t } = useI18n();

  const isForegroundRunning = isAgentForegroundRunning(agent);
  const isRunning = isForegroundRunning || hasRunningSubagents;
  const hasUnsentDraft = useHasThreadDraft(threadKey, !selected);

  useEffect(() => {
    if (!isRenaming) return;
    renameCancelledRef.current = false;
    renameCommittedRef.current = false;
    setRenameDraft(effectiveTitle);
    const raf = window.requestAnimationFrame(() => {
      renameInputRef.current?.focus();
      renameInputRef.current?.select();
    });
    return () => window.cancelAnimationFrame(raf);
  }, [isRenaming, effectiveTitle]);

  const setCombinedRef = useCallback(
    (el: HTMLDivElement | null) => {
      onRegisterNavRow?.(navId, el);
      if (onRegisterThreadRow && dragProjectId && dragScopeKey) {
        const refKey = `${host.host_id}::${agent.agent_id}`;
        onRegisterThreadRow(
          refKey,
          {
            hostId: host.host_id,
            projectId: dragProjectId,
            agentId: agent.agent_id,
            scopeKey: dragScopeKey,
            workspaceId: dragWorkspaceId,
          },
          el,
        );
      }
    },
    [
      onRegisterNavRow,
      navId,
      onRegisterThreadRow,
      dragProjectId,
      dragScopeKey,
      dragWorkspaceId,
      host.host_id,
      agent.agent_id,
    ],
  );

  const handlePointerDown = useCallback(
    (e: React.PointerEvent<HTMLDivElement>) => {
      if (isRenaming || !onStartThreadDrag || !dragProjectId || !dragScopeKey) return;
      onStartThreadDrag(e, {
        hostId: host.host_id,
        projectId: dragProjectId,
        agentId: agent.agent_id,
        title: effectiveTitle,
        scopeKey: dragScopeKey,
        workspaceId: dragWorkspaceId,
      });
    },
    [
      isRenaming,
      onStartThreadDrag,
      dragProjectId,
      dragScopeKey,
      dragWorkspaceId,
      host.host_id,
      agent.agent_id,
      effectiveTitle,
    ],
  );

  const commitRename = () => {
    if (renameCommittedRef.current) return;
    renameCommittedRef.current = true;
    if (renameCancelledRef.current) {
      renameCancelledRef.current = false;
      onFinishRename();
      return;
    }
    orbitActions.renameThread(host.host_id, agent.agent_id, renameDraft);
    onFinishRename();
  };

  const semanticHandleLabel =
    peerHandle && !isGenericHarnessHandle(peerHandle) ? `@${peerHandle}` : null;
  const hoverTooltip = [
    isPinned || contextLabel ? `${effectiveTitle} · ${projectName}` : effectiveTitle,
    semanticHandleLabel,
    hasUnsentDraft ? t("sidebar.unsentDraft") : null,
    agent.task_summary ? agent.task_summary : null,
  ]
    .filter(Boolean)
    .join("\n");

  return (
    <div
      ref={setCombinedRef}
      onPointerDown={onStartThreadDrag ? handlePointerDown : undefined}
      title={hoverTooltip}
      onClick={() => {
        if (shouldSuppressClick?.()) return;
        if (!isRenaming) {
          orbitActions.selectAgent(host.host_id, agent.agent_id);
        }
      }}
      onContextMenu={(e) => {
        if (isRenaming) return;
        e.preventDefault();
        onOpenMenu(threadKey, host, agent, isPinned, isUnread, {
          left: e.clientX,
          top: e.clientY,
          width: 1,
          height: 1,
        });
      }}
      className={cn(
        "group/item relative flex h-[30px] cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 transition-colors",
        isChild && !inGroupRail && "border-border-subtle ml-[24px] border-l pl-2.5",
        isDragging && "opacity-45",
        selected
          ? "bg-accent text-ink-primary font-medium"
          : isUnread
            ? "text-ink-primary hover:bg-accent/55 font-medium"
            : isRunning ||
                agent.is_awaiting_permission ||
                agent.status_label === "Error"
              ? "text-ink-primary hover:bg-accent/55"
              : "text-ink-secondary hover:bg-accent/55 hover:text-ink-primary",
        keyboardFocused && "bg-accent text-ink-primary ring-border-strong ring-1 ring-inset",
      )}
    >
      {dropIndicator === "before" && (
        <div className="bg-primary pointer-events-none absolute -top-[1px] right-1.5 left-1.5 z-20 h-[2px] rounded-full" />
      )}
      {dropIndicator === "after" && (
        <div className="bg-primary pointer-events-none absolute right-1.5 -bottom-[1px] left-1.5 z-20 h-[2px] rounded-full" />
      )}

      <div
        className={cn("flex min-w-0 flex-1 items-center", inGroupRail || isChild ? "gap-1.5" : "gap-2.5")}
      >
        <span className="inline-flex size-4 shrink-0 items-center justify-center">
          <ConversationStatusGlyph agent={agent} isWorktree={isWorktree} isUnread={isUnread} />
        </span>

        {isRenaming ? (
          <input
            ref={renameInputRef}
            type="text"
            value={renameDraft}
            onClick={(e) => e.stopPropagation()}
            onChange={(e) => setRenameDraft(e.target.value)}
            onBlur={commitRename}
            {...ime.compositionProps}
            onKeyDown={(e) => {
              e.stopPropagation();
              if (ime.shouldIgnoreKeyDown(e)) return;
              if (e.key === "Enter") {
                e.preventDefault();
                commitRename();
              } else if (e.key === "Escape") {
                e.preventDefault();
                renameCancelledRef.current = true;
                onFinishRename();
              }
            }}
            className="bg-surface-base text-ink-primary ring-border-strong -mx-1.5 h-[22px] w-full min-w-0 rounded-[5px] px-1.5 text-[13px] font-normal ring-1 outline-none"
          />
        ) : (
          <span
            onDoubleClick={(e) => {
              e.stopPropagation();
              onStartRename(threadKey);
            }}
            className="min-w-0 flex-1 truncate text-[13px]"
          >
            {highlightIndices && highlightIndices.length > 0
              ? splitHighlightSegments(effectiveTitle, highlightIndices).map((seg, idx) =>
                  seg.matched ? (
                    <mark
                      key={idx}
                      className="text-ink-primary bg-ink-primary/10 rounded-[2px] font-semibold"
                    >
                      {seg.text}
                    </mark>
                  ) : (
                    <span key={idx}>{seg.text}</span>
                  ),
                )
              : effectiveTitle}
          </span>
        )}
      </div>

      {!isRenaming && (
        <div className="grid shrink-0 items-center justify-items-end">
          <div
            className={cn(
              "col-start-1 row-start-1 flex items-center gap-1.5 transition-opacity duration-100 motion-reduce:transition-none",
              menuOpen
                ? "pointer-events-none opacity-0"
                : "group-hover/item:pointer-events-none group-hover/item:opacity-0 group-focus-within/item:pointer-events-none group-focus-within/item:opacity-0",
            )}
          >
            {contextLabel && (
              <span className="text-ink-faint max-w-[74px] truncate text-[11px] leading-none">
                {contextLabel}
              </span>
            )}
            {isRunning ? (
              <span
                className="text-ink-secondary inline-flex"
                role="img"
                aria-label={isForegroundRunning ? t("sidebar.running") : t("sidebar.subagentRunning")}
              >
                <OrbitSpinner size={13} />
              </span>
            ) : hasUnsentDraft ? (
              <span data-draft-indicator="true" className={DRAFT_BADGE_CLASS}>
                {t("sidebar.draft")}
              </span>
            ) : (
              <SidebarRelativeTime
                rfc3339={agent.updated_at_rfc3339}
                className="text-ink-faint tabular-nums text-[11.5px] leading-none"
              />
            )}
          </div>

          <div
            data-thread-action="true"
            className={cn(
              "col-start-1 row-start-1 flex items-center gap-0.5 transition-opacity duration-100 motion-reduce:transition-none",
              !menuOpen &&
                "pointer-events-none opacity-0 group-hover/item:pointer-events-auto group-hover/item:opacity-100 group-focus-within/item:pointer-events-auto group-focus-within/item:opacity-100",
            )}
            onClick={(e) => e.stopPropagation()}
          >
            <IconButton
              size="xs"
              tooltip={t("sidebar.archiveConversation")}
              onClick={() => orbitActions.archiveThread(host.host_id, agent.agent_id)}
            >
              <Archive className="size-[13px] stroke-[1.75]" />
            </IconButton>

            <IconButton
              size="xs"
              aria-label={t("sidebar.conversationOptions")}
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                onOpenMenu(threadKey, host, agent, isPinned, isUnread, {
                  left: r.left,
                  top: r.top,
                  width: r.width,
                  height: r.height,
                });
              }}
            >
              <MoreHorizontal className="size-[13.5px]" />
            </IconButton>
          </div>
        </div>
      )}
    </div>
  );
});
