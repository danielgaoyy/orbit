import { useEffect, useState } from "react";
import {
  Archive,
  ArrowLeftRight,
  CheckCircle2,
  Circle,
  Columns2,
  Copy,
  Download,
  FileText,
  FolderInput,
  FolderOpen,
  FolderPlus,
  GitBranch,
  GitFork,
  GitMerge,
  Hash,
  Inbox,
  Laptop,
  Layers,
  Loader2,
  MoreHorizontal,
  Pencil,
  Pin,
  PinOff,
  Plus,
  RefreshCw,
  Sparkles,
  Trash2,
  Unlink,
  UserPlus,
} from "lucide-react";
import {
  type AgentCardView,
  type HostCardView,
  isWindowsDesktop,
  type SidebarProjectGroupView,
  type SidebarWorkspaceGroupView,
  isAgentBackgroundWaiting,
  isAgentForegroundRunning,
  orbitIpc,
  semanticAgentTabLabel,
} from "@/lib/orbit-ipc";
import { isAgentInGitRepo, isLocalHostCard, normalizeProjectPath } from "@/lib/projects";
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
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuSub,
  DropdownMenuSubContent,
  DropdownMenuSubTrigger,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
import {
  type ArchivedThreadEntry,
  orbitActions,
  threadScopeKey,
  useOrbitSelector,
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

export interface ActiveWorkspaceMenuState {
  workspace: SidebarWorkspaceGroupView;
  project: SidebarProjectGroupView;
  host: HostCardView;
  anchor: MenuAnchorBox;
}

export interface CreateWorkspaceDialogTarget {
  hostId: string;
  projectRoot: string;
  projectName: string;
  isGitRepo: boolean;
  moveAgentId?: string | null;
}

export interface DeleteConversationTarget {
  host: HostCardView;
  agent: AgentCardView;
  title: string;
}

const COMPACT_MENU_ITEM_CLASS = "h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]";

/**
 * Single shared Conversation Action/Context Menu for the entire `FleetSidebar`.
 * Replaces mounting N idle `DropdownMenu` roots across every sidebar row.
 */
export function SharedConversationMenu({
  target,
  onClose,
  onStartRename,
  onRequestCreateWorkspace,
  onRequestDelete,
}: {
  target: ActiveConversationMenuState | null;
  onClose: () => void;
  onStartRename: (threadKey: string) => void;
  onRequestCreateWorkspace?: (req: CreateWorkspaceDialogTarget) => void;
  onRequestDelete: (target: DeleteConversationTarget) => void;
}) {
  const hostDetail = useOrbitSelector((s) =>
    target ? (s.hostDetails[target.host.host_id] ?? null) : null,
  );
  const customTitle = useOrbitSelector((s) =>
    target ? s.threadCustomTitles[target.threadKey] : undefined,
  );
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  if (!target) return null;
  const { threadKey, host, agent, isPinned, isUnread, anchor } = target;
  const effectiveTitle = customTitle || agent.title;
  const isForegroundRunning = isAgentForegroundRunning(agent);
  const activeWs = hostDetail?.workspaces.find((w) => w.workspace_id === agent.workspace_id);
  const isCuratedWs = Boolean(activeWs?.is_curated);
  const projectRoot = activeWs?.project_root || activeWs?.source_cwd || activeWs?.cwd || agent.cwd;
  const normRoot = normalizeProjectPath(projectRoot);
  const isGitRepo = isAgentInGitRepo(host.host_id, agent, hostDetail, sidebarView);
  const otherWorkspaces = (hostDetail?.workspaces ?? [])
    .filter((w) => {
      if (!w.is_curated || w.workspace_id === agent.workspace_id || w.archived_at || w.background) {
        return false;
      }
      const wRoot = normalizeProjectPath(w.project_root || w.source_cwd || w.cwd);
      const wCwd = normalizeProjectPath(w.cwd);
      return (
        wRoot === normRoot ||
        wCwd === normRoot ||
        wRoot.startsWith(`${normRoot}/`) ||
        normRoot.startsWith(`${wRoot}/`)
      );
    })
    .slice(0, 8);
  const availableProviders =
    hostDetail?.host.available_providers.length
      ? hostDetail.host.available_providers
      : [agent.provider];
  const directProviders = availableProviders.filter((p) => p !== "acp");
  const altProvider =
    directProviders.find((p) => p !== agent.provider) ??
    (agent.provider !== "acp" ? agent.provider : (directProviders[0] ?? "claude_code"));
  const tabLabel = semanticAgentTabLabel(agent);

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
        className="w-max min-w-[184px] rounded-[12px] p-1"
      >
        <DropdownMenuItem
          onSelect={() => {
            onClose();
            window.requestAnimationFrame(() => onStartRename(threadKey));
          }}
          className={COMPACT_MENU_ITEM_CLASS}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Pencil className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span>Rename</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => orbitActions.togglePinThread(host.host_id, agent.agent_id)}
          className={COMPACT_MENU_ITEM_CLASS}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            {isPinned ? (
              <PinOff className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            ) : (
              <Pin className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            )}
            <span>{isPinned ? "Unpin" : "Pin"}</span>
          </span>
        </DropdownMenuItem>

        {!isForegroundRunning && (
          <DropdownMenuItem
            onSelect={() => {
              if (isUnread) {
                orbitActions.markThreadRead(host.host_id, agent.agent_id);
              } else {
                orbitActions.markThreadUnread(host.host_id, agent.agent_id);
              }
            }}
            className={COMPACT_MENU_ITEM_CLASS}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              {isUnread ? (
                <CheckCircle2 className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              ) : (
                <Circle className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              )}
              <span>{isUnread ? "Mark read" : "Mark unread"}</span>
            </span>
          </DropdownMenuItem>
        )}

        <DropdownMenuSeparator className="my-1" />

        <DropdownMenuItem
          onSelect={() => {
            orbitActions.openPeerSplitTab(host.host_id, agent.agent_id, tabLabel);
          }}
          className={COMPACT_MENU_ITEM_CLASS}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Columns2 className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span>Open split</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => {
            void orbitActions.forkAgent({
              hostId: host.host_id,
              sourceAgentId: agent.agent_id,
            });
          }}
          className={COMPACT_MENU_ITEM_CLASS}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <GitFork className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span>Fork</span>
          </span>
        </DropdownMenuItem>

        {isGitRepo && (
          <DropdownMenuItem
            onSelect={() => {
              void orbitActions.forkAgent({
                hostId: host.host_id,
                sourceAgentId: agent.agent_id,
                worktree: true,
              });
            }}
            className={COMPACT_MENU_ITEM_CLASS}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <GitBranch className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>Fork in worktree</span>
            </span>
          </DropdownMenuItem>
        )}

        {activeWs?.is_worktree && (
          <DropdownMenuItem
            onSelect={() => {
              onClose();
              orbitActions.openWorktreeHandoffDialog({
                hostId: host.host_id,
                workspaceId: activeWs.workspace_id,
                direction: "worktree_to_local",
                defaultMode: "apply",
                sourceCwd: activeWs.cwd,
              });
            }}
            className={COMPACT_MENU_ITEM_CLASS}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <ArrowLeftRight className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>Apply to Local…</span>
            </span>
          </DropdownMenuItem>
        )}

        <DropdownMenuSub>
          <DropdownMenuSubTrigger className={COMPACT_MENU_ITEM_CLASS}>
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Layers className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>Group</span>
            </span>
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent className="w-max max-w-[240px] min-w-[172px] rounded-[12px] p-1">
            {otherWorkspaces.length > 0 && (
              <>
                <DropdownMenuLabel className="px-2 py-1 text-[11px] font-medium">
                  Move to
                </DropdownMenuLabel>
                {otherWorkspaces.map((ws) => (
                  <DropdownMenuItem
                    key={ws.workspace_id}
                    onSelect={() => {
                      void orbitActions.moveAgentToWorkspace(
                        host.host_id,
                        agent.agent_id,
                        ws.workspace_id,
                      );
                    }}
                    className={COMPACT_MENU_ITEM_CLASS}
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <FolderInput className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                      <span className="truncate">{ws.title}</span>
                    </span>
                  </DropdownMenuItem>
                ))}
                <DropdownMenuSeparator className="my-1" />
              </>
            )}

            {onRequestCreateWorkspace && (
              <DropdownMenuItem
                onSelect={() => {
                  onClose();
                  onRequestCreateWorkspace({
                    hostId: host.host_id,
                    projectRoot,
                    projectName: projectRoot.split("/").filter(Boolean).pop() || projectRoot,
                    isGitRepo: Boolean(activeWs?.git_branch),
                    moveAgentId: agent.agent_id,
                  });
                }}
                className={COMPACT_MENU_ITEM_CLASS}
              >
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <FolderPlus className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                  <span>New group</span>
                </span>
              </DropdownMenuItem>
            )}

            {!isCuratedWs && (
              <DropdownMenuItem
                onSelect={() => {
                  void orbitActions.organizeProjectWorkspaces(
                    host.host_id,
                    projectRoot,
                    [agent.agent_id],
                    true,
                  );
                }}
                className={COMPACT_MENU_ITEM_CLASS}
              >
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <Sparkles className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                  <span>Assign automatically</span>
                </span>
              </DropdownMenuItem>
            )}

            {isCuratedWs && (
              <DropdownMenuItem
                onSelect={() => {
                  orbitActions.openWorkspaceBoardTab(
                    host.host_id,
                    agent.workspace_id,
                    activeWs?.title,
                  );
                }}
                className={COMPACT_MENU_ITEM_CLASS}
              >
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <FileText className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                  <span>Group notes</span>
                </span>
              </DropdownMenuItem>
            )}

            {isCuratedWs && (
              <>
                <DropdownMenuSeparator className="my-1" />
                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.moveAgentToWorkspace(host.host_id, agent.agent_id, null, {
                      makeUnclassified: true,
                    });
                  }}
                  className={COMPACT_MENU_ITEM_CLASS}
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <Inbox className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                    <span>Remove from group</span>
                  </span>
                </DropdownMenuItem>
              </>
            )}
          </DropdownMenuSubContent>
        </DropdownMenuSub>

        <DropdownMenuSub>
          <DropdownMenuSubTrigger className={COMPACT_MENU_ITEM_CLASS}>
            <span className="flex items-center gap-2 whitespace-nowrap">
              <MoreHorizontal className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>More</span>
            </span>
          </DropdownMenuSubTrigger>
          <DropdownMenuSubContent className="w-max min-w-[172px] rounded-[12px] p-1">
            <DropdownMenuItem
              onSelect={() => {
                void orbitActions.spawnWorkspacePeer(
                  {
                    host_id: host.host_id,
                    workspace_id: agent.workspace_id,
                    cwd: agent.cwd,
                    provider: altProvider,
                    mode: "plan",
                    handle: "reviewer",
                    initial_prompt: `Join this group alongside @${tabLabel}. Read BOARD.md and inspect recent changes to provide a concise code review.`,
                  },
                  { openInSplit: true },
                );
              }}
              className={COMPACT_MENU_ITEM_CLASS}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <UserPlus className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                <span>Add reviewer</span>
              </span>
            </DropdownMenuItem>

            <DropdownMenuItem
              onSelect={() => {
                void orbitActions.summarizeAgent(host.host_id, agent.agent_id);
              }}
              className={COMPACT_MENU_ITEM_CLASS}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <Sparkles className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                <span>Summarize</span>
              </span>
            </DropdownMenuItem>

            <DropdownMenuSeparator className="my-1" />

            <DropdownMenuItem
              onSelect={() => {
                onClose();
                orbitActions.openExportDialog(host.host_id, agent.agent_id);
              }}
              className={COMPACT_MENU_ITEM_CLASS}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <Download className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                <span>Export…</span>
              </span>
            </DropdownMenuItem>

            <DropdownMenuItem
              onSelect={() => {
                void orbitActions.copyThread(host.host_id, agent.agent_id);
              }}
              className={COMPACT_MENU_ITEM_CLASS}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <Copy className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                <span>Copy transcript</span>
              </span>
            </DropdownMenuItem>

            <DropdownMenuItem
              onSelect={() => {
                void navigator.clipboard.writeText(agent.agent_id).then(() => {
                  orbitActions.showToast(`Copied ID ${agent.agent_id}`);
                });
              }}
              className={COMPACT_MENU_ITEM_CLASS}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <Hash className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                <span>Copy ID</span>
              </span>
            </DropdownMenuItem>
          </DropdownMenuSubContent>
        </DropdownMenuSub>

        <DropdownMenuSeparator className="my-1" />

        {agent.parent_agent_id && (
          <DropdownMenuItem
            onSelect={() => {
              void orbitActions.detachAgent(host.host_id, agent.agent_id);
            }}
            className={COMPACT_MENU_ITEM_CLASS}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Unlink className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>Detach</span>
            </span>
          </DropdownMenuItem>
        )}

        <DropdownMenuItem
          onSelect={() => orbitActions.archiveThread(host.host_id, agent.agent_id)}
          className={COMPACT_MENU_ITEM_CLASS}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Archive className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span>Archive</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => {
            onClose();
            onRequestDelete({ host, agent, title: effectiveTitle });
          }}
          className={COMPACT_MENU_ITEM_CLASS}
        >
          <span className="flex items-center gap-2 whitespace-nowrap text-rose-600 dark:text-rose-400">
            <Trash2 className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
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
  onStartRenameProject,
  onRequestDelete,
  onRequestCreateWorkspace,
}: {
  target: ActiveProjectMenuState | null;
  isMac: boolean;
  onClose: () => void;
  onStartRenameProject: (project: SidebarProjectGroupView) => void;
  onRequestDelete: (project: SidebarProjectGroupView) => void;
  onRequestCreateWorkspace?: (req: CreateWorkspaceDialogTarget) => void;
}) {
  if (!target) return null;
  const { project: proj, host, anchor } = target;
  const isLocal = isLocalHostCard(host);
  const isDisconnected = host.connection_state === "disconnected";
  const canAutoGroup =
    (proj.unclassified_conversations ?? proj.conversations).filter(
      (c) => !c.agent.background && !c.agent.archived_at_rfc3339,
    ).length >= 2;

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
        className="w-max min-w-[180px] rounded-[12px] p-1"
      >
        <DropdownMenuItem
          onSelect={() => {
            onClose();
            window.requestAnimationFrame(() => onStartRenameProject(proj));
          }}
          className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Pencil className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span>Rename</span>
          </span>
        </DropdownMenuItem>

        {isLocal && (
          <DropdownMenuItem
            onSelect={() => {
              void orbitIpc.openInEditor("file_manager", proj.primary_cwd, null);
            }}
            className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <FolderOpen className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>
                {isMac
                  ? "Reveal in Finder"
                  : isWindowsDesktop()
                    ? "Reveal in File Explorer"
                    : "Open folder"}
              </span>
            </span>
          </DropdownMenuItem>
        )}

        <DropdownMenuItem
          onSelect={() => {
            void navigator.clipboard.writeText(proj.primary_cwd);
            orbitActions.showToast("Copied project path");
          }}
          className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Copy className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span>Copy path</span>
          </span>
        </DropdownMenuItem>

        {!isLocal && isDisconnected && (
          <DropdownMenuItem
            onSelect={() => {
              void orbitIpc.connectHost(host.host_id);
              void orbitIpc.refreshHost(host.host_id);
            }}
            className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <RefreshCw className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>Reconnect</span>
            </span>
          </DropdownMenuItem>
        )}

        {(onRequestCreateWorkspace || canAutoGroup) && <DropdownMenuSeparator className="my-1" />}

        {onRequestCreateWorkspace && (
          <DropdownMenuItem
            onSelect={() => {
              onClose();
              onRequestCreateWorkspace({
                hostId: proj.host_id,
                projectRoot: proj.primary_cwd,
                projectName: proj.name,
                isGitRepo: Boolean(proj.is_git_repo),
              });
            }}
            className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <FolderPlus className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>New thread group</span>
            </span>
          </DropdownMenuItem>
        )}

        {canAutoGroup && (
          <DropdownMenuItem
            onSelect={() => {
              void orbitActions.organizeProjectWorkspaces(proj.host_id, proj.primary_cwd, [], true);
            }}
            className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Sparkles className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <span>Organize threads</span>
            </span>
          </DropdownMenuItem>
        )}

        <DropdownMenuSeparator className="my-1" />

        <DropdownMenuItem
          onSelect={() => {
            onClose();
            onRequestDelete(proj);
          }}
          className="h-[28px] rounded-[7px] px-2 py-0 text-[12.5px]"
        >
          <span className="flex items-center gap-2 whitespace-nowrap text-rose-600 dark:text-rose-400">
            <Trash2 className="h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span>Delete</span>
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

export function DeleteConversationDialog({
  target,
  onClose,
}: {
  target: DeleteConversationTarget | null;
  onClose: () => void;
}) {
  const [deleting, setDeleting] = useState(false);

  const isRunning = Boolean(
    target &&
      (isAgentForegroundRunning(target.agent) ||
        isAgentBackgroundWaiting(target.agent) ||
        target.agent.is_awaiting_permission),
  );

  const handleConfirm = async () => {
    if (!target || deleting) return;
    setDeleting(true);
    try {
      await orbitActions.deleteThreadPermanently(target.host.host_id, target.agent.agent_id);
      onClose();
    } catch {
      orbitActions.showToast("Failed to delete conversation");
    } finally {
      setDeleting(false);
    }
  };

  return (
    <Dialog
      open={Boolean(target)}
      onOpenChange={(open) => {
        if (!open && !deleting) onClose();
      }}
    >
      <DialogContent className="max-w-md gap-4 p-5 select-none">
        <DialogHeader className="gap-1.5 pr-6">
          <DialogTitle className="text-[15.5px]">Delete Conversation</DialogTitle>
          <DialogDescription className="text-foreground/80 text-[13.5px] leading-relaxed">
            Are you sure you want to permanently delete{" "}
            <strong className="text-foreground font-semibold">{target?.title ?? ""}</strong>? This
            action cannot be undone.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 text-[13px] leading-relaxed">
          {isRunning && (
            <p className="text-amber-600 dark:text-amber-400">
              This conversation is currently running and will be stopped first.
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
          <DialogTitle className="text-[15.5px]">Delete Project &amp; Threads</DialogTitle>
          <DialogDescription className="text-foreground/80 text-[13.5px] leading-relaxed">
            Are you sure you want to permanently delete{" "}
            <strong className="text-foreground font-semibold">{project?.name ?? ""}</strong> from
            Orbit?
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

/**
 * Shared Context Menu for Curated Mission Workspaces in `FleetSidebar`.
 */
export function SharedWorkspaceMenu({
  target,
  onClose,
  onStartRenameWorkspace,
}: {
  target: ActiveWorkspaceMenuState | null;
  onClose: () => void;
  onStartRenameWorkspace: (workspaceId: string) => void;
}) {
  const wsSummary = useOrbitSelector((s) =>
    target
      ? (s.hostDetails[target.project.host_id]?.workspaces.find(
          (w) => w.workspace_id === target.workspace.workspace_id,
        ) ?? null)
      : null,
  );
  if (!target) return null;
  const { workspace: ws, project: proj, anchor } = target;
  const isWorktreeWs = Boolean(wsSummary?.is_worktree);
  const canHandoff = isWorktreeWs || Boolean(proj.is_git_repo);
  const siblingWorkspaces = (proj.workspaces ?? []).filter(
    (w) => w.workspace_id !== ws.workspace_id && w.is_curated !== false,
  );

  return (
    <DropdownMenu
      key={`${ws.workspace_id}:${anchor.left}:${anchor.top}`}
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
        className="w-56 p-1.5"
      >
        <DropdownMenuItem
          onSelect={() => {
            onClose();
            orbitActions.startNewThreadDraft(
              proj.host_id,
              ws.cwd || proj.primary_cwd,
              undefined,
              undefined,
              ws.workspace_id,
            );
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Plus className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>New thread in group</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => {
            onClose();
            window.requestAnimationFrame(() => onStartRenameWorkspace(ws.workspace_id));
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Pencil className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Rename group</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => {
            onClose();
            orbitActions.openWorkspaceBoardTab(proj.host_id, ws.workspace_id, ws.title);
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <FileText className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Group notes</span>
          </span>
        </DropdownMenuItem>

        {canHandoff && (
          <DropdownMenuItem
            onSelect={() => {
              onClose();
              orbitActions.openWorktreeHandoffDialog({
                hostId: proj.host_id,
                workspaceId: ws.workspace_id,
                direction: isWorktreeWs ? "worktree_to_local" : "local_to_worktree",
                defaultMode: isWorktreeWs ? "apply" : "move",
                sourceCwd: wsSummary?.cwd || ws.cwd || proj.primary_cwd,
              });
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <ArrowLeftRight className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
              <span>{isWorktreeWs ? "Apply to Local…" : "Move to Worktree…"}</span>
            </span>
          </DropdownMenuItem>
        )}

        {siblingWorkspaces.length > 0 && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuLabel className="text-[10.5px]">Merge into</DropdownMenuLabel>
            {siblingWorkspaces.slice(0, 6).map((other) => (
              <DropdownMenuItem
                key={other.workspace_id}
                onSelect={() => {
                  void orbitActions.mergeWorkspaces(
                    proj.host_id,
                    ws.workspace_id,
                    other.workspace_id,
                  );
                }}
              >
                <span className="flex min-w-0 items-center gap-2">
                  <GitMerge className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                  <span className="truncate">{other.title}</span>
                </span>
              </DropdownMenuItem>
            ))}
          </>
        )}

        <DropdownMenuSeparator />

        <DropdownMenuItem
          onSelect={() => {
            void orbitActions.archiveWorkspace(proj.host_id, ws.workspace_id, true);
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap">
            <Inbox className="text-muted-foreground h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Ungroup threads</span>
          </span>
        </DropdownMenuItem>

        <DropdownMenuItem
          onSelect={() => {
            void orbitActions.archiveWorkspace(proj.host_id, ws.workspace_id, false);
          }}
        >
          <span className="flex items-center gap-2 whitespace-nowrap text-rose-600 dark:text-rose-400">
            <Archive className="h-[14px] w-[14px] shrink-0 stroke-[1.75]" />
            <span>Archive group & threads</span>
          </span>
        </DropdownMenuItem>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

/**
 * Dialog for manually creating a curated Mission Workspace (and optionally moving an agent into it).
 */
export function CreateWorkspaceDialog({
  target,
  onClose,
}: {
  target: CreateWorkspaceDialogTarget | null;
  onClose: () => void;
}) {
  const [title, setTitle] = useState("");
  const [goal, setGoal] = useState("");
  const [isolation, setIsolation] = useState<"local" | "worktree">("local");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (target) {
      setTitle("");
      setGoal("");
      setIsolation("local");
    }
  }, [target]);

  const handleSubmit = async () => {
    if (!target || !title.trim() || submitting) return;
    setSubmitting(true);
    try {
      if (target.moveAgentId) {
        await orbitActions.moveAgentToWorkspace(target.hostId, target.moveAgentId, null, {
          newWorkspaceTitle: title.trim(),
        });
      } else {
        await orbitActions.createMissionWorkspace({
          hostId: target.hostId,
          sourceCwd: target.projectRoot,
          title: title.trim(),
          missionGoal: goal.trim() || null,
          isolation: target.isGitRepo ? isolation : "local",
        });
      }
      onClose();
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog
      open={Boolean(target)}
      onOpenChange={(open) => {
        if (!open && !submitting) onClose();
      }}
    >
      <DialogContent className="max-w-md gap-4 p-5 select-none">
        <DialogHeader className="gap-1.5 pr-6">
          <DialogTitle className="flex items-center gap-2 text-[15.5px]">
            <Layers className="text-muted-foreground h-[16px] w-[16px]" />
            <span>{target?.moveAgentId ? "New group" : "Create group"}</span>
          </DialogTitle>
          <DialogDescription className="text-muted-foreground text-[12.5px]">
            Project: <span className="text-foreground font-medium">{target?.projectName}</span> ·
            Threads in the same group share context and notes automatically.
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-3">
          <div className="space-y-1">
            <label className="text-foreground/85 text-[12px] font-medium">Name</label>
            <input
              autoFocus
              value={title}
              onChange={(e) => setTitle(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && title.trim()) {
                  e.preventDefault();
                  void handleSubmit();
                }
              }}
              placeholder="e.g. Payments refactor, Release v0.8"
              className="border-border bg-canvas text-foreground focus:ring-foreground/25 w-full rounded-lg border px-3 py-2 text-[13px] outline-none focus:ring-1"
            />
          </div>

          {!target?.moveAgentId && (
            <div className="space-y-1">
              <label className="text-foreground/85 text-[12px] font-medium">
                Goal (optional)
              </label>
              <input
                value={goal}
                onChange={(e) => setGoal(e.target.value)}
                onKeyDown={(e) => {
                  if (e.key === "Enter" && title.trim()) {
                    e.preventDefault();
                    void handleSubmit();
                  }
                }}
                placeholder="One-line goal shared across collaborating threads…"
                className="border-border bg-canvas text-foreground focus:ring-foreground/25 w-full rounded-lg border px-3 py-2 text-[12.5px] outline-none focus:ring-1"
              />
            </div>
          )}

          {!target?.moveAgentId && target?.isGitRepo && (
            <div className="space-y-1">
              <label className="text-foreground/85 text-[12px] font-medium">Directory</label>
              <div className="grid grid-cols-2 gap-2">
                <button
                  type="button"
                  onClick={() => setIsolation("local")}
                  className={
                    isolation === "local"
                      ? "border-foreground/40 bg-foreground/[0.06] text-foreground flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-left text-[12.5px] font-medium"
                      : "border-border text-muted-foreground hover:text-foreground flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-left text-[12.5px]"
                  }
                >
                  <Laptop className="h-[14px] w-[14px] shrink-0" />
                  <span>Local</span>
                </button>
                <button
                  type="button"
                  onClick={() => setIsolation("worktree")}
                  className={
                    isolation === "worktree"
                      ? "border-foreground/40 bg-foreground/[0.06] text-foreground flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-left text-[12.5px] font-medium"
                      : "border-border text-muted-foreground hover:text-foreground flex cursor-pointer items-center gap-2 rounded-lg border px-3 py-2 text-left text-[12.5px]"
                  }
                >
                  <GitBranch className="h-[14px] w-[14px] shrink-0" />
                  <span>Git Worktree</span>
                </button>
              </div>
            </div>
          )}
        </div>

        <div className="flex justify-end gap-2 pt-1">
          <Button
            type="button"
            variant="secondary"
            size="sm"
            disabled={submitting}
            onClick={onClose}
          >
            Cancel
          </Button>
          <Button
            type="button"
            size="sm"
            disabled={!title.trim() || submitting}
            onClick={() => void handleSubmit()}
          >
            {submitting ? (
              <>
                <Loader2 className="h-[13.5px] w-[13.5px] animate-spin" />
                <span>Creating…</span>
              </>
            ) : (
              "Create"
            )}
          </Button>
        </div>
      </DialogContent>
    </Dialog>
  );
}
