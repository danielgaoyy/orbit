import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  Columns2,
  FileText,
  FolderPlus,
  GitBranch,
  Layers,
  Loader2,
  Pencil,
  Plus,
  Users,
  X,
} from "lucide-react";
import {
  formatProviderShortLabel,
  type ProviderKind,
  type SessionMode,
  semanticAgentTabLabel,
} from "@/lib/orbit-ipc";
import { useImeCompositionGuard } from "@/lib/ime";
import { normalizeProjectPath } from "@/lib/projects";
import { ProviderIcon } from "@/components/ui/provider-icons";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { orbitActions, threadScopeKey, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

function providerShortName(provider: ProviderKind): string {
  return formatProviderShortLabel(provider);
}

function pickAlternateProvider(
  current: ProviderKind,
  available: ProviderKind[],
): ProviderKind {
  const direct = available.filter((p) => p !== "acp");
  const alt = direct.find((p) => p !== current);
  if (alt) return alt;
  if (current !== "acp") return current;
  return direct[0] ?? "claude_code";
}

/**
 * Quiet single icon button shown in the main `TopBar` ONLY when the active conversation is a
 * standalone thread (not in a Group and no sibling peers). Lets users add a collaborator or
 * organize the thread into a Group on demand, keeping single-thread chats completely free of
 * extra toolbars.
 */
export const StandaloneGroupButton = memo(function StandaloneGroupButton() {
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const gitReposCache = useOrbitSelector((s) => s.gitReposCache);

  const [open, setOpen] = useState(false);
  const [newGroupTitle, setNewGroupTitle] = useState("");
  const [spawning, setSpawning] = useState(false);
  const ime = useImeCompositionGuard();

  const activeAgent = useMemo(() => {
    if (!hostDetail || !selectedAgentId) return null;
    return hostDetail.agents.find((a) => a.agent_id === selectedAgentId) ?? null;
  }, [hostDetail, selectedAgentId]);

  const workspace = useMemo(() => {
    if (!hostDetail || !activeAgent) return null;
    return hostDetail.workspaces.find((w) => w.workspace_id === activeAgent.workspace_id) ?? null;
  }, [hostDetail, activeAgent]);

  const isGrouped = Boolean(workspace?.is_curated);
  const peerCount = useMemo(() => {
    if (!hostDetail || !activeAgent) return 0;
    return hostDetail.agents.filter(
      (a) =>
        a.workspace_id === activeAgent.workspace_id &&
        !a.archived_at_rfc3339 &&
        !a.background &&
        !a.parent_agent_id,
    ).length;
  }, [hostDetail, activeAgent]);

  const rootCwd =
    workspace?.project_root || workspace?.source_cwd || workspace?.cwd || activeAgent?.cwd || "";
  const normRootCwd = normalizeProjectPath(rootCwd);

  const projectGroups = useMemo(() => {
    if (!hostDetail || !activeAgent) return [];
    return hostDetail.workspaces
      .filter((w) => {
        if (!w.is_curated || w.archived_at || w.background) return false;
        const wRoot = normalizeProjectPath(w.project_root || w.source_cwd || w.cwd);
        const wCwd = normalizeProjectPath(w.cwd);
        return (
          wRoot === normRootCwd ||
          wCwd === normRootCwd ||
          wRoot.startsWith(`${normRootCwd}/`) ||
          normRootCwd.startsWith(`${wRoot}/`)
        );
      })
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  }, [hostDetail, activeAgent, normRootCwd]);

  if (
    workspacePanel !== "conversation" ||
    isDraftNewSession ||
    !selectedHostId ||
    !selectedAgentId ||
    !activeAgent ||
    !hostDetail ||
    activeAgent.parent_agent_id ||
    isGrouped ||
    peerCount > 1
  ) {
    return null;
  }

  const availableProviders =
    hostDetail.host.available_providers.length > 0
      ? hostDetail.host.available_providers
      : ([activeAgent.provider] as ProviderKind[]);

  const gitInsp =
    gitReposCache[`${selectedHostId}::${rootCwd}`] ??
    hostDetail.git_repos_by_cwd?.[rootCwd] ??
    null;
  const canWorktree =
    Boolean(workspace?.git_branch) ||
    Boolean(gitInsp?.is_git_repo) ||
    (gitInsp?.nested_git_repos.length ?? 0) > 0;
  const worktreeCwd =
    gitInsp && !gitInsp.is_git_repo && gitInsp.nested_git_repos.length > 0
      ? gitInsp.nested_git_repos[0].path
      : rootCwd;

  const altProvider = pickAlternateProvider(activeAgent.provider, availableProviders);

  const handleLaunchPreset = async (preset: "reviewer" | "worktree_worker") => {
    if (spawning) return;
    setSpawning(true);
    try {
      const peerLabel = semanticAgentTabLabel(activeAgent);
      if (preset === "reviewer") {
        await orbitActions.spawnWorkspacePeer(
          {
            host_id: selectedHostId,
            workspace_id: activeAgent.workspace_id,
            cwd: activeAgent.cwd,
            provider: altProvider,
            mode: "plan",
            handle: "reviewer",
            initial_prompt: `Review the work in this group (coordinate with @${peerLabel} via workspace_board_read / read_peer_context). Inspect code changes for correctness, edge cases, and test coverage, and write a concise review verdict to BOARD.md.`,
          },
          { openInSplit: true },
        );
      } else {
        await orbitActions.spawnWorkspacePeer(
          {
            host_id: selectedHostId,
            workspace_id: activeAgent.workspace_id,
            cwd: worktreeCwd,
            extra_dirs: worktreeCwd !== rootCwd ? [rootCwd] : [],
            provider: altProvider,
            mode: "accept_edits",
            worktree_branch: "auto",
            handle: "worker",
            initial_prompt: `Join this group in an isolated worktree. Start by reading the shared notes (workspace_board_read) and peer context (@${peerLabel}) so you understand the goal and avoid duplicating work, then report readiness.`,
          },
          { openInSplit: true },
        );
      }
      setOpen(false);
    } catch (err) {
      orbitActions.showToast(`Failed to add collaborator: ${String(err)}`);
    } finally {
      setSpawning(false);
    }
  };

  const handleCreateGroup = async () => {
    const clean = newGroupTitle.trim();
    if (!clean) return;
    await orbitActions.moveAgentToWorkspace(selectedHostId, activeAgent.agent_id, null, {
      newWorkspaceTitle: clean,
    });
    setNewGroupTitle("");
    setOpen(false);
  };

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <SimpleTooltip label="Collaborate or group">
        <PopoverTrigger asChild>
          <button
            type="button"
            aria-label="Collaborate or group"
            className={cn(
              "inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[7px] transition-colors",
              open
                ? "bg-black/[0.06] text-foreground dark:bg-white/[0.085]"
                : "text-foreground/65 hover:text-foreground hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
            )}
          >
            <Users className="h-[15px] w-[15px] stroke-[1.75]" />
          </button>
        </PopoverTrigger>
      </SimpleTooltip>

      <PopoverContent
        align="end"
        side="bottom"
        sideOffset={6}
        finalFocus={false}
        className="w-[276px] space-y-2.5 p-2.5 select-none"
      >
        <div className="space-y-1">
          <div className="text-muted-foreground px-1 text-[11px] font-medium">Collaborate</div>
          <button
            type="button"
            disabled={spawning}
            onClick={() => void handleLaunchPreset("reviewer")}
            className="hover:bg-accent flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left transition-colors disabled:opacity-50"
          >
            <div className="min-w-0">
              <div className="text-foreground text-[12.5px] font-medium">Add code reviewer</div>
              <div className="text-muted-foreground text-[11px]">
                {providerShortName(altProvider)} · read-only review in split pane
              </div>
            </div>
            {spawning ? (
              <Loader2 className="text-muted-foreground h-[13px] w-[13px] shrink-0 animate-spin" />
            ) : (
              <Plus className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
            )}
          </button>

          {canWorktree && (
            <button
              type="button"
              disabled={spawning}
              onClick={() => void handleLaunchPreset("worktree_worker")}
              className="hover:bg-accent flex w-full cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left transition-colors disabled:opacity-50"
            >
              <div className="min-w-0">
                <div className="text-foreground flex items-center gap-1.5 text-[12.5px] font-medium">
                  <GitBranch className="h-[12px] w-[12px] opacity-75" />
                  <span>Parallel worker</span>
                </div>
                <div className="text-muted-foreground text-[11px]">
                  Work in an isolated Git worktree branch
                </div>
              </div>
              <Plus className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
            </button>
          )}
        </div>

        <div className="border-border/70 space-y-1.5 border-t pt-2">
          <div className="text-muted-foreground px-1 text-[11px] font-medium">Move to group</div>
          {projectGroups.length > 0 && (
            <div className="max-h-36 space-y-0.5 overflow-y-auto">
              {projectGroups.map((ws) => (
                <button
                  key={ws.workspace_id}
                  type="button"
                  onClick={() => {
                    void orbitActions.moveAgentToWorkspace(
                      selectedHostId,
                      activeAgent.agent_id,
                      ws.workspace_id,
                    );
                    setOpen(false);
                  }}
                  className="text-foreground/85 hover:bg-accent hover:text-foreground flex w-full cursor-pointer items-center gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors"
                >
                  <Layers className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0" />
                  <span className="truncate">{ws.title}</span>
                </button>
              ))}
            </div>
          )}

          <div className="flex items-center gap-1.5 pt-0.5">
            <input
              value={newGroupTitle}
              onChange={(e) => setNewGroupTitle(e.target.value)}
              {...ime.compositionProps}
              onKeyDown={(e) => {
                if (ime.shouldIgnoreKeyDown(e)) return;
                if (e.key === "Enter") {
                  e.preventDefault();
                  void handleCreateGroup();
                }
              }}
              placeholder="New group name…"
              className="border-border bg-canvas text-foreground h-[26px] min-w-0 flex-1 rounded-md border px-2 text-[12px] outline-none"
            />
            <button
              type="button"
              disabled={!newGroupTitle.trim()}
              onClick={() => void handleCreateGroup()}
              className="bg-foreground text-background inline-flex h-[26px] shrink-0 cursor-pointer items-center rounded-md px-2.5 text-[11.5px] font-medium disabled:opacity-35"
            >
              Create
            </button>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
});

/**
 * Progressive-disclosure contextual strip rendered ONLY when the active conversation belongs to a
 * Group or has multiple collaborating agents (0px for standalone threads and subagent threads).
 */
export const WorkspacePeerStrip = memo(function WorkspacePeerStrip() {
  const workspacePanel = useOrbitSelector((s) => s.workspacePanel);
  const isDraftNewSession = useOrbitSelector((s) => s.isDraftNewSession);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const threadCustomTitles = useOrbitSelector((s) => s.threadCustomTitles);
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const gitReposCache = useOrbitSelector((s) => s.gitReposCache);

  const [groupSwitcherOpen, setGroupSwitcherOpen] = useState(false);
  const [groupRenameDraft, setGroupRenameDraft] = useState("");
  const [newGroupTitle, setNewGroupTitle] = useState("");
  const [editingPeerId, setEditingPeerId] = useState<string | null>(null);
  const [peerHandleDraft, setPeerHandleDraft] = useState("");

  const [spawnOpen, setSpawnOpen] = useState(false);
  const [customPrompt, setCustomPrompt] = useState("");
  const [customHandle, setCustomHandle] = useState("");
  const [customProvider, setCustomProvider] = useState<ProviderKind | null>(null);
  const [customMode, setCustomMode] = useState<SessionMode>("accept_edits");
  const [customWorktree, setCustomWorktree] = useState(false);
  const [spawning, setSpawning] = useState(false);

  const peerInputRef = useRef<HTMLInputElement | null>(null);
  const ime = useImeCompositionGuard();

  const activeAgent = useMemo(() => {
    if (!hostDetail || !selectedAgentId) return null;
    return hostDetail.agents.find((a) => a.agent_id === selectedAgentId) ?? null;
  }, [hostDetail, selectedAgentId]);

  const workspace = useMemo(() => {
    if (!hostDetail || !activeAgent) return null;
    return hostDetail.workspaces.find((w) => w.workspace_id === activeAgent.workspace_id) ?? null;
  }, [hostDetail, activeAgent]);

  const isCuratedGroup = Boolean(workspace?.is_curated);

  const peers = useMemo(() => {
    if (!hostDetail || !activeAgent || activeAgent.parent_agent_id) return [];
    return hostDetail.agents
      .filter(
        (a) =>
          a.workspace_id === activeAgent.workspace_id &&
          !a.archived_at_rfc3339 &&
          !a.background &&
          !a.parent_agent_id,
      )
      .sort((a, b) => a.updated_at_rfc3339.localeCompare(b.updated_at_rfc3339));
  }, [hostDetail, activeAgent]);

  const rootCwd =
    workspace?.project_root || workspace?.source_cwd || workspace?.cwd || activeAgent?.cwd || "";
  const normRootCwd = normalizeProjectPath(rootCwd);

  const projectGroups = useMemo(() => {
    if (!hostDetail || !activeAgent) return [];
    return hostDetail.workspaces
      .filter((w) => {
        if (!w.is_curated || w.archived_at || w.background) return false;
        const wRoot = normalizeProjectPath(w.project_root || w.source_cwd || w.cwd);
        const wCwd = normalizeProjectPath(w.cwd);
        return (
          wRoot === normRootCwd ||
          wCwd === normRootCwd ||
          wRoot.startsWith(`${normRootCwd}/`) ||
          normRootCwd.startsWith(`${wRoot}/`)
        );
      })
      .sort((a, b) => b.updated_at.localeCompare(a.updated_at));
  }, [hostDetail, activeAgent, normRootCwd]);

  const workspaceEvents = useMemo(() => {
    if (!hostDetail || !activeAgent || (!isCuratedGroup && peers.length <= 1)) return [];
    return hostDetail.workspace_events?.[activeAgent.workspace_id] ?? [];
  }, [hostDetail, activeAgent, isCuratedGroup, peers.length]);

  const recentConflicts = useMemo(
    () => workspaceEvents.filter((ev) => ev.kind === "conflict_warning").slice(-3),
    [workspaceEvents],
  );

  useEffect(() => {
    if (groupSwitcherOpen && workspace) {
      setGroupRenameDraft(workspace.title);
    }
  }, [groupSwitcherOpen, workspace]);

  useEffect(() => {
    if (!editingPeerId) return;
    const raf = window.requestAnimationFrame(() => {
      peerInputRef.current?.focus();
      peerInputRef.current?.select();
    });
    return () => window.cancelAnimationFrame(raf);
  }, [editingPeerId]);

  useEffect(() => {
    setEditingPeerId(null);
  }, [selectedAgentId]);

  // Progressive disclosure: standalone single conversations and subagents render 0px here.
  if (
    workspacePanel !== "conversation" ||
    isDraftNewSession ||
    !selectedHostId ||
    !selectedAgentId ||
    !activeAgent ||
    !hostDetail ||
    activeAgent.parent_agent_id ||
    (!isCuratedGroup && peers.length <= 1 && recentConflicts.length === 0)
  ) {
    return null;
  }

  const boardRev = workspace?.board_revision ?? 0;

  const availableProviders =
    hostDetail.host.available_providers.length > 0
      ? hostDetail.host.available_providers
      : ([activeAgent.provider] as ProviderKind[]);

  const gitInsp =
    gitReposCache[`${selectedHostId}::${rootCwd}`] ??
    hostDetail.git_repos_by_cwd?.[rootCwd] ??
    null;
  const canWorktree =
    Boolean(workspace?.git_branch) ||
    Boolean(gitInsp?.is_git_repo) ||
    (gitInsp?.nested_git_repos.length ?? 0) > 0;
  const worktreeCwd =
    gitInsp && !gitInsp.is_git_repo && gitInsp.nested_git_repos.length > 0
      ? gitInsp.nested_git_repos[0].path
      : rootCwd;

  const effectiveCustomProvider =
    customProvider && availableProviders.includes(customProvider)
      ? customProvider
      : pickAlternateProvider(activeAgent.provider, availableProviders);

  const commitPeerRename = (peerId: string) => {
    const clean = peerHandleDraft.trim().replace(/^@+/, "");
    setEditingPeerId(null);
    if (!clean) return;
    void orbitActions.updateAgentIdentity({
      hostId: selectedHostId,
      agentId: peerId,
      handle: clean,
      lockHandle: true,
    });
  };

  const handleRenameCurrentGroup = async () => {
    if (!workspace) return;
    const clean = groupRenameDraft.trim();
    if (!clean || clean === workspace.title) return;
    await orbitActions.renameWorkspace(selectedHostId, workspace.workspace_id, clean);
    setGroupSwitcherOpen(false);
  };

  const handleCreateAndMoveGroup = async () => {
    const clean = newGroupTitle.trim();
    if (!clean) return;
    await orbitActions.moveAgentToWorkspace(selectedHostId, activeAgent.agent_id, null, {
      newWorkspaceTitle: clean,
    });
    setNewGroupTitle("");
    setGroupSwitcherOpen(false);
  };

  const launchPresetPeer = async (preset: "reviewer" | "worktree_worker") => {
    if (spawning) return;
    setSpawning(true);
    try {
      const peerLabel = semanticAgentTabLabel(activeAgent);
      if (preset === "reviewer") {
        await orbitActions.spawnWorkspacePeer(
          {
            host_id: selectedHostId,
            workspace_id: activeAgent.workspace_id,
            cwd: activeAgent.cwd,
            provider: pickAlternateProvider(activeAgent.provider, availableProviders),
            mode: "plan",
            handle: "reviewer",
            initial_prompt: `Review the work in this group (coordinate with @${peerLabel} via workspace_board_read / read_peer_context). Inspect code changes for correctness, edge cases, and test coverage, and write a concise review verdict to BOARD.md.`,
          },
          { openInSplit: true },
        );
      } else {
        await orbitActions.spawnWorkspacePeer(
          {
            host_id: selectedHostId,
            workspace_id: activeAgent.workspace_id,
            cwd: worktreeCwd,
            extra_dirs: worktreeCwd !== rootCwd ? [rootCwd] : [],
            provider: effectiveCustomProvider,
            mode: "accept_edits",
            worktree_branch: "auto",
            handle: "worker",
            initial_prompt: `Join this group in an isolated worktree. Start by reading the shared notes (workspace_board_read) and peer context (@${peerLabel}) so you understand the goal and avoid duplicating work, then report readiness.`,
          },
          { openInSplit: true },
        );
      }
      setSpawnOpen(false);
    } catch (err) {
      orbitActions.showToast(`Failed to add collaborator: ${String(err)}`);
    } finally {
      setSpawning(false);
    }
  };

  const launchCustomPeer = async () => {
    const trimmed = customPrompt.trim();
    if (!trimmed || spawning) return;
    setSpawning(true);
    try {
      const useWt = customWorktree && canWorktree;
      await orbitActions.spawnWorkspacePeer(
        {
          host_id: selectedHostId,
          workspace_id: activeAgent.workspace_id,
          cwd: useWt ? worktreeCwd : activeAgent.cwd,
          extra_dirs: useWt && worktreeCwd !== rootCwd ? [rootCwd] : [],
          provider: effectiveCustomProvider,
          mode: customMode,
          worktree_branch: useWt ? "auto" : null,
          handle: customHandle.trim() || null,
          initial_prompt: trimmed,
        },
        { openInSplit: true },
      );
      setCustomPrompt("");
      setCustomHandle("");
      setSpawnOpen(false);
    } catch (err) {
      orbitActions.showToast(`Failed to add collaborator: ${String(err)}`);
    } finally {
      setSpawning(false);
    }
  };

  return (
    <div className="bg-canvas/95 z-10 flex h-[32px] shrink-0 items-center justify-between gap-2 border-b border-black/[0.055] px-3 select-none dark:border-white/[0.06]">
      {/* Left: Quiet Group Switcher + Compact Sibling Thread Tabs */}
      <div className="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
        <Popover open={groupSwitcherOpen} onOpenChange={setGroupSwitcherOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              title={workspace?.title ? `Group: ${workspace.title}` : "Manage group"}
              className="text-foreground/78 hover:text-foreground inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[6px] px-1.5 text-[12px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              <Layers className="text-muted-foreground h-[12px] w-[12px] shrink-0 stroke-[1.75]" />
              <span className="max-w-36 truncate">{workspace?.title || "Group"}</span>
              <ChevronDown className="h-[11px] w-[11px] shrink-0 opacity-50" />
            </button>
          </PopoverTrigger>

          <PopoverContent
            align="start"
            side="bottom"
            sideOffset={6}
            finalFocus={false}
            className="w-[280px] space-y-2.5 p-2.5 select-none"
          >
            {workspace && (
              <div className="border-border/70 space-y-1.5 border-b pb-2.5">
                <div className="text-muted-foreground flex items-center justify-between text-[11px] font-medium">
                  <span>Current group</span>
                  <button
                    type="button"
                    onClick={() => {
                      setGroupSwitcherOpen(false);
                      orbitActions.openWorkspaceBoardTab(
                        selectedHostId,
                        workspace.workspace_id,
                        workspace.title,
                      );
                    }}
                    className="text-foreground/75 hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[11px]"
                  >
                    <FileText className="h-[11px] w-[11px]" />
                    <span>Notes</span>
                  </button>
                </div>
                <div className="flex items-center gap-1.5">
                  <input
                    value={groupRenameDraft}
                    onChange={(e) => setGroupRenameDraft(e.target.value)}
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      if (ime.shouldIgnoreKeyDown(e)) return;
                      if (e.key === "Enter") {
                        e.preventDefault();
                        void handleRenameCurrentGroup();
                      }
                    }}
                    placeholder="Group name"
                    className="border-border bg-canvas text-foreground h-[26px] min-w-0 flex-1 rounded-md border px-2 text-[12px] outline-none"
                  />
                  <button
                    type="button"
                    disabled={!groupRenameDraft.trim() || groupRenameDraft.trim() === workspace.title}
                    onClick={() => void handleRenameCurrentGroup()}
                    className="bg-foreground text-background inline-flex h-[26px] shrink-0 cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium disabled:opacity-35"
                  >
                    Save
                  </button>
                </div>
              </div>
            )}

            {projectGroups.length > 1 && (
              <div className="space-y-1">
                <div className="text-muted-foreground px-0.5 text-[11px] font-medium">
                  Move thread to
                </div>
                <div className="max-h-36 space-y-0.5 overflow-y-auto">
                  {projectGroups.map((ws) => {
                    const isCurrent = ws.workspace_id === activeAgent.workspace_id;
                    const count = hostDetail.agents.filter(
                      (a) =>
                        a.workspace_id === ws.workspace_id &&
                        !a.archived_at_rfc3339 &&
                        !a.background &&
                        !a.parent_agent_id,
                    ).length;
                    return (
                      <button
                        key={ws.workspace_id}
                        type="button"
                        disabled={isCurrent}
                        onClick={() => {
                          void orbitActions.moveAgentToWorkspace(
                            selectedHostId,
                            activeAgent.agent_id,
                            ws.workspace_id,
                          );
                          setGroupSwitcherOpen(false);
                        }}
                        className={cn(
                          "flex w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors",
                          isCurrent
                            ? "bg-foreground/[0.06] text-foreground font-medium"
                            : "text-foreground/82 hover:bg-accent hover:text-foreground",
                        )}
                      >
                        <span className="flex min-w-0 items-center gap-1.5">
                          <Layers className="text-muted-foreground h-[12px] w-[12px] shrink-0" />
                          <span className="truncate">{ws.title}</span>
                        </span>
                        <span className="flex shrink-0 items-center gap-1.5">
                          <span className="text-muted-foreground text-[11px] tabular-nums">
                            {count}
                          </span>
                          {isCurrent && <Check className="h-[12px] w-[12px]" />}
                        </span>
                      </button>
                    );
                  })}
                </div>
              </div>
            )}

            <div className="border-border/70 space-y-1.5 border-t pt-2">
              <div className="text-muted-foreground flex items-center gap-1 text-[11px] font-medium">
                <FolderPlus className="h-[11.5px] w-[11.5px]" />
                <span>New group</span>
              </div>
              <div className="flex items-center gap-1.5">
                <input
                  value={newGroupTitle}
                  onChange={(e) => setNewGroupTitle(e.target.value)}
                  {...ime.compositionProps}
                  onKeyDown={(e) => {
                    if (ime.shouldIgnoreKeyDown(e)) return;
                    if (e.key === "Enter") {
                      e.preventDefault();
                      void handleCreateAndMoveGroup();
                    }
                  }}
                  placeholder="Group name…"
                  className="border-border bg-canvas text-foreground h-[26px] min-w-0 flex-1 rounded-md border px-2 text-[12px] outline-none"
                />
                <button
                  type="button"
                  disabled={!newGroupTitle.trim()}
                  onClick={() => void handleCreateAndMoveGroup()}
                  className="bg-foreground text-background inline-flex h-[26px] shrink-0 cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium disabled:opacity-35"
                >
                  Create
                </button>
              </div>
            </div>

            {isCuratedGroup && (
              <div className="border-border/70 border-t pt-1.5">
                <button
                  type="button"
                  onClick={() => {
                    void orbitActions.moveAgentToWorkspace(
                      selectedHostId,
                      activeAgent.agent_id,
                      null,
                      { makeUnclassified: true },
                    );
                    setGroupSwitcherOpen(false);
                  }}
                  className="text-muted-foreground hover:text-foreground hover:bg-accent flex w-full cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left text-[12px] transition-colors"
                >
                  <X className="h-[12.5px] w-[12.5px]" />
                  <span>Remove from group</span>
                </button>
              </div>
            )}
          </PopoverContent>
        </Popover>

        <span className="h-3 w-[1px] shrink-0 bg-black/[0.08] dark:bg-white/[0.10]" />

        {/* Segmented Sibling Thread Switcher */}
        <div className="flex min-w-0 items-center gap-0.5">
          {peers.map((peer) => {
            const isActive = peer.agent_id === activeAgent.agent_id;
            const tKey = threadScopeKey(selectedHostId, peer.agent_id);
            const label = semanticAgentTabLabel(peer, threadCustomTitles[tKey]);
            const isEditingThis = editingPeerId === peer.agent_id;
            const tooltipParts = [
              `${label} (${providerShortName(peer.provider)}${peer.model ? ` · ${peer.model}` : ""})`,
              peer.task_summary || peer.title,
              isActive ? "Double-click to rename" : "Click to switch · ⌘/Ctrl+click to split",
            ];

            return (
              <div
                key={peer.agent_id}
                className={cn(
                  "group/peer inline-flex h-[22px] shrink-0 items-center gap-1 rounded-[6px] px-2 text-[12px] transition-colors",
                  isActive
                    ? "bg-black/[0.06] text-foreground font-medium dark:bg-white/[0.09]"
                    : "text-muted-foreground hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.06]",
                )}
              >
                {isEditingThis ? (
                  <div className="inline-flex items-center gap-1">
                    <ProviderIcon
                      provider={peer.provider}
                      className="h-[11.5px] w-[11.5px] shrink-0 opacity-75"
                    />
                    <input
                      ref={peerInputRef}
                      value={peerHandleDraft}
                      onChange={(e) => setPeerHandleDraft(e.target.value)}
                      onBlur={() => commitPeerRename(peer.agent_id)}
                      {...ime.compositionProps}
                      onKeyDown={(e) => {
                        e.stopPropagation();
                        if (ime.shouldIgnoreKeyDown(e)) return;
                        if (e.key === "Enter") {
                          e.preventDefault();
                          commitPeerRename(peer.agent_id);
                        } else if (e.key === "Escape") {
                          e.preventDefault();
                          setEditingPeerId(null);
                        }
                      }}
                      className="bg-background text-foreground ring-foreground/25 h-[18px] w-24 rounded px-1 text-[11.5px] font-normal ring-1 outline-none"
                    />
                  </div>
                ) : (
                  <>
                    <button
                      type="button"
                      title={tooltipParts.join("\n")}
                      onClick={(e) => {
                        if (e.metaKey || e.ctrlKey) {
                          orbitActions.openPeerSplitTab(selectedHostId, peer.agent_id, label);
                        } else if (!isActive) {
                          orbitActions.selectAgent(selectedHostId, peer.agent_id);
                        }
                      }}
                      onDoubleClick={(e) => {
                        e.stopPropagation();
                        setPeerHandleDraft(label);
                        setEditingPeerId(peer.agent_id);
                      }}
                      className="inline-flex cursor-pointer items-center gap-1.5"
                    >
                      {peer.is_running ? (
                        <Loader2 className="text-foreground/70 h-[10.5px] w-[10.5px] shrink-0 animate-spin" />
                      ) : peer.is_awaiting_permission ? (
                        <span className="h-[5.5px] w-[5.5px] shrink-0 rounded-full bg-amber-500" />
                      ) : peer.is_unread && !isActive ? (
                        <span className="h-[5.5px] w-[5.5px] shrink-0 rounded-full bg-sky-500" />
                      ) : (
                        <ProviderIcon
                          provider={peer.provider}
                          className="h-[11.5px] w-[11.5px] shrink-0 opacity-70"
                        />
                      )}
                      <span className="max-w-32 truncate">{label}</span>
                      {peer.worktree_branch && (
                        <span className="text-muted-foreground/70 hidden max-w-20 items-center gap-0.5 truncate font-mono text-[10.5px] sm:inline-flex">
                          <GitBranch className="h-[9.5px] w-[9.5px] shrink-0" />
                          <span className="truncate">{peer.worktree_branch}</span>
                        </span>
                      )}
                    </button>

                    <SimpleTooltip label="Rename">
                      <button
                        type="button"
                        onClick={(e) => {
                          e.stopPropagation();
                          setPeerHandleDraft(label);
                          setEditingPeerId(peer.agent_id);
                        }}
                        className="text-muted-foreground hover:text-foreground hidden h-[15px] w-[15px] cursor-pointer items-center justify-center rounded hover:bg-black/[0.08] group-hover/peer:inline-flex dark:hover:bg-white/[0.12]"
                      >
                        <Pencil className="h-[9px] w-[9px]" />
                      </button>
                    </SimpleTooltip>

                    {!isActive && (
                      <SimpleTooltip label={`Open ${label} side-by-side`}>
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            orbitActions.openPeerSplitTab(selectedHostId, peer.agent_id, label);
                          }}
                          className="text-muted-foreground hover:text-foreground -mr-0.5 hidden h-[15px] w-[15px] cursor-pointer items-center justify-center rounded hover:bg-black/[0.08] group-hover/peer:inline-flex dark:hover:bg-white/[0.12]"
                        >
                          <Columns2 className="h-[10px] w-[10px]" />
                        </button>
                      </SimpleTooltip>
                    )}
                  </>
                )}
              </div>
            );
          })}

          <Popover open={spawnOpen} onOpenChange={setSpawnOpen}>
            <SimpleTooltip label="Add collaborator">
              <PopoverTrigger asChild>
                <button
                  type="button"
                  aria-label="Add collaborator"
                  className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] shrink-0 cursor-pointer items-center justify-center rounded-[6px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                >
                  <Plus className="h-[12.5px] w-[12.5px] stroke-[2]" />
                </button>
              </PopoverTrigger>
            </SimpleTooltip>
            <PopoverContent
              align="start"
              side="bottom"
              sideOffset={6}
              finalFocus={false}
              className="w-[312px] space-y-3 p-3 select-none"
            >
              <div className="flex items-center justify-between">
                <span className="text-foreground flex items-center gap-1.5 text-[12.5px] font-medium">
                  <Users className="text-muted-foreground h-[13.5px] w-[13.5px]" />
                  <span>Add collaborator</span>
                </span>
                <span className="text-muted-foreground text-[11px]">{peers.length} active</span>
              </div>

              <div className="grid grid-cols-1 gap-1.5">
                <button
                  type="button"
                  disabled={spawning}
                  onClick={() => void launchPresetPeer("reviewer")}
                  className="border-border hover:bg-accent flex cursor-pointer items-center justify-between rounded-lg border px-2.5 py-1.5 text-left transition-colors"
                >
                  <div className="min-w-0">
                    <div className="text-foreground text-[12.5px] font-medium">Code Reviewer</div>
                    <div className="text-muted-foreground text-[11px]">
                      {providerShortName(
                        pickAlternateProvider(activeAgent.provider, availableProviders),
                      )}{" "}
                      · read-only review, writes findings to notes
                    </div>
                  </div>
                </button>

                {canWorktree && (
                  <button
                    type="button"
                    disabled={spawning}
                    onClick={() => void launchPresetPeer("worktree_worker")}
                    className="border-border hover:bg-accent flex cursor-pointer items-center justify-between rounded-lg border px-2.5 py-1.5 text-left transition-colors"
                  >
                    <div className="min-w-0">
                      <div className="text-foreground flex items-center gap-1.5 text-[12.5px] font-medium">
                        <GitBranch className="h-[12px] w-[12px]" />
                        <span>Parallel Worker</span>
                      </div>
                      <div className="text-muted-foreground text-[11px]">
                        Isolated Git worktree branch in the same group
                      </div>
                    </div>
                  </button>
                )}
              </div>

              <div className="border-border/70 space-y-2 border-t pt-2.5">
                <div className="text-muted-foreground text-[11px] font-medium">Custom task</div>
                <div className="grid grid-cols-2 gap-1.5">
                  <input
                    value={customHandle}
                    onChange={(e) => setCustomHandle(e.target.value)}
                    placeholder="Label (e.g. backend)"
                    className="border-border bg-canvas text-foreground rounded-md border px-2 py-1 text-[12px] outline-none"
                  />
                  <select
                    value={effectiveCustomProvider}
                    onChange={(e) => setCustomProvider(e.target.value as ProviderKind)}
                    className="border-border bg-canvas text-foreground rounded-md border px-2 py-1 text-[12px]"
                  >
                    {availableProviders.map((p) => (
                      <option key={p} value={p}>
                        {providerShortName(p)}
                      </option>
                    ))}
                  </select>
                </div>
                <textarea
                  rows={2}
                  value={customPrompt}
                  onChange={(e) => setCustomPrompt(e.target.value)}
                  placeholder="Describe the task. It will share context and notes with this group…"
                  className="border-border bg-canvas text-foreground w-full resize-none rounded-md border p-2 text-[12px] outline-none"
                />
                <div className="flex items-center justify-between gap-2">
                  <div className="flex items-center gap-2">
                    <select
                      value={typeof customMode === "string" ? customMode : "accept_edits"}
                      onChange={(e) => setCustomMode(e.target.value as SessionMode)}
                      className="border-border bg-canvas text-foreground rounded-md border px-1.5 py-0.5 text-[11.5px]"
                    >
                      <option value="accept_edits">Accept Edits</option>
                      <option value="plan">Plan</option>
                      <option value="full_access">Full Access</option>
                      <option value="default">Ask</option>
                    </select>
                    {canWorktree && (
                      <label className="text-muted-foreground inline-flex cursor-pointer items-center gap-1 text-[11.5px]">
                        <input
                          type="checkbox"
                          checked={customWorktree}
                          onChange={(e) => setCustomWorktree(e.target.checked)}
                        />
                        <span>Worktree</span>
                      </label>
                    )}
                  </div>
                  <button
                    type="button"
                    disabled={!customPrompt.trim() || spawning}
                    onClick={() => void launchCustomPeer()}
                    className="bg-foreground text-background inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2.5 text-[12px] font-medium disabled:opacity-40"
                  >
                    {spawning ? (
                      <Loader2 className="h-[12px] w-[12px] animate-spin" />
                    ) : (
                      <Plus className="h-[12px] w-[12px]" />
                    )}
                    <span>Start</span>
                  </button>
                </div>
              </div>
            </PopoverContent>
          </Popover>
        </div>
      </div>

      {/* Right: Conditional Conflict Pill + Quiet Notes Link */}
      <div className="flex shrink-0 items-center gap-1">
        {recentConflicts.length > 0 && (
          <SimpleTooltip label={recentConflicts[recentConflicts.length - 1].summary}>
            <button
              type="button"
              onClick={() =>
                orbitActions.openWorkspaceBoardTab(
                  selectedHostId,
                  activeAgent.workspace_id,
                  workspace?.title,
                )
              }
              className="inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-full border border-amber-500/30 bg-amber-500/[0.08] px-2 text-[11px] font-medium text-amber-700 transition-colors hover:bg-amber-500/15 dark:text-amber-300"
            >
              <AlertTriangle className="h-[11px] w-[11px] shrink-0" />
              <span className="max-w-28 truncate">
                Conflict
                {recentConflicts[recentConflicts.length - 1].file_path
                  ? ` · ${recentConflicts[recentConflicts.length - 1].file_path?.split("/").pop()}`
                  : ""}
              </span>
            </button>
          </SimpleTooltip>
        )}

        <button
          type="button"
          onClick={() =>
            orbitActions.openWorkspaceBoardTab(
              selectedHostId,
              activeAgent.workspace_id,
              workspace?.title,
            )
          }
          className="text-muted-foreground hover:text-foreground inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-[6px] px-1.5 text-[11.5px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
        >
          <FileText className="h-[12px] w-[12px] stroke-[1.75]" />
          <span>Notes</span>
          {boardRev > 0 && (
            <span className="text-muted-foreground/65 font-mono text-[10.5px]">v{boardRev}</span>
          )}
        </button>
      </div>
    </div>
  );
});
