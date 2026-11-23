import { useState } from "react";
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
  Layers,
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
import { useI18n } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
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
  DropdownMenuGroup,
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

export interface CreateWorkspaceRequest {
  projectId?: string;
  hostId: string;
  projectRoot: string;
  moveAgentId?: string | null;
}

export interface DeleteConversationTarget {
  host: HostCardView;
  agent: AgentCardView;
  title: string;
}

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
  onRequestCreateWorkspace?: (req: CreateWorkspaceRequest) => void;
  onRequestDelete: (target: DeleteConversationTarget) => void;
}) {
  const { t } = useI18n();
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

      <DropdownMenuContent align="start" sideOffset={4} finalFocus={false}>
        <DropdownMenuGroup>
          <DropdownMenuItem
            onSelect={() => {
              onClose();
              window.requestAnimationFrame(() => onStartRename(threadKey));
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Pencil className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("common.rename")}</span>
            </span>
          </DropdownMenuItem>

          <DropdownMenuItem
            onSelect={() => orbitActions.togglePinThread(host.host_id, agent.agent_id)}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              {isPinned ? (
                <PinOff className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              ) : (
                <Pin className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              )}
              <span>{isPinned ? t("sidebar.unpin") : t("sidebar.pin")}</span>
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
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                {isUnread ? (
                  <CheckCircle2 className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                ) : (
                  <Circle className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                )}
                <span>{isUnread ? t("sidebar.markRead") : t("sidebar.markUnread")}</span>
              </span>
            </DropdownMenuItem>
          )}
        </DropdownMenuGroup>

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          <DropdownMenuItem
            onSelect={() => {
              orbitActions.openPeerSplitTab(host.host_id, agent.agent_id, tabLabel);
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Columns2 className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("sidebar.openSplit")}</span>
            </span>
          </DropdownMenuItem>

          {isGitRepo ? (
            <DropdownMenuSub>
              <DropdownMenuSubTrigger>
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <GitFork className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                  <span>{t("sidebar.fork")}</span>
                </span>
              </DropdownMenuSubTrigger>
              <DropdownMenuSubContent>
                <DropdownMenuGroup>
                  <DropdownMenuItem
                    onSelect={() => {
                      void orbitActions.forkAgent({
                        hostId: host.host_id,
                        sourceAgentId: agent.agent_id,
                      });
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <GitFork className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("sidebar.forkHere")}</span>
                    </span>
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() => {
                      void orbitActions.forkAgent({
                        hostId: host.host_id,
                        sourceAgentId: agent.agent_id,
                        worktree: true,
                      });
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <GitBranch className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("sidebar.forkInNewWorktree")}</span>
                    </span>
                  </DropdownMenuItem>
                </DropdownMenuGroup>
              </DropdownMenuSubContent>
            </DropdownMenuSub>
          ) : (
            <DropdownMenuItem
              onSelect={() => {
                void orbitActions.forkAgent({
                  hostId: host.host_id,
                  sourceAgentId: agent.agent_id,
                });
              }}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <GitFork className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                <span>{t("sidebar.fork")}</span>
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
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <ArrowLeftRight className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                <span>{t("topbar.applyToLocal")}</span>
              </span>
            </DropdownMenuItem>
          )}

          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <span className="flex items-center gap-2 whitespace-nowrap">
                <Layers className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                <span>{t("sidebar.group")}</span>
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              {otherWorkspaces.length > 0 && (
                <>
                  <DropdownMenuGroup>
                    <DropdownMenuLabel>{t("sidebar.moveTo")}</DropdownMenuLabel>
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
                      >
                        <span className="flex min-w-0 items-center gap-2">
                          <FolderInput className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                          <span className="truncate">{ws.title}</span>
                        </span>
                      </DropdownMenuItem>
                    ))}
                  </DropdownMenuGroup>
                  <DropdownMenuSeparator />
                </>
              )}

              <DropdownMenuGroup>
                {onRequestCreateWorkspace && (
                  <DropdownMenuItem
                    onSelect={() => {
                      onClose();
                      onRequestCreateWorkspace({
                        hostId: host.host_id,
                        projectRoot,
                        moveAgentId: agent.agent_id,
                      });
                    }}
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <FolderPlus className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("sidebar.newGroup")}</span>
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
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <Sparkles className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("sidebar.assignAutomatically")}</span>
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
                  >
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <FileText className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("sidebar.groupNotes")}</span>
                    </span>
                  </DropdownMenuItem>
                )}
              </DropdownMenuGroup>

              {isCuratedWs && (
                <>
                  <DropdownMenuSeparator />
                  <DropdownMenuGroup>
                    <DropdownMenuItem
                      onSelect={() => {
                        void orbitActions.moveAgentToWorkspace(host.host_id, agent.agent_id, null, {
                          makeUnclassified: true,
                        });
                      }}
                    >
                      <span className="flex items-center gap-2 whitespace-nowrap">
                        <Inbox className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                        <span>{t("sidebar.removeFromGroup")}</span>
                      </span>
                    </DropdownMenuItem>
                  </DropdownMenuGroup>
                </>
              )}
            </DropdownMenuSubContent>
          </DropdownMenuSub>

          <DropdownMenuSub>
            <DropdownMenuSubTrigger>
              <span className="flex items-center gap-2 whitespace-nowrap">
                <MoreHorizontal className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                <span>{t("common.more")}</span>
              </span>
            </DropdownMenuSubTrigger>
            <DropdownMenuSubContent>
              <DropdownMenuGroup>
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
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <UserPlus className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>{t("sidebar.addReviewer")}</span>
                  </span>
                </DropdownMenuItem>

                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.summarizeAgent(host.host_id, agent.agent_id);
                  }}
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <Sparkles className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>{t("topbar.summarize")}</span>
                  </span>
                </DropdownMenuItem>
              </DropdownMenuGroup>

              <DropdownMenuSeparator />

              <DropdownMenuGroup>
                <DropdownMenuItem
                  onSelect={() => {
                    onClose();
                    orbitActions.openExportDialog(host.host_id, agent.agent_id);
                  }}
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <Download className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>{t("topbar.export")}</span>
                  </span>
                </DropdownMenuItem>

                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.copyThread(host.host_id, agent.agent_id);
                  }}
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <Copy className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>{t("topbar.copyTranscript")}</span>
                  </span>
                </DropdownMenuItem>

                <DropdownMenuItem
                  onSelect={() => {
                    void navigator.clipboard.writeText(agent.agent_id).then(() => {
                      orbitActions.showToast(t("sidebar.copiedId", { id: agent.agent_id }));
                    });
                  }}
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <Hash className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>{t("sidebar.copyId")}</span>
                  </span>
                </DropdownMenuItem>
              </DropdownMenuGroup>
            </DropdownMenuSubContent>
          </DropdownMenuSub>
        </DropdownMenuGroup>

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          {agent.parent_agent_id && (
            <DropdownMenuItem
              onSelect={() => {
                void orbitActions.detachAgent(host.host_id, agent.agent_id);
              }}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <Unlink className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                <span>{t("sidebar.detach")}</span>
              </span>
            </DropdownMenuItem>
          )}

          <DropdownMenuItem
            onSelect={() => orbitActions.archiveThread(host.host_id, agent.agent_id)}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Archive className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("common.archive")}</span>
            </span>
          </DropdownMenuItem>

          <DropdownMenuItem
            variant="destructive"
            onSelect={() => {
              onClose();
              onRequestDelete({ host, agent, title: effectiveTitle });
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Trash2 className="size-[13.5px] stroke-[1.75]" />
              <span>{t("common.delete")}</span>
            </span>
          </DropdownMenuItem>
        </DropdownMenuGroup>
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
  onRequestCreateWorkspace?: (req: CreateWorkspaceRequest) => void;
}) {
  const { t } = useI18n();
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

      <DropdownMenuContent align="start" sideOffset={4} finalFocus={false}>
        <DropdownMenuGroup>
          <DropdownMenuItem
            onSelect={() => {
              onClose();
              window.requestAnimationFrame(() => onStartRenameProject(proj));
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Pencil className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("common.rename")}</span>
            </span>
          </DropdownMenuItem>

          {isLocal && (
            <DropdownMenuItem
              onSelect={() => {
                void orbitIpc.openInEditor("file_manager", proj.primary_cwd, null);
              }}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <FolderOpen className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                <span>
                  {isMac
                    ? t("sidebar.revealInFinder")
                    : isWindowsDesktop()
                      ? t("sidebar.revealInExplorer")
                      : t("sidebar.openFolder")}
                </span>
              </span>
            </DropdownMenuItem>
          )}

          <DropdownMenuItem
            onSelect={() => {
              void navigator.clipboard.writeText(proj.primary_cwd);
              orbitActions.showToast(t("sidebar.copiedProjectPath"));
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Copy className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("sidebar.copyPath")}</span>
            </span>
          </DropdownMenuItem>

          {!isLocal && isDisconnected && (
            <DropdownMenuItem
              onSelect={() => {
                void orbitIpc.connectHost(host.host_id);
                void orbitIpc.refreshHost(host.host_id);
              }}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <RefreshCw className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                <span>{t("banner.reconnect")}</span>
              </span>
            </DropdownMenuItem>
          )}
        </DropdownMenuGroup>

        {(onRequestCreateWorkspace || canAutoGroup) && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
              {onRequestCreateWorkspace && (
                <DropdownMenuItem
                  onSelect={() => {
                    onClose();
                    onRequestCreateWorkspace({
                      projectId: proj.id,
                      hostId: proj.host_id,
                      projectRoot: proj.primary_cwd,
                    });
                  }}
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <FolderPlus className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>{t("sidebar.newThreadGroup")}</span>
                  </span>
                </DropdownMenuItem>
              )}

              {canAutoGroup && (
                <DropdownMenuItem
                  onSelect={() => {
                    void orbitActions.organizeProjectWorkspaces(
                      proj.host_id,
                      proj.primary_cwd,
                      [],
                      true,
                    );
                  }}
                >
                  <span className="flex items-center gap-2 whitespace-nowrap">
                    <Sparkles className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>{t("sidebar.organizeThreads")}</span>
                  </span>
                </DropdownMenuItem>
              )}
            </DropdownMenuGroup>
          </>
        )}

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          <DropdownMenuItem
            variant="destructive"
            onSelect={() => {
              onClose();
              onRequestDelete(proj);
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Trash2 className="size-[13.5px] stroke-[1.75]" />
              <span>{t("common.delete")}</span>
            </span>
          </DropdownMenuItem>
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}

function formatConversationDeleteSummary(
  activeCount: number,
  archivedCount: number,
  t: ReturnType<typeof useI18n>["t"],
): string {
  const parts: string[] = [];
  if (activeCount > 0) {
    parts.push(
      activeCount === 1
        ? t("sidebar.deleteProjectActiveOne")
        : t("sidebar.deleteProjectActiveMany", { count: activeCount }),
    );
  }
  if (archivedCount > 0) {
    parts.push(
      archivedCount === 1
        ? t("sidebar.deleteProjectArchivedOne")
        : t("sidebar.deleteProjectArchivedMany", { count: archivedCount }),
    );
  }
  if (parts.length === 2) {
    return t("sidebar.deleteProjectSummaryJoin", { a: parts[0], b: parts[1] });
  }
  return parts[0] ?? "";
}

export function DeleteConversationDialog({
  target,
  onClose,
}: {
  target: DeleteConversationTarget | null;
  onClose: () => void;
}) {
  const { t } = useI18n();
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
      orbitActions.showToast(t("sidebar.failedDeleteConversation"));
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
          <DialogTitle className="text-[15.5px]">
            {t("sidebar.deleteConversationTitle")}
          </DialogTitle>
          <DialogDescription className="text-ink-secondary text-[13px] leading-relaxed">
            {t("sidebar.deleteConversationConfirm", { title: target?.title ?? "" })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 text-[12.5px] leading-relaxed">
          {isRunning && (
            <p className="text-signal-warning">{t("sidebar.deleteConversationRunningWarn")}</p>
          )}
          <p className="text-ink-muted">{t("sidebar.filesOnDiskUntouched")}</p>
        </div>

        <div className="flex justify-end gap-2 pt-1">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={deleting}
            onClick={onClose}
          >
            {t("common.cancel")}
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
                <OrbitSpinner size={13} />
                <span>{t("sidebar.deleting")}</span>
              </>
            ) : (
              t("common.delete")
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
  const { t } = useI18n();
  const [deleting, setDeleting] = useState(false);

  let activeCount = 0;
  let archivedCount = 0;
  let runningCount = 0;
  if (project) {
    for (const { agent } of project.conversations) {
      if (agent.background) continue;
      const isArchived = Boolean(
        agent.archived_at_rfc3339 ||
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
  const conversationSummary = formatConversationDeleteSummary(activeCount, archivedCount, t);

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
          <DialogTitle className="text-[15.5px]">{t("sidebar.deleteProjectTitle")}</DialogTitle>
          <DialogDescription className="text-ink-secondary text-[13px] leading-relaxed">
            {t("sidebar.deleteProjectConfirm", { project: project?.name ?? "" })}
          </DialogDescription>
        </DialogHeader>

        <div className="space-y-2 text-[12.5px] leading-relaxed">
          {conversationSummary && (
            <p className="text-ink-secondary">
              {t("sidebar.deleteProjectSummary", { summary: conversationSummary })}
            </p>
          )}
          {runningCount > 0 && (
            <p className="text-signal-warning">
              {runningCount === 1
                ? t("sidebar.deleteProjectRunningOne")
                : t("sidebar.deleteProjectRunningMany", { count: runningCount })}
            </p>
          )}
          <p className="text-ink-muted">{t("sidebar.filesOnDiskUntouched")}</p>
        </div>

        <div className="flex justify-end gap-2 pt-1">
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={deleting}
            onClick={onClose}
          >
            {t("common.cancel")}
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
                <OrbitSpinner size={13} />
                <span>{t("sidebar.deleting")}</span>
              </>
            ) : (
              t("common.delete")
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
  const { t } = useI18n();
  const hostDetail = useOrbitSelector((s) =>
    target ? (s.hostDetails[target.project.host_id] ?? null) : null,
  );
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
  const activeThreadCount = hostDetail
    ? hostDetail.agents.filter(
        (a) =>
          a.workspace_id === ws.workspace_id &&
          !a.archived_at_rfc3339 &&
          !a.background &&
          !a.parent_agent_id,
      ).length
    : ws.conversations.length || ws.peer_count;
  const hasThreads = activeThreadCount > 0;

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

      <DropdownMenuContent align="start" sideOffset={4} finalFocus={false}>
        <DropdownMenuGroup>
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
              <Plus className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("sidebar.groupNewConversation")}</span>
            </span>
          </DropdownMenuItem>

          <DropdownMenuItem
            onSelect={() => {
              onClose();
              window.requestAnimationFrame(() => onStartRenameWorkspace(ws.workspace_id));
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <Pencil className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("common.rename")}</span>
            </span>
          </DropdownMenuItem>

          <DropdownMenuItem
            onSelect={() => {
              onClose();
              orbitActions.openWorkspaceBoardTab(proj.host_id, ws.workspace_id, ws.title);
            }}
          >
            <span className="flex items-center gap-2 whitespace-nowrap">
              <FileText className="text-ink-muted size-[13.5px] stroke-[1.75]" />
              <span>{t("sidebar.groupNotes")}</span>
            </span>
          </DropdownMenuItem>
        </DropdownMenuGroup>

        {(canHandoff || siblingWorkspaces.length > 0) && (
          <>
            <DropdownMenuSeparator />
            <DropdownMenuGroup>
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
                    <ArrowLeftRight className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                    <span>
                      {isWorktreeWs ? t("topbar.applyToLocal") : t("sidebar.moveToWorktree")}
                    </span>
                  </span>
                </DropdownMenuItem>
              )}

              {siblingWorkspaces.length > 0 && (
                <DropdownMenuSub>
                  <DropdownMenuSubTrigger>
                    <span className="flex items-center gap-2 whitespace-nowrap">
                      <GitMerge className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                      <span>{t("sidebar.mergeInto")}</span>
                    </span>
                  </DropdownMenuSubTrigger>
                  <DropdownMenuSubContent>
                    <DropdownMenuGroup>
                      {siblingWorkspaces.slice(0, 8).map((other) => (
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
                            <Layers className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                            <span className="truncate">{other.title}</span>
                          </span>
                        </DropdownMenuItem>
                      ))}
                    </DropdownMenuGroup>
                  </DropdownMenuSubContent>
                </DropdownMenuSub>
              )}
            </DropdownMenuGroup>
          </>
        )}

        <DropdownMenuSeparator />

        <DropdownMenuGroup>
          {hasThreads ? (
            <>
              <DropdownMenuItem
                onSelect={() => {
                  void orbitActions.archiveWorkspace(proj.host_id, ws.workspace_id, true);
                }}
              >
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <Inbox className="text-ink-muted size-[13.5px] stroke-[1.75]" />
                  <span>{t("sidebar.ungroup")}</span>
                </span>
              </DropdownMenuItem>

              <DropdownMenuItem
                variant="destructive"
                onSelect={() => {
                  void orbitActions.archiveWorkspace(proj.host_id, ws.workspace_id, false);
                }}
              >
                <span className="flex items-center gap-2 whitespace-nowrap">
                  <Archive className="size-[13.5px] stroke-[1.75]" />
                  <span>{t("sidebar.archiveAll")}</span>
                </span>
              </DropdownMenuItem>
            </>
          ) : (
            <DropdownMenuItem
              variant="destructive"
              onSelect={() => {
                void orbitActions.archiveWorkspace(proj.host_id, ws.workspace_id, true, {
                  emptyDelete: true,
                });
              }}
            >
              <span className="flex items-center gap-2 whitespace-nowrap">
                <Trash2 className="size-[13.5px] stroke-[1.75]" />
                <span>{t("common.delete")}</span>
              </span>
            </DropdownMenuItem>
          )}
        </DropdownMenuGroup>
      </DropdownMenuContent>
    </DropdownMenu>
  );
}
