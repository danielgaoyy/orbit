import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  Check,
  ChevronDown,
  Columns2,
  FileText,
  FolderPlus,
  GitBranch,
  Inbox,
  Layers,
  Loader2,
  Pencil,
  Plus,
  Sparkles,
  Undo2,
  Users,
  X,
} from "lucide-react";
import {
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
  switch (provider) {
    case "claude_code":
      return "Claude";
    case "codex":
      return "Codex";
    case "antigravity":
      return "AGY";
  }
}

function pickAlternateProvider(
  current: ProviderKind,
  available: ProviderKind[],
): ProviderKind {
  const alt = available.find((p) => p !== current);
  return alt ?? current;
}

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

  const [wsSwitcherOpen, setWsSwitcherOpen] = useState(false);
  const [wsRenameDraft, setWsRenameDraft] = useState("");
  const [newWsTitle, setNewWsTitle] = useState("");
  const [editingPeerId, setEditingPeerId] = useState<string | null>(null);
  const [peerHandleDraft, setPeerHandleDraft] = useState("");
  const [editingSummary, setEditingSummary] = useState(false);
  const [summaryDraft, setSummaryDraft] = useState("");
  const [summarizing, setSummarizing] = useState(false);

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

  const isCuratedWorkspace = Boolean(workspace?.is_curated);

  const peers = useMemo(() => {
    if (!hostDetail || !activeAgent) return [];
    if (!isCuratedWorkspace) {
      return [activeAgent];
    }
    return hostDetail.agents
      .filter(
        (a) =>
          a.workspace_id === activeAgent.workspace_id &&
          !a.archived_at_rfc3339 &&
          !a.background,
      )
      .sort((a, b) => a.updated_at_rfc3339.localeCompare(b.updated_at_rfc3339));
  }, [hostDetail, activeAgent, isCuratedWorkspace]);

  const rootCwd =
    workspace?.project_root || workspace?.source_cwd || workspace?.cwd || activeAgent?.cwd || "";
  const normRootCwd = normalizeProjectPath(rootCwd);

  const projectCuratedWorkspaces = useMemo(() => {
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
    if (!hostDetail || !activeAgent || !isCuratedWorkspace) return [];
    return hostDetail.workspace_events?.[activeAgent.workspace_id] ?? [];
  }, [hostDetail, activeAgent, isCuratedWorkspace]);

  const recentConflicts = useMemo(
    () => workspaceEvents.filter((ev) => ev.kind === "conflict_warning").slice(-3),
    [workspaceEvents],
  );

  const organizeStatus =
    hostDetail?.workspace_organize_status?.[normRootCwd] ??
    hostDetail?.workspace_organize_status?.[rootCwd] ??
    null;
  const isOrganizing = organizeStatus?.phase === "running";

  useEffect(() => {
    if (wsSwitcherOpen && workspace) {
      setWsRenameDraft(workspace.title);
    }
  }, [wsSwitcherOpen, workspace]);

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
    setEditingSummary(false);
  }, [selectedAgentId]);

  if (
    workspacePanel !== "conversation" ||
    isDraftNewSession ||
    !selectedHostId ||
    !selectedAgentId ||
    !activeAgent ||
    !hostDetail
  ) {
    return null;
  }

  const suggestion = activeAgent.cluster_suggestion ?? null;
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

  const commitSummaryEdit = () => {
    const clean = summaryDraft.trim();
    setEditingSummary(false);
    void orbitActions.updateAgentIdentity({
      hostId: selectedHostId,
      agentId: activeAgent.agent_id,
      taskSummary: clean,
    });
  };

  const triggerSummarize = async () => {
    if (summarizing) return;
    setSummarizing(true);
    try {
      await orbitActions.summarizeAgent(selectedHostId, activeAgent.agent_id);
    } finally {
      window.setTimeout(() => setSummarizing(false), 1200);
    }
  };

  const handleRenameCurrentWorkspace = async () => {
    if (!workspace) return;
    const clean = wsRenameDraft.trim();
    if (!clean || clean === workspace.title) return;
    await orbitActions.renameWorkspace(selectedHostId, workspace.workspace_id, clean);
    setWsSwitcherOpen(false);
  };

  const handleCreateAndMoveWorkspace = async () => {
    const clean = newWsTitle.trim();
    if (!clean) return;
    await orbitActions.moveAgentToWorkspace(selectedHostId, activeAgent.agent_id, null, {
      newWorkspaceTitle: clean,
    });
    setNewWsTitle("");
    setWsSwitcherOpen(false);
  };

  const launchPresetPeer = async (preset: "reviewer" | "worktree_worker" | "sibling") => {
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
            initial_prompt: `Review the work in this Mission Workspace (coordinate with @${peerLabel} via workspace_board_read / read_peer_context). Inspect code changes for correctness, edge cases, and test coverage, and write a concise review verdict to BOARD.md.`,
          },
          { openInSplit: true },
        );
      } else if (preset === "worktree_worker") {
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
            initial_prompt: `Join this Mission Workspace in an isolated worktree. Start by reading the Shared Blackboard (workspace_board_read) and peer context (@${peerLabel}) so you understand the goal and avoid duplicating work, then report readiness.`,
          },
          { openInSplit: true },
        );
      } else {
        await orbitActions.spawnWorkspacePeer(
          {
            host_id: selectedHostId,
            workspace_id: activeAgent.workspace_id,
            cwd: activeAgent.cwd,
            provider: effectiveCustomProvider,
            mode: "accept_edits",
            handle: "collaborator",
            initial_prompt: `Join this Mission Workspace alongside @${peerLabel}. Read the Shared Blackboard (workspace_board_read) and summarize how you can assist.`,
          },
          { openInSplit: true },
        );
      }
      setSpawnOpen(false);
    } catch (err) {
      orbitActions.showToast(`Failed to spawn peer: ${String(err)}`);
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
      orbitActions.showToast(`Failed to spawn peer: ${String(err)}`);
    } finally {
      setSpawning(false);
    }
  };

  const activeSummary = activeAgent.task_summary?.trim() || "";

  return (
    <div className="border-border/80 bg-canvas/95 z-10 flex shrink-0 flex-col border-b select-none">
      {/* Row 1: Always-on Workspace Switcher + Semantic Editable Agent Tabs + Actions */}
      <div className="flex h-[36px] items-center justify-between gap-2 px-3">
        {/* Left: Workspace Switcher & Manager Popover + Semantic Peer Tabs */}
        <div className="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
          <Popover open={wsSwitcherOpen} onOpenChange={setWsSwitcherOpen}>
            <PopoverTrigger asChild>
              <button
                type="button"
                title={
                  isCuratedWorkspace
                    ? `${workspace?.title ?? "Workspace"} — 点击切换、重命名或管理 Workspace`
                    : "待归类 — 点击移入 Workspace 或新建 Workspace"
                }
                className={cn(
                  "inline-flex h-[24px] shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-2 text-[12px] font-medium transition-colors",
                  isCuratedWorkspace
                    ? "text-foreground/85 hover:text-foreground bg-black/[0.04] hover:bg-black/[0.07] dark:bg-white/[0.06] dark:hover:bg-white/[0.10]"
                    : "border border-dashed border-black/15 text-muted-foreground hover:border-foreground/30 hover:text-foreground dark:border-white/20",
                )}
              >
                {isCuratedWorkspace ? (
                  <Layers className="h-[12.5px] w-[12.5px] shrink-0 stroke-[1.8] text-sky-500" />
                ) : (
                  <Inbox className="h-[12.5px] w-[12.5px] shrink-0 stroke-[1.75]" />
                )}
                <span className="max-w-36 truncate">
                  {isCuratedWorkspace ? workspace?.title || "Workspace" : "待归类"}
                </span>
                <ChevronDown className="h-[11.5px] w-[11.5px] shrink-0 opacity-55" />
              </button>
            </PopoverTrigger>

            <PopoverContent
              align="start"
              side="bottom"
              sideOffset={6}
              finalFocus={false}
              className="w-[300px] space-y-2.5 p-3 select-none"
            >
              {/* Rename Current Curated Workspace */}
              {isCuratedWorkspace && workspace && (
                <div className="border-border/70 space-y-1.5 border-b pb-2.5">
                  <div className="text-muted-foreground flex items-center justify-between text-[11px] font-medium">
                    <span>当前 Workspace</span>
                    <button
                      type="button"
                      onClick={() => {
                        setWsSwitcherOpen(false);
                        orbitActions.openWorkspaceBoardTab(
                          selectedHostId,
                          workspace.workspace_id,
                          workspace.title,
                        );
                      }}
                      className="text-foreground/75 hover:text-foreground inline-flex cursor-pointer items-center gap-1 text-[11px]"
                    >
                      <FileText className="h-[11px] w-[11px]" />
                      <span>Board</span>
                    </button>
                  </div>
                  <div className="flex items-center gap-1.5">
                    <input
                      value={wsRenameDraft}
                      onChange={(e) => setWsRenameDraft(e.target.value)}
                      {...ime.compositionProps}
                      onKeyDown={(e) => {
                        if (ime.shouldIgnoreKeyDown(e)) return;
                        if (e.key === "Enter") {
                          e.preventDefault();
                          void handleRenameCurrentWorkspace();
                        }
                      }}
                      placeholder="Workspace 名称"
                      className="border-border bg-canvas text-foreground h-[26px] min-w-0 flex-1 rounded-md border px-2 text-[12px] outline-none"
                    />
                    <button
                      type="button"
                      disabled={!wsRenameDraft.trim() || wsRenameDraft.trim() === workspace.title}
                      onClick={() => void handleRenameCurrentWorkspace()}
                      className="bg-foreground text-background inline-flex h-[26px] shrink-0 cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium disabled:opacity-35"
                    >
                      保存
                    </button>
                  </div>
                </div>
              )}

              {/* Switch To Existing Curated Workspaces */}
              <div className="space-y-1">
                <div className="text-muted-foreground text-[11px] font-medium">
                  切换当前会话到 Workspace
                </div>
                {projectCuratedWorkspaces.length === 0 ? (
                  <div className="text-muted-foreground/75 py-1 text-[11.5px]">
                    当前项目还没有已创建的 Workspace
                  </div>
                ) : (
                  <div className="max-h-40 space-y-0.5 overflow-y-auto">
                    {projectCuratedWorkspaces.map((ws) => {
                      const isCurrent = ws.workspace_id === activeAgent.workspace_id;
                      const peerCount = hostDetail.agents.filter(
                        (a) =>
                          a.workspace_id === ws.workspace_id &&
                          !a.archived_at_rfc3339 &&
                          !a.background,
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
                            setWsSwitcherOpen(false);
                          }}
                          className={cn(
                            "flex w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors",
                            isCurrent
                              ? "bg-foreground/[0.07] text-foreground font-medium"
                              : "text-foreground/82 hover:bg-accent hover:text-foreground",
                          )}
                        >
                          <span className="flex min-w-0 items-center gap-1.5">
                            <Layers className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0" />
                            <span className="truncate">{ws.title}</span>
                          </span>
                          <span className="flex shrink-0 items-center gap-1.5">
                            <span className="text-muted-foreground text-[11px] tabular-nums">
                              {peerCount}
                            </span>
                            {isCurrent && <Check className="h-[12px] w-[12px]" />}
                          </span>
                        </button>
                      );
                    })}
                  </div>
                )}
              </div>

              {/* Create New Workspace & Move Current Conversation Into It */}
              <div className="border-border/70 space-y-1.5 border-t pt-2.5">
                <div className="text-muted-foreground flex items-center gap-1 text-[11px] font-medium">
                  <FolderPlus className="h-[11.5px] w-[11.5px]" />
                  <span>新建 Workspace 并移入当前会话</span>
                </div>
                <div className="flex items-center gap-1.5">
                  <input
                    value={newWsTitle}
                    onChange={(e) => setNewWsTitle(e.target.value)}
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      if (ime.shouldIgnoreKeyDown(e)) return;
                      if (e.key === "Enter") {
                        e.preventDefault();
                        void handleCreateAndMoveWorkspace();
                      }
                    }}
                    placeholder="输入新 Workspace 名称…"
                    className="border-border bg-canvas text-foreground h-[26px] min-w-0 flex-1 rounded-md border px-2 text-[12px] outline-none"
                  />
                  <button
                    type="button"
                    disabled={!newWsTitle.trim()}
                    onClick={() => void handleCreateAndMoveWorkspace()}
                    className="bg-foreground text-background inline-flex h-[26px] shrink-0 cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium disabled:opacity-35"
                  >
                    创建
                  </button>
                </div>
              </div>

              {/* Return To Unclassified */}
              {isCuratedWorkspace && (
                <div className="border-border/70 border-t pt-2">
                  <button
                    type="button"
                    onClick={() => {
                      void orbitActions.moveAgentToWorkspace(
                        selectedHostId,
                        activeAgent.agent_id,
                        null,
                        { makeUnclassified: true },
                      );
                      setWsSwitcherOpen(false);
                    }}
                    className="text-muted-foreground hover:text-foreground hover:bg-accent flex w-full cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left text-[12px] transition-colors"
                  >
                    <Inbox className="h-[12.5px] w-[12.5px]" />
                    <span>移回「待归类」</span>
                  </button>
                </div>
              )}
            </PopoverContent>
          </Popover>

          {/* 1-Click AI Triage Pill when conversation is Unclassified (`待归类`) */}
          {!isCuratedWorkspace && (
            <SimpleTooltip label="让 AI 理解此会话上下文并自动归类到合适的 Workspace">
              <button
                type="button"
                disabled={isOrganizing}
                onClick={() =>
                  void orbitActions.organizeProjectWorkspaces(
                    selectedHostId,
                    rootCwd,
                    [activeAgent.agent_id],
                    true,
                  )
                }
                className="inline-flex h-[23px] shrink-0 cursor-pointer items-center gap-1 rounded-full border border-sky-500/30 bg-sky-500/10 px-2 text-[11.5px] font-medium text-sky-600 transition-colors hover:bg-sky-500/18 disabled:opacity-60 dark:text-sky-400"
              >
                {isOrganizing ? (
                  <Loader2 className="h-[11px] w-[11px] animate-spin" />
                ) : (
                  <Sparkles className="h-[11px] w-[11px]" />
                )}
                <span>AI 归类</span>
              </button>
            </SimpleTooltip>
          )}

          <span className="bg-border/80 h-3.5 w-[1px] shrink-0" />

          {/* Semantic Editable Agent Tabs (Never Bare Harness Names!) */}
          <div className="flex min-w-0 items-center gap-1">
            {peers.map((peer) => {
              const isActive = peer.agent_id === activeAgent.agent_id;
              const tKey = threadScopeKey(selectedHostId, peer.agent_id);
              const label = semanticAgentTabLabel(peer, threadCustomTitles[tKey]);
              const isEditingThis = editingPeerId === peer.agent_id;
              const tooltipParts = [
                `${label} (${providerShortName(peer.provider)}${peer.model ? ` · ${peer.model}` : ""})`,
                peer.task_summary ? `当前进展：${peer.task_summary}` : peer.title,
                isActive ? "双击标签可重命名" : "点击切换 · 双击重命名 · ⌘/Ctrl+点击分屏",
              ];

              return (
                <div
                  key={peer.agent_id}
                  className={cn(
                    "group/peer inline-flex h-[24px] shrink-0 items-center gap-1 rounded-full border px-2 text-[12px] transition-colors",
                    isActive
                      ? "border-foreground/20 bg-foreground/[0.07] text-foreground font-medium dark:bg-white/[0.10]"
                      : "border-border/70 text-foreground/75 hover:border-foreground/20 hover:bg-black/[0.04] hover:text-foreground dark:hover:bg-white/[0.06]",
                  )}
                >
                  {isEditingThis ? (
                    <div className="inline-flex items-center gap-1">
                      <ProviderIcon
                        provider={peer.provider}
                        className="h-[12px] w-[12px] shrink-0 opacity-75"
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
                        className="bg-background text-foreground h-[18px] w-28 rounded px-1 text-[11.5px] font-normal outline-none ring-1 ring-sky-500/50"
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
                          <Loader2 className="h-[11px] w-[11px] shrink-0 animate-spin text-sky-500" />
                        ) : peer.is_awaiting_permission ? (
                          <span className="h-[6px] w-[6px] shrink-0 rounded-full bg-amber-500" />
                        ) : peer.is_unread && !isActive ? (
                          <span className="h-[6px] w-[6px] shrink-0 rounded-full bg-sky-500" />
                        ) : (
                          <ProviderIcon
                            provider={peer.provider}
                            className="h-[12px] w-[12px] shrink-0 opacity-75"
                          />
                        )}
                        <span className="max-w-36 truncate">{label}</span>
                        {peer.worktree_branch && (
                          <span className="text-muted-foreground/80 hidden max-w-20 items-center gap-0.5 truncate font-mono text-[10.5px] sm:inline-flex">
                            <GitBranch className="h-[10px] w-[10px] shrink-0" />
                            <span className="truncate">{peer.worktree_branch}</span>
                          </span>
                        )}
                      </button>

                      <SimpleTooltip label="重命名会话标签">
                        <button
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            setPeerHandleDraft(label);
                            setEditingPeerId(peer.agent_id);
                          }}
                          className="text-muted-foreground hover:text-foreground hidden h-[16px] w-[16px] cursor-pointer items-center justify-center rounded-full hover:bg-black/[0.08] group-hover/peer:inline-flex dark:hover:bg-white/[0.12]"
                        >
                          <Pencil className="h-[9.5px] w-[9.5px]" />
                        </button>
                      </SimpleTooltip>

                      {!isActive && (
                        <SimpleTooltip label={`分屏并排打开「${label}」`}>
                          <button
                            type="button"
                            onClick={(e) => {
                              e.stopPropagation();
                              orbitActions.openPeerSplitTab(selectedHostId, peer.agent_id, label);
                            }}
                            className="text-muted-foreground hover:text-foreground -mr-0.5 hidden h-[16px] w-[16px] cursor-pointer items-center justify-center rounded-full hover:bg-black/[0.08] group-hover/peer:inline-flex dark:hover:bg-white/[0.12]"
                          >
                            <Columns2 className="h-[10.5px] w-[10.5px]" />
                          </button>
                        </SimpleTooltip>
                      )}
                    </>
                  )}
                </div>
              );
            })}
          </div>
        </div>

        {/* Right: Conflict Radar + AI Summarize + Shared Board + Spawn Peer */}
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
                className="inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-full border border-amber-500/35 bg-amber-500/10 px-2 text-[11.5px] font-medium text-amber-700 transition-colors hover:bg-amber-500/18 dark:text-amber-300"
              >
                <AlertTriangle className="h-[11.5px] w-[11.5px] shrink-0" />
                <span className="max-w-32 truncate">
                  Conflict
                  {recentConflicts[recentConflicts.length - 1].file_path
                    ? ` · ${recentConflicts[recentConflicts.length - 1].file_path?.split("/").pop()}`
                    : ""}
                </span>
              </button>
            </SimpleTooltip>
          )}

          <SimpleTooltip label="AI 自动理解上下文并更新会话标签与一句话摘要">
            <button
              type="button"
              disabled={summarizing}
              onClick={() => void triggerSummarize()}
              className="text-muted-foreground hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-[7px] px-1.5 text-[11.5px] transition-colors hover:bg-black/[0.045] disabled:opacity-50 dark:hover:bg-white/[0.06]"
            >
              {summarizing ? (
                <Loader2 className="h-[12px] w-[12px] animate-spin text-sky-500" />
              ) : (
                <Sparkles className="h-[12px] w-[12px] stroke-[1.75]" />
              )}
              <span className="hidden md:inline">总结</span>
            </button>
          </SimpleTooltip>

          {(isCuratedWorkspace || boardRev > 0) && (
            <button
              type="button"
              onClick={() =>
                orbitActions.openWorkspaceBoardTab(
                  selectedHostId,
                  activeAgent.workspace_id,
                  workspace?.title,
                )
              }
              className="text-muted-foreground hover:text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-[7px] px-2 text-[12px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              <FileText className="h-[12.5px] w-[12.5px] stroke-[1.75]" />
              <span>Board</span>
              {boardRev > 0 && (
                <span className="text-muted-foreground/70 font-mono text-[10.5px]">
                  v{boardRev}
                </span>
              )}
            </button>
          )}

          <Popover open={spawnOpen} onOpenChange={setSpawnOpen}>
            <PopoverTrigger asChild>
              <button
                type="button"
                className="text-foreground/80 hover:text-foreground border-border/80 inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-full border px-2 text-[12px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
              >
                <Plus className="h-[12px] w-[12px] stroke-[2]" />
                <span>Peer</span>
              </button>
            </PopoverTrigger>
            <PopoverContent
              align="end"
              side="bottom"
              sideOffset={6}
              finalFocus={false}
              className="w-[320px] space-y-3 p-3 select-none"
            >
              <div className="flex items-center justify-between">
                <span className="text-foreground flex items-center gap-1.5 text-[12.5px] font-semibold">
                  <Users className="h-[13.5px] w-[13.5px]" />
                  <span>添加协作 Peer</span>
                </span>
                <span className="text-muted-foreground text-[11px]">
                  {peers.length} active
                </span>
              </div>

              {/* 1-Click Presets */}
              <div className="grid grid-cols-1 gap-1.5">
                <button
                  type="button"
                  disabled={spawning}
                  onClick={() => void launchPresetPeer("reviewer")}
                  className="border-border hover:bg-accent flex cursor-pointer items-center justify-between rounded-lg border px-2.5 py-1.5 text-left transition-colors"
                >
                  <div className="min-w-0">
                    <div className="text-foreground text-[12.5px] font-medium">
                      Code Reviewer (@reviewer)
                    </div>
                    <div className="text-muted-foreground text-[11px]">
                      {providerShortName(
                        pickAlternateProvider(activeAgent.provider, availableProviders),
                      )}{" "}
                      · 只读评审模式 · 审查改动并写入 BOARD.md
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
                        <span>Parallel Worker (独立 Worktree)</span>
                      </div>
                      <div className="text-muted-foreground text-[11px]">
                        在同一 Workspace 内使用独立 Git 分支并行开发
                      </div>
                    </div>
                  </button>
                )}
              </div>

              {/* Custom Peer Launch */}
              <div className="border-border/70 space-y-2 border-t pt-2.5">
                <div className="text-muted-foreground text-[11px] font-medium">
                  自定义协作任务
                </div>
                <div className="grid grid-cols-2 gap-1.5">
                  <input
                    value={customHandle}
                    onChange={(e) => setCustomHandle(e.target.value)}
                    placeholder="标签名 (如 后端联调)"
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
                  placeholder="输入任务说明，它会自动感知同 Workspace 内的其他会话与 BOARD.md…"
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
                    <span>Spawn</span>
                  </button>
                </div>
              </div>
            </PopoverContent>
          </Popover>
        </div>
      </div>

      {/* Row 2: Live AI Task Summary Bar (editable inline) */}
      {(activeSummary || editingSummary) && (
        <div className="border-border/50 bg-muted/25 group/sum flex min-h-[25px] items-center justify-between gap-2 border-t px-3 py-0.5 text-[11.5px]">
          {editingSummary ? (
            <div className="flex min-w-0 flex-1 items-center gap-1.5">
              <Sparkles className="h-[11px] w-[11px] shrink-0 text-sky-500" />
              <input
                autoFocus
                value={summaryDraft}
                onChange={(e) => setSummaryDraft(e.target.value)}
                onBlur={commitSummaryEdit}
                {...ime.compositionProps}
                onKeyDown={(e) => {
                  if (ime.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter") {
                    e.preventDefault();
                    commitSummaryEdit();
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    setEditingSummary(false);
                  }
                }}
                placeholder="一句话概括这个会话正在做什么…"
                className="bg-background text-foreground h-[20px] min-w-0 flex-1 rounded px-1.5 text-[11.5px] outline-none ring-1 ring-sky-500/40"
              />
            </div>
          ) : (
            <>
              <div
                onDoubleClick={() => {
                  setSummaryDraft(activeSummary);
                  setEditingSummary(true);
                }}
                title={`${activeSummary}\n双击修改摘要`}
                className="text-muted-foreground flex min-w-0 flex-1 items-center gap-1.5 truncate"
              >
                <Sparkles className="h-[11px] w-[11px] shrink-0 text-sky-500/80" />
                <span className="truncate">{activeSummary}</span>
              </div>
              <button
                type="button"
                title="编辑一句话摘要"
                onClick={() => {
                  setSummaryDraft(activeSummary);
                  setEditingSummary(true);
                }}
                className="text-muted-foreground hover:text-foreground hidden h-[18px] w-[18px] shrink-0 cursor-pointer items-center justify-center rounded hover:bg-black/[0.06] group-hover/sum:inline-flex dark:hover:bg-white/[0.08]"
              >
                <Pencil className="h-[10px] w-[10px]" />
              </button>
            </>
          )}
        </div>
      )}

      {/* Row 3: Reversible Auto-Cluster / AI Triage Banner */}
      {suggestion && (
        <div className="border-border/60 bg-muted/40 flex flex-wrap items-center justify-between gap-2 border-t px-3 py-1.5 text-[12px]">
          <div className="flex min-w-0 items-center gap-2">
            <Sparkles className="h-[13px] w-[13px] shrink-0 text-sky-500" />
            <span className="text-foreground/85 truncate">
              {suggestion.auto_applied ? (
                <>
                  已自动归类到{" "}
                  <strong className="text-foreground font-medium">
                    {suggestion.target_workspace_title}
                  </strong>{" "}
                  <span className="text-muted-foreground">· {suggestion.reason}</span>
                </>
              ) : (
                <>
                  建议归类到{" "}
                  <strong className="text-foreground font-medium">
                    {suggestion.target_workspace_title}
                  </strong>{" "}
                  <span className="text-muted-foreground">· {suggestion.reason}</span>
                </>
              )}
            </span>
          </div>

          <div className="flex shrink-0 items-center gap-1.5">
            {suggestion.auto_applied ? (
              <button
                type="button"
                onClick={() => {
                  void orbitActions.moveAgentToWorkspace(
                    selectedHostId,
                    activeAgent.agent_id,
                    null,
                    { makeUnclassified: true },
                  );
                  void orbitActions.dismissClusterSuggestion(
                    selectedHostId,
                    activeAgent.agent_id,
                  );
                }}
                className="border-border bg-canvas hover:bg-accent inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium"
              >
                <Undo2 className="h-[11px] w-[11px]" />
                <span>撤销移回待归类</span>
              </button>
            ) : (
              <button
                type="button"
                onClick={() => {
                  void orbitActions.moveAgentToWorkspace(
                    selectedHostId,
                    activeAgent.agent_id,
                    suggestion.target_workspace_id,
                  );
                  void orbitActions.dismissClusterSuggestion(
                    selectedHostId,
                    activeAgent.agent_id,
                  );
                }}
                className="bg-foreground text-background inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium hover:opacity-90"
              >
                <Check className="h-[11px] w-[11px]" />
                <span>移入该 Workspace</span>
              </button>
            )}

            <button
              type="button"
              aria-label="Dismiss grouping banner"
              onClick={() =>
                void orbitActions.dismissClusterSuggestion(
                  selectedHostId,
                  activeAgent.agent_id,
                )
              }
              className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded-md hover:bg-black/[0.06] dark:hover:bg-white/[0.08]"
            >
              <X className="h-[12px] w-[12px]" />
            </button>
          </div>
        </div>
      )}
    </div>
  );
});
