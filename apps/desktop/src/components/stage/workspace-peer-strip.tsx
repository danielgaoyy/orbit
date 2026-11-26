import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  AlarmClock,
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
  isAgentForegroundRunning,
  isAgentTimerWaiting,
  isMacDesktop,
  isSubagentCard,
  type ProviderKind,
  type SessionMode,
  semanticAgentTabLabel,
} from "@/lib/orbit-ipc";
import { useI18n } from "@/lib/i18n";
import { useImeCompositionGuard } from "@/lib/ime";
import { normalizeProjectPath } from "@/lib/projects";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { Select, type SelectOption } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
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

type PresetSessionMode = "auto" | "accept_edits" | "plan" | "full_access" | "default";

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
 * Progressive-disclosure contextual strip rendered ONLY when the active conversation belongs to a
 * Group or has multiple collaborating agents (0px for standalone threads and subagent threads).
 * Outer boolean gate prevents standalone sessions from subscribing to `gitReposCache`/
 * `threadCustomTitles` or running filter/sort passes on every `host_changed` heartbeat.
 */
export const WorkspacePeerStrip = memo(function WorkspacePeerStrip() {
  const shouldShow = useOrbitSelector((s) => {
    if (
      s.workspacePanel !== "conversation" ||
      s.isDraftNewSession ||
      !s.selectedHostId ||
      !s.selectedAgentId
    ) {
      return false;
    }
    const hostDetail = s.hostDetails[s.selectedHostId];
    if (!hostDetail) return false;
    const activeAgent = hostDetail.agents.find((a) => a.agent_id === s.selectedAgentId);
    if (!activeAgent || isSubagentCard(activeAgent)) return false;
    const workspace = hostDetail.workspaces.find(
      (w) => w.workspace_id === activeAgent.workspace_id,
    );
    if (workspace?.is_curated) return true;
    let peerCount = 0;
    for (const a of hostDetail.agents) {
      if (
        a.workspace_id === activeAgent.workspace_id &&
        !a.archived_at_rfc3339 &&
        !a.background &&
        !isSubagentCard(a)
      ) {
        peerCount += 1;
        if (peerCount > 1) return true;
      }
    }
    const events = hostDetail.workspace_events?.[activeAgent.workspace_id];
    return Boolean(events?.some((ev) => ev.kind === "conflict_warning"));
  });

  if (!shouldShow) return null;
  return <WorkspacePeerStripContent />;
});

