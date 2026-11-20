import { memo, useEffect, useMemo, useState } from "react";
import { ChevronRight, Folder, FolderOpen, RefreshCw, Search } from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_FILES: readonly string[] = [];

interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
}

function buildFileTree(paths: readonly string[]): TreeNode[] {
  const root: TreeNode = { name: "", path: "", isDir: true, children: [] };
  const dirIndex = new Map<string, TreeNode>([["", root]]);

  for (const raw of paths) {
    const clean = raw.replace(/\\/g, "/").replace(/^\/+/, "");
    if (!clean) continue;
    const parts = clean.split("/");
    let curPath = "";
    let parent = root;

    for (let i = 0; i < parts.length; i += 1) {
      const part = parts[i];
      const isLast = i === parts.length - 1;
      curPath = curPath ? `${curPath}/${part}` : part;

      if (isLast) {
        parent.children.push({
          name: part,
          path: clean,
          isDir: false,
          children: [],
        });
      } else {
        let dir = dirIndex.get(curPath);
        if (!dir) {
          dir = { name: part, path: curPath, isDir: true, children: [] };
          dirIndex.set(curPath, dir);
          parent.children.push(dir);
        }
        parent = dir;
      }
    }
  }

  const sortNodes = (nodes: TreeNode[]) => {
    nodes.sort((a, b) => {
      if (a.isDir !== b.isDir) return a.isDir ? -1 : 1;
      return a.name.localeCompare(b.name);
    });
    for (const n of nodes) {
      if (n.isDir) sortNodes(n.children);
    }
  };
  sortNodes(root.children);
  return root.children;
}

export const FilesExplorerTab = memo(function FilesExplorerTab() {
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const cwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const cacheKey = hostId && cwd ? `${hostId}::${cwd}` : "";
  const files = useOrbitSelector((s) =>
    cacheKey ? (s.workspaceFilesCache[cacheKey] ?? EMPTY_FILES) : EMPTY_FILES,
  );
  const gitFiles = useOrbitSelector((s) =>
    cacheKey ? (s.gitReviews[cacheKey]?.status?.files ?? null) : null,
  );

  const [query, setQuery] = useState("");
  const [expandedDirs, setExpandedDirs] = useState<Record<string, boolean>>({});

  useEffect(() => {
    if (hostId && cwd && cwd !== "~") {
      void orbitActions.ensureWorkspaceFiles(hostId, cwd);
    }
  }, [hostId, cwd]);

  const gitStatusMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const f of gitFiles ?? []) {
      m.set(f.path, f.status);
    }
    return m;
  }, [gitFiles]);

  const filteredList = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return null;
    return files.filter((p) => p.toLowerCase().includes(q)).slice(0, 150);
  }, [files, query]);

  const tree = useMemo(() => (filteredList ? [] : buildFileTree(files)), [files, filteredList]);

  const toggleDir = (dirPath: string, currentlyOpen: boolean) => {
    setExpandedDirs((prev) => ({ ...prev, [dirPath]: !currentlyOpen }));
  };

  const renderTree = (nodes: TreeNode[], depth: number): React.ReactNode =>
    nodes.map((node) => {
      if (node.isDir) {
        const isOpen = expandedDirs[node.path] ?? depth === 0;
        return (
          <div key={node.path}>
            <button
              type="button"
              onClick={() => toggleDir(node.path, isOpen)}
              style={{ paddingLeft: `${8 + depth * 13}px` }}
              className="hover:bg-accent/60 text-foreground/85 flex h-[27px] w-full cursor-pointer items-center gap-1.5 rounded-md pr-2 text-left text-[12.5px] transition-colors"
            >
              <ChevronRight
                className={cn(
                  "text-muted-foreground h-[12px] w-[12px] shrink-0 transition-transform duration-150",
                  isOpen && "rotate-90",
                )}
              />
              {isOpen ? (
                <FolderOpen className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              ) : (
                <Folder className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              )}
              <span className="truncate">{node.name}</span>
            </button>
            {isOpen && node.children.length > 0 && renderTree(node.children, depth + 1)}
          </div>
        );
      }

      const Icon = resolveSemanticFileIcon(node.path);
      const status = gitStatusMap.get(node.path);

      return (
        <button
          key={node.path}
          type="button"
          onClick={() =>
            hostId && orbitActions.openFileInSidePanel(node.path, { hostId, cwd })
          }
          style={{ paddingLeft: `${22 + depth * 13}px` }}
          className="hover:bg-accent/60 text-foreground/80 hover:text-foreground flex h-[27px] w-full cursor-pointer items-center justify-between gap-2 rounded-md pr-2.5 text-left text-[12.5px] transition-colors"
        >
          <span className="flex min-w-0 items-center gap-1.5">
            <Icon className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
            <span className="truncate">{node.name}</span>
          </span>
          {status && (
            <span
              className={cn(
                "shrink-0 font-mono text-[10.5px] font-medium",
                status === "??" || status === "A"
                  ? "text-emerald-600 dark:text-emerald-400"
                  : status === "D"
                    ? "text-rose-500"
                    : "text-amber-600 dark:text-amber-400",
              )}
            >
              {status}
            </span>
          )}
        </button>
      );
    });

  return (
    <div className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Compact Search Header */}
      <div className="border-border flex h-[36px] shrink-0 items-center gap-2 border-b px-3">
        <Search className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0" />
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter workspace files…"
          className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent text-[12.5px] outline-none"
        />
        <span className="text-muted-foreground tabular-nums shrink-0 text-[11px]">
          {files.length}
        </span>
        {hostId && cwd && (
          <button
            type="button"
            title="Refresh file index"
            onClick={() => void orbitActions.ensureWorkspaceFiles(hostId, cwd, true)}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
          >
            <RefreshCw className="h-[13px] w-[13px]" />
          </button>
        )}
      </div>

      {/* Tree / Filtered Results */}
      <ScrollArea className="min-h-0 flex-1" viewportClassName="p-2">
        {files.length === 0 ? (
          <div className="text-muted-foreground flex h-32 items-center justify-center text-[12.5px]">
            No indexed files in this workspace.
          </div>
        ) : filteredList ? (
          filteredList.length === 0 ? (
            <div className="text-muted-foreground flex h-28 items-center justify-center text-[12.5px]">
              No files match “{query}”.
            </div>
          ) : (
            <div className="space-y-0.5">
              {filteredList.map((filePath) => {
                const Icon = resolveSemanticFileIcon(filePath);
                const status = gitStatusMap.get(filePath);
                return (
                  <button
                    key={filePath}
                    type="button"
                    onClick={() =>
                      hostId && orbitActions.openFileInSidePanel(filePath, { hostId, cwd })
                    }
                    className="hover:bg-accent/60 text-foreground/85 hover:text-foreground flex w-full cursor-pointer items-center justify-between gap-2 rounded-md px-2 py-1.5 text-left text-[12px] transition-colors"
                  >
                    <span className="flex min-w-0 items-center gap-2">
                      <Icon className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                      <span className="truncate font-mono">{filePath}</span>
                    </span>
                    {status && (
                      <span className="text-muted-foreground shrink-0 font-mono text-[10.5px]">
                        {status}
                      </span>
                    )}
                  </button>
                );
              })}
            </div>
          )
        ) : (
          <div className="space-y-0.5">{renderTree(tree, 0)}</div>
        )}
      </ScrollArea>
    </div>
  );
});
