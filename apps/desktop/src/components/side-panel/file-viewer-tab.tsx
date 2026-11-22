import { memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  AlertCircle,
  AlertTriangle,
  ArrowDown,
  Check,
  ChevronDown,
  ChevronUp,
  Code2,
  Copy,
  Eye,
  FileWarning,
  GitCompareArrows,
  Info,
  Loader2,
  MessageSquarePlus,
  Pencil,
  RefreshCw,
  Save,
  Search,
  WrapText,
  X,
} from "lucide-react";
import type { LspDiagnostic } from "@/lib/orbit-ipc";
import {
  MarkdownBlocksView,
  MarkdownLinkProvider,
} from "@/components/markdown-blocks";
import { WorkspaceCodeEditor } from "@/components/side-panel/workspace-code-editor";
import { MediaPlayer } from "@/components/viewers/media-player";
import { PdfViewer } from "@/components/viewers/pdf-viewer";
import { formatLineRangeLabel, normalizeLineRange } from "@/lib/file-links";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import { ScrollArea } from "@/components/ui/scroll-area";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import {
  type SidePanelTab,
  fileCacheKey,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { adaptTokenColor } from "@/lib/code-highlight";
import { parseAnsiLogLines, resolveAnsiColor } from "@/lib/ansi";
import { cn } from "@/lib/utils";

export const FileViewerTab = memo(function FileViewerTab({
  tab,
}: {
  tab: SidePanelTab;
}) {
  const fallbackHostId = useOrbitSelector((s) => s.selectedHostId);
  const fallbackCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme);
  const isDark = resolvedTheme === "dark";

  const hostId = tab.hostId ?? fallbackHostId ?? "";
  const cwd = tab.cwd ?? fallbackCwd ?? "~";
  const path = tab.filePath ?? "";
  const cKey = fileCacheKey(hostId, cwd, path);

  const cacheEntry = useOrbitSelector((s) => s.fileViewCache[cKey] ?? null);
  const draft = useOrbitSelector((s) => s.fileEditorDrafts[cKey] ?? null);
  const pendingUnsaved = useOrbitSelector((s) =>
    s.pendingUnsavedFileAction?.fileKey === cKey ? s.pendingUnsavedFileAction : null,
  );
  const gitStatusFiles = useOrbitSelector((s) =>
    hostId && cwd ? (s.gitReviews[`${hostId}::${cwd}`]?.status?.files ?? null) : null,
  );
  const lspDiagEntry = useOrbitSelector((s) => s.lspDiagnosticsByFile[cKey] ?? null);
  const diagnostics = useMemo<readonly LspDiagnostic[]>(
    () => lspDiagEntry?.diagnostics ?? [],
    [lspDiagEntry],
  );

  const isTaskLogPath = useMemo(
    () =>
      /[/\\]\.system_generated[/\\]tasks[/\\][^/\\]+$/i.test(path) ||
      /[/\\]claude[/\\][^/\\]*[/\\]tasks[/\\][^/\\]+$/i.test(path),
    [path],
  );
  const isLogFile = useMemo(
    () => isTaskLogPath || /\.(log|out|output)$/i.test(path),
    [isTaskLogPath, path],
  );

  const [markdownMode, setMarkdownMode] = useState<"preview" | "source">("preview");
  const [pendingMarkdownTarget, setPendingMarkdownTarget] = useState<"preview" | "source" | null>(
    null,
  );
  const [logWrapLines, setLogWrapLines] = useState(true);
  const [logFollow, setLogFollow] = useState(isTaskLogPath);
  const [copied, setCopied] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [activeMatchIdx, setActiveMatchIdx] = useState(0);
  const [renderedLineCap, setRenderedLineCap] = useState(800);
  const [diagDrawerOpen, setDiagDrawerOpen] = useState(false);
  const [editorJumpTarget, setEditorJumpTarget] = useState<{
    line: number;
    col?: number;
    nonce: number;
  } | null>(null);

  const findInputRef = useRef<HTMLInputElement | null>(null);
  const logViewportRef = useRef<HTMLDivElement | null>(null);
  const lineRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const ime = useImeCompositionGuard();
  const mod = modSymbol();

  useEffect(() => {
    setRenderedLineCap(800);
    setLogFollow(isTaskLogPath);
  }, [hostId, cwd, path, isTaskLogPath]);

  useEffect(() => {
    if (hostId && path && !cacheEntry) {
      void orbitActions.loadWorkspaceFile(hostId, cwd, path, false);
    }
  }, [hostId, cwd, path, cacheEntry]);

  // Gently auto-refresh live background task logs while Follow mode is active
  const isCacheError = cacheEntry?.status === "error";
  useEffect(() => {
    if (!isLogFile || !logFollow || !hostId || !path || isCacheError) return;
    const id = window.setInterval(() => {
      const snap = orbitActions.getSnapshot();
      if (!snap.sidePanelOpen || snap.activeSidePanelTabId !== tab.id) return;
      if (typeof document !== "undefined" && document.hidden) return;
      void orbitActions.loadWorkspaceFile(hostId, cwd, path, true);
    }, 2000);
    return () => window.clearInterval(id);
  }, [isLogFile, logFollow, hostId, cwd, path, tab.id, isCacheError]);

  const data = cacheEntry?.data ?? null;
  const relPath = useMemo(() => stripWorkspacePrefix(path, cwd), [path, cwd]);
  const displayHeaderPath = useMemo(() => {
    if (!isTaskLogPath) return relPath;
    const normalized = path.replace(/\\/g, "/");
    const base = normalized.slice(normalized.lastIndexOf("/") + 1);
    return base || relPath;
  }, [isTaskLogPath, path, relPath]);

  const isMarkdown = data?.language === "markdown" && (data?.markdown_blocks?.length ?? 0) > 0;
  const isEditable = Boolean(
    data && data.content_kind === "text" && !data.truncated && !isLogFile && data.writable !== false,
  );
  const isReadOnlyText = Boolean(
    data && data.content_kind === "text" && !isLogFile && (data.writable === false || data.truncated),
  );
  const isEditMode = Boolean(isEditable && draft?.mode === "edit");
  const isDirty = Boolean(draft?.dirty);
  const isLspEligible = Boolean(
    data &&
      data.content_kind === "text" &&
      !data.truncated &&
      !isLogFile &&
      !isMarkdown &&
      hostId &&
      cwd &&
      cwd !== "~" &&
      relPath,
  );

  useEffect(() => {
    if (!isLspEligible) return;
    void orbitActions.loadFileDiagnostics(hostId, cwd, path, true);
  }, [isLspEligible, hostId, cwd, path, data?.etag]);

  const diagCounts = useMemo(() => {
    let errors = 0;
    let warnings = 0;
    let others = 0;
    for (const d of diagnostics) {
      if (d.severity === "error") errors += 1;
      else if (d.severity === "warning") warnings += 1;
      else others += 1;
    }
    return { errors, warnings, others, total: diagnostics.length };
  }, [diagnostics]);

  const diagByLine = useMemo(() => {
    const map = new Map<number, LspDiagnostic>();
    if (diagnostics.length === 0) return map;
    const rank = (sev: LspDiagnostic["severity"]) =>
      sev === "error" ? 0 : sev === "warning" ? 1 : sev === "info" ? 2 : 3;
    for (const d of diagnostics) {
      const existing = map.get(d.line);
      if (!existing || rank(d.severity) < rank(existing.severity)) {
        map.set(d.line, d);
      }
    }
    return map;
  }, [diagnostics]);

  const jumpToDiagnosticLine = useCallback(
    (line: number, col = 1) => {
      if (isEditMode) {
        setEditorJumpTarget({ line, col, nonce: Date.now() });
        return;
      }
      if (line > renderedLineCap) {
        setRenderedLineCap(line + 200);
      }
      orbitActions.openFileInSidePanel(relPath, {
        hostId,
        cwd,
        focusLine: line,
      });
    },
    [isEditMode, renderedLineCap, relPath, hostId, cwd],
  );

  const handleGotoDefinition = useCallback(
    (line: number, col: number) => {
      if (!isLspEligible) return;
      void orbitActions.gotoLspDefinition(hostId, cwd, path, line, col);
    },
    [isLspEligible, hostId, cwd, path],
  );

  // Apply deferred Markdown mode switch once the unsaved-changes guard resolves out of Edit mode
  useEffect(() => {
    if (!pendingMarkdownTarget) return;
    if (!isEditMode) {
      setMarkdownMode(pendingMarkdownTarget);
      setPendingMarkdownTarget(null);
    } else if (!pendingUnsaved) {
      // Guard was cancelled while still in Edit mode
      setPendingMarkdownTarget(null);
    }
  }, [pendingMarkdownTarget, isEditMode, pendingUnsaved]);

  const logLines = useMemo(() => {
    if (!isLogFile || !data || data.content_kind !== "text") return [];
    return parseAnsiLogLines(data.content);
  }, [isLogFile, data]);

  // Keep scrolled to latest log lines when Follow mode is enabled
  useLayoutEffect(() => {
    if (!isLogFile || !logFollow || logLines.length === 0) return;
    const el = logViewportRef.current;
    if (!el) return;
    el.scrollTop = el.scrollHeight;
  }, [isLogFile, logFollow, logLines.length, data?.size_bytes, logWrapLines]);

  const focusRange = useMemo(
    () => normalizeLineRange(tab.focusLine, tab.focusEndLine),
    [tab.focusLine, tab.focusEndLine],
  );
  const focusLabel = useMemo(
    () => formatLineRangeLabel(focusRange.startLine, focusRange.endLine),
    [focusRange.startLine, focusRange.endLine],
  );

  // Automatically switch Markdown tabs to Source view when jumping to a specific line or range
  useEffect(() => {
    if (focusRange.startLine != null && isMarkdown) {
      setMarkdownMode("source");
    }
  }, [focusRange.startLine, tab.focusNonce, isMarkdown]);

  // Sync tab.focusNonce into WorkspaceCodeEditor so same-file Go-to-Definition works in Edit mode
  useEffect(() => {
    if (!isEditMode || !tab.focusNonce || focusRange.startLine == null) return;
    setEditorJumpTarget({
      line: focusRange.startLine,
      col: 1,
      nonce: tab.focusNonce,
    });
  }, [isEditMode, tab.focusNonce, focusRange.startLine]);

  const hasGitDiff = useMemo(() => {
    if (!gitStatusFiles) return false;
    return gitStatusFiles.some(
      (f) => f.path === relPath || f.path === path || relPath.endsWith(`/${f.path}`),
    );
  }, [gitStatusFiles, relPath, path]);

  const rawLowerLines = useMemo(() => {
    if (!data || data.content_kind !== "text") return [];
    if (isLogFile) return logLines.map((l) => l.plain.toLowerCase());
    return data.content.split(/\r?\n/).map((l) => l.toLowerCase());
  }, [data, isLogFile, logLines]);

  const matchLineNumbers = useMemo(() => {
    const q = findQuery.trim().toLowerCase();
    if (!q || rawLowerLines.length === 0) return [];
    const matched: number[] = [];
    for (let i = 0; i < rawLowerLines.length; i += 1) {
      if (rawLowerLines[i].includes(q)) {
        matched.push(i + 1);
      }
    }
    return matched;
  }, [findQuery, rawLowerLines]);

  const matchLineSet = useMemo(() => new Set(matchLineNumbers), [matchLineNumbers]);

  useEffect(() => {
    setActiveMatchIdx(0);
  }, [findQuery]);

  const activeTargetLine =
    matchLineNumbers.length > 0
      ? matchLineNumbers[Math.min(activeMatchIdx, matchLineNumbers.length - 1)]
      : focusRange.startLine;

  const requiredMaxLine =
    matchLineNumbers.length > 0
      ? activeTargetLine
      : (focusRange.endLine ?? focusRange.startLine);

  // Ensure line window expands automatically to reach any requested focus range or find match
  useEffect(() => {
    if (requiredMaxLine && requiredMaxLine > renderedLineCap) {
      setRenderedLineCap(requiredMaxLine + 200);
    }
  }, [requiredMaxLine, renderedLineCap]);

  // Scroll to requested line or active find match once DOM nodes finish layout
  useEffect(() => {
    if (!activeTargetLine || isEditMode) return;
    const rafId = window.requestAnimationFrame(() => {
      const node = lineRefs.current.get(activeTargetLine);
      if (node) {
        node.scrollIntoView({ block: "center", behavior: "smooth" });
      }
    });
    return () => window.cancelAnimationFrame(rafId);
  }, [
    activeTargetLine,
    tab.focusNonce,
    data?.resolved_path,
    markdownMode,
    renderedLineCap,
    isEditMode,
  ]);

  const handleCopyContent = async () => {
    const sourceText = isEditMode && draft ? draft.content : data?.content;
    if (sourceText == null) return;
    try {
      const textToCopy = isLogFile
        ? logLines.map((l) => l.plain).join("\n")
        : sourceText;
      await navigator.clipboard.writeText(textToCopy);
      setCopied(true);
      setTimeout(() => setCopied(false), 1400);
    } catch {
      orbitActions.showToast("Failed to copy file contents");
    }
  };

  const openFind = () => {
    setFindOpen(true);
    if (isMarkdown) setMarkdownMode("source");
    requestAnimationFrame(() => findInputRef.current?.focus());
  };

  const canFindInFile = !data || data.content_kind === "text";

  useEffect(() => {
    const onWindowKeyDown = (e: KeyboardEvent) => {
      const snap = orbitActions.getSnapshot();
      if (
        !snap.sidePanelOpen ||
        snap.activeSidePanelTabId !== tab.id ||
        snap.terminalFocused ||
        snap.commandPaletteOpen ||
        snap.quickOpenFilesOpen ||
        snap.mediaPreviewTarget !== null
      ) {
        return;
      }
      if (
        canFindInFile &&
        !isEditMode &&
        (e.metaKey || e.ctrlKey) &&
        !e.shiftKey &&
        !e.altKey &&
        e.key.toLowerCase() === "f"
      ) {
        e.preventDefault();
        setFindOpen(true);
        if (isMarkdown) setMarkdownMode("source");
        requestAnimationFrame(() => findInputRef.current?.focus());
      } else if (
        (e.metaKey || e.ctrlKey) &&
        !e.shiftKey &&
        !e.altKey &&
        e.key.toLowerCase() === "e" &&
        isEditable
      ) {
        e.preventDefault();
        if (isEditMode) {
          orbitActions.requestFileViewMode(hostId, cwd, path);
        } else {
          setFindOpen(false);
          orbitActions.enterFileEditMode(hostId, cwd, path);
        }
      } else if (
        (e.metaKey || e.ctrlKey) &&
        !e.shiftKey &&
        !e.altKey &&
        e.key.toLowerCase() === "s" &&
        isEditMode
      ) {
        e.preventDefault();
        if (!orbitActions.getSnapshot().fileEditorDrafts[cKey]?.saving) {
          void orbitActions.saveWorkspaceFile(hostId, cwd, path, false);
        }
      } else if (e.key === "Escape" && findOpen) {
        e.preventDefault();
        setFindOpen(false);
        setFindQuery("");
      }
    };
    window.addEventListener("keydown", onWindowKeyDown);
    return () => window.removeEventListener("keydown", onWindowKeyDown);
  }, [findOpen, isMarkdown, canFindInFile, isEditable, isEditMode, cKey, hostId, cwd, path, tab.id]);

  const stepMatch = (delta: number) => {
    if (matchLineNumbers.length === 0) return;
    setActiveMatchIdx((prev) => (prev + delta + matchLineNumbers.length) % matchLineNumbers.length);
  };

  return (
    <div
      tabIndex={-1}
      className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden outline-none"
      onKeyDown={(e) => {
        if (canFindInFile && !isEditMode && (e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
          e.preventDefault();
          openFind();
        } else if (e.key === "Escape" && findOpen) {
          e.preventDefault();
          setFindOpen(false);
          setFindQuery("");
        }
      }}
    >
      {/* Slim File Breadcrumb & Actions Sub-Bar */}
      <div className="border-border flex h-[36px] shrink-0 items-center justify-between gap-2 border-b px-3">
        <div className="flex min-w-0 items-center gap-2">
          {isTaskLogPath && (
            <span
              className={cn(
                "size-[6px] shrink-0 rounded-full",
                logFollow ? "bg-emerald-500/85" : "bg-muted-foreground/45",
              )}
            />
          )}
          <span
            title={data?.resolved_path || path}
            className="selectable-text text-foreground/85 truncate font-mono text-[12px]"
          >
            {displayHeaderPath}
          </span>
          {isDirty && (
            <span
              title="Unsaved changes"
              aria-label="Unsaved changes"
              className="size-[6px] shrink-0 rounded-full bg-amber-500/90"
            />
          )}
          {data && (
            <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[11px]">
              {isLogFile
                ? `${logLines.length.toLocaleString()} lines · ${formatFileSizeCompact(data.size_bytes)}`
                : formatFileSizeCompact(data.size_bytes)}
            </span>
          )}
          {isReadOnlyText && (
            <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px]">
              Read-only
            </span>
          )}
          {diagCounts.total > 0 && (
            <button
              type="button"
              title="Toggle LSP diagnostics"
              onClick={() => setDiagDrawerOpen((v) => !v)}
              className={cn(
                "tabular-nums inline-flex h-[21px] shrink-0 cursor-pointer items-center gap-1.5 rounded-md border px-1.5 text-[11px] font-medium transition-colors",
                diagDrawerOpen
                  ? "border-foreground/20 bg-accent text-foreground"
                  : "border-border/80 bg-background/70 text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              {diagCounts.errors > 0 && (
                <span className="inline-flex items-center gap-1 text-rose-600 dark:text-rose-400">
                  <AlertCircle className="h-[11px] w-[11px]" />
                  {diagCounts.errors}
                </span>
              )}
              {diagCounts.warnings > 0 && (
                <span className="inline-flex items-center gap-1 text-amber-600 dark:text-amber-400">
                  <AlertTriangle className="h-[11px] w-[11px]" />
                  {diagCounts.warnings}
                </span>
              )}
              {diagCounts.errors === 0 && diagCounts.warnings === 0 && diagCounts.others > 0 && (
                <span className="inline-flex items-center gap-1 text-sky-600 dark:text-sky-400">
                  <Info className="h-[11px] w-[11px]" />
                  {diagCounts.others}
                </span>
              )}
            </button>
          )}
          {focusLabel && focusRange.startLine != null && !isEditMode && (
            <button
              type="button"
              title="Jump to focused line range"
              onClick={() => {
                if (isMarkdown && markdownMode !== "source") {
                  setMarkdownMode("source");
                }
                const targetLine = focusRange.startLine;
                if (targetLine == null) return;
                window.requestAnimationFrame(() => {
                  const node = lineRefs.current.get(targetLine);
                  node?.scrollIntoView({ block: "center", behavior: "smooth" });
                });
              }}
              className="bg-primary/[0.12] text-primary hover:bg-primary/[0.18] tabular-nums shrink-0 cursor-pointer rounded px-1.5 py-0.5 font-mono text-[11px] font-medium transition-colors"
            >
              {focusLabel}
            </button>
          )}
        </div>

        <div className="flex shrink-0 items-center gap-1">
          {isMarkdown ? (
            <div className="bg-muted inline-flex h-[24px] items-center rounded-md p-0.5 text-[11px]">
              <button
                type="button"
                onClick={() => {
                  if (isEditMode) {
                    if (isDirty) {
                      setPendingMarkdownTarget("preview");
                    } else {
                      setMarkdownMode("preview");
                    }
                    orbitActions.requestFileViewMode(hostId, cwd, path);
                  } else {
                    setMarkdownMode("preview");
                  }
                }}
                className={cn(
                  "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                  !isEditMode && markdownMode === "preview"
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Eye className="h-[11.5px] w-[11.5px]" />
                Preview
              </button>
              <button
                type="button"
                onClick={() => {
                  if (isEditMode) {
                    if (isDirty) {
                      setPendingMarkdownTarget("source");
                    } else {
                      setMarkdownMode("source");
                    }
                    orbitActions.requestFileViewMode(hostId, cwd, path);
                  } else {
                    setMarkdownMode("source");
                  }
                }}
                className={cn(
                  "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                  !isEditMode && markdownMode === "source"
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Code2 className="h-[11.5px] w-[11.5px]" />
                View
              </button>
              {isEditable && (
                <button
                  type="button"
                  title={`Edit file (${mod}E)`}
                  onClick={() => {
                    setFindOpen(false);
                    setPendingMarkdownTarget(null);
                    orbitActions.enterFileEditMode(hostId, cwd, path);
                  }}
                  className={cn(
                    "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                    isEditMode
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <Pencil className="h-[11px] w-[11px]" />
                  Edit
                </button>
              )}
            </div>
          ) : (
            isEditable && (
              <div className="bg-muted inline-flex h-[24px] items-center rounded-md p-0.5 text-[11px]">
                <button
                  type="button"
                  onClick={() => orbitActions.requestFileViewMode(hostId, cwd, path)}
                  className={cn(
                    "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                    !isEditMode
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  View
                </button>
                <button
                  type="button"
                  title={`Edit file (${mod}E)`}
                  onClick={() => {
                    setFindOpen(false);
                    orbitActions.enterFileEditMode(hostId, cwd, path);
                  }}
                  className={cn(
                    "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                    isEditMode
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  Edit
                </button>
              </div>
            )
          )}

          {isEditMode && (
            <button
              type="button"
              title={`Save file (${mod}S)`}
              disabled={draft?.saving || !isDirty}
              onClick={() => void orbitActions.saveWorkspaceFile(hostId, cwd, path, false)}
              className={cn(
                "inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors disabled:cursor-default disabled:opacity-45",
                isDirty
                  ? "bg-foreground text-background hover:opacity-90"
                  : "text-muted-foreground hover:bg-accent hover:text-foreground",
              )}
            >
              {draft?.saving ? (
                <Loader2 className="h-[12px] w-[12px] animate-spin" />
              ) : (
                <Save className="h-[12px] w-[12px]" />
              )}
              Save
            </button>
          )}

          {hasGitDiff && (
            <button
              type="button"
              title="View Git diff for this file"
              onClick={() => orbitActions.openReviewTab({ path: relPath, scope: "working_tree" })}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-1.5 text-[11.5px] transition-colors"
            >
              <GitCompareArrows className="h-[13px] w-[13px]" />
              <span>Diff</span>
            </button>
          )}

          {data?.content_kind === "text" && isLogFile && (
            <button
              type="button"
              title={logWrapLines ? "Disable line wrap" : "Wrap long lines"}
              onClick={() => setLogWrapLines((v) => !v)}
              className={cn(
                "inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors hover:bg-accent hover:text-foreground",
                logWrapLines ? "text-foreground/85" : "text-muted-foreground/65",
              )}
            >
              <WrapText className="h-[13px] w-[13px]" />
            </button>
          )}

          {data?.content_kind === "text" && (
            <>
              {!isEditMode && (
                <button
                  type="button"
                  title={`Find in file (${mod}F)`}
                  onClick={openFind}
                  className={cn(
                    "inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors",
                    findOpen
                      ? "bg-accent text-foreground"
                      : "text-muted-foreground hover:bg-accent hover:text-foreground",
                  )}
                >
                  <Search className="h-[13px] w-[13px]" />
                </button>
              )}
              <button
                type="button"
                title="Copy file contents"
                onClick={() => void handleCopyContent()}
                className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
              >
                {copied ? (
                  <Check className="h-[13px] w-[13px] text-emerald-500" />
                ) : (
                  <Copy className="h-[13px] w-[13px]" />
                )}
              </button>
            </>
          )}

          <button
            type="button"
            title="Reload file"
            onClick={() => orbitActions.requestFileReload(hostId, cwd, path)}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
          >
            <RefreshCw
              className={cn("h-[13px] w-[13px]", cacheEntry?.status === "loading" && "animate-spin")}
            />
          </button>
        </div>
      </div>

      {/* Collapsible LSP Diagnostics Drawer */}
      {diagDrawerOpen && diagnostics.length > 0 && (
        <div className="border-border bg-elevated/75 max-h-[168px] shrink-0 overflow-auto border-b">
          <div className="divide-border/60 divide-y">
            {diagnostics.slice(0, 50).map((d, idx) => (
              <button
                key={`${d.line}:${d.col}:${idx}`}
                type="button"
                onClick={() => jumpToDiagnosticLine(d.line, d.col)}
                className="hover:bg-accent/70 flex w-full cursor-pointer items-start gap-2 px-3 py-1.5 text-left text-[11.5px] transition-colors"
              >
                <span
                  className={cn(
                    "mt-[5px] size-[6px] shrink-0 rounded-full",
                    d.severity === "error"
                      ? "bg-rose-500"
                      : d.severity === "warning"
                        ? "bg-amber-500"
                        : "bg-sky-500",
                  )}
                />
                <span className="text-foreground/90 min-w-0 flex-1 leading-snug">
                  {d.message}
                  {(d.source || d.code) && (
                    <span className="text-muted-foreground/75 ml-1.5 font-mono text-[10.5px]">
                      {[d.source, d.code].filter(Boolean).join(" ")}
                    </span>
                  )}
                </span>
                <span className="text-muted-foreground tabular-nums shrink-0 font-mono text-[10.5px]">
                  {d.line}:{d.col}
                </span>
              </button>
            ))}
          </div>
        </div>
      )}

      {/* Quiet Inline Unsaved-Changes Guard Strip */}
      {pendingUnsaved && (
        <div className="border-border bg-elevated/85 flex shrink-0 items-center justify-between gap-2 border-b px-3 py-1.5 text-[12px]">
          <div className="flex min-w-0 items-center gap-1.5">
            <span className="size-[6px] shrink-0 rounded-full bg-amber-500" />
            <span className="text-foreground font-medium">Unsaved changes</span>
            <span className="text-muted-foreground truncate text-[11.5px]">
              · {pendingUnsaved.label}
            </span>
          </div>
          <div className="flex shrink-0 items-center gap-1.5">
            <button
              type="button"
              disabled={draft?.saving}
              onClick={() => void orbitActions.resolveUnsavedFileAction("save")}
              className="bg-foreground text-background inline-flex h-[23px] cursor-pointer items-center rounded-md px-2 text-[11.5px] font-medium transition-opacity hover:opacity-90 disabled:opacity-50"
            >
              Save
            </button>
            <button
              type="button"
              disabled={draft?.saving}
              onClick={() => void orbitActions.resolveUnsavedFileAction("discard")}
              className="inline-flex h-[23px] cursor-pointer items-center rounded-md border border-rose-500/30 bg-rose-500/10 px-2 text-[11.5px] font-medium text-rose-700 transition-colors hover:bg-rose-500/20 disabled:opacity-50 dark:text-rose-300"
            >
              Discard
            </button>
            <button
              type="button"
              onClick={() => void orbitActions.resolveUnsavedFileAction("cancel")}
              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[23px] cursor-pointer items-center rounded-md px-2 text-[11.5px] transition-colors"
            >
              Cancel
            </button>
          </div>
        </div>
      )}

      {/* Collapsible Find-in-File Strip */}
      {findOpen && canFindInFile && !isEditMode && (
        <div className="border-border bg-elevated/70 flex h-[34px] shrink-0 items-center gap-1.5 border-b px-3">
          <Search className="text-muted-foreground h-[13px] w-[13px] shrink-0" />
          <input
            ref={findInputRef}
            type="text"
            value={findQuery}
            onChange={(e) => setFindQuery(e.target.value)}
            {...ime.compositionProps}
            onKeyDown={(e) => {
              if (ime.shouldIgnoreKeyDown(e)) return;
              if (e.key === "Enter") {
                e.preventDefault();
                stepMatch(e.shiftKey ? -1 : 1);
              }
            }}
            placeholder="Find in file…"
            className="text-foreground placeholder:text-muted-foreground min-w-0 flex-1 bg-transparent text-[12px] outline-none"
          />
          {findQuery.trim() && (
            <span className="text-muted-foreground tabular-nums shrink-0 text-[11px]">
              {matchLineNumbers.length > 0
                ? `${Math.min(activeMatchIdx + 1, matchLineNumbers.length)}/${matchLineNumbers.length}`
                : "0"}
            </span>
          )}
          <button
            type="button"
            onClick={() => stepMatch(-1)}
            disabled={matchLineNumbers.length === 0}
            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded disabled:opacity-35"
          >
            <ChevronUp className="h-[13px] w-[13px]" />
          </button>
          <button
            type="button"
            onClick={() => stepMatch(1)}
            disabled={matchLineNumbers.length === 0}
            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded disabled:opacity-35"
          >
            <ChevronDown className="h-[13px] w-[13px]" />
          </button>
          <button
            type="button"
            onClick={() => {
              setFindOpen(false);
              setFindQuery("");
            }}
            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded"
          >
            <X className="h-[13px] w-[13px]" />
          </button>
        </div>
      )}

      {/* Main Content Surface */}
      {cacheEntry?.status === "loading" && !data ? (
        <div className="text-muted-foreground flex flex-1 items-center justify-center gap-2 text-[12.5px]">
          <Loader2 className="h-[15px] w-[15px] animate-spin" />
          <span>Reading {relPath}…</span>
        </div>
      ) : cacheEntry?.status === "error" && !data ? (
        <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
          <FileWarning className="h-5 w-5 opacity-60" />
          <div className="text-foreground/85 text-[13px] font-medium">Unable to open file</div>
          <div className="selectable-text max-w-sm font-mono text-[11.5px] opacity-75">
            {cacheEntry.error}
          </div>
          <button
            type="button"
            onClick={() => void orbitActions.loadWorkspaceFile(hostId, cwd, path, true)}
            className="border-border hover:bg-accent text-foreground mt-1 cursor-pointer rounded-lg border px-3 py-1 text-[12px] transition-colors"
          >
            Retry
          </button>
        </div>
      ) : !data ? null : data.content_kind === "image" && data.mime_type ? (
        <ScrollArea
          className="min-h-0 flex-1"
          viewportClassName="flex min-h-full items-center justify-center p-6"
        >
          <img
            src={`data:${data.mime_type};base64,${data.content}`}
            alt={relPath}
            className="border-border max-h-[75vh] max-w-full rounded-xl border object-contain shadow-xs"
          />
        </ScrollArea>
      ) : data.content_kind === "pdf" ? (
        <PdfViewer
          filename={relPath}
          path={path}
          hostId={hostId}
          cwd={cwd}
          base64Content={data.content}
          sizeBytes={data.size_bytes}
          truncated={data.truncated}
          hideHeaderTitle
        />
      ) : data.content_kind === "video" || data.content_kind === "audio" ? (
        <MediaPlayer
          kind={data.content_kind}
          filename={relPath}
          path={path}
          hostId={hostId}
          cwd={cwd}
          base64Content={data.content}
          mimeType={data.mime_type}
          sizeBytes={data.size_bytes}
          truncated={data.truncated}
          hideHeaderTitle
        />
      ) : data.content_kind === "binary" ? (
        <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
          <FileWarning className="h-5 w-5 opacity-60" />
          <div className="text-foreground/85 text-[13px] font-medium">Binary file</div>
          <div className="text-[12px]">
            {relPath} ({formatFileSizeCompact(data.size_bytes)}) cannot be displayed as text.
          </div>
        </div>
      ) : isEditMode && draft ? (
        <WorkspaceCodeEditor
          value={draft.content}
          language={data.language}
          lineEnding={draft.lineEnding}
          initialHighlightedLines={data.highlighted_lines}
          initialContent={data.content.replace(/\r\n/g, "\n")}
          isDark={isDark}
          dirty={draft.dirty}
          saving={draft.saving}
          saveError={draft.saveError}
          conflict={draft.conflict}
          initialFocusLine={focusRange.startLine}
          diagnostics={diagnostics}
          jumpTarget={editorJumpTarget}
          onGotoDefinition={isLspEligible ? handleGotoDefinition : undefined}
          onChange={(nextValue) =>
            orbitActions.updateFileEditorDraft(hostId, cwd, path, nextValue)
          }
          onSave={(force) => {
            void orbitActions.saveWorkspaceFile(hostId, cwd, path, Boolean(force));
          }}
          onReloadFromConflict={() => {
            void orbitActions.reloadWorkspaceFileFromConflict(hostId, cwd, path);
          }}
          onDismissError={() => orbitActions.clearFileEditorError(hostId, cwd, path)}
          onCopyMine={() => {
            void navigator.clipboard.writeText(draft.content);
            orbitActions.showToast("Copied editor buffer to clipboard");
          }}
        />
      ) : isMarkdown && markdownMode === "preview" ? (
        <ScrollArea className="min-h-0 flex-1" viewportClassName="px-5 py-4">
          <MarkdownLinkProvider hostId={hostId} cwd={cwd} baseFilePath={relPath}>
            <MarkdownBlocksView blocks={data.markdown_blocks} streaming={false} />
          </MarkdownLinkProvider>
        </ScrollArea>
      ) : isLogFile ? (
        <div className="relative flex min-h-0 flex-1 flex-col overflow-hidden">
          <ScrollArea
            orientation={logWrapLines ? "vertical" : "both"}
            viewportRef={logViewportRef}
            onViewportScroll={(e) => {
              const el = e.currentTarget;
              const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
              if (distanceFromBottom > 48 && logFollow) {
                setLogFollow(false);
              } else if (distanceFromBottom <= 20 && !logFollow && isTaskLogPath) {
                setLogFollow(true);
              }
            }}
            className="bg-canvas min-h-0 flex-1"
            viewportClassName="selectable-text px-3.5 py-2.5 font-mono text-[11.5px] leading-[1.65]"
          >
            {data.truncated && (
              <div className="border-border text-muted-foreground mb-2 rounded-lg border bg-amber-500/[0.06] px-3 py-1.5 font-sans text-[11.5px]">
                Showing partial log — file exceeds transfer cap ({formatFileSizeCompact(data.size_bytes)}).
              </div>
            )}

            <div className="space-y-[1px]">
              {(() => {
                // Tail-window large logs to keep WebView rendering fast while live-tailing
                let effectiveCap = renderedLineCap;
                if (activeTargetLine != null && logLines.length - activeTargetLine + 20 > effectiveCap) {
                  effectiveCap = logLines.length - activeTargetLine + 40;
                }
                const logStartIdx = Math.max(0, logLines.length - effectiveCap);
                const visibleLogLines =
                  logStartIdx > 0 ? logLines.slice(logStartIdx) : logLines;
                return (
                  <>
                    {logStartIdx > 0 && (
                      <div className="mb-1.5 flex items-center justify-between rounded-md bg-black/[0.03] px-2.5 py-1 font-sans text-[11px] text-muted-foreground dark:bg-white/[0.04]">
                        <span>
                          Showing latest {visibleLogLines.length.toLocaleString()} of{" "}
                          {logLines.length.toLocaleString()} lines
                        </span>
                        <button
                          type="button"
                          onClick={() => setRenderedLineCap((c) => c + 1200)}
                          className="text-foreground/85 hover:text-foreground cursor-pointer font-medium transition-colors"
                        >
                          Show earlier lines
                        </button>
                      </div>
                    )}
                    {visibleLogLines.map((line, relIdx) => {
                      const lineNumber = logStartIdx + relIdx + 1;
                      const isMatched = matchLineSet.has(lineNumber);
                      const isFocused =
                        matchLineNumbers.length > 0 &&
                        matchLineNumbers[Math.min(activeMatchIdx, matchLineNumbers.length - 1)] ===
                          lineNumber;

                      return (
                        <div
                          key={lineNumber}
                          ref={(el) => {
                            if (el) lineRefs.current.set(lineNumber, el);
                            else lineRefs.current.delete(lineNumber);
                          }}
                          className={cn(
                            "text-foreground/84 rounded-[4px] px-1 py-[0.5px]",
                            logWrapLines ? "break-words whitespace-pre-wrap" : "whitespace-pre",
                            isFocused &&
                              "bg-primary/[0.14] shadow-[inset_2px_0_0_0_var(--color-primary)]",
                            !isFocused && isMatched && "bg-amber-500/[0.14]",
                          )}
                        >
                          {line.plain.length === 0 ? (
                            <span className="block h-[6px]" />
                          ) : line.spans ? (
                            line.spans.map((span, sIdx) =>
                              span.style ? (
                                <span
                                  key={sIdx}
                                  style={{
                                    color: resolveAnsiColor(span.style.fg, isDark),
                                    backgroundColor: resolveAnsiColor(span.style.bg, isDark),
                                    fontWeight: span.style.bold ? 600 : undefined,
                                    opacity: span.style.dim ? 0.58 : undefined,
                                    fontStyle: span.style.italic ? "italic" : undefined,
                                    textDecoration:
                                      span.style.underline && span.style.strike
                                        ? "underline line-through"
                                        : span.style.underline
                                          ? "underline"
                                          : span.style.strike
                                            ? "line-through"
                                            : undefined,
                                  }}
                                >
                                  {span.text}
                                </span>
                              ) : (
                                <span key={sIdx}>{span.text}</span>
                              ),
                            )
                          ) : (
                            line.plain
                          )}
                        </div>
                      );
                    })}
                  </>
                );
              })()}
            </div>
          </ScrollArea>

          {/* Subtle floating jump-to-bottom button only when user scrolls up away from live tail */}
          {isTaskLogPath && !logFollow && (
            <button
              type="button"
              title="Jump to latest output"
              aria-label="Jump to latest output"
              onClick={() => {
                setLogFollow(true);
                void orbitActions.loadWorkspaceFile(hostId, cwd, path, true);
                window.requestAnimationFrame(() => {
                  const el = logViewportRef.current;
                  if (el) el.scrollTop = el.scrollHeight;
                });
              }}
              className="floating-surface border-border bg-elevated/95 text-muted-foreground hover:text-foreground absolute right-3.5 bottom-3.5 inline-flex size-[28px] cursor-pointer items-center justify-center rounded-full border transition-colors"
            >
              <ArrowDown className="size-3.5 stroke-[2]" />
            </button>
          )}
        </div>
      ) : (
        <ScrollArea
          orientation="both"
          className="bg-canvas min-h-0 flex-1"
          viewportClassName="selectable-text py-2.5 font-mono text-[12px] leading-[1.6]"
        >
          {data.truncated && (
            <div className="border-border text-muted-foreground mx-3 mb-2 rounded-lg border bg-amber-500/[0.06] px-3 py-1.5 font-sans text-[11.5px]">
              Showing initial preview — file exceeds transfer cap ({formatFileSizeCompact(data.size_bytes)}).
            </div>
          )}

          {data.highlighted_lines.slice(0, renderedLineCap).map((line) => {
            const isMatched = matchLineSet.has(line.line_number);
            const isRangeFocused =
              matchLineNumbers.length === 0 &&
              focusRange.startLine != null &&
              line.line_number >= focusRange.startLine &&
              line.line_number <= (focusRange.endLine ?? focusRange.startLine);
            const isFocused =
              (matchLineNumbers.length > 0 &&
                matchLineNumbers[Math.min(activeMatchIdx, matchLineNumbers.length - 1)] ===
                  line.line_number) ||
              isRangeFocused;
            const rawLineText = line.tokens.map((t) => t.text).join("");
            const lineDiag = diagByLine.get(line.line_number);
            let runningCol = 1;

            return (
              <div
                key={line.line_number}
                ref={(el) => {
                  if (el) lineRefs.current.set(line.line_number, el);
                  else lineRefs.current.delete(line.line_number);
                }}
                onDoubleClick={() => {
                  if (isEditable) {
                    setFindOpen(false);
                    orbitActions.enterFileEditMode(hostId, cwd, path);
                  }
                }}
                className={cn(
                  "group/fln flex min-w-fit items-start px-2 transition-colors hover:bg-black/[0.03] dark:hover:bg-white/[0.035]",
                  isFocused && "bg-primary/[0.12] shadow-[inset_2px_0_0_0_var(--color-primary)]",
                  !isFocused && isMatched && "bg-amber-500/[0.12]",
                )}
              >
                <span
                  title={lineDiag ? `${lineDiag.severity}: ${lineDiag.message}` : undefined}
                  className="tabular-nums inline-flex min-w-[2.75rem] shrink-0 items-center justify-end gap-1 pr-2 text-right text-[11px] text-zinc-400 select-none dark:text-zinc-600"
                >
                  {lineDiag && (
                    <span
                      className={cn(
                        "size-[5px] shrink-0 rounded-full",
                        lineDiag.severity === "error"
                          ? "bg-rose-500"
                          : lineDiag.severity === "warning"
                            ? "bg-amber-500"
                            : "bg-sky-500",
                      )}
                    />
                  )}
                  <span>{line.line_number}</span>
                </span>
                <button
                  type="button"
                  title="Quote line into Composer"
                  onClick={() =>
                    orbitActions.quoteDiffLineToComposer(relPath, line.line_number, rawLineText)
                  }
                  className="text-muted-foreground hover:text-foreground mt-[2px] mr-1.5 inline-flex h-[16px] w-[16px] shrink-0 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover/fln:opacity-100 hover:bg-black/[0.08] dark:hover:bg-white/[0.12]"
                >
                  <MessageSquarePlus className="h-[11.5px] w-[11.5px]" />
                </button>
                <span className="pr-4 whitespace-pre">
                  {line.tokens.length === 0
                    ? " "
                    : line.tokens.map((tok, idx) => {
                        const tokStartCol = runningCol;
                        runningCol += tok.text.length;
                        return (
                          <span
                            key={idx}
                            onClick={(e) => {
                              if (
                                !isLspEligible ||
                                !(e.metaKey || e.ctrlKey) ||
                                e.shiftKey ||
                                e.altKey
                              ) {
                                return;
                              }
                              if (!tok.text.trim()) return;
                              e.preventDefault();
                              e.stopPropagation();
                              let offsetInTok = Math.floor(tok.text.length / 2);
                              const doc = document as Document & {
                                caretPositionFromPoint?: (
                                  x: number,
                                  y: number,
                                ) => { offsetNode: Node; offset: number } | null;
                                caretRangeFromPoint?: (x: number, y: number) => Range | null;
                              };
                              const caretPos = doc.caretPositionFromPoint?.(e.clientX, e.clientY);
                              if (
                                caretPos &&
                                e.currentTarget.contains(caretPos.offsetNode) &&
                                Number.isFinite(caretPos.offset)
                              ) {
                                offsetInTok = Math.max(
                                  0,
                                  Math.min(caretPos.offset, Math.max(0, tok.text.length - 1)),
                                );
                              } else {
                                const caretRange = doc.caretRangeFromPoint?.(e.clientX, e.clientY);
                                if (
                                  caretRange &&
                                  e.currentTarget.contains(caretRange.startContainer) &&
                                  Number.isFinite(caretRange.startOffset)
                                ) {
                                  offsetInTok = Math.max(
                                    0,
                                    Math.min(
                                      caretRange.startOffset,
                                      Math.max(0, tok.text.length - 1),
                                    ),
                                  );
                                }
                              }
                              handleGotoDefinition(
                                line.line_number,
                                Math.max(1, tokStartCol + offsetInTok),
                              );
                            }}
                            style={{
                              color: adaptTokenColor(tok.fg_hex, isDark),
                              fontWeight: tok.bold ? 600 : undefined,
                              fontStyle: tok.italic ? "italic" : undefined,
                            }}
                          >
                            {tok.text}
                          </span>
                        );
                      })}
                </span>
              </div>
            );
          })}

          {data.highlighted_lines.length > renderedLineCap && (
            <div className="mx-3 mt-3 flex items-center justify-between gap-2 rounded-lg border border-black/[0.08] bg-black/[0.02] px-3 py-2 font-sans text-[11.5px] dark:border-white/[0.08] dark:bg-white/[0.02]">
              <span className="text-muted-foreground tabular-nums">
                Showing {renderedLineCap.toLocaleString()} of{" "}
                {data.highlighted_lines.length.toLocaleString()} lines
              </span>
              <div className="flex items-center gap-1.5">
                <button
                  type="button"
                  onClick={() => setRenderedLineCap((c) => c + 1200)}
                  className="border-border bg-elevated text-foreground hover:bg-accent cursor-pointer rounded-md border px-2 py-0.5 font-medium transition-colors"
                >
                  Load more
                </button>
                <button
                  type="button"
                  onClick={() => setRenderedLineCap(data.highlighted_lines.length)}
                  className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-2 py-0.5 transition-colors"
                >
                  Show all
                </button>
              </div>
            </div>
          )}
        </ScrollArea>
      )}
    </div>
  );
});