const WorkspacePeerStripContent = memo(function WorkspacePeerStripContent() {
  const { t } = useI18n();
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
  const peerTabsRef = useRef<HTMLDivElement | null>(null);
  const ime = useImeCompositionGuard();

  const peerModeOptions = useMemo<readonly SelectOption<PresetSessionMode>[]>(
    () => [
      { value: "auto", label: t("composer.modeAuto") },
      { value: "accept_edits", label: t("composer.modeAcceptEdits") },
      { value: "plan", label: t("peerStrip.modePlan") },
      { value: "full_access", label: t("composer.modeFullAccess") },
      { value: "default", label: t("composer.modeAlwaysAsk") },
    ],
    [t],
  );

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
    if (!hostDetail || !activeAgent || isSubagentCard(activeAgent)) return [];
    return hostDetail.agents
      .filter(
        (a) =>
          a.workspace_id === activeAgent.workspace_id &&
          !a.archived_at_rfc3339 &&
          !a.background &&
          !isSubagentCard(a),
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
    isSubagentCard(activeAgent) ||
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
    (gitInsp?.nested_git_repos?.length ?? 0) > 0;
  const worktreeCwd =
    gitInsp && !gitInsp.is_git_repo && (gitInsp.nested_git_repos?.length ?? 0) > 0
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
      orbitActions.showToast(t("peerStrip.failedAddCollaborator", { error: String(err) }));
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
      orbitActions.showToast(t("peerStrip.failedAddCollaborator", { error: String(err) }));
    } finally {
      setSpawning(false);
    }
  };

  const notesTooltip =
    boardRev > 0
      ? t("peerStrip.notesTooltipRev", { rev: boardRev })
      : t("peerStrip.notes");
  const latestConflict =
    recentConflicts.length > 0 ? recentConflicts[recentConflicts.length - 1] : null;
  const conflictFileSuffix = latestConflict?.file_path
    ? ` · ${latestConflict.file_path.split("/").pop()}`
    : "";

  return (
    <div className="@container bg-surface-base border-border-subtle z-10 flex h-[30px] min-w-0 shrink-0 items-center justify-between gap-1.5 overflow-hidden border-b px-2.5 select-none @[520px]:gap-2 @[520px]:px-3">
      {/* Left: Quiet Group Switcher + Scrollable Sibling Thread Tabs + Pinned Add Button */}
      <div className="flex min-w-0 flex-1 items-center gap-1.5">
        <Popover open={groupSwitcherOpen} onOpenChange={setGroupSwitcherOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              title={
                workspace?.title
                  ? t("peerStrip.groupTitleTooltip", { title: workspace.title })
                  : t("peerStrip.manageGroup")
              }
              className="text-foreground/78 hover:text-foreground inline-flex h-[22px] shrink-0 cursor-pointer items-center gap-1 rounded-[6px] px-1.5 text-[13px] font-medium transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06] @[480px]:gap-1.5"
            >
              <Layers className="text-muted-foreground h-[12px] w-[12px] shrink-0 stroke-[1.75]" />
              <span className="max-w-[72px] truncate @[480px]:max-w-[112px] @[640px]:max-w-[144px]">
                {workspace?.title || t("peerStrip.group")}
              </span>
              <ChevronDown className="h-[11px] w-[11px] shrink-0 opacity-50" />
            </button>
          </PopoverTrigger>

          <PopoverContent
            align="start"
            side="bottom"
            sideOffset={6}
            finalFocus={false}
            className="w-[280px] space-y-2.5 overflow-hidden p-2.5 select-none"
          >
            {workspace && (
              <div className="border-border/60 space-y-1.5 border-b pb-2.5">
                <div className="text-muted-foreground flex items-center justify-between text-[11px] font-medium">
                  <span>{t("peerStrip.currentGroup")}</span>
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
                    <span>{t("peerStrip.notes")}</span>
                  </button>
                </div>
                <div className="flex items-center gap-1.5">
                  <Input
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
                    placeholder={t("peerStrip.groupNameInputPlaceholder")}
                    className="h-[26px] flex-1 rounded-[6px] px-2 text-[13px]"
                  />
                  <Button
                    type="button"
                    size="xs"
                    disabled={!groupRenameDraft.trim() || groupRenameDraft.trim() === workspace.title}
                    onClick={() => void handleRenameCurrentGroup()}
                    className="h-[26px] shrink-0 px-2 text-[13px]"
                  >
                    {t("peerStrip.save")}
                  </Button>
                </div>
              </div>
            )}

            {projectGroups.length > 1 && (
              <div className="space-y-1">
                <div className="text-muted-foreground px-0.5 text-[11px] font-medium">
                  {t("peerStrip.moveThreadTo")}
                </div>
                <div className="orbit-native-scroll -mx-2.5 max-h-36 space-y-0.5 overflow-y-auto overscroll-contain px-2.5">
                  {projectGroups.map((ws) => {
                    const isCurrent = ws.workspace_id === activeAgent.workspace_id;
                    const count = hostDetail.agents.filter(
                      (a) =>
                        a.workspace_id === ws.workspace_id &&
                        !a.archived_at_rfc3339 &&
                        !a.background &&
                        !isSubagentCard(a),
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
                          "flex w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[13px] transition-colors",
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

            <div className="border-border/60 space-y-1.5 border-t pt-2">
              <div className="text-muted-foreground flex items-center gap-1 text-[11px] font-medium">
                <FolderPlus className="h-[11.5px] w-[11.5px]" />
                <span>{t("peerStrip.newGroup")}</span>
              </div>
              <div className="flex items-center gap-1.5">
                <Input
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
                  placeholder={t("peerStrip.groupNamePlaceholder")}
                  className="h-[26px] flex-1 rounded-[6px] px-2 text-[13px]"
                />
                <Button
                  type="button"
                  size="xs"
                  disabled={!newGroupTitle.trim()}
                  onClick={() => void handleCreateAndMoveGroup()}
                  className="h-[26px] shrink-0 px-2 text-[13px]"
                >
                  {t("peerStrip.create")}
                </Button>
              </div>
            </div>

            {isCuratedGroup && (
              <div className="border-border/60 border-t pt-1.5">
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
                  className="text-muted-foreground hover:text-foreground hover:bg-accent flex w-full cursor-pointer items-center gap-1.5 rounded-md px-2 py-1 text-left text-[13px] transition-colors"
                >
                  <X className="h-[12.5px] w-[12.5px]" />
                  <span>{t("peerStrip.removeFromGroup")}</span>
                </button>
              </div>
            )}
          </PopoverContent>
        </Popover>

        <span className="h-3 w-[1px] shrink-0 bg-black/[0.08] dark:bg-white/[0.10]" />

        {/* Segmented Sibling Thread Switcher (horizontally scrollable without clipping `+`) */}
        <div
          ref={peerTabsRef}
          onWheel={(e) => {
            const el = peerTabsRef.current;
            if (!el || el.scrollWidth <= el.clientWidth) return;
            if (Math.abs(e.deltaY) > Math.abs(e.deltaX)) {
              el.scrollLeft += e.deltaY;
            }
          }}
          className="flex min-w-0 items-center gap-0.5 overflow-x-auto pr-2 [mask-image:linear-gradient(to_right,black_calc(100%-10px),transparent)] [scrollbar-width:none] [&::-webkit-scrollbar]:hidden"
        >
          {peers.map((peer) => {
            const isActive = peer.agent_id === activeAgent.agent_id;
            const tKey = threadScopeKey(selectedHostId, peer.agent_id);
            const label = semanticAgentTabLabel(peer, threadCustomTitles[tKey]);
            const isEditingThis = editingPeerId === peer.agent_id;
            const tooltipParts = [
              `${label} (${providerShortName(peer.provider)}${peer.model ? ` · ${peer.model}` : ""})`,
              peer.task_summary || peer.title,
              isActive
                ? t("peerStrip.doubleClickToRename")
                : t("peerStrip.clickToSwitchOrSplit", { mod: isMacDesktop() ? "⌘" : "Ctrl" }),
            ];

            return (
              <div
                key={peer.agent_id}
                className={cn(
                  "group/peer inline-flex h-[22px] shrink-0 items-center gap-1 rounded-[6px] px-1.5 text-[13px] transition-colors @[520px]:px-2",
                  isActive
                    ? "bg-black/[0.055] text-foreground font-medium dark:bg-white/[0.085]"
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
                      className="bg-background text-foreground ring-foreground/25 h-[18px] w-24 rounded px-1 text-[13px] font-normal ring-1 outline-none"
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
                      {isAgentForegroundRunning(peer) ? (
                        <OrbitSpinner size={11} className="text-foreground/70" />
                      ) : peer.is_awaiting_permission ? (
                        <span className="h-[5.5px] w-[5.5px] shrink-0 rounded-full bg-amber-500" />
                      ) : peer.is_unread && !isActive ? (
                        <span className="h-[5.5px] w-[5.5px] shrink-0 rounded-full bg-sky-500" />
                      ) : isAgentTimerWaiting(peer) ? (
                        <AlarmClock className="text-ink-faint h-[11.5px] w-[11.5px] shrink-0 stroke-[1.7]" />
                      ) : (
                        <ProviderIcon
                          provider={peer.provider}
                          className="h-[11.5px] w-[11.5px] shrink-0 opacity-70"
                        />
                      )}
                      <span className="max-w-[96px] truncate @[520px]:max-w-[132px]">{label}</span>
                      {peer.worktree_branch && (
                        <span className="text-muted-foreground/70 hidden max-w-20 items-center gap-0.5 truncate font-mono text-[11px] @[640px]:inline-flex">
                          <GitBranch className="h-[9.5px] w-[9.5px] shrink-0" />
                          <span className="truncate">{peer.worktree_branch}</span>
                        </span>
                      )}
                    </button>

                    <SimpleTooltip label={t("common.rename")}>
                      <button
                        type="button"
                        aria-label={t("common.rename")}
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
                      <SimpleTooltip label={t("peerStrip.openSideBySide", { label })}>
                        <button
                          type="button"
                          aria-label={t("peerStrip.openSideBySide", { label })}
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
        </div>

        {/* Pinned Add Collaborator Button (never clipped by scrolling peer tabs) */}
        <Popover
          open={spawnOpen}
          onOpenChange={(nextOpen, eventDetails) => {
            if (!nextOpen && eventDetails?.reason === "outside-press") {
              const target = eventDetails.event?.target;
              if (
                target instanceof Element &&
                target.closest('[data-slot="select-content"], [data-slot="select-item"], [role="listbox"]')
              ) {
                return;
              }
            }
            setSpawnOpen(nextOpen);
          }}
        >
          <SimpleTooltip label={t("peerStrip.addCollaborator")}>
            <PopoverTrigger asChild>
              <button
                type="button"
                aria-label={t("peerStrip.addCollaborator")}
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
            className="w-[312px] space-y-2.5 p-2.5 select-none"
          >
            <div className="flex items-center justify-between px-1">
              <span className="text-foreground flex items-center gap-1.5 text-[13px] font-medium">
                <Users className="text-muted-foreground h-[13.5px] w-[13.5px]" />
                <span>{t("peerStrip.addCollaborator")}</span>
              </span>
              <span className="text-muted-foreground text-[11px] tabular-nums">
                {t("peerStrip.activeCount", { count: peers.length })}
              </span>
            </div>

            <div className="grid grid-cols-1 gap-0.5">
              <button
                type="button"
                disabled={spawning}
                onClick={() => void launchPresetPeer("reviewer")}
                className="hover:bg-accent flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left transition-colors disabled:opacity-50"
              >
                <div className="min-w-0">
                  <div className="text-foreground text-[13px] font-medium">
                    {t("peerStrip.codeReviewerTitle")}
                  </div>
                  <div className="text-muted-foreground text-[11px]">
                    {t("peerStrip.reviewerNotesDesc", {
                      provider: providerShortName(
                        pickAlternateProvider(activeAgent.provider, availableProviders),
                      ),
                    })}
                  </div>
                </div>
                <Plus className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
              </button>

              {canWorktree && (
                <button
                  type="button"
                  disabled={spawning}
                  onClick={() => void launchPresetPeer("worktree_worker")}
                  className="hover:bg-accent flex cursor-pointer items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left transition-colors disabled:opacity-50"
                >
                  <div className="min-w-0">
                    <div className="text-foreground flex items-center gap-1.5 text-[13px] font-medium">
                      <GitBranch className="h-[12px] w-[12px] opacity-75" />
                      <span>{t("peerStrip.parallelWorkerTitle")}</span>
                    </div>
                    <div className="text-muted-foreground text-[11px]">
                      {t("peerStrip.parallelWorkerGroupDesc")}
                    </div>
                  </div>
                  <Plus className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
                </button>
              )}
            </div>

            <div className="border-border/60 space-y-2 border-t pt-2.5">
              <div className="text-muted-foreground px-0.5 text-[11px] font-medium">
                {t("peerStrip.customTask")}
              </div>
              <div className="grid grid-cols-2 gap-1.5">
                <Input
                  value={customHandle}
                  onChange={(e) => setCustomHandle(e.target.value)}
                  placeholder={t("peerStrip.labelPlaceholder")}
                  className="h-[28px] rounded-[6px] px-2 text-[13px]"
                />
                <Select<ProviderKind>
                  size="compact"
                  align="end"
                  ariaLabel={t("peerStrip.peerProviderAria")}
                  value={effectiveCustomProvider}
                  options={availableProviders.map((p) => ({
                    value: p,
                    label: providerShortName(p),
                  }))}
                  triggerClassName="h-[28px] w-full bg-canvas text-[13px]"
                  menuClassName="min-w-[144px]"
                  onChange={(provider) => setCustomProvider(provider)}
                />
              </div>
              <Textarea
                rows={2}
                value={customPrompt}
                onChange={(e) => setCustomPrompt(e.target.value)}
                placeholder={t("peerStrip.customPromptPlaceholder")}
                className="min-h-[54px] resize-none rounded-[6px] p-2 text-[13px]"
              />
              <div className="flex items-center justify-between gap-2">
                <div className="flex items-center gap-2">
                  <Select<PresetSessionMode>
                    size="compact"
                    align="start"
                    ariaLabel={t("peerStrip.peerPermissionModeAria")}
                    value={typeof customMode === "string" ? customMode : "accept_edits"}
                    options={peerModeOptions}
                    triggerClassName="h-[25px] bg-canvas text-[11px]"
                    menuClassName="min-w-[152px]"
                    onChange={(mode) => setCustomMode(mode)}
                  />
                  {canWorktree && (
                    <label className="text-muted-foreground inline-flex cursor-pointer items-center gap-1.5 text-[11px] select-none">
                      <Checkbox
                        size="sm"
                        checked={customWorktree}
                        onCheckedChange={setCustomWorktree}
                      />
                      <span>{t("peerStrip.worktree")}</span>
                    </label>
                  )}
                </div>
                <Button
                  type="button"
                  size="xs"
                  disabled={!customPrompt.trim() || spawning}
                  onClick={() => void launchCustomPeer()}
                  className="h-[26px] gap-1 px-2.5 text-[13px]"
                >
                  {spawning ? (
                    <Loader2 className="h-[12px] w-[12px] animate-spin" />
                  ) : (
                    <Plus className="h-[12px] w-[12px]" />
                  )}
                  <span>{t("common.start")}</span>
                </Button>
              </div>
            </div>
          </PopoverContent>
        </Popover>
      </div>

      {/* Right: Conditional Conflict Indicator + Quiet Notes Link */}
      <div className="flex shrink-0 items-center gap-1">
        {latestConflict && (
          <SimpleTooltip label={latestConflict.summary}>
            <button
              type="button"
              aria-label={latestConflict.summary}
              onClick={() =>
                orbitActions.openWorkspaceBoardTab(
                  selectedHostId,
                  activeAgent.workspace_id,
                  workspace?.title,
                )
              }
              className="inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center gap-1 rounded-full bg-amber-500/[0.1] px-0 text-[11px] font-medium text-amber-700 transition-colors hover:bg-amber-500/[0.16] dark:text-amber-300 @[520px]:w-auto @[520px]:px-2"
            >
              <AlertTriangle className="h-[11px] w-[11px] shrink-0" />
              <span className="hidden max-w-28 truncate @[520px]:inline">
                {t("peerStrip.conflict")}
                {conflictFileSuffix}
              </span>
            </button>
          </SimpleTooltip>
        )}

        <SimpleTooltip label={notesTooltip}>
          <button
            type="button"
            aria-label={notesTooltip}
            onClick={() =>
              orbitActions.openWorkspaceBoardTab(
                selectedHostId,
                activeAgent.workspace_id,
                workspace?.title,
              )
            }
            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center gap-1 rounded-[6px] px-0 text-[11px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06] @[480px]:w-auto @[480px]:px-1.5"
          >
            <FileText className="h-[12px] w-[12px] shrink-0 stroke-[1.75]" />
            <span className="hidden @[480px]:inline">{t("peerStrip.notes")}</span>
            {boardRev > 0 && (
              <span className="text-muted-foreground/65 hidden font-mono text-[11px] @[560px]:inline">
                v{boardRev}
              </span>
            )}
          </button>
        </SimpleTooltip>
      </div>
    </div>
  );
});
