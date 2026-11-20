import { useState } from "react";
import {
  Archive,
  CheckCircle2,
  Circle,
  Copy,
  FolderOpen,
  Hash,
  Loader2,
  Pencil,
  Pin,
  PinOff,
  Trash2,
  Unlink,
} from "lucide-react";
import {
  type AgentCardView,
  type HostCardView,
  type SidebarProjectGroupView,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { isLocalHostCard } from "@/lib/projects";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import {
  type ArchivedThreadEntry,
  orbitActions,
  threadScopeKey,
} from "@/state/orbit-store";

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
  project: SidebarProjectGroupView;
  host: HostCardView;
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
          disabled={isAgentForegroundRunning(agent)}
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

        <DropdownMenuItem
          onSelect={() => {
            void navigator.clipboard.writeText(agent.agent_id).then(() => {
              orbitActions.showToast(`Copied ID ${agent.agent_id}`);
            });
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Hash className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Copy Agent ID</span>
          </span>
        </DropdownMenuItem>

        {agent.parent_agent_id && (
          <DropdownMenuItem
            onSelect={() => {
              void orbitActions.detachAgent(host.host_id, agent.agent_id);
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Unlink className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
              <span>Detach</span>
            </span>
          </DropdownMenuItem>
        )}

        <DropdownMenuItem
          onSelect={() => orbitActions.archiveThread(host.host_id, agent.agent_id)}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Archive className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Archive</span>
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
  onRequestDelete,
}: {
  target: ActiveProjectMenuState | null;
  isMac: boolean;
  onClose: () => void;
  onRequestDelete: (project: SidebarProjectGroupView) => void;
}) {
  if (!target) return null;
  const { project: proj, host, anchor } = target;
  const isLocal = isLocalHostCard(host);

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
            void navigator.clipboard.writeText(proj.primary_cwd);
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
              void orbitIpc.openInEditor("file_manager", proj.primary_cwd, null);
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <FolderOpen className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
              <span>{isMac ? "Reveal in Finder" : "Open Folder"}</span>
            </span>
          </DropdownMenuItem>
        )}

        <DropdownMenuSeparator />

        <DropdownMenuItem
          onSelect={() => {
            onClose();
            onRequestDelete(proj);
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap text-rose-600 dark:text-rose-400">
            <Trash2 className="h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Delete Project</span>
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function formatConversationDeleteSummary(
  activeCount: number,
  archivedCount: number,
): string {
  const parts: string[] = [];
  if (activeCount > 0) {
    parts.push(
      activeCount === 1 ? "1 active conversation" : `${activeCount} active conversations`,
    );
  }
  if (archivedCount > 0) {
    parts.push(
      archivedCount === 1
        ? "1 archived conversation"
        : `${archivedCount} archived conversations`,
    );
  }
  return parts.join(" and ");
}

export function DeleteProjectDialog({
  project,
  archivedThreads,
  onClose,
}: {
  project: SidebarProjectGroupView | null;
  archivedThreads: Record<string, ArchivedThreadEntry>;
  onClose: () => void;
}) {
  const [deleting, setDeleting] = useState(false);

  let activeCount = 0;
  let archivedCount = 0;
  let runningCount = 0;
  if (project) {
    for (const { agent } of project.conversations) {
      const isArchived = Boolean(
        archivedThreads[threadScopeKey(project.host_id, agent.agent_id)],
      );
      if (isArchived) {
        archivedCount += 1;
      } else {
        activeCount += 1;
        if (
          isAgentForegroundRunning(agent) ||
          isAgentBackgroundWaiting(agent) ||
          agent.is_awaiting_permission
        ) {
          runningCount += 1;
        }
      }
    }
  }
  const conversationSummary = formatConversationDeleteSummary(activeCount, archivedCount);

  const handleConfirm = async () => {
    if (!project || deleting) return;
    setDeleting(true);
    try {
      await orbitActions.deleteProject(project);
      onClose();
    } catch {
      // Error toast is already surfaced by `orbitActions.deleteProject`.
    } finally {
      setDeleting(false);
    }
  };

  return (
    <Dialog
      open={Boolean(project)}
      onOpenChange={(open) => {
        if (!open && !deleting) onClose();
      }}
    >
      <DialogContent className="max-w-md gap-4 p-5 select-none">
        <DialogHeader className="gap-1.5 pr-6">
          <DialogTitle className="text-[15.5px]">Delete Project</DialogTitle>
          <DialogDescription className="text-foreground/80 text-[13.5px] leading-relaxed">
            Are you sure you want to delete the project{" "}
            <strong className="text-foreground font-semibold">{project?.name ?? ""}</strong>?
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 text-[13px] leading-relaxed">
          {conversationSummary && (
            <p className="text-foreground/80">
              This will permanently delete{" "}
              <strong className="text-foreground font-semibold">
                {conversationSummary}
              </strong>{" "}
              within it.
            </p>
          )}
          {runningCount > 0 && (
            <p className="text-amber-600 dark:text-amber-400">
              {runningCount === 1
                ? "1 running conversation"
                : `${runningCount} running conversations`}{" "}
              will be stopped first.
            </p>
          )}
          <p className="text-muted-foreground text-[12.5px]">
            Files on disk will not be deleted.
          </p>
        </div>

        <div className="flex justify-end gap-2.5 pt-1">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={deleting}
            onClick={onClose}
          >
            Cancel
          </Button>
          <Button
            type="button"
            variant="destructive"
            size="sm"
            autoFocus
            disabled={deleting}
            onClick={() => void handleConfirm()}
          >
            {deleting ? (
              <>
                <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
                <span>Deleting…</span>
              </>
            ) : (
              "Delete"
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
