import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
  Check,
  ChevronRight,
  FilePlus,
  Folder,
  FolderOpen,
  FolderPlus,
  Loader2,
  Pencil,
  RefreshCw,
  Search,
  Trash2,
  X,
} from "lucide-react";
import { resolveSemanticFileIcon } from "@/components/composer/mention-menu";
import { ScrollArea } from "@/components/ui/scroll-area";
import { useImeCompositionGuard } from "@/lib/ime";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_FILES: readonly string[] = [];

interface TreeNode {
  name: string;
  path: string;
  isDir: boolean;
  children: TreeNode[];
}

function buildFileTree(
  paths: readonly string[],
  emptyDirs: readonly string[],
): TreeNode[] {
  const root: TreeNode = { name: "", path: "", isDir: true, children: [] };
  const dirIndex = new Map<string, TreeNode>([["", root]]);

  const ensureDir = (dirPath: string): TreeNode => {
    const clean = dirPath.replace(/\\/g, "/").replace(/^\/+|\/+$/g, "");
    if (!clean) return root;
    const parts = clean.split("/").filter(Boolean);
    let curPath = "";
    let parent = root;
    for (const part of parts) {
      curPath = curPath ? `${curPath}/${part}` : part;
      let dir = dirIndex.get(curPath);
      if (!dir) {
        dir = { name: part, path: curPath, isDir: true, children: [] };
        dirIndex.set(curPath, dir);
        parent.children.push(dir);
      }
      parent = dir;
    }
    return parent;
  };

  for (const dirPath of emptyDirs) {
    ensureDir(dirPath);
  }

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

function ancestorDirsOf(relPath: string): string[] {
  const parts = relPath.replace(/\\/g, "/").split("/").filter(Boolean);
  const dirs: string[] = [];
  let acc = "";
  for (let i = 0; i < parts.length - 1; i += 1) {
    acc = acc ? `${acc}/${parts[i]}` : parts[i];
    dirs.push(acc);
  }
  return dirs;
}

interface DeleteConfirmTarget {
  hostId: string;
  cwd: string;
  path: string;
  name: string;
  isDir: boolean;
}

export const FilesExplorerTab = memo(function FilesExplorerTab() {
  const hostId = useOrbitSelector((s) => s.selectedHostId);
  const cwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const cacheKey = hostId && cwd ? `${hostId}::${cwd}` : "";
  const files = useOrbitSelector((s) =>
    cacheKey ? (s.workspaceFilesCache[cacheKey] ?? EMPTY_FILES) : EMPTY_FILES,
  );
  const emptyDirs = useOrbitSelector((s) =>
    cacheKey ? (s.workspaceEmptyDirsCache[cacheKey] ?? EMPTY_FILES) : EMPTY_FILES,
  );
  const gitFiles = useOrbitSelector((s) =>
    cacheKey ? (s.gitReviews[cacheKey]?.status?.files ?? null) : null,
  );
  const fileEditorDrafts = useOrbitSelector((s) => s.fileEditorDrafts);

  const [query, setQuery] = useState("");
  const [expandedDirs, setExpandedDirs] = useState<Record<string, boolean>>({});

  // Inline Create state
  const [createMode, setCreateMode] = useState<"file" | "dir" | null>(null);
  const [createValue, setCreateValue] = useState("");
  const [createBusy, setCreateBusy] = useState(false);

  // Inline Rename state
  const [renamingPath, setRenamingPath] = useState<string | null>(null);
  const [renameValue, setRenameValue] = useState("");
  const [renameBusy, setRenameBusy] = useState(false);

  // Restrained Inline Delete confirmation state (scoped to captured hostId + cwd)
  const [confirmDeleteTarget, setConfirmDeleteTarget] = useState<DeleteConfirmTarget | null>(null);
  const [deleteBusy, setDeleteBusy] = useState(false);

  const [actionError, setActionError] = useState<string | null>(null);

  const createInputRef = useRef<HTMLInputElement | null>(null);
  const renameInputRef = useRef<HTMLInputElement | null>(null);
  const ime = useImeCompositionGuard();

  // Reset inline mutation UI whenever active host or workspace changes (prevents cross-workspace mutations)
  useEffect(() => {
    setCreateMode(null);
    setCreateValue("");
    setRenamingPath(null);
    setRenameValue("");
    setConfirmDeleteTarget(null);
    setActionError(null);
  }, [hostId, cwd]);

  // Allow Escape key to dismiss inline delete confirmation cleanly
  useEffect(() => {
    if (!confirmDeleteTarget) return;
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape" && !deleteBusy) {
        e.preventDefault();
        setConfirmDeleteTarget(null);
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [confirmDeleteTarget, deleteBusy]);

  useEffect(() => {
    if (hostId && cwd && cwd !== "~") {
      void orbitActions.ensureWorkspaceFiles(hostId, cwd);
    }
  }, [hostId, cwd]);

  const canMutate = Boolean(hostId && cwd && cwd !== "~");

  const confirmDeleteHasUnsavedEdits = useMemo(() => {
    if (!confirmDeleteTarget) return false;
    const prefix = `${confirmDeleteTarget.hostId}::${confirmDeleteTarget.cwd}::`;
    return Object.entries(fileEditorDrafts).some(([k, d]) => {
      if (!d.dirty || !k.startsWith(prefix)) return false;
      const rel = k.slice(prefix.length);
      return rel === confirmDeleteTarget.path || rel.startsWith(`${confirmDeleteTarget.path}/`);
    });
  }, [confirmDeleteTarget, fileEditorDrafts]);

  const startCreate = (mode: "file" | "dir", prefix = "") => {
    setActionError(null);
    setRenamingPath(null);
    setConfirmDeleteTarget(null);
    setCreateMode(mode);
    setCreateValue(prefix);
    window.requestAnimationFrame(() => {
      const el = createInputRef.current;
      if (el) {
        el.focus();
        el.setSelectionRange(prefix.length, prefix.length);
      }
    });
  };

  const cancelCreate = () => {
    setCreateMode(null);
    setCreateValue("");
    setActionError(null);
  };

  const submitCreate = async () => {
    if (!canMutate || !hostId || !cwd || !createMode || createBusy) return;
    const rawPath = createValue.trim().replace(/^\/+|\/+$/g, "");
    if (!rawPath) {
      setActionError(createMode === "dir" ? "Enter a folder path" : "Enter a file path");
      return;
    }
    setCreateBusy(true);
    setActionError(null);
    try {
      const created = await orbitActions.createWorkspaceEntry(
        hostId,
        cwd,
        rawPath,
        createMode === "dir",
      );
      setExpandedDirs((prev) => {
        const next = { ...prev };
        for (const anc of ancestorDirsOf(created)) {
          next[anc] = true;
        }
        if (createMode === "dir") {
          next[created] = true;
        }
        return next;
      });
      setCreateMode(null);
      setCreateValue("");
    } catch (err) {
      setActionError(String(err));
    } finally {
      setCreateBusy(false);
    }
  };

  const startRename = (nodePath: string, name: string) => {
    setActionError(null);
    setCreateMode(null);
    setConfirmDeleteTarget(null);
    setRenamingPath(nodePath);
    setRenameValue(name);
    window.requestAnimationFrame(() => {
      const el = renameInputRef.current;
      if (el) {
        el.focus();
        const dotIdx = name.lastIndexOf(".");
        el.setSelectionRange(0, dotIdx > 0 ? dotIdx : name.length);
      }
    });
  };

  const cancelRename = (clearError = true) => {
    setRenamingPath(null);
    setRenameValue("");
    if (clearError) {
      setActionError(null);
    }
  };

  const submitRename = async (oldPath: string) => {
    if (!canMutate || !hostId || !cwd || renameBusy) return;
    const trimmed = renameValue.trim().replace(/^\/+|\/+$/g, "");
    if (!trimmed) {
      cancelRename(false);
      return;
    }
    const parentIdx = oldPath.lastIndexOf("/");
    const parentPrefix = parentIdx >= 0 ? oldPath.slice(0, parentIdx + 1) : "";
    const newPath = trimmed.includes("/") ? trimmed : `${parentPrefix}${trimmed}`;
    if (newPath === oldPath) {
      cancelRename(false);
      return;
    }
    setRenameBusy(true);
    setActionError(null);
    try {
      const renamedTo = await orbitActions.renameWorkspaceEntry(hostId, cwd, oldPath, newPath);
      setExpandedDirs((prev) => {
        const next: Record<string, boolean> = {};
        for (const [dir, open] of Object.entries(prev)) {
          if (dir === oldPath) {
            next[renamedTo] = open;
          } else if (dir.startsWith(`${oldPath}/`)) {
            next[`${renamedTo}${dir.slice(oldPath.length)}`] = open;
          } else {
            next[dir] = open;
          }
        }
        for (const anc of ancestorDirsOf(renamedTo)) {
          next[anc] = true;
        }
        return next;
      });
      setRenamingPath(null);
      setRenameValue("");
    } catch (err) {
      setActionError(String(err));
    } finally {
      setRenameBusy(false);
    }
  };

  const submitDelete = async () => {
    if (!confirmDeleteTarget || deleteBusy) return;
    const target = confirmDeleteTarget;
    // Refuse if workspace switched unexpectedly before click completed
    if (target.hostId !== hostId || target.cwd !== cwd) {
      setConfirmDeleteTarget(null);
      return;
    }
    setDeleteBusy(true);
    setActionError(null);
    try {
      await orbitActions.deleteWorkspaceEntry(target.hostId, target.cwd, target.path, {
        allowDirty: true,
      });
      setConfirmDeleteTarget(null);
      orbitActions.showToast(`Deleted ${target.name}`);
    } catch (err) {
      setActionError(String(err));
    } finally {
      setDeleteBusy(false);
    }
  };

  const gitStatusMap = useMemo(() => {
    const m = new Map<string, string>();
    for (const f of gitFiles ?? []) {
      m.set(f.path, f.status);
    }
    return m;
  }, [gitFiles]);

  // Bubble git dirty status up to ancestor directories so collapsed folders show a subtle dot
  const dirtyDirs = useMemo(() => {
    const dirs = new Set<string>();
    for (const f of gitFiles ?? []) {
      const parts = f.path.split("/");
      let acc = "";
      for (let i = 0; i < parts.length - 1; i += 1) {
        acc = acc ? `${acc}/${parts[i]}` : parts[i];
        dirs.add(acc);
      }
    }
    return dirs;
  }, [gitFiles]);

  const filteredList = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return null;
    return files.filter((p) => p.toLowerCase().includes(q)).slice(0, 150);
  }, [files, query]);

  const tree = useMemo(() => buildFileTree(files, emptyDirs), [files, emptyDirs]);

  const toggleDir = (dirPath: string, currentlyOpen: boolean) => {
    setExpandedDirs((prev) => ({ ...prev, [dirPath]: !currentlyOpen }));
  };

  const renderTree = (nodes: TreeNode[], depth: number): React.ReactNode =>
    nodes.map((node) => {
      const isRenaming = renamingPath === node.path;

      if (node.isDir) {
        const isOpen = expandedDirs[node.path] ?? depth === 0;
        const isDirty = dirtyDirs.has(node.path);
        return (
          <div key={node.path}>
            <div
              style={{ paddingLeft: `${8 + depth * 13}px` }}
              className="group/drow hover:bg-accent/60 text-foreground/85 flex h-[27px] w-full items-center justify-between gap-1.5 rounded-md pr-1.5 text-left text-[12.5px] transition-colors"
            >
              {isRenaming ? (
                <div className="flex min-w-0 flex-1 items-center gap-1.5">
                  <Folder className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                  <input
                    ref={renameInputRef}
                    type="text"
                    value={renameValue}
                    readOnly={renameBusy}
                    onChange={(e) => setRenameValue(e.target.value)}
                    {...ime.compositionProps}
                    onKeyDown={(e) => {
                      if (ime.shouldIgnoreKeyDown(e)) return;
                      if (e.key === "Enter") {
                        e.preventDefault();
                        void submitRename(node.path);
                      } else if (e.key === "Escape") {
                        e.preventDefault();
                        cancelRename(true);
                      }
                    }}
                    onBlur={() => {
                      if (!renameBusy) cancelRename(false);
                    }}
                    className="border-border bg-background text-foreground h-[21px] min-w-0 flex-1 rounded border px-1.5 font-mono text-[12px] outline-none read-only:opacity-60"
                  />
                </div>
              ) : (
                <>
                  <button
                    type="button"
                    onClick={() => toggleDir(node.path, isOpen)}
                    className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
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
                  <div className="flex shrink-0 items-center gap-0.5">
                    {canMutate && hostId && cwd && (
                      <>
                        <button
                          type="button"
                          title={`New file in ${node.name}`}
                          aria-label={`New file in ${node.name}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setExpandedDirs((prev) => ({ ...prev, [node.path]: true }));
                            startCreate("file", `${node.path}/`);
                          }}
                          className="text-muted-foreground hover:text-foreground pointer-events-none inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded opacity-0 transition-opacity hover:bg-black/10 focus-visible:pointer-events-auto focus-visible:opacity-100 group-focus-within/drow:pointer-events-auto group-focus-within/drow:opacity-100 group-hover/drow:pointer-events-auto group-hover/drow:opacity-100 dark:hover:bg-white/10"
                        >
                          <FilePlus className="h-[11.5px] w-[11.5px]" />
                        </button>
                        <button
                          type="button"
                          title="Rename folder"
                          aria-label={`Rename ${node.name}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            startRename(node.path, node.name);
                          }}
                          className="text-muted-foreground hover:text-foreground pointer-events-none inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded opacity-0 transition-opacity hover:bg-black/10 focus-visible:pointer-events-auto focus-visible:opacity-100 group-focus-within/drow:pointer-events-auto group-focus-within/drow:opacity-100 group-hover/drow:pointer-events-auto group-hover/drow:opacity-100 dark:hover:bg-white/10"
                        >
                          <Pencil className="h-[11px] w-[11px]" />
                        </button>
                        <button
                          type="button"
                          title="Delete folder"
                          aria-label={`Delete ${node.name}`}
                          onClick={(e) => {
                            e.stopPropagation();
                            setActionError(null);
                            setCreateMode(null);
                            setRenamingPath(null);
                            setConfirmDeleteTarget({
                              hostId,
                              cwd,
                              path: node.path,
                              name: node.name,
                              isDir: true,
                            });
                          }}
                          className="text-muted-foreground pointer-events-none inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded opacity-0 transition-opacity hover:bg-black/10 hover:text-rose-600 focus-visible:pointer-events-auto focus-visible:opacity-100 group-focus-within/drow:pointer-events-auto group-focus-within/drow:opacity-100 group-hover/drow:pointer-events-auto group-hover/drow:opacity-100 dark:hover:bg-white/10 dark:hover:text-rose-400"
                        >
                          <Trash2 className="h-[11px] w-[11px]" />
                        </button>
                      </>
                    )}
                    {isDirty && !isOpen && (
                      <span
                        title="Contains modified files"
                        className="ml-0.5 h-1.5 w-1.5 shrink-0 rounded-full bg-amber-500/80"
                      />
                    )}
                  </div>
                </>
              )}
            </div>
            {isOpen && node.children.length > 0 && renderTree(node.children, depth + 1)}
          </div>
        );
      }

      const Icon = resolveSemanticFileIcon(node.path);
      const status = gitStatusMap.get(node.path);

      return (
        <div
          key={node.path}
          style={{ paddingLeft: `${22 + depth * 13}px` }}
          className="group/frow hover:bg-accent/60 text-foreground/80 hover:text-foreground flex h-[27px] w-full items-center justify-between gap-1.5 rounded-md pr-1.5 text-[12.5px] transition-colors"
        >
          {isRenaming ? (
            <div className="flex min-w-0 flex-1 items-center gap-1.5">
              <Icon className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
              <input
                ref={renameInputRef}
                type="text"
                value={renameValue}
                readOnly={renameBusy}
                onChange={(e) => setRenameValue(e.target.value)}
                {...ime.compositionProps}
                onKeyDown={(e) => {
                  if (ime.shouldIgnoreKeyDown(e)) return;
                  if (e.key === "Enter") {
                    e.preventDefault();
                    void submitRename(node.path);
                  } else if (e.key === "Escape") {
                    e.preventDefault();
                    cancelRename(true);
                  }
                }}
                onBlur={() => {
                  if (!renameBusy) cancelRename(false);
                }}
                className="border-border bg-background text-foreground h-[21px] min-w-0 flex-1 rounded border px-1.5 font-mono text-[12px] outline-none read-only:opacity-60"
              />
            </div>
          ) : (
            <>
              <button
                type="button"
                onClick={() =>
                  hostId && orbitActions.openFileInSidePanel(node.path, { hostId, cwd })
                }
                className="flex min-w-0 flex-1 cursor-pointer items-center gap-1.5 text-left"
              >
                <Icon className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0 stroke-[1.75]" />
                <span className="truncate">{node.name}</span>
              </button>
              <div className="flex shrink-0 items-center gap-0.5">
                <button
                  type="button"
                  title="Mention in Composer (@)"
                  aria-label={`Mention ${node.name} in Composer`}
                  onClick={() => {
                    orbitActions.addContextMention({
                      id: `file_${Date.now()}_${Math.random().toString(36).slice(2, 6)}`,
                      kind: "file",
                      path: node.path,
                    });
                    orbitActions.showToast(`Attached ${node.name} to Composer`);
                  }}
                  className="text-muted-foreground hover:text-foreground pointer-events-none inline-flex h-[18px] cursor-pointer items-center rounded px-1 font-mono text-[10.5px] opacity-0 transition-opacity hover:bg-black/10 focus-visible:pointer-events-auto focus-visible:opacity-100 group-focus-within/frow:pointer-events-auto group-focus-within/frow:opacity-100 group-hover/frow:pointer-events-auto group-hover/frow:opacity-100 dark:hover:bg-white/10"
                >
                  @
                </button>
                <button
                  type="button"
                  title="Copy relative path"
                  aria-label={`Copy path of ${node.name}`}
                  onClick={() => {
                    void navigator.clipboard.writeText(node.path);
                    orbitActions.showToast("Copied relative path");
                  }}
                  className="text-muted-foreground hover:text-foreground pointer-events-none inline-flex h-[18px] cursor-pointer items-center rounded px-1 text-[10.5px] opacity-0 transition-opacity hover:bg-black/10 focus-visible:pointer-events-auto focus-visible:opacity-100 group-focus-within/frow:pointer-events-auto group-focus-within/frow:opacity-100 group-hover/frow:pointer-events-auto group-hover/frow:opacity-100 dark:hover:bg-white/10"
                >
                  Copy
                </button>
                {canMutate && hostId && cwd && (
                  <>
                    <button
                      type="button"
                      title="Rename file"
                      aria-label={`Rename ${node.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        startRename(node.path, node.name);
                      }}
                      className="text-muted-foreground hover:text-foreground pointer-events-none inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded opacity-0 transition-opacity hover:bg-black/10 focus-visible:pointer-events-auto focus-visible:opacity-100 group-focus-within/frow:pointer-events-auto group-focus-within/frow:opacity-100 group-hover/frow:pointer-events-auto group-hover/frow:opacity-100 dark:hover:bg-white/10"
                    >
                      <Pencil className="h-[11px] w-[11px]" />
                    </button>
                    <button
                      type="button"
                      title="Delete file"
                      aria-label={`Delete ${node.name}`}
                      onClick={(e) => {
                        e.stopPropagation();
                        setActionError(null);
                        setCreateMode(null);
                        setRenamingPath(null);
                        setConfirmDeleteTarget({
                          hostId,
                          cwd,
                          path: node.path,
                          name: node.name,
                          isDir: false,
                        });
                      }}
                      className="text-muted-foreground pointer-events-none inline-flex h-[18px] w-[18px] cursor-pointer items-center justify-center rounded opacity-0 transition-opacity hover:bg-black/10 hover:text-rose-600 focus-visible:pointer-events-auto focus-visible:opacity-100 group-focus-within/frow:pointer-events-auto group-focus-within/frow:opacity-100 group-hover/frow:pointer-events-auto group-hover/frow:opacity-100 dark:hover:bg-white/10 dark:hover:text-rose-400"
                    >
                      <Trash2 className="h-[11px] w-[11px]" />
                    </button>
                  </>
                )}
                {status && (
                  <span
                    className={cn(
                      "ml-0.5 shrink-0 font-mono text-[10.5px] font-medium",
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
              </div>
            </>
          )}
        </div>
      );
    });

  return (
    <div className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* Compact Search & File Tree Actions Header */}
      <div className="border-border flex h-[36px] shrink-0 items-center gap-1.5 border-b px-3">
        <Search className="text-muted-foreground h-[13.5px] w-[13.5px] shrink-0" />
        <input
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Filter workspace files…"
          className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent text-[12.5px] outline-none"
        />
        <span className="text-muted-foreground tabular-nums mr-0.5 shrink-0 text-[11px]">
          {files.length}
        </span>
        {canMutate && (
          <>
            <button
              type="button"
              title="New file"
              aria-label="New file"
              onClick={() => startCreate("file")}
              className={cn(
                "inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors",
                createMode === "file"
                  ? "bg-accent text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              <FilePlus className="h-[13.5px] w-[13.5px]" />
            </button>
            <button
              type="button"
              title="New folder"
              aria-label="New folder"
              onClick={() => startCreate("dir")}
              className={cn(
                "inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors",
                createMode === "dir"
                  ? "bg-accent text-foreground"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              <FolderPlus className="h-[13.5px] w-[13.5px]" />
            </button>
          </>
        )}
        {hostId && cwd && (
          <button
            type="button"
            title="Refresh file index"
            aria-label="Refresh file index"
            onClick={() => void orbitActions.ensureWorkspaceFiles(hostId, cwd, true)}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
          >
            <RefreshCw className="h-[13px] w-[13px]" />
          </button>
        )}
      </div>

      {/* Inline Create File / Folder Strip */}
      {createMode && (
        <div className="border-border bg-elevated/70 flex h-[34px] shrink-0 items-center gap-1.5 border-b px-3">
          {createMode === "dir" ? (
            <FolderPlus className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
          ) : (
            <FilePlus className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
          )}
          <input
            ref={createInputRef}
            type="text"
            value={createValue}
            readOnly={createBusy}
            onChange={(e) => {
              setCreateValue(e.target.value);
              if (actionError) setActionError(null);
            }}
            {...ime.compositionProps}
            onKeyDown={(e) => {
              if (ime.shouldIgnoreKeyDown(e)) return;
              if (e.key === "Enter") {
                e.preventDefault();
                void submitCreate();
              } else if (e.key === "Escape") {
                e.preventDefault();
                cancelCreate();
              }
            }}
            placeholder={
              createMode === "dir" ? "Folder path (e.g. src/utils)" : "File path (e.g. src/index.ts)"
            }
            className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent font-mono text-[12px] outline-none read-only:opacity-60"
          />
          <button
            type="button"
            disabled={createBusy || !createValue.trim()}
            onClick={() => void submitCreate()}
            aria-label="Create"
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded disabled:opacity-35"
          >
            {createBusy ? (
              <Loader2 className="h-[12px] w-[12px] animate-spin" />
            ) : (
              <Check className="h-[13px] w-[13px]" />
            )}
          </button>
          <button
            type="button"
            onClick={cancelCreate}
            aria-label="Cancel"
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded"
          >
            <X className="h-[13px] w-[13px]" />
          </button>
        </div>
      )}

      {/* Restrained Inline Delete Confirmation Strip */}
      {confirmDeleteTarget && (
        <div className="border-b border-rose-500/25 bg-rose-500/[0.06] px-3 py-1.5 text-[12px]">
          <div className="flex items-center justify-between gap-2">
            <span className="text-foreground/90 min-w-0 truncate">
              Delete{" "}
              <span className="font-mono font-medium">{confirmDeleteTarget.path}</span>
              {confirmDeleteTarget.isDir ? " and its contents" : ""}
              {confirmDeleteHasUnsavedEdits && (
                <span className="ml-1 font-medium text-amber-700 dark:text-amber-300">
                  (has unsaved edits)
                </span>
              )}
              ?
            </span>
            <div className="flex shrink-0 items-center gap-1.5">
              <button
                type="button"
                disabled={deleteBusy}
                onClick={() => void submitDelete()}
                className="inline-flex h-[22px] cursor-pointer items-center gap-1 rounded-md border border-rose-500/35 bg-rose-500/15 px-2 text-[11.5px] font-medium text-rose-700 transition-colors hover:bg-rose-500/25 disabled:opacity-50 dark:text-rose-300"
              >
                {deleteBusy && <Loader2 className="h-[11px] w-[11px] animate-spin" />}
                Delete
              </button>
              <button
                type="button"
                disabled={deleteBusy}
                onClick={() => setConfirmDeleteTarget(null)}
                className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[22px] cursor-pointer items-center rounded-md px-2 text-[11.5px] transition-colors"
              >
                Cancel
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Subtle Action Error Banner */}
      {actionError && (
        <div className="flex items-center justify-between gap-2 border-b border-rose-500/25 bg-rose-500/[0.08] px-3 py-1.5 text-[11.5px] text-rose-700 dark:text-rose-300">
          <span className="min-w-0 truncate">{actionError}</span>
          <button
            type="button"
            onClick={() => setActionError(null)}
            aria-label="Dismiss error"
            className="inline-flex h-[18px] w-[18px] shrink-0 cursor-pointer items-center justify-center rounded hover:bg-rose-500/15"
          >
            <X className="h-[12px] w-[12px]" />
          </button>
        </div>
      )}

      {/* Tree / Filtered Results */}
      <ScrollArea className="min-h-0 flex-1" viewportClassName="p-2">
        {files.length === 0 && emptyDirs.length === 0 ? (
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
