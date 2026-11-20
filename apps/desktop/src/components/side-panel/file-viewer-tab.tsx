import { memo, useEffect, useMemo, useRef, useState } from "react";
import {
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
  X,
} from "lucide-react";
import {
  MarkdownBlocksView,
  MarkdownLinkProvider,
} from "@/components/markdown-blocks";
import { formatLineRangeLabel, normalizeLineRange } from "@/lib/file-links";
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

  const [markdownMode, setMarkdownMode] = useState<"preview" | "source">("preview");
  const [copied, setCopied] = useState(false);
  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [activeMatchIdx, setActiveMatchIdx] = useState(0);
  const [renderedLineCap, setRenderedLineCap] = useState(800);

  const findInputRef = useRef<HTMLInputElement | null>(null);
  const lineRefs = useRef<Map<number, HTMLDivElement>>(new Map());

  useEffect(() => {
    setRenderedLineCap(800);
  }, [hostId, cwd, path]);

  useEffect(() => {
    if (hostId && path && !cacheEntry) {
      void orbitActions.loadWorkspaceFile(hostId, cwd, path, false);
    }
  }, [hostId, cwd, path, cacheEntry]);

  const data = cacheEntry?.data ?? null;
  const relPath = useMemo(() => stripWorkspacePrefix(path, cwd), [path, cwd]);
  const isMarkdown = data?.language === "markdown" && (data?.markdown_blocks?.length ?? 0) > 0;

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
    return data.content.split(/\r?\n/).map((l) => l.toLowerCase());
  }, [data]);

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
      await navigator.clipboard.writeText(data.content);
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
      if (snap.terminalFocused || snap.commandPaletteOpen || snap.quickOpenFilesOpen) return;
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
          <span
            title={data?.resolved_path || path}
            className="selectable-text text-foreground/85 truncate font-mono text-[12px]"
          >
            {relPath}
          </span>
          {data && (
            <span className="text-muted-foreground tabular-nums shrink-0 text-[11px]">
              {formatFileSizeCompact(data.size_bytes)}
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
            onKeyDown={(e) => {
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
