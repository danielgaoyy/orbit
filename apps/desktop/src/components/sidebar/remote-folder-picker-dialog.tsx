import { useMemo, useRef } from "react";
import {
  ArrowUp,
  ChevronRight,
  Folder,
  FolderGit2,
  Home,
  Server,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { IconButton } from "@/components/ui/icon-button";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useI18n } from "@/lib/i18n";
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
  const { t } = useI18n();
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
  const hostName = hostDetail?.host.display_name ?? t("sidebar.remoteDefaultHost");
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
          <span>{t("sidebar.remoteFolderTitle")}</span>
          <Badge variant="secondary" className="gap-1 font-normal">
            <Server className="size-[11.5px]" />
            <span>{hostName}</span>
          </Badge>
        </DialogTitle>
        <DialogDescription className="text-[12.5px]">
          {t("sidebar.remoteFolderDesc", { host: hostName })}
        </DialogDescription>
      </DialogHeader>

      {/* Quick-Pick Discovered Git Repositories on Remote Host */}
      {unpinnedRepos.length > 0 && (
        <div className="space-y-1.5">
          <div className="text-ink-muted px-0.5 text-[11.5px] font-medium">
            {t("sidebar.remoteRecentGitRepos")}
          </div>
          <div className="flex flex-wrap gap-1.5">
            {unpinnedRepos.map((repo) => (
              <Button
                key={repo.path}
                type="button"
                variant="outline"
                size="sm"
                title={repo.path}
                onClick={() => selectFolder(repo.path, repo.name)}
                className="h-[28px] max-w-52 px-2.5 text-[12.5px]"
              >
                <FolderGit2 className="text-ink-muted size-[13.5px] shrink-0 stroke-[1.75]" />
                <span className="truncate">{repo.name}</span>
              </Button>
            ))}
          </div>
        </div>
      )}

      {/* Click-Only Directory Navigator */}
      <div className="border-border-subtle bg-surface-base flex flex-col overflow-hidden rounded-xl border">
        {/* Breadcrumb Toolbar */}
        <div className="border-border-subtle bg-surface-recessed flex h-[36px] items-center gap-1 border-b px-2">
          <IconButton
            size="sm"
            tooltip={t("sidebar.remoteHomeDir")}
            onClick={() => navigateTo(null)}
          >
            <Home className="size-[13.5px] stroke-[1.75]" />
          </IconButton>

          <IconButton
            size="sm"
            tooltip={t("sidebar.remoteParentFolder")}
            disabled={!listing?.parent_path}
            onClick={() => {
              if (listing?.parent_path) navigateTo(listing.parent_path);
            }}
          >
            <ArrowUp className="size-[13.5px] stroke-[1.75]" />
          </IconButton>

          <div className="bg-border-subtle mx-1 h-3.5 w-px shrink-0" />

          <div className="flex min-w-0 flex-1 items-center gap-0.5 overflow-x-auto text-[12px]">
            {breadcrumbs.map((crumb, idx) => {
              const isLast = idx === breadcrumbs.length - 1;
              return (
                <div key={crumb.path} className="flex shrink-0 items-center gap-0.5">
                  {idx > 0 && (
                    <ChevronRight className="text-ink-faint size-[12px] shrink-0" />
                  )}
                  <button
                    type="button"
                    onClick={() => navigateTo(crumb.path)}
                    className={
                      isLast
                        ? "text-ink-primary cursor-pointer rounded px-1.5 py-0.5 font-medium"
                        : "text-ink-muted hover:text-ink-primary cursor-pointer rounded px-1.5 py-0.5 transition-colors"
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
            <div className="text-ink-muted flex h-full items-center justify-center gap-2 text-[13px]">
              <OrbitSpinner size={14} />
              <span>{t("sidebar.remoteLoadingFolders")}</span>
            </div>
          ) : listing.entries.length === 0 ? (
            <div className="text-ink-muted flex h-full items-center justify-center text-[13px]">
              {t("sidebar.remoteNoSubdirs")}
            </div>
          ) : (
            <div className="space-y-0.5">
              {listing.entries.map((entry) => (
                <div
                  key={entry.path}
                  onClick={() => navigateTo(entry.path)}
                  onDoubleClick={() => selectFolder(entry.path, entry.name)}
                  className="group hover:bg-accent/65 flex h-[32px] cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2.5 text-[13px] transition-colors"
                >
                  <div className="flex min-w-0 items-center gap-2">
                    {entry.is_git_repo ? (
                      <FolderGit2 className="text-ink-secondary size-[15px] shrink-0 stroke-[1.75]" />
                    ) : (
                      <Folder className="text-ink-muted size-[15px] shrink-0 stroke-[1.75]" />
                    )}
                    <span className="text-ink-primary truncate">{entry.name}</span>
                    {entry.is_git_repo && (
                      <Badge variant="secondary" className="h-[18px] px-1.5 text-[10.5px]">
                        Git
                      </Badge>
                    )}
                  </div>

                  <Button
                    type="button"
                    size="sm"
                    onClick={(e) => {
                      e.stopPropagation();
                      selectFolder(entry.path, entry.name);
                    }}
                    className="hidden h-[22px] shrink-0 rounded-md px-2 text-[11.5px] group-hover:inline-flex"
                  >
                    {t("sidebar.remoteOpenAction")}
                  </Button>
                </div>
              ))}
            </div>
          )}
        </ScrollArea>
      </div>

      {/* Clean Action Footer */}
      <DialogFooter className="pt-0.5">
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => orbitActions.closeRemoteFolderPicker()}
        >
          {t("common.cancel")}
        </Button>
        <Button
          type="button"
          size="sm"
          disabled={!currentPath}
          onClick={() => {
            if (currentPath) selectFolder(currentPath);
          }}
        >
          {currentBasename && currentBasename !== "/"
            ? t("sidebar.remoteOpenNamed", { name: currentBasename })
            : t("sidebar.remoteOpenCurrent")}
        </Button>
      </DialogFooter>
    </>
  );
}
