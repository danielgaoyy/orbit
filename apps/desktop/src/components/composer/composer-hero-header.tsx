import { memo, useEffect, useMemo } from "react";
import {
  Check,
  ChevronDown,
  Folder,
  FolderGit2,
  FolderPlus,
  GitBranch,
  Laptop,
  Server,
} from "lucide-react";
import { basenameOfCwd } from "@/components/composer/composer-constants";
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
} from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

/**
 * Isolated Hero Launchpad Header (`[Project ▾]`, optional `[Host ▾]`, `[Local / Worktree ▾]`).
 * Never subscribes to `composerDrafts`, so typing in the `<textarea>` triggers zero re-renders here.
 */
export const ComposerHeroHeader = memo(function ComposerHeroHeader() {
  const fleet = useOrbitSelector((s) => s.fleet);
  const sidebarView = useOrbitSelector((s) => s.sidebarView);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const draftUseWorktree = useOrbitSelector((s) => s.draftUseWorktree);
  const draftWorktreeRepoPath = useOrbitSelector((s) => s.draftWorktreeRepoPath);
  const gitReposCache = useOrbitSelector((s) => s.gitReposCache);
  const hostDetails = useOrbitSelector((s) => s.hostDetails);
  const hiddenProjectKeys = useOrbitSelector((s) => s.hiddenProjectKeys);
  const hostDetail = selectedHostId ? (hostDetails[selectedHostId] ?? null) : null;

  const heroProjects = useMemo(() => {
    const hostNameById = new Map(fleet.hosts.map((h) => [h.host_id, h.display_name]));
    const dirsById = new Map(
      (sidebarView?.projects ?? []).map((p) => [p.id, p.directories]),
    );
    const items = (sidebarView?.launchpad_projects ?? [])
      .filter((lp) => {
        const clean = normalizeProjectPath(lp.path);
        const hiddenKey = `${lp.host_id}::${clean}`;
        const hasConvs = (sidebarView?.projects ?? []).some(
          (p) => p.id === lp.id && p.conversations.length > 0,
        );
        return hasConvs || !hiddenProjectKeys[hiddenKey];
      })
      .map((lp) => ({
        id: lp.id,
        hostId: lp.host_id,
        hostName: hostNameById.get(lp.host_id) ?? "Host",
        name: lp.name,
        path: lp.path,
        directories: dirsById.get(lp.id) ?? [normalizeProjectPath(lp.path)],
        branch: lp.branch,
      }));

    const cleanSelected = normalizeProjectPath(selectedCwd);
    const hasSelected =
      Boolean(selectedHostId) &&
      items.some(
        (it) =>
          it.hostId === selectedHostId &&
          matchesProjectDirectory(it.directories, cleanSelected),
      );

    if (selectedHostId && cleanSelected && !hasSelected && !isBareHomeDir(cleanSelected)) {
      const branch =
        hostDetail?.workspaces.find((w) => normalizeProjectPath(w.cwd) === cleanSelected)
          ?.git_branch ??
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
      });
    }

    return items;
  }, [
    fleet.hosts,
    sidebarView,
    hiddenProjectKeys,
    selectedHostId,
    selectedCwd,
    hostDetail,
  ]);

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
  const multiHost = fleet.hosts.length > 1;
  const activeHostLabel =
    hostDetail?.host.display_name ??
    fleet.hosts.find((h) => h.host_id === selectedHostId)?.display_name ??
    "Host";

  return (
    <div className="flex flex-wrap items-center justify-between gap-2 px-1 pb-2">
      <div className="flex min-w-0 items-center gap-1">
        {/* 1. Unified Project Switcher Menu (Zero Raw Input) */}
        <DropdownMenu>
          <DropdownMenuTrigger asChild>
            <button
              type="button"
              title={selectedCwd}
              className="text-foreground/72 hover:text-foreground inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
            >
              <Folder className="size-3.5 shrink-0 stroke-[1.75] opacity-75" />
              <span className="max-w-48 truncate font-normal">{activeProjectLabel}</span>
              <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
            </button>
          </DropdownMenuTrigger>
          <DropdownMenuContent
            side="bottom"
            align="start"
            finalFocus={false}
            className="w-[248px] p-1.5"
          >
            <DropdownMenuLabel>Switch Project</DropdownMenuLabel>
            {heroProjects.length > 0 && (
              <ScrollArea viewportClassName="max-h-64">
                <div className="space-y-0.5">
                  {heroProjects.map((proj) => {
                    const active =
                      proj.hostId === selectedHostId &&
                      matchesProjectDirectory(proj.directories, selectedCwd);
                    return (
                      <DropdownMenuItem
                        key={proj.id}
                        onSelect={() =>
                          orbitActions.selectProjectWorkspace(proj.hostId, proj.path)
                        }
                      >
                        <span className="flex min-w-0 flex-1 items-center gap-2">
                          <Folder className="text-muted-foreground size-4 shrink-0 stroke-[1.75]" />
                          <span
                            className={cn(
                              "truncate text-[13px]",
                              active && "text-foreground font-medium",
                            )}
                          >
                            {proj.name}
                          </span>
                        </span>
                        <span className="flex shrink-0 items-center gap-1.5">
                          {multiHost && (
                            <span className="text-muted-foreground max-w-20 truncate text-[11.5px]">
                              {proj.hostName}
                            </span>
                          )}
                          {active && (
                            <Check className="text-foreground size-3.5 shrink-0 stroke-[1.9]" />
                          )}
                        </span>
                      </DropdownMenuItem>
                    );
                  })}
                </div>
              </ScrollArea>
            )}
            <DropdownMenuSeparator />
            <DropdownMenuItem
              onSelect={() => void orbitActions.promptAddProject(selectedHostId ?? undefined)}
            >
              <span className="flex items-center gap-2">
                <FolderPlus className="text-muted-foreground size-4 shrink-0 stroke-[1.75]" />
                <span className="text-[13px]">Add new project…</span>
              </span>
            </DropdownMenuItem>
          </DropdownMenuContent>
        </DropdownMenu>

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
                    onSelect={() => orbitActions.selectHost(h.host_id)}
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
