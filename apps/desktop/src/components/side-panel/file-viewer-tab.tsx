import { memo, useEffect, useLayoutEffect, useMemo, useRef, useState } from "react";
import {
  ArrowDown,
  Check,
  ChevronDown,
  ChevronUp,
  Code2,
  Copy,
  Eye,
  FileWarning,
  GitCompareArrows,
  Loader2,
  MessageSquarePlus,
  RefreshCw,
  Search,
  WrapText,
  X,
} from "lucide-react";
import {
  MarkdownBlocksView,
  MarkdownLinkProvider,
} from "@/components/markdown-blocks";
import { formatLineRangeLabel, normalizeLineRange } from "@/lib/file-links";
import { useImeCompositionGuard } from "@/lib/ime";
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
  const gitStatusFiles = useOrbitSelector((s) =>
    hostId && cwd ? (s.gitReviews[`${hostId}::${cwd}`]?.status?.files ?? null) : null,
  );

  const isTaskLogPath = useMemo(
    () =>
      /\/\.system_generated\/tasks\/[^/]+$/i.test(path) ||
      /\/claude\/[^/]*\/tasks\/[^/]+$/i.test(path),
    [path],
  );
  const isLogFile = useMemo(
    () => isTaskLogPath || /\.(log|out|output)$/i.test(path),
    [isTaskLogPath, path],
  );

  const [markdownMode, setMarkdownMode] = useState<"preview" | "source">("preview");
  const [logWrapLines, setLogWrapLines] = useState(true);
  const [logFollow, setLogFollow] = useState(isTaskLogPath);
  const [copied, setCopied] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [activeMatchIdx, setActiveMatchIdx] = useState(0);
  const [renderedLineCap, setRenderedLineCap] = useState(800);

  const findInputRef = useRef<HTMLInputElement | null>(null);
  const logViewportRef = useRef<HTMLDivElement | null>(null);
  const lineRefs = useRef<Map<number, HTMLDivElement>>(new Map());
  const ime = useImeCompositionGuard();

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
  useEffect(() => {
    if (!isLogFile || !logFollow || !hostId || !path) return;
    if (cacheEntry?.status === "error") return;
    const id = window.setInterval(() => {
      const snap = orbitActions.getSnapshot();
      if (!snap.sidePanelOpen || snap.activeSidePanelTabId !== tab.id) return;
      if (typeof document !== "undefined" && document.hidden) return;
      void orbitActions.loadWorkspaceFile(hostId, cwd, path, true);
    }, 2000);
    return () => window.clearInterval(id);
  }, [isLogFile, logFollow, hostId, cwd, path, tab.id, cacheEntry?.status]);

  const data = cacheEntry?.data ?? null;
  const relPath = useMemo(() => stripWorkspacePrefix(path, cwd), [path, cwd]);
  const displayHeaderPath = useMemo(() => {
    if (!isTaskLogPath) return relPath;
    const normalized = path.replace(/\\/g, "/");
    const base = normalized.slice(normalized.lastIndexOf("/") + 1);
    return base || relPath;
  }, [isTaskLogPath, path, relPath]);
  const isMarkdown = data?.language === "markdown" && (data?.markdown_blocks?.length ?? 0) > 0;
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
    if (!activeTargetLine) return;
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
  ]);

  const handleCopyContent = async () => {
    if (!data?.content) return;
    try {
      const textToCopy = isLogFile
        ? logLines.map((l) => l.plain).join("\n")
        : data.content;
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

  useEffect(() => {
    const onWindowKeyDown = (e: KeyboardEvent) => {
      const snap = orbitActions.getSnapshot();
      if (
        !snap.sidePanelOpen ||
        snap.terminalFocused ||
        snap.commandPaletteOpen ||
        snap.quickOpenFilesOpen
      ) {
        return;
      }
      if ((e.metaKey || e.ctrlKey) && !e.shiftKey && !e.altKey && e.key.toLowerCase() === "f") {
        e.preventDefault();
        setFindOpen(true);
        if (isMarkdown) setMarkdownMode("source");
        requestAnimationFrame(() => findInputRef.current?.focus());
      } else if (e.key === "Escape" && findOpen) {
        e.preventDefault();
        setFindOpen(false);
        setFindQuery("");
      }
    };
    window.addEventListener("keydown", onWindowKeyDown);
    return () => window.removeEventListener("keydown", onWindowKeyDown);
  }, [findOpen, isMarkdown]);

  const stepMatch = (delta: number) => {
    if (matchLineNumbers.length === 0) return;
    setActiveMatchIdx((prev) => (prev + delta + matchLineNumbers.length) % matchLineNumbers.length);
  };

  return (
    <div
      tabIndex={-1}
      className="orbit-stage-enter flex min-h-0 flex-1 flex-col overflow-hidden outline-none"
      onKeyDown={(e) => {
        if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "f") {
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
          {data && (
            <span className="text-muted-foreground/75 tabular-nums shrink-0 text-[11px]">
              {isLogFile
                ? `${logLines.length.toLocaleString()} lines · ${formatFileSizeCompact(data.size_bytes)}`
                : formatFileSizeCompact(data.size_bytes)}
            </span>
          )}
          {focusLabel && focusRange.startLine != null && (
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
          {isMarkdown && (
            <div className="bg-muted inline-flex h-[24px] items-center rounded-md p-0.5 text-[11px]">
              <button
                type="button"
                onClick={() => setMarkdownMode("preview")}
                className={cn(
                  "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                  markdownMode === "preview"
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Eye className="h-[11.5px] w-[11.5px]" />
                Preview
              </button>
              <button
                type="button"
                onClick={() => setMarkdownMode("source")}
                className={cn(
                  "inline-flex h-[20px] cursor-pointer items-center gap-1 rounded px-1.5 font-medium transition-colors",
                  markdownMode === "source"
                    ? "bg-background text-foreground shadow-2xs"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                <Code2 className="h-[11.5px] w-[11.5px]" />
                Source
              </button>
            </div>
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
              <button
                type="button"
                title="Find in file (⌘F)"
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
            onClick={() => void orbitActions.loadWorkspaceFile(hostId, cwd, path, true)}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-[24px] w-[24px] cursor-pointer items-center justify-center rounded-md transition-colors"
          >
            <RefreshCw
              className={cn("h-[13px] w-[13px]", cacheEntry?.status === "loading" && "animate-spin")}
            />
          </button>
        </div>
      </div>

      {/* Collapsible Find-in-File Strip */}
      {findOpen && (
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
      ) : data.content_kind === "binary" ? (
        <div className="text-muted-foreground flex flex-1 flex-col items-center justify-center gap-2 px-6 text-center">
          <FileWarning className="h-5 w-5 opacity-60" />
          <div className="text-foreground/85 text-[13px] font-medium">Binary file</div>
          <div className="text-[12px]">
            {relPath} ({formatFileSizeCompact(data.size_bytes)}) cannot be displayed as text.
          </div>
        </div>
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
              {logLines.map((line, idx) => {
                const lineNumber = idx + 1;
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
                      isFocused && "bg-primary/[0.14] shadow-[inset_2px_0_0_0_var(--color-primary)]",
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

            return (
              <div
                key={line.line_number}
                ref={(el) => {
                  if (el) lineRefs.current.set(line.line_number, el);
                  else lineRefs.current.delete(line.line_number);
                }}
                className={cn(
                  "group/fln flex min-w-fit items-start px-2 transition-colors hover:bg-black/[0.03] dark:hover:bg-white/[0.035]",
                  isFocused && "bg-primary/[0.12] shadow-[inset_2px_0_0_0_var(--color-primary)]",
                  !isFocused && isMatched && "bg-amber-500/[0.12]",
                )}
              >
                <span className="tabular-nums min-w-[2.75rem] shrink-0 pr-2 text-right text-[11px] text-zinc-400 select-none dark:text-zinc-600">
                  {line.line_number}
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
                    : line.tokens.map((tok, idx) => (
                        <span
                          key={idx}
                          style={{
                            color: adaptTokenColor(tok.fg_hex, isDark),
                            fontWeight: tok.bold ? 600 : undefined,
                            fontStyle: tok.italic ? "italic" : undefined,
                          }}
                        >
                          {tok.text}
                        </span>
                      ))}
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
