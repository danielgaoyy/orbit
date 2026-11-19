import {
  CheckCircle2,
  Circle,
  Copy,
  FolderOpen,
  Pencil,
  Pin,
  PinOff,
  Trash2,
} from "lucide-react";
import { type AgentCardView, type HostCardView, orbitIpc } from "@/lib/orbit-ipc";
import { type UnifiedProjectEntry, isLocalHostCard } from "@/lib/projects";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import { orbitActions } from "@/state/orbit-store";

export interface MenuAnchorBox {
  left: number;
  top: number;
  width: number;
  height: number;
}

export interface ActiveConversationMenuState {
  threadKey: string;
  host: HostCardView;
  agent: AgentCardView;
  isPinned: boolean;
  isUnread: boolean;
  anchor: MenuAnchorBox;
}

export interface ActiveProjectMenuState {
  project: UnifiedProjectEntry;
  anchor: MenuAnchorBox;
}

/**
 * Single shared Conversation Action/Context Menu for the entire `FleetSidebar`.
 * Replaces mounting N idle `DropdownMenu` roots across every sidebar row.
 */
export function SharedConversationMenu({
  target,
  onClose,
  onStartRename,
}: {
  target: ActiveConversationMenuState | null;
  onClose: () => void;
  onStartRename: (threadKey: string) => void;
}) {
  if (!target) return null;
  const { threadKey, host, agent, isPinned, isUnread, anchor } = target;

  return (
    <DropdownMenu
      key={`${threadKey}:${anchor.left}:${anchor.top}`}
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DropdownMenuTrigger asChild>
        <span
          aria-hidden="true"
          style={{
            position: "fixed",
            left: `${anchor.left}px`,
            top: `${anchor.top}px`,
            width: `${anchor.width}px`,
            height: `${anchor.height}px`,
            pointerEvents: "none",
          }}
        />
      </DropdownMenuTrigger>

      <DropdownMenuContent
        align="start"
        sideOffset={4}
        finalFocus={false}
        className="w-44 p-1.5"
      >
        <DropdownMenuItem
          onSelect={() => {
            onClose();
            window.requestAnimationFrame(() => onStartRename(threadKey));
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Pencil className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Rename</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => orbitActions.togglePinThread(host.host_id, agent.agent_id)}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            {isPinned ? (
              <PinOff className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            ) : (
              <Pin className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            )}
            <span>{isPinned ? "Unpin" : "Pin"}</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          disabled={agent.is_running}
          onSelect={() => {
            if (isUnread) {
              orbitActions.markThreadRead(host.host_id, agent.agent_id);
            } else {
              orbitActions.markThreadUnread(host.host_id, agent.agent_id);
            }
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            {isUnread ? (
              <CheckCircle2 className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            ) : (
              <Circle className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            )}
            <span>{isUnread ? "Mark Read" : "Mark Unread"}</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => {
            void orbitActions.copyThread(host.host_id, agent.agent_id);
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Copy className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Copy</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuSeparator />

        <DropdownMenuItem
          onSelect={() =>
            void orbitActions.deleteThreadPermanently(host.host_id, agent.agent_id)
          }
        >
          <span className="flex items-center gap-2 whitespace-nowrap text-rose-600 dark:text-rose-400">
            <Trash2 className="h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Delete</span>
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Single shared Project Options Menu for the entire `FleetSidebar`.
 */
export function SharedProjectMenu({
  target,
  isMac,
  onClose,
}: {
  target: ActiveProjectMenuState | null;
  isMac: boolean;
  onClose: () => void;
}) {
  if (!target) return null;
  const { project: proj, anchor } = target;
  const isLocal = isLocalHostCard(proj.host);

  return (
    <DropdownMenu
      key={`${proj.id}:${anchor.left}:${anchor.top}`}
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <DropdownMenuTrigger asChild>
        <span
          aria-hidden="true"
          style={{
            position: "fixed",
            left: `${anchor.left}px`,
            top: `${anchor.top}px`,
            width: `${anchor.width}px`,
            height: `${anchor.height}px`,
            pointerEvents: "none",
          }}
        />
      </DropdownMenuTrigger>

      <DropdownMenuContent
        align="start"
        sideOffset={4}
        finalFocus={false}
        className="w-44 p-1.5"
      >
        <DropdownMenuItem
          onSelect={() => {
            void navigator.clipboard.writeText(proj.primaryCwd);
            orbitActions.showToast("Copied project path");
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Copy className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Copy Path</span>
          </span>
        </DropdownMenuItem>

        {isLocal && (
          <DropdownMenuItem
            onSelect={() => {
              void orbitIpc.openInEditor("file_manager", proj.primaryCwd, null);
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <FolderOpen className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
              <span>{isMac ? "Reveal in Finder" : "Open Folder"}</span>
            </span>
          </DropdownMenuItem>
        )}
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
