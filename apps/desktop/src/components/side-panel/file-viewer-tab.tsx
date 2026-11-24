import { Suspense, lazy, memo, useCallback, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
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
  MessageSquarePlus,
  Pencil,
  RefreshCw,
  Save,
  Search,
  WrapText,
  X,
} from "lucide-react";
import { type HighlightedLineView, type LspDiagnostic, orbitIpc } from "@/lib/orbit-ipc";
import {
  MarkdownBlocksView,
  MarkdownLinkProvider,
} from "@/components/markdown-blocks";
const LazyWorkspaceCodeEditor = lazy(
  () => import("@/components/side-panel/workspace-code-editor"),
);
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { MediaPlayer } from "@/components/viewers/media-player";
import { PdfViewer } from "@/components/viewers/pdf-viewer";
import { formatLineRangeLabel, normalizeLineRange } from "@/lib/file-links";
import {
  isFocusInsideFileViewerPane,
  isFocusInsideModalDialog,
  resolveFileViewerShortcut,
} from "@/lib/file-viewer-shortcuts";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import { SimpleTooltip } from "@/components/ui/overlays";
import { ScrollArea } from "@/components/ui/scroll-area";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { stripWorkspacePrefix } from "@/lib/timeline-coalesce";
import { useDelayedLoading } from "@/lib/use-delayed-loading";
import {
  type SidePanelTab,
  fileCacheKey,
  orbitActions,
  useOrbitSelector,
} from "@/state/orbit-store";
import { DARK_PLAIN_TOKEN_FG, LIGHT_PLAIN_TOKEN_FG, resolveTokenColor } from "@/lib/code-highlight";
import { useI18n } from "@/lib/i18n";
import { parseAnsiLogLines, resolveAnsiColor } from "@/lib/ansi";
import {
  LOG_INITIAL_LINE_CAP,
  LOG_PAGE_INCREMENT,
  computeLogWindowSlice,
  computeNextLogLineCap,
  computePrependRestoredScrollTop,
  shouldTriggerTopLogPagination,
} from "@/lib/stage-pagination";
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
  const { t } = useI18n();

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
  const [renderedLineCap, setRenderedLineCap] = useState(LOG_INITIAL_LINE_CAP);
  const [diagDrawerOpen, setDiagDrawerOpen] = useState(false);
  const [lazyHighlights, setLazyHighlights] = useState<Record<number, HighlightedLineView>>({});
  const lazyHighlightRequestedChunksRef = useRef<Set<number>>(new Set());
  const lazyHighlightSeqRef = useRef(0);
  const [editorJumpTarget, setEditorJumpTarget] = useState<{
    line: number;
    col?: number;
    nonce: number;
  } | null>(null);
  const [editEntryFocusLine, setEditEntryFocusLine] = useState<number | null>(null);

  const containerRef = useRef<HTMLDivElement | null>(null);
  const findInputRef = useRef<HTMLInputElement | null>(null);
  const logViewportRef = useRef<HTMLDivElement | null>(null);
  const codeViewportRef = useRef<HTMLDivElement | null>(null);
  const lineRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const pendingLogPrependAnchorRef = useRef<{
    prevScrollHeight: number;
    prevScrollTop: number;
  } | null>(null);
  const ime = useImeCompositionGuard();
  const mod = modSymbol();

  useEffect(() => {
    pendingLogPrependAnchorRef.current = null;
    setRenderedLineCap(LOG_INITIAL_LINE_CAP);
    setLogFollow(isTaskLogPath);
    lazyHighlightSeqRef.current += 1;
    lazyHighlightRequestedChunksRef.current.clear();
    setLazyHighlights({});
    setEditEntryFocusLine(null);
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
  const isInitialLoading = Boolean((!cacheEntry || cacheEntry.status === "loading") && !data);
  const showLoadingIndicator = useDelayedLoading(isInitialLoading);
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

  // Defer LSP diagnostics slightly so panel slide-in & initial file paint complete without jank
  useEffect(() => {
    if (!isLspEligible) return;
    const timer = window.setTimeout(() => {
      void orbitActions.loadFileDiagnostics(hostId, cwd, path, true);
    }, 260);
    return () => window.clearTimeout(timer);
  }, [isLspEligible, hostId, cwd, path, data?.etag]);

  // Reset lazy highlight overlays whenever file content/etag changes
  useEffect(() => {
    lazyHighlightSeqRef.current += 1;
    lazyHighlightRequestedChunksRef.current.clear();
    setLazyHighlights({});
  }, [data?.etag, data?.resolved_path]);

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

  const handleJumpToFileLocation = useCallback(
    (targetPath: string, line: number, col = 1) => {
      const normalizedTarget = targetPath.replace(/\\/g, "/").replace(/^\.\//, "");
      const normalizedCurrent = relPath.replace(/\\/g, "/").replace(/^\.\//, "");
      if (normalizedTarget === normalizedCurrent) {
        jumpToDiagnosticLine(line, col);
        return;
      }
      orbitActions.openFileInSidePanel(targetPath, {
        hostId,
        cwd,
        focusLine: line,
        initialMode: isEditMode ? "edit" : "view",
      });
    },
    [relPath, jumpToDiagnosticLine, hostId, cwd, isEditMode],
  );

  const handleAfterLspWorkspaceEdit = useCallback(
    async (modifiedFiles: readonly string[]) => {
      await orbitActions.refreshWorkspaceFilesAfterLspEdit(hostId, cwd, modifiedFiles);
    },
    [hostId, cwd],
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

  // Build virtualized code lines lazily:
  // - Non-markdown source files receive a pre-highlighted prefix from Rust (`data.highlighted_lines`,
  //   up to 250 lines) so large files never duplicate trailing plain lines across IPC or JS heap.
  // - Deeper ranges (and Markdown Source mode) render immediately from raw lines and highlight
  //   visible 250-line chunks lazily in the background.
  const isCodeViewActive = Boolean(
    data &&
      data.content_kind === "text" &&
      !isLogFile &&
      !isEditMode &&
      (!isMarkdown || markdownMode === "source"),
  );

  const rawCodeLines = useMemo<readonly string[]>(() => {
    if (!isCodeViewActive || !data) return [];
    return data.content.split(/\r?\n/);
  }, [isCodeViewActive, data]);

  const codeLineCount = rawCodeLines.length;
  const preHighlightedLineCount = data?.highlighted_lines.length ?? 0;

  const codeVirtualizer = useVirtualizer({
    count: isCodeViewActive ? codeLineCount : 0,
    getScrollElement: () => codeViewportRef.current,
    estimateSize: () => 20,
    overscan: 28,
  });

  const virtualCodeItems = codeVirtualizer.getVirtualItems();

  // Lazily syntax-highlight unhighlighted visible 250-line chunks (e.g. lines beyond pre-highlighted prefix or Markdown Source)
  const firstVisibleIdx = virtualCodeItems[0]?.index ?? -1;
  const lastVisibleIdx = virtualCodeItems[virtualCodeItems.length - 1]?.index ?? -1;

  const enterEditPreservingLine = useCallback(
    (explicitLine?: number | null) => {
      let targetLine = explicitLine ?? null;
      if (targetLine == null) {
        const scrollEl = codeViewportRef.current;
        if (scrollEl && isCodeViewActive && codeLineCount > 0) {
          const topLine = Math.floor(scrollEl.scrollTop / 20) + 1;
          targetLine = Math.max(1, Math.min(topLine + 4, codeLineCount));
        } else if (firstVisibleIdx >= 0) {
          targetLine = firstVisibleIdx + 1;
        }
      }
      setEditEntryFocusLine(targetLine);
      setFindOpen(false);
      setPendingMarkdownTarget(null);
      orbitActions.enterFileEditMode(hostId, cwd, path);
    },
    [isCodeViewActive, codeLineCount, firstVisibleIdx, hostId, cwd, path],
  );
  useEffect(() => {
    if (!isCodeViewActive || !data || data.language === "text" || firstVisibleIdx < 0) return;
    const CHUNK_SIZE = 250;
    const startChunk = Math.floor(firstVisibleIdx / CHUNK_SIZE);
    const endChunk = Math.floor(Math.max(firstVisibleIdx, lastVisibleIdx) / CHUNK_SIZE);
    const missingChunks: number[] = [];

    for (let c = startChunk; c <= endChunk; c += 1) {
      if (lazyHighlightRequestedChunksRef.current.has(c)) continue;
      const chunkStart = c * CHUNK_SIZE;
      const chunkEnd = Math.min(codeLineCount, chunkStart + CHUNK_SIZE);
      // Skip chunks fully covered by Rust's eager highlighted prefix
      if (preHighlightedLineCount > 0 && chunkEnd <= preHighlightedLineCount) {
        lazyHighlightRequestedChunksRef.current.add(c);
        continue;
      }
      if (chunkStart < chunkEnd) {
        missingChunks.push(c);
      }
    }
    if (missingChunks.length === 0) return;

    const reqSeq = lazyHighlightSeqRef.current;
    const timer = window.setTimeout(() => {
      for (const c of missingChunks) {
        if (lazyHighlightRequestedChunksRef.current.has(c)) continue;
        lazyHighlightRequestedChunksRef.current.add(c);
        const chunkStart = c * CHUNK_SIZE;
        const chunkEnd = Math.min(codeLineCount, chunkStart + CHUNK_SIZE);
        const chunkText = rawCodeLines.slice(chunkStart, chunkEnd).join("\n");
        void orbitIpc
          .highlightWorkspaceCode(data.language, chunkText, CHUNK_SIZE)
          .then((highlighted) => {
            if (lazyHighlightSeqRef.current !== reqSeq) return;
            setLazyHighlights((prev) => {
              const next = { ...prev };
              for (let i = 0; i < highlighted.length; i += 1) {
                const lineNum = chunkStart + i + 1;
                next[lineNum] = {
                  line_number: lineNum,
                  tokens: highlighted[i].tokens,
                };
              }
              return next;
            });
          })
          .catch(() => {
            lazyHighlightRequestedChunksRef.current.delete(c);
          });
      }
    }, 60);

    return () => window.clearTimeout(timer);
  }, [
    isCodeViewActive,
    data,
    rawCodeLines,
    codeLineCount,
    preHighlightedLineCount,
    firstVisibleIdx,
    lastVisibleIdx,
  ]);

  // Compute lowercase search buffer strictly on demand when Find bar is actively searching
  const activeSearchQuery = findOpen ? findQuery.trim().toLowerCase() : "";
  const rawLowerLines = useMemo(() => {
    if (!activeSearchQuery || !data || data.content_kind !== "text") return [];
    if (isLogFile) return logLines.map((l) => l.plain.toLowerCase());
    return data.content.split(/\r?\n/).map((l) => l.toLowerCase());
  }, [activeSearchQuery, data, isLogFile, logLines]);

  const matchLineNumbers = useMemo(() => {
    if (!activeSearchQuery || rawLowerLines.length === 0) return [];
    const matched: number[] = [];
    for (let i = 0; i < rawLowerLines.length; i += 1) {
      if (rawLowerLines[i].includes(activeSearchQuery)) {
        matched.push(i + 1);
      }
    }
    return matched;
  }, [activeSearchQuery, rawLowerLines]);

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

  const logWindow = useMemo(
    () =>
      isLogFile
        ? computeLogWindowSlice(logLines.length, renderedLineCap, activeTargetLine)
        : { effectiveCap: renderedLineCap, startIdx: 0 },
    [isLogFile, logLines.length, renderedLineCap, activeTargetLine],
  );
  const logStartIdx = logWindow.startIdx;
  const visibleLogLines = useMemo(
    () => (isLogFile ? (logStartIdx > 0 ? logLines.slice(logStartIdx) : logLines) : []),
    [isLogFile, logLines, logStartIdx],
  );

  // Restore scroll position seamlessly after prepending earlier log lines above the viewport
  useLayoutEffect(() => {
    const snap = pendingLogPrependAnchorRef.current;
    const el = logViewportRef.current;
    if (!snap || !el) return;
    pendingLogPrependAnchorRef.current = null;
    el.scrollTop = computePrependRestoredScrollTop(
      snap.prevScrollTop,
      snap.prevScrollHeight,
      el.scrollHeight,
    );
  }, [logStartIdx, renderedLineCap]);

  const maybeLoadEarlierLogPage = useCallback(
    (el: HTMLDivElement) => {
      if (
        !shouldTriggerTopLogPagination(
          logStartIdx,
          el.scrollTop,
          pendingLogPrependAnchorRef.current !== null,
        )
      ) {
        return;
      }
      if (logFollow) {
        setLogFollow(false);
      }
      pendingLogPrependAnchorRef.current = {
        prevScrollHeight: el.scrollHeight,
        prevScrollTop: el.scrollTop,
      };
      setRenderedLineCap((c) => computeNextLogLineCap(logLines.length, c, LOG_PAGE_INCREMENT));
    },
    [logFollow, logLines.length, logStartIdx],
  );

  // Only log viewer uses `renderedLineCap`; code viewer is fully virtualized (O(1) jump to any line)
  useEffect(() => {
    if (isLogFile && requiredMaxLine && requiredMaxLine > renderedLineCap) {
      setRenderedLineCap(requiredMaxLine + 200);
    }
  }, [isLogFile, requiredMaxLine, renderedLineCap]);

  // Scroll to requested line or active find match (O(1) virtualizer jump for code, DOM scroll for logs)
  useEffect(() => {
    if (!activeTargetLine || isEditMode) return;
    const rafId = window.requestAnimationFrame(() => {
      if (isCodeViewActive && codeLineCount > 0) {
        const clampedIdx = Math.max(0, Math.min(activeTargetLine - 1, codeLineCount - 1));
        codeVirtualizer.scrollToIndex(clampedIdx, { align: "center" });
        return;
      }
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
    isCodeViewActive,
    codeLineCount,
    codeVirtualizer,
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

  const closeFindBar = useCallback(() => {
    setFindOpen(false);
    setFindQuery("");
    requestAnimationFrame(() => containerRef.current?.focus({ preventScroll: true }));
  }, []);

  const openFind = () => {
    setFindOpen(true);
    if (isMarkdown) setMarkdownMode("source");
    requestAnimationFrame(() => findInputRef.current?.focus());
  };

  const canFindInFile = !data || data.content_kind === "text";

  useEffect(() => {
    const onWindowKeyDown = (e: KeyboardEvent) => {
      const snap = orbitActions.getSnapshot();
      const action = resolveFileViewerShortcut(
        {
          activeView: snap.activeView,
          sidePanelOpen: snap.sidePanelOpen,
          activeSidePanelTabId: snap.activeSidePanelTabId,
          tabId: tab.id,
          terminalFocused: snap.terminalFocused,
          commandPaletteOpen: snap.commandPaletteOpen,
          quickOpenFilesOpen: snap.quickOpenFilesOpen,
          mediaPreviewOpen: snap.mediaPreviewTarget !== null,
          focusInsideSidePanel: isFocusInsideFileViewerPane(
            containerRef.current,
            e.target,
            document.activeElement,
          ),
          targetInsideModalDialog: isFocusInsideModalDialog(e.target, document.activeElement),
          canFindInFile,
          isEditable,
          isEditMode,
          findOpen,
        },
        e,
      );
      if (!action) return;

      if (action === "find") {
        e.preventDefault();
        setFindOpen(true);
        if (isMarkdown) setMarkdownMode("source");
        requestAnimationFrame(() => findInputRef.current?.focus());
      } else if (action === "toggle_edit") {
        e.preventDefault();
        if (isEditMode) {
          orbitActions.requestFileViewMode(hostId, cwd, path);
        } else {
          enterEditPreservingLine();
        }
      } else if (action === "save") {
        e.preventDefault();
        if (!orbitActions.getSnapshot().fileEditorDrafts[cKey]?.saving) {
          void orbitActions.saveWorkspaceFile(hostId, cwd, path, false);
        }
      } else if (action === "close_find") {
        e.preventDefault();
        closeFindBar();
      }
    };
    window.addEventListener("keydown", onWindowKeyDown);
    return () => window.removeEventListener("keydown", onWindowKeyDown);
  }, [
    findOpen,
    isMarkdown,
    canFindInFile,
    isEditable,
    isEditMode,
    cKey,
    hostId,
    cwd,
    path,
    tab.id,
    closeFindBar,
    enterEditPreservingLine,
  ]);

  const stepMatch = (delta: number) => {
    if (matchLineNumbers.length === 0) return;
    setActiveMatchIdx((prev) => (prev + delta + matchLineNumbers.length) % matchLineNumbers.length);
  };

  return (
    <div
      ref={containerRef}
      tabIndex={-1}
      className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden outline-none"
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
          {data && (isLogFile || isMarkdown || data.content_kind !== "text") && (
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
                  if (isCodeViewActive && codeLineCount > 0) {
                    const clampedIdx = Math.max(
                      0,
                      Math.min(targetLine - 1, codeLineCount - 1),
                    );
                    codeVirtualizer.scrollToIndex(clampedIdx, { align: "center" });
                    return;
                  }
                  const node = lineRefs.current.get(targetLine);
                  node?.scrollIntoView({ block: "center", behavior: "smooth" });
                });
              }}
              className="bg-signal-info/[0.12] text-signal-info hover:bg-signal-info/[0.18] tabular-nums shrink-0 cursor-pointer rounded px-1.5 py-0.5 font-mono text-[11px] font-medium transition-colors"
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
                <SimpleTooltip label="Edit file" shortcut={`${mod}E`}>
                  <button
                    type="button"
                    onClick={() => enterEditPreservingLine()}
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
                </SimpleTooltip>
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
                <SimpleTooltip label="Edit file" shortcut={`${mod}E`}>
                  <button
                    type="button"
                    onClick={() => enterEditPreservingLine()}
                    className={cn(
                      "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                      isEditMode
                        ? "bg-background text-foreground shadow-2xs"
                        : "text-muted-foreground hover:text-foreground",
                    )}
                  >
                    Edit
                  </button>
                </SimpleTooltip>
              </div>
            )
          )}

          {isEditMode && (
            <SimpleTooltip label="Save file" shortcut={`${mod}S`}>
              <button
                type="button"
                disabled={draft?.saving || !isDirty}
                onClick={() => void orbitActions.saveWorkspaceFile(hostId, cwd, path, false)}
                className={cn(
                  "inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors disabled:cursor-default disabled:opacity-45",
                  isDirty
                    ? "bg-action-primary-bg text-action-primary-fg hover:bg-action-primary-hover"
                    : "text-muted-foreground hover:bg-accent hover:text-foreground",
                )}
              >
                {draft?.saving ? (
                  <OrbitSpinner size={12} />
                ) : (
                  <Save className="h-[12px] w-[12px]" />
                )}
                Save
              </button>
            </SimpleTooltip>
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
                <SimpleTooltip label="Find in file" shortcut={`${mod}F`}>
                  <button
                    type="button"
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
                </SimpleTooltip>
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
            onClick={closeFindBar}
            className="text-muted-foreground hover:text-foreground inline-flex h-[22px] w-[22px] cursor-pointer items-center justify-center rounded"
          >
            <X className="h-[13px] w-[13px]" />
          </button>
        </div>
      )}

      {/* Main Content Surface */}
      {isInitialLoading ? (
        showLoadingIndicator ? (
          <div className="orbit-stage-enter text-muted-foreground flex flex-1 items-center justify-center gap-2 text-[12.5px]">
            <OrbitSpinner size={14} />
            <span>Reading {relPath}…</span>
          </div>
        ) : (
          <div className="flex flex-1" />
        )
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
        <Suspense
          fallback={
            <div className="bg-surface-base text-ink-tertiary flex min-h-0 flex-1 items-center justify-center gap-2 text-[12px]">
              <OrbitSpinner size={14} />
            </div>
          }
        >
          <LazyWorkspaceCodeEditor
            key={cKey}
            hostId={hostId}
            cwd={cwd}
            filePath={path}
            relPath={relPath}
            value={draft.content}
            language={data.language}
            lineEnding={draft.lineEnding}
            isDark={isDark}
            dirty={draft.dirty}
            saving={draft.saving}
            saveError={draft.saveError}
            conflict={draft.conflict}
            isLspEligible={isLspEligible}
            initialFocusLine={editEntryFocusLine ?? focusRange.startLine}
            diagnostics={diagnostics}
            jumpTarget={editorJumpTarget}
            onGotoDefinition={isLspEligible ? handleGotoDefinition : undefined}
            onJumpToFileLocation={handleJumpToFileLocation}
            onAfterLspWorkspaceEdit={handleAfterLspWorkspaceEdit}
            onChange={(nextValue) =>
              orbitActions.updateFileEditorDraft(hostId, cwd, path, nextValue)
            }
            onSave={(force) =>
              orbitActions.saveWorkspaceFile(hostId, cwd, path, Boolean(force))
            }
            onReloadFromConflict={() => {
              void orbitActions.reloadWorkspaceFileFromConflict(hostId, cwd, path);
            }}
            onDismissError={() => orbitActions.clearFileEditorError(hostId, cwd, path)}
            onCopyMine={() => {
              void navigator.clipboard.writeText(draft.content);
              orbitActions.showToast("Copied editor buffer to clipboard");
            }}
          />
        </Suspense>
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
            onWheel={(e) => {
              if (e.deltaY < 0 && logViewportRef.current) {
                maybeLoadEarlierLogPage(logViewportRef.current);
              }
            }}
            onViewportScroll={(e) => {
              const el = e.currentTarget;
              const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
              if (distanceFromBottom > 48 && logFollow) {
                setLogFollow(false);
              } else if (distanceFromBottom <= 20 && !logFollow && isTaskLogPath) {
                setLogFollow(true);
              }
              maybeLoadEarlierLogPage(el);
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
                        "bg-signal-info/[0.14] shadow-[inset_2px_0_0_0_var(--signal-info)]",
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
        <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
          <ScrollArea
            orientation="both"
            viewportRef={codeViewportRef}
            className="bg-canvas min-h-0 flex-1"
            viewportClassName="selectable-text py-2.5 font-mono text-[12px] leading-[20px]"
          >
            {data.truncated && (
              <div className="border-border text-muted-foreground mx-3 mb-2 rounded-lg border bg-amber-500/[0.06] px-3 py-1.5 font-sans text-[11.5px]">
                Showing initial preview — file exceeds transfer cap ({formatFileSizeCompact(data.size_bytes)}).
              </div>
            )}

            <div
              style={{ height: `${codeVirtualizer.getTotalSize()}px` }}
              onClick={(e) => {
                if (
                  !isLspEligible ||
                  !(e.metaKey || e.ctrlKey) ||
                  e.shiftKey ||
                  e.altKey
                ) {
                  return;
                }
                const targetEl = (e.target as HTMLElement | null)?.closest<HTMLElement>(
                  "[data-tok-col]",
                );
                if (!targetEl) return;
                const lineNum = Number(targetEl.dataset.tokLine);
                const tokStartCol = Number(targetEl.dataset.tokCol);
                const tokText = targetEl.textContent ?? "";
                if (!Number.isFinite(lineNum) || !Number.isFinite(tokStartCol) || !tokText.trim()) {
                  return;
                }
                e.preventDefault();
                e.stopPropagation();
                let offsetInTok = Math.floor(tokText.length / 2);
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
                  targetEl.contains(caretPos.offsetNode) &&
                  Number.isFinite(caretPos.offset)
                ) {
                  offsetInTok = Math.max(
                    0,
                    Math.min(caretPos.offset, Math.max(0, tokText.length - 1)),
                  );
                } else {
                  const caretRange = doc.caretRangeFromPoint?.(e.clientX, e.clientY);
                  if (
                    caretRange &&
                    targetEl.contains(caretRange.startContainer) &&
                    Number.isFinite(caretRange.startOffset)
                  ) {
                    offsetInTok = Math.max(
                      0,
                      Math.min(caretRange.startOffset, Math.max(0, tokText.length - 1)),
                    );
                  }
                }
                handleGotoDefinition(lineNum, Math.max(1, tokStartCol + offsetInTok));
              }}
              className="relative min-w-full"
            >
              {virtualCodeItems.map((vRow) => {
                const rawLineText = rawCodeLines[vRow.index];
                if (rawLineText === undefined) return null;
                const lineNumber = vRow.index + 1;
                const preHighlighted =
                  vRow.index < data.highlighted_lines.length &&
                  data.highlighted_lines[vRow.index]?.line_number === lineNumber
                    ? data.highlighted_lines[vRow.index]
                    : undefined;
                const highlightedLine = lazyHighlights[lineNumber] ?? preHighlighted;
                const isMatched = matchLineSet.has(lineNumber);
                const isRangeFocused =
                  matchLineNumbers.length === 0 &&
                  focusRange.startLine != null &&
                  lineNumber >= focusRange.startLine &&
                  lineNumber <= (focusRange.endLine ?? focusRange.startLine);
                const isFocused =
                  (matchLineNumbers.length > 0 &&
                    matchLineNumbers[Math.min(activeMatchIdx, matchLineNumbers.length - 1)] ===
                      lineNumber) ||
                  isRangeFocused;
                const lineDiag = diagByLine.get(lineNumber);
                let runningCol = 1;

                return (
                  <div
                    key={lineNumber}
                    style={{
                      position: "absolute",
                      top: 0,
                      left: 0,
                      width: "100%",
                      height: `${vRow.size}px`,
                      transform: `translate3d(0, ${vRow.start}px, 0)`,
                    }}
                    onDoubleClick={() => {
                      if (isEditable) {
                        enterEditPreservingLine(lineNumber);
                      }
                    }}
                    className={cn(
                      "group/fln flex h-[20px] min-w-fit items-center px-2 transition-colors hover:bg-black/[0.03] dark:hover:bg-white/[0.035]",
                      isFocused &&
                        "bg-signal-info/[0.12] shadow-[inset_2px_0_0_0_var(--signal-info)]",
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
                      <span>{lineNumber}</span>
                    </span>
                    <button
                      type="button"
                      title={t("sidePanel.quoteLineTooltip")}
                      onClick={() => {
                        orbitActions.quoteDiffLineToComposer(relPath, lineNumber, rawLineText);
                      }}
                      className="text-muted-foreground hover:text-foreground mr-1.5 inline-flex h-[16px] w-[16px] shrink-0 cursor-pointer items-center justify-center rounded opacity-0 transition-opacity group-hover/fln:opacity-100 hover:bg-black/[0.08] dark:hover:bg-white/[0.12]"
                    >
                      <MessageSquarePlus className="h-[11.5px] w-[11.5px]" />
                    </button>
                    <span className="pr-4 whitespace-pre">
                      {highlightedLine ? (
                        highlightedLine.tokens.length === 0 ? (
                          " "
                        ) : (
                          highlightedLine.tokens.map((tok, idx) => {
                            const tokStartCol = runningCol;
                            runningCol += tok.text.length;
                            return (
                              <span
                                key={idx}
                                data-tok-line={isLspEligible ? lineNumber : undefined}
                                data-tok-col={isLspEligible ? tokStartCol : undefined}
                                style={{
                                  color: resolveTokenColor(tok, isDark),
                                  fontWeight: tok.bold ? 600 : undefined,
                                  fontStyle: tok.italic ? "italic" : undefined,
                                }}
                              >
                                {tok.text}
                              </span>
                            );
                          })
                        )
                      ) : rawLineText.length === 0 ? (
                        " "
                      ) : (
                        <span
                          data-tok-line={isLspEligible ? lineNumber : undefined}
                          data-tok-col={isLspEligible ? 1 : undefined}
                          style={{
                            color: isDark ? DARK_PLAIN_TOKEN_FG : LIGHT_PLAIN_TOKEN_FG,
                          }}
                        >
                          {rawLineText}
                        </span>
                      )}
                    </span>
                  </div>
                );
              })}
            </div>
          </ScrollArea>

          {/* Subtle bottom metadata strip (keeps top 36px breadcrumb bar airy & restrained) */}
          <div className="border-border/60 text-muted-foreground/70 tabular-nums flex h-[22px] shrink-0 items-center justify-between gap-2 border-t px-3 font-mono text-[10.5px] select-none">
            <span>{t("common.linesCount", { count: codeLineCount.toLocaleString() })}</span>
            <div className="flex items-center gap-2.5">
              <span>{data.language.toUpperCase()}</span>
              <span>{formatFileSizeCompact(data.size_bytes)}</span>
            </div>
          </div>
        </div>
      )}
    </div>
  );
});
