import { memo, useEffect, useRef, useState } from "react";
import { Archive, MoreHorizontal } from "lucide-react";
import type { AgentCardView, HostCardView } from "@/lib/orbit-ipc";
import { useImeCompositionGuard } from "@/lib/ime";
import type { MenuAnchorBox } from "@/components/sidebar/sidebar-menus";
import {
  ConversationStatusGlyph,
  formatCompactRelativeTime,
} from "@/components/sidebar/sidebar-primitives";
import { orbitActions } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

export interface SidebarConversationRowProps {
  threadKey: string;
  host: HostCardView;
  agent: AgentCardView;
  projectName: string;
  effectiveTitle: string;
  isWorktree: boolean;
  isPinned: boolean;
  isUnread: boolean;
  selected: boolean;
  isChild: boolean;
  nowMs: number;
  menuOpen: boolean;
  isRenaming: boolean;
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
  host,
  agent,
  projectName,
  effectiveTitle,
  isWorktree,
  isPinned,
  isUnread,
  selected,
  isChild,
  nowMs,
  menuOpen,
  isRenaming,
  onOpenMenu,
  onStartRename,
  onFinishRename,
}: SidebarConversationRowProps) {
  const [renameDraft, setRenameDraft] = useState(effectiveTitle);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const renameCancelledRef = useRef(false);
  const ime = useImeCompositionGuard();

  const relativeLabel = agent.is_running
    ? "now"
    : formatCompactRelativeTime(agent.updated_at_rfc3339, nowMs);

  useEffect(() => {
    if (!isRenaming) return;
    renameCancelledRef.current = false;
    setRenameDraft(effectiveTitle);
    const raf = window.requestAnimationFrame(() => {
      renameInputRef.current?.focus();
      renameInputRef.current?.select();
    });
    return () => window.cancelAnimationFrame(raf);
  }, [isRenaming, effectiveTitle]);

  const commitRename = () => {
    if (renameCancelledRef.current) {
      renameCancelledRef.current = false;
      onFinishRename();
      return;
    }
    orbitActions.renameThread(host.host_id, agent.agent_id, renameDraft);
    onFinishRename();
  };

  return (
    <div
      title={isPinned ? `${effectiveTitle} · ${projectName}` : effectiveTitle}
      onClick={() => {
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
        "group/item flex h-[30px] cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2 transition-colors",
        isChild && "ml-[24px] border-l border-black/[0.08] pl-2.5 dark:border-white/[0.10]",
        selected
          ? "text-foreground bg-black/[0.06] font-medium dark:bg-white/[0.085]"
          : isUnread
            ? "text-foreground font-medium hover:bg-black/[0.038] dark:hover:bg-white/[0.05]"
            : "text-foreground/76 hover:text-foreground hover:bg-black/[0.038] dark:hover:bg-white/[0.05]",
      )}
    >
      {/* Left `16×16` Status Gutter (`x = 24px` center) + Title (`x = 42px` left edge) */}
      <div className="flex min-w-0 flex-1 items-center gap-2.5">
        {!isChild ? (
          <span className="inline-flex h-[16px] w-[16px] shrink-0 items-center justify-center">
            <ConversationStatusGlyph
              agent={agent}
              isWorktree={isWorktree}
              isUnread={isUnread}
            />
          </span>
        ) : (
          <ConversationStatusGlyph
            agent={agent}
            isWorktree={isWorktree}
            isUnread={isUnread}
          />
        )}

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
            className="bg-background text-foreground ring-foreground/25 -mx-1.5 h-[22px] w-full min-w-0 rounded-[5px] px-1.5 text-[13.5px] font-normal ring-1 outline-none"
          />
        ) : (
          <span
            onDoubleClick={(e) => {
              e.stopPropagation();
              onStartRename(threadKey);
            }}
            className="truncate text-[13.5px]"
          >
            {effectiveTitle}
          </span>
        )}
      </div>

      {/* Right Fixed-Width Slot (`46px`): Resting Relative Time ↔ Hover Actions (Zero Radix Instances!) */}
      {!isRenaming && (
        <div className="flex w-[46px] shrink-0 items-center justify-end">
          <span
            className={cn(
              "text-muted-foreground/58 tabular-nums text-[12px] leading-none",
              menuOpen ? "hidden" : "group-hover/item:hidden",
            )}
          >
            {relativeLabel}
          </span>

          <div
            className={cn(
              "items-center gap-0.5",
              menuOpen ? "inline-flex" : "hidden group-hover/item:inline-flex",
            )}
            onClick={(e) => e.stopPropagation()}
          >
            <button
              type="button"
              title="Archive conversation"
              onClick={() => orbitActions.archiveThread(host.host_id, agent.agent_id)}
              className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
            >
              <Archive className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
            </button>

            <button
              type="button"
              aria-label="Conversation options"
              onClick={(e) => {
                const r = e.currentTarget.getBoundingClientRect();
                onOpenMenu(threadKey, host, agent, isPinned, isUnread, {
                  left: r.left,
                  top: r.top,
                  width: r.width,
                  height: r.height,
                });
              }}
              className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-[5px] hover:bg-black/[0.06] dark:hover:bg-white/[0.09]"
            >
              <MoreHorizontal className="h-[13.5px] w-[13.5px]" />
            </button>
          </div>
        </div>
      )}
    </div>
  );
});
