import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronDown,
  Folder,
  FolderGit2,
  FolderPlus,
  GitBranch,
  Inbox,
  Laptop,
  Layers,
  Search,
  Server,
} from "lucide-react";
import { basenameOfCwd } from "@/components/composer/composer-constants";
import { useImeCompositionGuard } from "@/lib/ime";
import {
  isBareHomeDir,
  matchesProjectDirectory,
  normalizeProjectPath,
} from "@/lib/projects";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

interface HeroProjectOption {
  id: string;
  hostId: string;
  hostName: string;
  name: string;
  path: string;
  directories: string[];
  branch: string | null;
  isSidebarProject: boolean;
  sidebarRank: number;
}

function formatCompactParentDir(path: string): string {
  const clean = normalizeProjectPath(path);
  const idx = clean.lastIndexOf("/");
  if (idx <= 0) return clean;
  const parent = clean.slice(0, idx);
  return parent.replace(/^\/(?:Users|home)\/[^/]+/, "~");
}

/**
 * Isolated Hero Launchpad Header (`[Project ▾] / [Workspace ▾]`, optional `[Host ▾]`, `[Local / Worktree ▾]`).
 * Never subscribes to `composerDrafts`, so typing in the `<textarea>` triggers zero re-renders here.
 */
export const ComposerHeroHeader = memo(function ComposerHeroHeader() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const draftWorktreeRepoPath = useOrbitSelector((s) => s.draftWorktreeRepoPath);
  const draftWorkspaceId = useOrbitSelector((s) => s.draftWorkspaceId);
  const gitReposCache = useOrbitSelector((s) => s.gitReposCache);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const hiddenProjectKeys = useOrbitSelector((s) => s.hiddenProjectKeys);
  const hostDetail = selectedHostId ? (hostDetails[selectedHostId] ?? null) : null;

  const [projectPickerOpen, setProjectPickerOpen] = useState(false);
  const [projectQuery, setProjectQuery] = useState("");
  const projectSearchInputRef = useRef<HTMLInputElement | null>(null);
  const ime = useImeCompositionGuard();

  const heroProjects = useMemo<HeroProjectOption[]>(() => {
    const hostNameById = new Map(fleet.hosts.map((h) => [h.host_id, h.display_name]));
    const sidebarHostProjects = (sidebarView?.projects ?? []).filter(
      (p) => (!selectedHostId || p.host_id === selectedHostId) && !p.is_home_workspace,
    );
    const sidebarRankByPath = new Map<string, number>();
    const sidebarProjByPath = new Map<string, (typeof sidebarHostProjects)[number]>();
    sidebarHostProjects.forEach((p, idx) => {
      const norm = normalizeProjectPath(p.primary_cwd);
      sidebarRankByPath.set(norm, idx);
      sidebarProjByPath.set(norm, p);
      for (const dir of p.directories) {
        const dNorm = normalizeProjectPath(dir);
        if (!sidebarRankByPath.has(dNorm)) {
          sidebarRankByPath.set(dNorm, idx);
          sidebarProjByPath.set(dNorm, p);
        }
      }
    });

    const items: HeroProjectOption[] = [];
    const seenPaths = new Set<string>();

    const pushOption = (opt: HeroProjectOption) => {
      const key = `${opt.hostId}::${normalizeProjectPath(opt.path)}`;
      if (seenPaths.has(key)) return;
      seenPaths.add(key);
      items.push(opt);
    };

    // 1. Active & pinned sidebar projects first (matching exact sidebar order & pure project names)
    sidebarHostProjects.forEach((p, idx) => {
      const clean = normalizeProjectPath(p.primary_cwd);
      const hiddenKey = `${p.host_id}::${clean}`;
      const hasContent =
        p.conversations.length > 0 ||
        (p.workspaces ?? []).some((w) => w.is_curated !== false) ||
        Boolean(p.pinned_project_id);
      if (!hasContent && hiddenProjectKeys[hiddenKey]) return;
      pushOption({
        id: p.id,
        hostId: p.host_id,
        hostName: hostNameById.get(p.host_id) ?? "Host",
        name: p.name || basenameOfCwd(clean),
        path: clean,
        directories: p.directories.length > 0 ? p.directories : [clean],
        branch: p.git_branch ?? null,
        isSidebarProject: true,
        sidebarRank: idx,
      });
    });

    // 2. Additional discovered/launchpad directories on the active host
    for (const lp of sidebarView?.launchpad_projects ?? []) {
      if (selectedHostId && lp.host_id !== selectedHostId) continue;
      const clean = normalizeProjectPath(lp.path);
      if (isBareHomeDir(clean) || clean.includes("/worktrees/")) continue;
      const hiddenKey = `${lp.host_id}::${clean}`;
      const matchedSidebar = sidebarProjByPath.get(clean);
      const hasConvs = Boolean(matchedSidebar && matchedSidebar.conversations.length > 0);
      if (!hasConvs && hiddenProjectKeys[hiddenKey]) continue;

      pushOption({
        id: matchedSidebar?.id ?? lp.id,
        hostId: lp.host_id,
        hostName: hostNameById.get(lp.host_id) ?? "Host",
        name: matchedSidebar?.name || lp.name || basenameOfCwd(clean),
        path: clean,
        directories: matchedSidebar?.directories ?? [clean],
        branch: lp.branch ?? matchedSidebar?.git_branch ?? null,
        isSidebarProject: Boolean(matchedSidebar),
        sidebarRank: sidebarRankByPath.get(clean) ?? Number.MAX_SAFE_INTEGER,
      });
    }

    const cleanSelected = normalizeProjectPath(selectedCwd);
    const hasSelected =
      Boolean(selectedHostId) &&
      items.some(
        (it) =>
          it.hostId === selectedHostId &&
          matchesProjectDirectory(it.directories, cleanSelected),
      );

    if (
      selectedHostId &&
      cleanSelected &&
      !hasSelected &&
      !isBareHomeDir(cleanSelected) &&
      !cleanSelected.includes("/worktrees/")
    ) {
      const branch =
        hostDetail?.workspaces.find(
          (w) => !w.is_worktree && normalizeProjectPath(w.cwd) === cleanSelected,
        )?.git_branch ??
        hostDetail?.discovered_projects.find(
          (p) => normalizeProjectPath(p.path) === cleanSelected,
        )?.git_branch ??
        null;
      items.unshift({
        id: `${selectedHostId}::${cleanSelected}`,
        hostId: selectedHostId,
        hostName: hostNameById.get(selectedHostId) ?? "Host",
        name: basenameOfCwd(cleanSelected),
        path: cleanSelected,
        directories: [cleanSelected],
        branch,
        isSidebarProject: true,
        sidebarRank: -1,
      });
    }

    items.sort((a, b) => {
      if (a.isSidebarProject !== b.isSidebarProject) {
        return a.isSidebarProject ? -1 : 1;
      }
      if (a.sidebarRank !== b.sidebarRank) {
        return a.sidebarRank - b.sidebarRank;
      }
      return a.name.toLowerCase().localeCompare(b.name.toLowerCase());
    });

    return items;
  }, [
    fleet.hosts,
    sidebarView,
    hiddenProjectKeys,
    selectedHostId,
    selectedCwd,
    hostDetail,
  ]);

  const duplicateNames = useMemo(() => {
    const counts = new Map<string, number>();
    for (const proj of heroProjects) {
      const key = proj.name.toLowerCase();
      counts.set(key, (counts.get(key) ?? 0) + 1);
    }
    return new Set(
      Array.from(counts.entries())
        .filter(([, c]) => c > 1)
        .map(([k]) => k),
    );
  }, [heroProjects]);

  const filteredHeroProjects = useMemo(() => {
    const q = projectQuery.trim().toLowerCase();
    if (!q) return heroProjects;
    return heroProjects.filter(
      (p) =>
        p.name.toLowerCase().includes(q) ||
        p.path.toLowerCase().includes(q) ||
        (p.branch?.toLowerCase().includes(q) ?? false),
    );
  }, [heroProjects, projectQuery]);

  const activeProjectsSection = useMemo(
    () => filteredHeroProjects.filter((p) => p.isSidebarProject),
    [filteredHeroProjects],
  );
  const discoveredProjectsSection = useMemo(
    () => filteredHeroProjects.filter((p) => !p.isSidebarProject),
    [filteredHeroProjects],
  );

  useEffect(() => {
    if (!projectPickerOpen) {
      setProjectQuery("");
      return;
    }
    if (heroProjects.length >= 6) {
      const raf = window.requestAnimationFrame(() => {
        projectSearchInputRef.current?.focus();
      });
      return () => window.cancelAnimationFrame(raf);
    }
  }, [projectPickerOpen, heroProjects.length]);

  const activeHeroProject = useMemo(() => {
    if (!selectedHostId) return null;
    return (
      heroProjects.find(
        (it) =>
          it.hostId === selectedHostId &&
          matchesProjectDirectory(it.directories, selectedCwd),
      ) ?? null
    );
  }, [heroProjects, selectedHostId, selectedCwd]);

  const cleanSelectedCwd = normalizeProjectPath(selectedCwd);
  const gitCacheKey = selectedHostId ? `${selectedHostId}::${cleanSelectedCwd}` : "";
  const cachedInspection =
    (gitCacheKey ? gitReposCache[gitCacheKey] : undefined) ??
    (selectedHostId ? hostDetail?.git_repos_by_cwd?.[cleanSelectedCwd] : undefined) ??
    null;

  useEffect(() => {
    if (!selectedHostId || !cleanSelectedCwd || cleanSelectedCwd === "~") return;
    if (cachedInspection) return;
    void orbitActions.inspectGitRepos(
      selectedHostId,
      cleanSelectedCwd,
      activeHeroProject?.directories ?? [],
    );
  }, [selectedHostId, cleanSelectedCwd, cachedInspection, activeHeroProject?.directories]);

  const { isDirectGitRepo, nestedGitRepos } = useMemo(() => {
    const sidebarProj = sidebarView?.projects.find(
      (p) =>
        p.host_id === selectedHostId &&
        matchesProjectDirectory(p.directories, cleanSelectedCwd),
    );
    const lpProj = sidebarView?.launchpad_projects.find(
      (p) => p.host_id === selectedHostId && normalizeProjectPath(p.path) === cleanSelectedCwd,
    );
    const discProj = hostDetail?.discovered_projects.find(
      (p) => normalizeProjectPath(p.path) === cleanSelectedCwd,
    );
    const direct =
      cachedInspection?.is_git_repo ??
      sidebarProj?.is_git_repo ??
      lpProj?.is_git_repo ??
      discProj?.is_git_repo ??
      false;
    const nested =
      cachedInspection?.nested_git_repos ??
      sidebarProj?.nested_git_repos ??
      lpProj?.nested_git_repos ??
      discProj?.nested_git_repos ??
      [];
    return { isDirectGitRepo: direct, nestedGitRepos: nested };
  }, [cachedInspection, sidebarView, selectedHostId, cleanSelectedCwd, hostDetail]);

  const selectedNestedRepo = useMemo(
    () =>
      draftWorktreeRepoPath
        ? nestedGitRepos.find((r) => r.path === draftWorktreeRepoPath) ?? null
        : null,
    [draftWorktreeRepoPath, nestedGitRepos],
  );

  const canUseWorktree = isDirectGitRepo || nestedGitRepos.length > 0;
  const effectiveUsingWorktree =
    draftUseWorktree && (isDirectGitRepo || Boolean(selectedNestedRepo));

  const activeRepoBranch = useMemo(() => {
    if (selectedNestedRepo?.git_branch) return selectedNestedRepo.git_branch;
    if (cachedInspection?.git_branch) return cachedInspection.git_branch;
    if (activeHeroProject?.branch) return activeHeroProject.branch;
    if (!hostDetail) return null;
    const clean = normalizeProjectPath(selectedCwd);
    return (
      hostDetail.workspaces.find((w) => normalizeProjectPath(w.cwd) === clean)?.git_branch ??
      hostDetail.discovered_projects.find((p) => normalizeProjectPath(p.path) === clean)
        ?.git_branch ??
      null
    );
  }, [selectedNestedRepo, cachedInspection, activeHeroProject, hostDetail, selectedCwd]);

  const activeProjectLabel = activeHeroProject?.name || basenameOfCwd(selectedCwd);
  const curatedWorkspacesForProject = useMemo(() => {
    const sidebarProj = sidebarView?.projects.find(
      (p) =>
        p.host_id === selectedHostId &&
        matchesProjectDirectory(p.directories, cleanSelectedCwd),
    );
    return (sidebarProj?.workspaces ?? []).filter((w) => w.is_curated !== false);
  }, [sidebarView, selectedHostId, cleanSelectedCwd]);
  const selectedDraftWs = useMemo(
    () =>
      draftWorkspaceId
        ? curatedWorkspacesForProject.find((w) => w.workspace_id === draftWorkspaceId) ??
          hostDetail?.workspaces.find((w) => w.workspace_id === draftWorkspaceId) ??
          null
        : null,
    [draftWorkspaceId, curatedWorkspacesForProject, hostDetail],
  );
  const multiHost = fleet.hosts.length > 1;
  const activeHostCard = fleet.hosts.find((h) => h.host_id === selectedHostId) ?? null;
  const activeHostLabel =
    hostDetail?.host.display_name ?? activeHostCard?.display_name ?? "Host";
  const activeHostOnline = Boolean(
    activeHostCard?.connection_state.startsWith("online"),
  );

  const renderProjectRow = (proj: HeroProjectOption) => {
    const active =
      proj.hostId === selectedHostId &&
      matchesProjectDirectory(proj.directories, selectedCwd);
    const showParent = duplicateNames.has(proj.name.toLowerCase());
    return (
      <button
        key={proj.id}
        type="button"
        title={proj.path}
        onClick={() => {
          orbitActions.selectProjectWorkspace(proj.hostId, proj.path);
          setProjectPickerOpen(false);
        }}
        className={cn(
          "flex w-full cursor-pointer items-center justify-between gap-2.5 rounded-[9px] px-2.5 py-1.5 text-left text-[13px] transition-colors outline-none",
          active
            ? "bg-foreground/[0.07] text-foreground font-medium dark:bg-white/[0.09]"
            : "text-foreground/84 hover:bg-accent hover:text-foreground",
        )}
      >
        <span className="flex min-w-0 flex-1 items-center gap-2">
          <Folder
            className={cn(
              "size-4 shrink-0 stroke-[1.75]",
              active ? "text-foreground/85" : "text-muted-foreground",
            )}
          />
          <span className="flex min-w-0 flex-1 flex-col">
            <span className="truncate leading-tight">{proj.name}</span>
            {showParent && (
              <span className="text-muted-foreground/75 truncate text-[11px] leading-tight font-normal">
                {formatCompactParentDir(proj.path)}
              </span>
            )}
          </span>
        </span>
        <span className="flex shrink-0 items-center gap-1.5">
          {proj.branch && (
            <span className="text-muted-foreground/70 max-w-20 truncate font-mono text-[11px] font-normal">
              {proj.branch}
            </span>
          )}
          {active && (
            <Check className="text-foreground size-3.5 shrink-0 stroke-[2]" />
          )}
        </span>
      </button>
    );
  };

  return (
    <div className="flex flex-wrap items-center justify-between gap-2 px-1 pb-2">
      <div className="flex min-w-0 items-center gap-0.5">
        {/* 1. Clean Project Switcher (Strictly Directories — Never Workspaces!) */}
        <Popover open={projectPickerOpen} onOpenChange={setProjectPickerOpen}>
          <PopoverTrigger asChild>
            <button
              type="button"
              title={`Project: ${selectedCwd}`}
              className="text-foreground/80 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              <Folder className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
              <span className="max-w-44 truncate font-medium">{activeProjectLabel}</span>
              <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
            </button>
          </PopoverTrigger>
          <PopoverContent
            side="bottom"
            align="start"
            sideOffset={6}
            finalFocus={false}
            className="w-[292px] p-1.5 select-none"
          >
            <div className="flex items-center justify-between gap-2 px-2.5 pt-1 pb-1.5">
              <span className="text-muted-foreground text-[11.5px] font-medium">
                Projects
              </span>
              {multiHost && (
                <span className="text-muted-foreground/80 inline-flex max-w-32 items-center gap-1.5 truncate text-[11px]">
                  <span
                    className={cn(
                      "size-1.5 shrink-0 rounded-full",
                      activeHostOnline ? "bg-emerald-500" : "bg-zinc-400",
                    )}
                  />
                  <span className="truncate">{activeHostLabel}</span>
                </span>
              )}
            </div>

            {heroProjects.length >= 6 && (
              <div className="px-1 pb-1.5">
                <div className="border-border/80 bg-canvas/80 flex h-[28px] items-center gap-1.5 rounded-lg border px-2">
                  <Search className="text-muted-foreground/70 size-3.5 shrink-0" />
                  <input
                    ref={projectSearchInputRef}
                    value={projectQuery}
                    onChange={(e) => setProjectQuery(e.target.value)}
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      if (ime.shouldIgnoreKeyDown(e)) return;
                      if (e.key === "Escape") {
                        e.stopPropagation();
                        setProjectPickerOpen(false);
                      } else if (e.key === "Enter" && filteredHeroProjects.length > 0) {
                        e.preventDefault();
                        const first = filteredHeroProjects[0];
                        orbitActions.selectProjectWorkspace(first.hostId, first.path);
                        setProjectPickerOpen(false);
                      }
                    }}
                    placeholder="Filter projects…"
                    className="text-foreground placeholder:text-muted-foreground/60 min-w-0 flex-1 bg-transparent text-[12px] outline-none"
                  />
                </div>
              </div>
            )}

            {filteredHeroProjects.length === 0 ? (
              <div className="text-muted-foreground/75 px-2.5 py-3 text-center text-[12px]">
                No matching projects
              </div>
            ) : (
              <ScrollArea viewportClassName="max-h-72">
                <div className="space-y-0.5">
                  {activeProjectsSection.map(renderProjectRow)}

                  {activeProjectsSection.length > 0 && discoveredProjectsSection.length > 0 && (
                    <div className="pt-1.5 pb-0.5">
                      <div className="bg-border/70 mx-1 mb-1.5 h-px" />
                      <div className="text-muted-foreground/75 px-2.5 pb-1 text-[11px] font-medium">
                        Other folders
                      </div>
                    </div>
                  )}

                  {discoveredProjectsSection.map(renderProjectRow)}
                </div>
              </ScrollArea>
            )}

            <div className="bg-border/80 -mx-1.5 my-1.5 h-px" />
            <button
              type="button"
              onClick={() => {
                setProjectPickerOpen(false);
                void orbitActions.promptAddProject(selectedHostId ?? undefined);
              }}
              className="text-foreground/82 hover:bg-accent hover:text-foreground flex w-full cursor-pointer items-center gap-2 rounded-[9px] px-2.5 py-1.5 text-left text-[13px] transition-colors"
            >
              <FolderPlus className="text-muted-foreground size-4 shrink-0 stroke-[1.75]" />
              <span>Add new project…</span>
            </button>
          </PopoverContent>
        </Popover>

        {/* Optional Curated Mission Workspace Selector (child of Project; defaults to 待归类) */}
        {(curatedWorkspacesForProject.length > 0 || selectedDraftWs) && (
          <>
            <span className="text-foreground/25 select-none text-[13px]">/</span>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  title={
                    selectedDraftWs
                      ? `Workspace: ${selectedDraftWs.title}`
                      : "未指定 Workspace（发送后将放入「待归类」并自动识别）"
                  }
                  className="text-foreground/72 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
                >
                  {selectedDraftWs ? (
                    <Layers className="size-3.5 shrink-0 stroke-[1.75] text-sky-500" />
                  ) : (
                    <Inbox className="size-3.5 shrink-0 stroke-[1.75] opacity-70" />
                  )}
                  <span className="max-w-36 truncate">
                    {selectedDraftWs ? selectedDraftWs.title : "待归类"}
                  </span>
                  <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent
                align="start"
                side="bottom"
                finalFocus={false}
                className="w-60 p-1.5"
              >
                <DropdownMenuLabel>归属 Workspace</DropdownMenuLabel>
                <DropdownMenuItem
                  onSelect={() => orbitActions.setDraftConfig({ workspaceId: null })}
                >
                  <span className="flex min-w-0 items-center gap-2">
                    <Inbox className="text-muted-foreground size-3.5 shrink-0" />
                    <span className="truncate">待归类（自动识别）</span>
                  </span>
                  {!selectedDraftWs && <Check className="size-3.5 shrink-0" />}
                </DropdownMenuItem>
                {curatedWorkspacesForProject.length > 0 && <DropdownMenuSeparator />}
                {curatedWorkspacesForProject.map((ws) => {
                  const active = draftWorkspaceId === ws.workspace_id;
                  return (
                    <DropdownMenuItem
                      key={ws.workspace_id}
                      onSelect={() =>
                        orbitActions.setDraftConfig({ workspaceId: ws.workspace_id })
                      }
                    >
                      <span className="flex min-w-0 flex-1 items-center gap-2">
                        <Layers className="text-muted-foreground size-3.5 shrink-0" />
                        <span className="truncate">{ws.title}</span>
                      </span>
                      <span className="flex shrink-0 items-center gap-1.5">
                        <span className="text-muted-foreground text-[11px] tabular-nums">
                          {ws.peer_count}
                        </span>
                        {active && <Check className="size-3.5 shrink-0" />}
                      </span>
                    </DropdownMenuItem>
                  );
                })}
              </DropdownMenuContent>
            </DropdownMenu>
          </>
        )}

        {/* Optional Multi-Host Fleet Target Selector (when >1 host paired) */}
        {multiHost && (
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <button
                type="button"
                className="text-foreground/65 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
              >
                <Server className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
                <span className="max-w-36 truncate">{activeHostLabel}</span>
                <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
              </button>
            </DropdownMenuTrigger>
            <DropdownMenuContent
              align="start"
              side="bottom"
              finalFocus={false}
              className="w-54 p-1.5"
            >
              <DropdownMenuLabel>Target Host</DropdownMenuLabel>
              {fleet.hosts.map((h) => {
                const active = h.host_id === selectedHostId;
                const online = h.connection_state.startsWith("online");
                return (
                  <DropdownMenuItem
                    key={h.host_id}
                    onSelect={() => orbitActions.selectHost(h.host_id, { keepDraft: true })}
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <span
                        className={cn(
                          "size-1.5 shrink-0 rounded-full",
                          online ? "bg-emerald-500" : "bg-zinc-400",
                        )}
                      />
                      <span className="truncate">{h.display_name}</span>
                    </span>
                    {active && <Check className="size-3.5 shrink-0" />}
                  </DropdownMenuItem>
                );
              })}
              <DropdownMenuSeparator />
              <DropdownMenuItem onSelect={() => orbitActions.openSettings("hosts")}>
                <span className="text-muted-foreground">Manage Hosts…</span>
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        )}
      </div>

      {/* 2. Right: Clean Execution Environment Menu (Non-Git Guard + Nested Git Repo Worktree Picker) */}
      {!canUseWorktree ? (
        <div
          title="Not a Git repository — running directly in local folder"
          className="text-foreground/60 inline-flex h-8 items-center gap-1.5 rounded-full px-2.5 text-[13px] select-none"
        >
          <Laptop className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
          <span>Local</span>
        </div>
      ) : (
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              className="text-foreground/72 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              {effectiveUsingWorktree ? (
                <GitBranch className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
              ) : (
                <Laptop className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
              )}
              <span>
                {effectiveUsingWorktree
                  ? selectedNestedRepo && !isDirectGitRepo
                    ? `Worktree · ${selectedNestedRepo.name}`
                    : "Worktree"
                  : "Local"}
              </span>
              {activeRepoBranch && (isDirectGitRepo || effectiveUsingWorktree) && (
                <span className="text-foreground/45 max-w-28 truncate font-mono text-[12px]">
                  · {activeRepoBranch}
                </span>
              )}
              <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            side="bottom"
            align="end"
            finalFocus={false}
            className="w-[320px] p-1.5"
          >
            <DropdownMenuLabel className="flex items-center justify-between gap-2">
              <span>Execution Environment</span>
              {activeRepoBranch && isDirectGitRepo && (
                <span className="text-muted-foreground max-w-28 truncate font-mono text-[11.5px] font-normal">
                  {activeRepoBranch}
                </span>
              )}
            </DropdownMenuLabel>

            <DropdownMenuItem
              onSelect={() =>
                orbitActions.setDraftConfig({
                  useWorktree: false,
                  worktreeBranch: "",
                  worktreeRepoPath: null,
                })
              }
              className="items-start gap-3 px-2.5 py-2"
            >
              <div className="flex min-w-0 flex-1 items-start gap-2.5">
                <Laptop
                  className={cn(
                    "mt-0.5 size-4 shrink-0 stroke-[1.75]",
                    !effectiveUsingWorktree ? "text-foreground" : "text-muted-foreground",
                  )}
                />
                <div className="flex min-w-0 flex-col gap-0.5">
                  <span className="text-foreground text-[13px] leading-tight font-medium">
                    Local
                  </span>
                  <span className="text-muted-foreground text-[11.5px] leading-snug">
                    Work directly in current project folder
                  </span>
                </div>
              </div>
              {!effectiveUsingWorktree && (
                <Check className="text-foreground mt-0.5 size-3.5 shrink-0" />
              )}
            </DropdownMenuItem>

            {isDirectGitRepo && (
              <DropdownMenuItem
                onSelect={() =>
                  orbitActions.setDraftConfig({
                    useWorktree: true,
                    worktreeBranch: "",
                    worktreeRepoPath: null,
                  })
                }
                className="items-start gap-3 px-2.5 py-2"
              >
                <div className="flex min-w-0 flex-1 items-start gap-2.5">
                  <GitBranch
                    className={cn(
                      "mt-0.5 size-4 shrink-0 stroke-[1.75]",
                      effectiveUsingWorktree && !selectedNestedRepo
                        ? "text-foreground"
                        : "text-muted-foreground",
                    )}
                  />
                  <div className="flex min-w-0 flex-col gap-0.5">
                    <span className="text-foreground text-[13px] leading-tight font-medium">
                      Worktree
                    </span>
                    <span className="text-muted-foreground text-[11.5px] leading-snug">
                      Isolated Git worktree for parallel tasks
                    </span>
                  </div>
                </div>
                {effectiveUsingWorktree && !selectedNestedRepo && (
                  <Check className="text-foreground mt-0.5 size-3.5 shrink-0" />
                )}
              </DropdownMenuItem>
            )}

            {nestedGitRepos.length > 0 && (
              <>
                <DropdownMenuSeparator />
                <DropdownMenuLabel className="text-[11px]">
                  {isDirectGitRepo
                    ? "Worktree · Nested Git Repos"
                    : "Worktree · Git Repos in Project"}
                </DropdownMenuLabel>
                <ScrollArea viewportClassName="max-h-48">
                  <div className="space-y-0.5">
                    {nestedGitRepos.map((repo) => {
                      const active =
                        effectiveUsingWorktree && draftWorktreeRepoPath === repo.path;
                      return (
                        <DropdownMenuItem
                          key={repo.path}
                          onSelect={() =>
                            orbitActions.setDraftConfig({
                              useWorktree: true,
                              worktreeBranch: "",
                              worktreeRepoPath: repo.path,
                            })
                          }
                          className="items-start gap-2.5 px-2.5 py-1.5"
                        >
                          <FolderGit2
                            className={cn(
                              "mt-0.5 size-4 shrink-0 stroke-[1.75]",
                              active ? "text-foreground" : "text-muted-foreground",
                            )}
                          />
                          <div className="flex min-w-0 flex-1 flex-col gap-0.5">
                            <span className="text-foreground truncate text-[12.5px] leading-tight font-medium">
                              {repo.relative_path || repo.name}
                            </span>
                            {repo.git_branch && (
                              <span className="text-muted-foreground truncate font-mono text-[11px]">
                                {repo.git_branch}
                              </span>
                            )}
                          </div>
                          {active && (
                            <Check className="text-foreground mt-0.5 size-3.5 shrink-0" />
                          )}
                        </DropdownMenuItem>
                      );
                    })}
                  </div>
                </ScrollArea>
              </>
            )}
          </DropdownMenuContent>
        </DropdownMenu>
      )}
    </div>
  );
});
