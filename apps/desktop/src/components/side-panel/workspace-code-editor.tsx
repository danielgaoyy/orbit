import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import { AlertTriangle, Check, Copy, Loader2, RefreshCw, Save, X } from "lucide-react";
import {
  type HighlightedLineView,
  type WorkspaceFileConflict,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { adaptTokenColor } from "@/lib/code-highlight";
import { useImeCompositionGuard } from "@/lib/ime";
import { modSymbol } from "@/lib/keybindings";
import { formatFileSizeCompact } from "@/lib/session-insights";
import { cn } from "@/lib/utils";

const LINE_HEIGHT_PX = 20;
const VIRTUAL_OVERSCAN_LINES = 30;
const SMALL_FILE_HIGHLIGHT_THRESHOLD_BYTES = 50 * 1024;
const MAX_LIVE_HIGHLIGHT_BYTES = 512 * 1024;

const LANGUAGE_LABELS: Record<string, string> = {
  rs: "Rust",
  ts: "TypeScript",
  tsx: "TSX",
  js: "JavaScript",
  jsx: "JSX",
  json: "JSON",
  toml: "TOML",
  yaml: "YAML",
  markdown: "Markdown",
  py: "Python",
  go: "Go",
  swift: "Swift",
  kotlin: "Kotlin",
  java: "Java",
  c: "C",
  cpp: "C++",
  sh: "Shell",
  css: "CSS",
  html: "HTML",
  sql: "SQL",
  dockerfile: "Dockerfile",
  makefile: "Makefile",
  text: "Plain Text",
};

function formatLanguageLabel(language: string): string {
  const key = language.trim().toLowerCase();
  return LANGUAGE_LABELS[key] ?? (key ? key.toUpperCase() : "Plain Text");
}

function formatLineEndingLabel(lineEnding?: string | null): "LF" | "CRLF" | "Mixed" {
  if (lineEnding === "crlf") return "CRLF";
  if (lineEnding === "mixed") return "Mixed";
  return "LF";
}

function computeLineCol(text: string, offset: number): { line: number; col: number } {
  const clamped = Math.max(0, Math.min(offset, text.length));
  let line = 1;
  let lastNewline = -1;
  for (let i = 0; i < clamped; i += 1) {
    if (text.charCodeAt(i) === 10) {
      line += 1;
      lastNewline = i;
    }
  }
  return { line, col: clamped - lastNewline };
}

export interface WorkspaceCodeEditorProps {
  value: string;
  language: string;
  lineEnding?: "lf" | "crlf" | "mixed" | string | null;
  initialHighlightedLines: HighlightedLineView[];
  initialContent: string;
  isDark: boolean;
  dirty: boolean;
  saving: boolean;
  saveError: string | null;
  conflict: WorkspaceFileConflict | null;
  onChange: (nextValue: string) => void;
  onSave: (force?: boolean) => void;
  onReloadFromConflict: () => void;
  onDismissError: () => void;
  onCopyMine: () => void;
  initialFocusLine?: number | null;
}

export const WorkspaceCodeEditor = memo(function WorkspaceCodeEditor({
  value,
  language,
  lineEnding,
  initialHighlightedLines,
  initialContent,
  isDark,
  dirty,
  saving,
  saveError,
  conflict,
  onChange,
  onSave,
  onReloadFromConflict,
  onDismissError,
  onCopyMine,
  initialFocusLine,
}: WorkspaceCodeEditorProps) {
  const viewportContainerRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);
  const backdropInnerRef = useRef<HTMLDivElement | null>(null);
  const gutterInnerRef = useRef<HTMLDivElement | null>(null);
  const highlightSeqRef = useRef(0);
  const scrollRafRef = useRef<number | null>(null);
  const copiedMineTimerRef = useRef<number | null>(null);
  const ime = useImeCompositionGuard();
  const mod = modSymbol();

  const [cursor, setCursor] = useState<{ line: number; col: number }>({
    line: initialFocusLine && initialFocusLine > 0 ? initialFocusLine : 1,
    col: 1,
  });
  const [copiedMine, setCopiedMine] = useState(false);
  const [viewport, setViewport] = useState<{ scrollTop: number; height: number }>({
    scrollTop: 0,
    height: 640,
  });

  // Cached syntax-highlighted lines from Orbit's pure-Rust `syntect` engine
  const [highlightState, setHighlightState] = useState<{
    lines: HighlightedLineView[];
    plainByLine: string[];
  }>(() => ({
    lines: initialHighlightedLines,
    plainByLine: initialContent.split("\n").map((l) => l.replace(/\r$/, "")),
  }));

  // Reset highlight cache & invalidate any in-flight highlight requests when switching files or reloading
  useEffect(() => {
    highlightSeqRef.current += 1;
    setHighlightState({
      lines: initialHighlightedLines,
      plainByLine: initialContent.split("\n").map((l) => l.replace(/\r$/, "")),
    });
  }, [initialHighlightedLines, initialContent]);

  useEffect(() => {
    return () => {
      if (copiedMineTimerRef.current !== null) {
        window.clearTimeout(copiedMineTimerRef.current);
      }
      if (scrollRafRef.current !== null) {
        window.cancelAnimationFrame(scrollRafRef.current);
      }
    };
  }, []);

  // Measure container height so virtualization window adapts seamlessly on side-panel resize
  useEffect(() => {
    const el = viewportContainerRef.current;
    if (!el) return;
    const updateHeight = () => {
      const h = Math.max(200, Math.round(el.clientHeight));
      setViewport((prev) => (prev.height === h ? prev : { ...prev, height: h }));
    };
    updateHeight();
    if (typeof ResizeObserver === "undefined") return;
    const ro = new ResizeObserver(updateHeight);
    ro.observe(el);
    return () => ro.disconnect();
  }, []);

  const rawLines = useMemo(
    () => value.split("\n").map((l) => l.replace(/\r$/, "")),
    [value],
  );
  const lineCount = Math.max(1, rawLines.length);

  const syncScroll = useCallback(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    const top = ta.scrollTop;
    const left = ta.scrollLeft;
    if (backdropInnerRef.current) {
      backdropInnerRef.current.style.transform = `translate3d(${-left}px, ${-top}px, 0)`;
    }
    if (gutterInnerRef.current) {
      gutterInnerRef.current.style.transform = `translate3d(0, ${-top}px, 0)`;
    }
    if (scrollRafRef.current === null) {
      scrollRafRef.current = window.requestAnimationFrame(() => {
        scrollRafRef.current = null;
        const latestTop = textareaRef.current?.scrollTop ?? top;
        setViewport((prev) =>
          Math.abs(prev.scrollTop - latestTop) < LINE_HEIGHT_PX
            ? prev
            : { ...prev, scrollTop: latestTop },
        );
      });
    }
  }, []);

  // Focus editor and jump to focused line on entry
  useEffect(() => {
    const ta = textareaRef.current;
    if (!ta) return;
    ta.focus();
    if (initialFocusLine && initialFocusLine > 1) {
      const targetIdx = Math.min(initialFocusLine - 1, rawLines.length - 1);
      let offset = 0;
      const splitRaw = value.split("\n");
      for (let i = 0; i < targetIdx && i < splitRaw.length; i += 1) {
        offset += splitRaw[i].length + 1;
      }
      ta.setSelectionRange(offset, offset);
      const scrollY = Math.max(0, (targetIdx - 4) * LINE_HEIGHT_PX);
      ta.scrollTop = scrollY;
      setViewport((prev) => ({ ...prev, scrollTop: scrollY }));
      syncScroll();
      setCursor({ line: targetIdx + 1, col: 1 });
    }
    // Only run on initial mount of editor surface
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Adaptive debounced Rust `syntect` re-highlighting while typing
  useEffect(() => {
    const seq = ++highlightSeqRef.current;
    if (value === initialContent && initialHighlightedLines.length > 0) {
      return;
    }
    if (value.length > MAX_LIVE_HIGHLIGHT_BYTES) {
      return;
    }
    const debounceMs = value.length < SMALL_FILE_HIGHLIGHT_THRESHOLD_BYTES ? 90 : 300;
    const timer = window.setTimeout(() => {
      const cap = Math.max(2500, rawLines.length + 100);
      void orbitIpc
        .highlightWorkspaceCode(language, value, cap)
        .then((lines) => {
          if (highlightSeqRef.current !== seq) return;
          setHighlightState({
            lines,
            plainByLine: value.split("\n").map((l) => l.replace(/\r$/, "")),
          });
        })
        .catch(() => {
          // Plain foreground fallback remains active on transient highlight failure
        });
    }, debounceMs);
    return () => window.clearTimeout(timer);
  }, [value, language, initialContent, initialHighlightedLines.length, rawLines.length]);

  const refreshCursor = useCallback(
    (ta: HTMLTextAreaElement | null, textOverride?: string) => {
      if (!ta) return;
      const src = textOverride ?? ta.value;
      setCursor(computeLineCol(src, ta.selectionStart ?? 0));
    },
    [],
  );

  const applyControlledEdit = useCallback(
    (nextValue: string, selStart: number, selEnd: number) => {
      onChange(nextValue);
      window.requestAnimationFrame(() => {
        const ta = textareaRef.current;
        if (!ta) return;
        ta.setSelectionRange(selStart, selEnd);
        refreshCursor(ta, nextValue);
        syncScroll();
      });
    },
    [onChange, refreshCursor, syncScroll],
  );

  const handleKeyDown = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (ime.shouldIgnoreKeyDown(e)) return;

    // Cmd+S / Ctrl+S atomic save
    if ((e.metaKey || e.ctrlKey) && !e.altKey && e.key.toLowerCase() === "s") {
      e.preventDefault();
      e.stopPropagation();
      if (!saving) {
        onSave(false);
      }
      return;
    }

    const ta = e.currentTarget;
    const start = ta.selectionStart ?? 0;
    const end = ta.selectionEnd ?? start;

    // Tab / Shift+Tab 2-space indentation
    if (e.key === "Tab" && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      if (start === end && !e.shiftKey) {
        const next = `${value.slice(0, start)}  ${value.slice(end)}`;
        applyControlledEdit(next, start + 2, start + 2);
        return;
      }

      const blockStart = value.lastIndexOf("\n", Math.max(0, start - 1)) + 1;
      const endCheckIdx = end > start && value[end - 1] === "\n" ? end - 1 : end;
      const nextNl = value.indexOf("\n", endCheckIdx);
      const blockEnd = nextNl === -1 ? value.length : nextNl;
      const block = value.slice(blockStart, blockEnd);
      const lines = block.split("\n");

      if (e.shiftKey) {
        let firstRemoved = 0;
        let totalRemoved = 0;
        const updated = lines.map((line, idx) => {
          let removed = 0;
          if (line.startsWith("  ")) removed = 2;
          else if (line.startsWith(" ") || line.startsWith("\t")) removed = 1;
          if (idx === 0) firstRemoved = removed;
          totalRemoved += removed;
          return line.slice(removed);
        });
        const next = `${value.slice(0, blockStart)}${updated.join("\n")}${value.slice(blockEnd)}`;
        const nextStart = Math.max(blockStart, start - firstRemoved);
        const nextEnd = Math.max(nextStart, end - totalRemoved);
        applyControlledEdit(next, nextStart, nextEnd);
      } else {
        const updated = lines.map((line) => `  ${line}`);
        const next = `${value.slice(0, blockStart)}${updated.join("\n")}${value.slice(blockEnd)}`;
        const nextStart = start + 2;
        const nextEnd = end + lines.length * 2;
        applyControlledEdit(next, nextStart, nextEnd);
      }
      return;
    }

    // Auto-indent on Enter (always insert `\n` inside normalized textarea; save restores CRLF)
    if (e.key === "Enter" && !e.metaKey && !e.ctrlKey && !e.altKey) {
      e.preventDefault();
      const lineStart = value.lastIndexOf("\n", Math.max(0, start - 1)) + 1;
      const lineBeforeCursor = value.slice(lineStart, start);
      const indentMatch = lineBeforeCursor.match(/^[ \t]*/);
      let indent = indentMatch ? indentMatch[0] : "";
      const trimmedBefore = lineBeforeCursor.trimEnd();
      if (
        trimmedBefore.endsWith("{") ||
        trimmedBefore.endsWith("[") ||
        trimmedBefore.endsWith("(") ||
        trimmedBefore.endsWith(":")
      ) {
        indent += "  ";
      }
      const insert = `\n${indent}`;
      const next = `${value.slice(0, start)}${insert}${value.slice(end)}`;
      const caret = start + insert.length;
      applyControlledEdit(next, caret, caret);
    }
  };

  // Virtualization window for large files (exact 20px integer row height)
  const visibleStart = Math.max(
    0,
    Math.floor(viewport.scrollTop / LINE_HEIGHT_PX) - VIRTUAL_OVERSCAN_LINES,
  );
  const visibleCount = Math.ceil(viewport.height / LINE_HEIGHT_PX) + VIRTUAL_OVERSCAN_LINES * 2;
  const visibleEnd = Math.min(rawLines.length, visibleStart + visibleCount);
  const topSpacerPx = visibleStart * LINE_HEIGHT_PX;
  const bottomSpacerPx = Math.max(0, (rawLines.length - visibleEnd) * LINE_HEIGHT_PX);
  const visibleSlice = rawLines.slice(visibleStart, visibleEnd);

  const lineEndingLabel = useMemo(() => formatLineEndingLabel(lineEnding), [lineEnding]);
  const languageLabel = useMemo(() => formatLanguageLabel(language), [language]);
  const gutterMinWidthCh = Math.max(3, String(lineCount).length + 1);

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-hidden">
      {/* External Disk Conflict Banner */}
      {conflict && (
        <div className="border-b border-amber-500/25 bg-amber-500/[0.08] px-3 py-2 text-[12px]">
          <div className="flex flex-wrap items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <AlertTriangle className="h-[14px] w-[14px] shrink-0 text-amber-600 dark:text-amber-400" />
              <span className="text-foreground font-medium">File changed on disk</span>
              <span className="text-muted-foreground tabular-nums text-[11px]">
                ({formatFileSizeCompact(conflict.current_size_bytes)})
              </span>
            </div>
            <div className="flex shrink-0 items-center gap-1.5">
              <button
                type="button"
                disabled={saving}
                onClick={onReloadFromConflict}
                className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors disabled:opacity-50"
              >
                <RefreshCw className="h-[11.5px] w-[11.5px]" />
                Reload
              </button>
              <button
                type="button"
                disabled={saving}
                onClick={() => onSave(true)}
                className="inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md border border-amber-500/35 bg-amber-500/15 px-2 text-[11.5px] font-medium text-amber-800 transition-colors hover:bg-amber-500/25 disabled:opacity-50 dark:text-amber-200"
              >
                <Save className="h-[11.5px] w-[11.5px]" />
                Overwrite
              </button>
              <button
                type="button"
                onClick={() => {
                  onCopyMine();
                  setCopiedMine(true);
                  if (copiedMineTimerRef.current !== null) {
                    window.clearTimeout(copiedMineTimerRef.current);
                  }
                  copiedMineTimerRef.current = window.setTimeout(() => {
                    copiedMineTimerRef.current = null;
                    setCopiedMine(false);
                  }, 1400);
                }}
                className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[24px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] transition-colors"
              >
                {copiedMine ? (
                  <Check className="h-[11.5px] w-[11.5px] text-emerald-500" />
                ) : (
                  <Copy className="h-[11.5px] w-[11.5px]" />
                )}
                Copy mine
              </button>
            </div>
          </div>
        </div>
      )}

      {/* Save Error Banner */}
      {saveError && (
        <div className="flex items-center justify-between gap-2 border-b border-rose-500/25 bg-rose-500/[0.08] px-3 py-1.5 text-[11.5px] text-rose-700 dark:text-rose-300">
          <span className="min-w-0 truncate">{saveError}</span>
          <button
            type="button"
            onClick={onDismissError}
            aria-label="Dismiss error"
            className="inline-flex h-[20px] w-[20px] shrink-0 cursor-pointer items-center justify-center rounded hover:bg-rose-500/15"
          >
            <X className="h-[12px] w-[12px]" />
          </button>
        </div>
      )}

      {/* Main Editor Surface: Synchronized Line Gutter + Highlight Backdrop + Transparent Textarea */}
      <div
        ref={viewportContainerRef}
        className="bg-canvas relative flex min-h-0 flex-1 overflow-hidden"
      >
        {/* Line Numbers Gutter */}
        <div
          aria-hidden="true"
          style={{ width: `${gutterMinWidthCh + 1.5}ch` }}
          className="border-border/60 bg-canvas relative shrink-0 overflow-hidden border-r select-none"
        >
          <div
            ref={gutterInnerRef}
            className="py-2.5 pr-2.5 pl-2 font-mono text-[11px] leading-[20px] will-change-transform"
          >
            {topSpacerPx > 0 && <div style={{ height: `${topSpacerPx}px` }} />}
            {visibleSlice.map((_, relIdx) => {
              const ln = visibleStart + relIdx + 1;
              const isActive = ln === cursor.line;
              return (
                <div
                  key={ln}
                  className={cn(
                    "tabular-nums h-[20px] text-right leading-[20px]",
                    isActive
                      ? "text-foreground/85 font-medium"
                      : "text-zinc-400 dark:text-zinc-600",
                  )}
                >
                  {ln}
                </div>
              );
            })}
            {bottomSpacerPx > 0 && <div style={{ height: `${bottomSpacerPx}px` }} />}
          </div>
        </div>

        {/* Synchronized Code Backdrop + Native Transparent Textarea */}
        <div className="relative min-w-0 flex-1 overflow-hidden">
          <div
            aria-hidden="true"
            className="pointer-events-none absolute inset-0 overflow-hidden select-none"
          >
            <div
              ref={backdropInnerRef}
              style={{ tabSize: 2 }}
              className="min-w-full w-max px-3 py-2.5 font-mono text-[12px] leading-[20px] will-change-transform"
            >
              {topSpacerPx > 0 && <div style={{ height: `${topSpacerPx}px` }} />}
              {visibleSlice.map((lineText, relIdx) => {
                const idx = visibleStart + relIdx;
                const isActive = idx + 1 === cursor.line;
                const cachedPlain = highlightState.plainByLine[idx];
                const hlLine =
                  cachedPlain === lineText ? highlightState.lines[idx] : undefined;

                return (
                  <div
                    key={idx}
                    className={cn(
                      "h-[20px] leading-[20px] whitespace-pre",
                      isActive && "bg-black/[0.028] dark:bg-white/[0.032]",
                    )}
                  >
                    {lineText.length === 0 ? (
                      " "
                    ) : hlLine && hlLine.tokens.length > 0 ? (
                      hlLine.tokens.map((tok, tIdx) => (
                        <span
                          key={tIdx}
                          style={{
                            color: adaptTokenColor(tok.fg_hex, isDark),
                            fontStyle: tok.italic ? "italic" : undefined,
                          }}
                        >
                          {tok.text}
                        </span>
                      ))
                    ) : (
                      <span className="text-foreground">{lineText}</span>
                    )}
                  </div>
                );
              })}
              {bottomSpacerPx > 0 && <div style={{ height: `${bottomSpacerPx}px` }} />}
            </div>
          </div>

          <textarea
            ref={textareaRef}
            value={value}
            spellCheck={false}
            autoCapitalize="off"
            autoCorrect="off"
            autoComplete="off"
            wrap="off"
            aria-label="Workspace code editor"
            style={{ tabSize: 2 }}
            {...ime.compositionProps}
            onChange={(e) => {
              onChange(e.target.value);
              refreshCursor(e.currentTarget, e.target.value);
            }}
            onScroll={syncScroll}
            onKeyDown={handleKeyDown}
            onKeyUp={(e) => refreshCursor(e.currentTarget)}
            onClick={(e) => refreshCursor(e.currentTarget)}
            onSelect={(e) => refreshCursor(e.currentTarget)}
            className="caret-foreground selection:bg-primary/25 selectable-text absolute inset-0 h-full w-full resize-none overflow-auto bg-transparent px-3 py-2.5 font-mono text-[12px] leading-[20px] whitespace-pre text-transparent outline-none"
          />
        </div>
      </div>

      {/* Subtle Editor Status Footer */}
      <div className="border-border bg-elevated/45 text-muted-foreground flex h-[26px] shrink-0 items-center justify-between gap-2 border-t px-3 font-mono text-[11px]">
        <div className="flex min-w-0 items-center gap-2">
          <span className="tabular-nums">
            Ln {cursor.line}, Col {cursor.col}
          </span>
          <span className="opacity-40">·</span>
          <span>{lineEndingLabel}</span>
          <span className="opacity-40">·</span>
          <span className="truncate font-sans">{languageLabel}</span>
        </div>
        <div className="flex shrink-0 items-center gap-1.5 font-sans">
          {saving ? (
            <span className="inline-flex items-center gap-1">
              <Loader2 className="h-[11px] w-[11px] animate-spin" />
              Saving…
            </span>
          ) : conflict ? (
            <span className="text-amber-600 dark:text-amber-400">Conflict</span>
          ) : dirty ? (
            <span>Unsaved · {mod}S</span>
          ) : (
            <span className="opacity-70">Saved</span>
          )}
        </div>
      </div>
    </div>
  );
});
