import { useMemo, useRef } from "react";
import {
  ArrowUp,
  ChevronRight,
  Folder,
  FolderGit2,
  Home,
  Loader2,
  Server,
} from "lucide-react";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { type ClientProjectView, orbitIpc } from "@/lib/orbit-ipc";
import {
  basenameOfProjectPath,
  isBareHomeDir,
  normalizeProjectPath,
} from "@/lib/projects";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";

const EMPTY_PROJECTS: readonly ClientProjectView[] = [];

export function RemoteFolderPickerDialog() {
  const hostId = useOrbitSelector((s) => s.remoteFolderPickerHostId);
  const lastHostIdRef = useRef<string | null>(null);
  if (hostId) {
    lastHostIdRef.current = hostId;
  }
  const activeHostId = hostId ?? lastHostIdRef.current;

  return (
    <Dialog
      open={Boolean(hostId)}
      onOpenChange={(v) => {
        if (!v) orbitActions.closeRemoteFolderPicker();
      }}
    >
      <DialogContent className="max-w-lg gap-3.5 p-5 select-none">
        {activeHostId ? <RemoteFolderPickerModal hostId={activeHostId} /> : null}
      </DialogContent>
    </Dialog>
  );
}

function RemoteFolderPickerModal({ hostId }: { hostId: string }) {
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const pinnedProjects = useOrbitSelector((s) => s.fleet.projects ?? EMPTY_PROJECTS);

  const breadcrumbs = useMemo(() => {
    const currentPath = hostDetail?.directory_listing?.current_path;
    if (!currentPath || currentPath === "/") {
      return [{ label: "/", path: "/" }];
    }
    const parts = currentPath.split("/").filter(Boolean);
    const items: { label: string; path: string }[] = [{ label: "/", path: "/" }];
    let acc = "";
    for (const seg of parts) {
      acc += `/${seg}`;
      items.push({ label: seg, path: acc });
    }
    return items;
  }, [hostDetail?.directory_listing?.current_path]);

  const unpinnedRepos = useMemo(() => {
    if (!hostDetail) return [];
    const pinnedPaths = new Set(
      pinnedProjects
        .filter((p) => p.host_id === hostId)
        .flatMap((p) => p.directories.map(normalizeProjectPath)),
    );
    return hostDetail.discovered_projects
      .filter((dp) => {
        const clean = normalizeProjectPath(dp.path);
        return !isBareHomeDir(clean) && !pinnedPaths.has(clean);
      })
      .slice(0, 6);
  }, [hostId, hostDetail, pinnedProjects]);

  const listing = hostDetail?.directory_listing ?? null;
  const hostName = hostDetail?.host.display_name ?? "Remote Host";
  const currentPath = listing?.current_path ?? null;
  const currentBasename = currentPath ? basenameOfProjectPath(currentPath) : "";

  const navigateTo = (path: string | null) => {
    void orbitIpc.listDirectory(hostId, path);
  };

  const selectFolder = (path: string, label?: string) => {
    void orbitActions.addProjectDirectory(hostId, path, label);
  };

  return (
    <>
      <DialogHeader className="gap-1 pr-6">
          <DialogTitle className="flex items-center gap-2 text-[15px]">
            <span>Open Project Folder</span>
            <span className="bg-muted text-muted-foreground inline-flex items-center gap-1 rounded-md px-2 py-0.5 text-[11.5px] font-medium">
              <Server className="h-[12px] w-[12px]" />
              {hostName}
            </span>
          </DialogTitle>
          <DialogDescription className="text-[12.5px]">
            Choose a repository or project directory on {hostName}.
          </DialogDescription>
        </DialogHeader>

        {/* Quick-Pick Discovered Git Repositories on Remote Host */}
        {unpinnedRepos.length > 0 && (
          <div className="space-y-1.5">
            <div className="text-muted-foreground px-0.5 text-[11.5px] font-medium">
              Recent Git Repositories
            </div>
            <div className="flex flex-wrap gap-1.5">
              {unpinnedRepos.map((repo) => (
                <button
                  key={repo.path}
                  type="button"
                  title={repo.path}
                  onClick={() => selectFolder(repo.path, repo.name)}
                  className="border-border bg-background/70 hover:bg-accent hover:text-foreground inline-flex h-[28px] max-w-52 cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12.5px] font-medium transition-colors"
                >
                  <FolderGit2 className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                  <span className="truncate">{repo.name}</span>
                </button>
              ))}
            </div>
          </div>
        )}

        {/* Click-Only Directory Navigator */}
        <div className="border-border bg-background/55 flex flex-col overflow-hidden rounded-xl border">
          {/* Breadcrumb Toolbar */}
          <div className="border-border bg-muted/45 flex h-[36px] items-center gap-1 border-b px-2">
            <button
              type="button"
              title="Home directory"
              onClick={() => navigateTo(null)}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-md transition-colors"
            >
              <Home className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
            </button>

            <button
              type="button"
              title="Parent folder"
              disabled={!listing?.parent_path}
              onClick={() => {
                if (listing?.parent_path) navigateTo(listing.parent_path);
              }}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] shrink-0 cursor-pointer items-center justify-center rounded-md transition-colors disabled:pointer-events-none disabled:opacity-35"
            >
              <ArrowUp className="h-[13.5px] w-[13.5px] stroke-[1.75]" />
            </button>

            <div className="mx-1 h-3.5 w-px shrink-0 bg-black/[0.08] dark:bg-white/[0.10]" />

            <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto text-[12px]">
              {breadcrumbs.map((crumb, idx) => {
                const isLast = idx === breadcrumbs.length - 1;
                return (
                  <div key={crumb.path} className="flex shrink-0 items-center gap-0.5">
                    {idx > 0 && (
                      <ChevronRight className="text-muted-foreground/60 h-[12px] w-[12px] shrink-0" />
                    )}
                    <button
                      type="button"
                      onClick={() => navigateTo(crumb.path)}
                      className={
                        isLast
                          ? "text-foreground cursor-pointer rounded px-1.5 py-0.5 font-medium"
                          : "text-muted-foreground hover:text-foreground cursor-pointer rounded px-1.5 py-0.5 transition-colors"
                      }
                    >
                      {crumb.label}
                    </button>
                  </div>
                );
              })}
            </div>
          </div>

          {/* Folder Entries */}
          <ScrollArea className="h-64" viewportClassName="p-1.5 [&>div]:h-full">
            {!listing ? (
              <div className="text-muted-foreground flex h-full items-center justify-center gap-2 text-[13px]">
                <Loader2 className="h-[15px] w-[15px] animate-spin" />
                <span>Loading folders…</span>
              </div>
            ) : listing.entries.length === 0 ? (
              <div className="text-muted-foreground flex h-full items-center justify-center text-[13px]">
                No subdirectories in this folder
              </div>
            ) : (
              <div className="space-y-0.5">
                {listing.entries.map((entry) => (
                  <div
                    key={entry.path}
                    onClick={() => navigateTo(entry.path)}
                    onDoubleClick={() => selectFolder(entry.path, entry.name)}
                    className="group hover:bg-accent flex h-[32px] cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2.5 text-[13px] transition-colors"
                  >
                    <div className="flex min-w-0 items-center gap-2">
                      {entry.is_git_repo ? (
                        <FolderGit2 className="text-foreground/80 h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
                      ) : (
                        <Folder className="text-muted-foreground h-[15px] w-[15px] shrink-0 stroke-[1.75]" />
                      )}
                      <span className="text-foreground truncate">{entry.name}</span>
                      {entry.is_git_repo && (
                        <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0 text-[10.5px] font-medium">
                          Git
                        </span>
                      )}
                    </div>

                    <button
                      type="button"
                      onClick={(e) => {
                        e.stopPropagation();
                        selectFolder(entry.path, entry.name);
                      }}
                      className="bg-foreground text-background hidden h-[22px] shrink-0 cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium group-hover:inline-flex hover:opacity-90"
                    >
                      Open
                    </button>
                  </div>
                ))}
              </div>
            )}
          </ScrollArea>
        </div>

        {/* Clean Action Footer */}
        <div className="flex items-center justify-end gap-2 pt-0.5">
          <button
            type="button"
            onClick={() => orbitActions.closeRemoteFolderPicker()}
            className="border-border hover:bg-accent inline-flex h-[32px] cursor-pointer items-center rounded-lg border px-3.5 text-[13px] font-medium transition-colors"
          >
            Cancel
          </button>
          <button
            type="button"
            disabled={!currentPath}
            onClick={() => {
              if (currentPath) selectFolder(currentPath);
            }}
            className="bg-foreground text-background inline-flex h-[32px] cursor-pointer items-center rounded-lg px-3.5 text-[13px] font-medium transition-opacity hover:opacity-90 disabled:pointer-events-none disabled:opacity-40"
          >
            {currentBasename && currentBasename !== "/"
              ? `Open "${currentBasename}"`
              : "Open Current Folder"}
          </button>
        </div>
    </>
  );
}
