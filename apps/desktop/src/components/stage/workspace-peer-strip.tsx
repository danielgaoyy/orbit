import { memo, useMemo, useState } from "react";
import {
  AlertTriangle,
  Check,
  Columns2,
  FileText,
  GitBranch,
  Layers,
  Loader2,
  Plus,
  Sparkles,
  Undo2,
  Users,
  X,
} from "lucide-react";
import {
  type AgentCardView,
  type ProviderKind,
  type SessionMode,
} from "@/lib/orbit-ipc";
import { ProviderIcon } from "@/components/ui/provider-icons";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
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

function peerDisplayHandle(agent: AgentCardView): string {
  return agent.handle ? `@${agent.handle}` : `@${providerShortName(agent.provider).toLowerCase()}`;
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
  const hostDetail = useOrbitSelector((s) =>
    s.selectedHostId ? (s.hostDetails[s.selectedHostId] ?? null) : null,
  );
  const gitReposCache = useOrbitSelector((s) => s.gitReposCache);

  const [spawnOpen, setSpawnOpen] = useState(false);
  const [customPrompt, setCustomPrompt] = useState("");
  const [customHandle, setCustomHandle] = useState("");
  const [customProvider, setCustomProvider] = useState<ProviderKind | null>(null);
  const [customMode, setCustomMode] = useState<SessionMode>("accept_edits");
  const [customWorktree, setCustomWorktree] = useState(false);
  const [spawning, setSpawning] = useState(false);

  const activeAgent = useMemo(() => {
    if (!hostDetail || !selectedAgentId) return null;
    return hostDetail.agents.find((a) => a.agent_id === selectedAgentId) ?? null;
  }, [hostDetail, selectedAgentId]);

  const workspace = useMemo(() => {
    if (!hostDetail || !activeAgent) return null;
    return hostDetail.workspaces.find((w) => w.workspace_id === activeAgent.workspace_id) ?? null;
  }, [hostDetail, activeAgent]);

  const peers = useMemo(() => {
    if (!hostDetail || !activeAgent) return [];
    return hostDetail.agents
      .filter(
        (a) =>
          a.workspace_id === activeAgent.workspace_id &&
          !a.archived_at_rfc3339 &&
          !a.background,
      )
      .sort((a, b) => a.updated_at_rfc3339.localeCompare(b.updated_at_rfc3339));
  }, [hostDetail, activeAgent]);

  const workspaceEvents = useMemo(() => {
    if (!hostDetail || !activeAgent) return [];
    return hostDetail.workspace_events?.[activeAgent.workspace_id] ?? [];
  }, [hostDetail, activeAgent]);

  const recentConflicts = useMemo(
    () => workspaceEvents.filter((ev) => ev.kind === "conflict_warning").slice(-3),
    [workspaceEvents],
  );

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
  const hasVisibleStrip =
    peers.length >= 2 || boardRev > 0 || recentConflicts.length > 0 || Boolean(suggestion);

  if (!hasVisibleStrip) {
    return null;
  }

  const availableProviders =
    hostDetail.host.available_providers.length > 0
      ? hostDetail.host.available_providers
      : ([activeAgent.provider] as ProviderKind[]);

  const rootCwd = workspace?.source_cwd || workspace?.cwd || activeAgent.cwd;
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

  const launchPresetPeer = async (preset: "reviewer" | "worktree_worker" | "sibling") => {
    if (spawning) return;
    setSpawning(true);
    try {
      const peerHandle = activeAgent.handle ?? "lead";
      if (preset === "reviewer") {
        await orbitActions.spawnWorkspacePeer(
          {
            host_id: selectedHostId,
            workspace_id: activeAgent.workspace_id,
            cwd: activeAgent.cwd,
            provider: pickAlternateProvider(activeAgent.provider, availableProviders),
            mode: "plan",
            handle: "reviewer",
            initial_prompt: `Review the work in this Mission Workspace (coordinate with @${peerHandle} via workspace_board_read / read_peer_context). Inspect code changes for correctness, edge cases, and test coverage, and write a concise review verdict to BOARD.md.`,
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
            initial_prompt: `Join this Mission Workspace in an isolated worktree. Start by reading the Shared Blackboard (workspace_board_read) and peer context (@${peerHandle}) so you understand the goal and avoid duplicating work, then report readiness.`,
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
            handle: "peer",
            initial_prompt: `Join this Mission Workspace alongside @${peerHandle}. Read the Shared Blackboard (workspace_board_read) and summarize how you can assist.`,
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

  return (
    <div className="border-border/80 bg-canvas/95 z-10 flex shrink-0 flex-col border-b select-none">
      {/* Row 1: Mission Workspace Peer Strip */}
      {(peers.length >= 2 || boardRev > 0 || recentConflicts.length > 0) && (
        <div className="flex h-[36px] items-center justify-between gap-2 px-3">
          {/* Left: Workspace Title + Live Peer Capsules */}
          <div className="flex min-w-0 flex-1 items-center gap-1.5 overflow-x-auto [scrollbar-width:none] [&::-webkit-scrollbar]:hidden">
            <SimpleTooltip label={workspace?.mission_goal || workspace?.title || "Mission Workspace"}>
              <button
                type="button"
                onClick={() =>
                  orbitActions.openWorkspaceBoardTab(
                    selectedHostId,
                    activeAgent.workspace_id,
                    workspace?.title,
                  )
                }
                className="text-muted-foreground hover:text-foreground inline-flex shrink-0 cursor-pointer items-center gap-1.5 rounded-[7px] px-1.5 py-1 text-[12px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
              >
                <Layers className="h-[13px] w-[13px] shrink-0 stroke-[1.75]" />
                <span className="max-w-36 truncate">{workspace?.title || "Workspace"}</span>
              </button>
            </SimpleTooltip>

            <span className="bg-border/80 h-3.5 w-[1px] shrink-0" />

            <div className="flex min-w-0 items-center gap-1">
              {peers.map((peer) => {
                const isActive = peer.agent_id === activeAgent.agent_id;
                const label = peerDisplayHandle(peer);
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
                    <button
                      type="button"
                      title={
                        isActive
                          ? `${label} (${peer.title})`
                          : `${label} · Click to switch, ⌘/Ctrl+Click to open side-by-side`
                      }
                      onClick={(e) => {
                        if (e.metaKey || e.ctrlKey) {
                          orbitActions.openPeerSplitTab(selectedHostId, peer.agent_id, label);
                        } else if (!isActive) {
                          orbitActions.selectAgent(selectedHostId, peer.agent_id);
                        }
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
                        <ProviderIcon provider={peer.provider} className="h-[12px] w-[12px] shrink-0 opacity-75" />
                      )}
                      <span className="max-w-28 truncate">{label}</span>
                      {peer.worktree_branch && (
                        <span className="text-muted-foreground/80 hidden max-w-20 items-center gap-0.5 truncate font-mono text-[10.5px] sm:inline-flex">
                          <GitBranch className="h-[10px] w-[10px] shrink-0" />
                          <span className="truncate">{peer.worktree_branch}</span>
                        </span>
                      )}
                    </button>

                    {!isActive && (
                      <SimpleTooltip label={`Open ${label} side-by-side`}>
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
                  </div>
                );
              })}
            </div>
          </div>

          {/* Right: Conflict Radar + Shared Board + Spawn Peer */}
          <div className="flex shrink-0 items-center gap-1">
            {recentConflicts.length > 0 && (
              <SimpleTooltip
                label={recentConflicts[recentConflicts.length - 1].summary}
              >
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
                    <span>Add Workspace Peer</span>
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
                        · Read-only plan mode · audits changes & writes to BOARD.md
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
                          <span>Parallel Worker (Isolated Worktree)</span>
                        </div>
                        <div className="text-muted-foreground text-[11px]">
                          Own Git branch in the same Mission Workspace
                        </div>
                      </div>
                    </button>
                  )}
                </div>

                {/* Custom Peer Launch */}
                <div className="border-border/70 space-y-2 border-t pt-2.5">
                  <div className="text-muted-foreground text-[11px] font-medium">
                    Custom Peer Task
                  </div>
                  <div className="grid grid-cols-2 gap-1.5">
                    <input
                      value={customHandle}
                      onChange={(e) => setCustomHandle(e.target.value)}
                      placeholder="@handle (optional)"
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
                    placeholder="What should this peer work on? It automatically sees sibling peers and BOARD.md..."
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
      )}

      {/* Row 2: Reversible Auto-Cluster / Grouping Suggestion Banner */}
      {suggestion && (
        <div className="border-border/60 bg-muted/40 flex flex-wrap items-center justify-between gap-2 border-t px-3 py-1.5 text-[12px]">
          <div className="flex min-w-0 items-center gap-2">
            <Sparkles className="h-[13px] w-[13px] shrink-0 text-sky-500" />
            <span className="text-foreground/85 truncate">
              {suggestion.auto_applied ? (
                <>
                  Auto-grouped into{" "}
                  <strong className="text-foreground font-medium">
                    {suggestion.target_workspace_title}
                  </strong>{" "}
                  <span className="text-muted-foreground">· {suggestion.reason}</span>
                </>
              ) : (
                <>
                  Looks related to{" "}
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
                    suggestion.previous_workspace_id ?? null,
                  );
                  void orbitActions.dismissClusterSuggestion(
                    selectedHostId,
                    activeAgent.agent_id,
                  );
                }}
                className="border-border bg-canvas hover:bg-accent inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium"
              >
                <Undo2 className="h-[11px] w-[11px]" />
                <span>Undo</span>
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
                <span>Group together</span>
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
