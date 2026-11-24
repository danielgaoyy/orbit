import { useCallback, useEffect, useRef, useState } from "react";
import { type ILink, type ITheme, Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { AlertCircle, ChevronDown, ChevronUp, Search, X } from "lucide-react";
import { useI18n } from "@/lib/i18n";
import { isTauriRuntime, orbitIpc } from "@/lib/orbit-ipc";
import {
  type XtermSearchMatch,
  resolveXtermSearchShortcut,
  revealXtermMatch,
  searchXtermBuffer,
  stepXtermMatchIndex,
} from "@/lib/xterm-search";
import { createXtermWriteBatcher } from "@/lib/xterm-write-batcher";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

import type { TerminalThemeMode } from "@/state/orbit-store";

/**
 * Hand-tuned 16-color OKLCH-balanced ANSI palettes harmonized with Orbit's 258° cool-neutral shell.
 * Backgrounds match `--surface-recessed` so the terminal sits naturally inside the workbench without
 * harsh navy/black cutouts or neon-glare 1990s default ANSI primaries.
 */
const DARK_HARMONIZED_XTERM_THEME: ITheme = {
  background: "#121318",
  foreground: "#E5E7EE",
  cursor: "#E5E7EE",
  cursorAccent: "#121318",
  selectionBackground: "rgba(148, 178, 232, 0.24)",
  black: "#22242D",
  red: "#E5737B",
  green: "#6BC490",
  yellow: "#DFB664",
  blue: "#7AA8F0",
  magenta: "#BA8FE8",
  cyan: "#6CC4CE",
  white: "#C8CCD8",
  brightBlack: "#5C6275",
  brightRed: "#F08C93",
  brightGreen: "#84D4A4",
  brightYellow: "#ECC87D",
  brightBlue: "#96BCF7",
  brightMagenta: "#CCA6F2",
  brightCyan: "#86D4DC",
  brightWhite: "#F3F4F8",
};

const LIGHT_HARMONIZED_XTERM_THEME: ITheme = {
  background: "#F5F6F8",
  foreground: "#1C1E26",
  cursor: "#1C1E26",
  cursorAccent: "#F5F6F8",
  selectionBackground: "rgba(59, 110, 210, 0.16)",
  black: "#262934",
  red: "#C83A4A",
  green: "#1E8550",
  yellow: "#9A6700",
  blue: "#2A63C8",
  magenta: "#8244BE",
  cyan: "#147E8C",
  white: "#5F6678",
  brightBlack: "#6E7587",
  brightRed: "#DC4C5B",
  brightGreen: "#289C60",
  brightYellow: "#B27A0A",
  brightBlue: "#3A75DC",
  brightMagenta: "#9557D1",
  brightCyan: "#1B94A3",
  brightWhite: "#14161D",
};

const DARK_HIGH_CONTRAST_XTERM_THEME: ITheme = {
  ...DARK_HARMONIZED_XTERM_THEME,
  background: "#0C0D12",
  foreground: "#F4F6FA",
  cursor: "#FFFFFF",
  red: "#FF7B85",
  green: "#6EE09B",
  yellow: "#F4C96B",
  blue: "#82B6FF",
  magenta: "#CE9EFF",
  cyan: "#72DCE8",
};

const LIGHT_HIGH_CONTRAST_XTERM_THEME: ITheme = {
  ...LIGHT_HARMONIZED_XTERM_THEME,
  background: "#FFFFFF",
  foreground: "#111318",
  cursor: "#111318",
  red: "#B91C1C",
  green: "#15803D",
  yellow: "#854D0E",
  blue: "#1D4ED8",
  magenta: "#6D28D9",
  cyan: "#0E7490",
};

function resolveXtermTheme(resolvedTheme: "light" | "dark", mode: TerminalThemeMode): ITheme {
  if (mode === "high_contrast") {
    return resolvedTheme === "dark"
      ? DARK_HIGH_CONTRAST_XTERM_THEME
      : LIGHT_HIGH_CONTRAST_XTERM_THEME;
  }
  return resolvedTheme === "dark"
    ? DARK_HARMONIZED_XTERM_THEME
    : LIGHT_HARMONIZED_XTERM_THEME;
}

const URL_REGEX = /https?:\/\/[^\s"'<>()[\]]+/g;
const FILE_LINE_REGEX = /(?:^|[\s("'`])([A-Za-z0-9_./-]+\.[A-Za-z0-9]{1,8}):(\d+)(?::(\d+))?/g;

export function XtermPane({
  hostId,
  terminalId,
  slot = 1,
  active,
  visible,
  detached = false,
}: {
  hostId: string;
  terminalId?: string | null;
  slot?: number;
  active: boolean;
  visible: boolean;
  detached?: boolean;
}) {
  const { t } = useI18n();
  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme);
  const terminalFontSize = useOrbitSelector((s) => s.terminalFontSize);
  const terminalCursorStyle = useOrbitSelector((s) => s.terminalCursorStyle);
  const terminalScrollback = useOrbitSelector((s) => s.terminalScrollback);
  const terminalThemeMode = useOrbitSelector((s) => s.terminalThemeMode);

  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const lastSizeRef = useRef<{ cols: number; rows: number }>({ cols: 0, rows: 0 });
  const resizeTimerRef = useRef<number | null>(null);
  const isFocusedRef = useRef<boolean>(false);

  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [caseSensitive, setCaseSensitive] = useState(false);
  const [useRegex, setUseRegex] = useState(false);
  const [findMatches, setFindMatches] = useState<XtermSearchMatch[]>([]);
  const [findMatchIdx, setFindMatchIdx] = useState(-1);
  const [searchError, setSearchError] = useState<string | null>(null);
  const [searchTruncated, setSearchTruncated] = useState(false);

  const findInputRef = useRef<HTMLInputElement | null>(null);
  const findOpenRef = useRef(false);
  findOpenRef.current = findOpen;

  const effectiveTerminalId = terminalId?.trim() ?? "";

  const syncRemoteSize = useCallback(
    (term: Terminal) => {
      if (term.cols <= 0 || term.rows <= 0) return;
      if (lastSizeRef.current.cols === term.cols && lastSizeRef.current.rows === term.rows) {
        return;
      }
      lastSizeRef.current = { cols: term.cols, rows: term.rows };
      if (effectiveTerminalId) {
        void orbitIpc.resizeTerminal(hostId, term.cols, term.rows, effectiveTerminalId);
      } else {
        void orbitIpc.resizeTerminalSlot(hostId, slot, term.cols, term.rows);
      }
    },
    [hostId, effectiveTerminalId, slot],
  );

  const runBufferSearch = useCallback(
    (query: string, opts?: { caseSensitive?: boolean; regex?: boolean }) => {
      const term = termRef.current;
      const nextCase = opts?.caseSensitive ?? caseSensitive;
      const nextRegex = opts?.regex ?? useRegex;
      if (!term || !query) {
        setFindMatches([]);
        setFindMatchIdx(-1);
        setSearchError(null);
        setSearchTruncated(false);
        if (term) {
          revealXtermMatch(term, null);
        }
        return;
      }

      const result = searchXtermBuffer(term.buffer.active, query, {
        caseSensitive: nextCase,
        regex: nextRegex,
      });
      setSearchError(result.error);
      setSearchTruncated(result.truncated);
      setFindMatches(result.matches);

      if (result.matches.length > 0) {
        const lastIdx = result.matches.length - 1;
        setFindMatchIdx(lastIdx);
        revealXtermMatch(term, result.matches[lastIdx]);
      } else {
        setFindMatchIdx(-1);
        revealXtermMatch(term, null);
      }
    },
    [caseSensitive, useRegex],
  );

  const stepSearch = useCallback(
    (delta: number) => {
      const term = termRef.current;
      if (!term || findMatches.length === 0) return;
      const next = stepXtermMatchIndex(findMatchIdx, findMatches.length, delta);
      if (next < 0) return;
      setFindMatchIdx(next);
      revealXtermMatch(term, findMatches[next]);
    },
    [findMatches, findMatchIdx],
  );

  const openSearch = useCallback(() => {
    setFindOpen(true);
    if (findQuery) {
      runBufferSearch(findQuery);
    }
    requestAnimationFrame(() => {
      findInputRef.current?.focus();
      findInputRef.current?.select();
    });
  }, [findQuery, runBufferSearch]);

  const closeSearch = useCallback(() => {
    setFindOpen(false);
    const term = termRef.current;
    if (term) {
      revealXtermMatch(term, null);
      term.focus();
    }
  }, []);

  // Keep latest callbacks in refs so xterm's custom key handler stays stable across re-renders
  const shortcutActionsRef = useRef({
    openSearch,
    closeSearch,
    stepSearch,
  });
  shortcutActionsRef.current = {
    openSearch,
    closeSearch,
    stepSearch,
  };

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const snap = orbitActions.getSnapshot();
    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: snap.terminalCursorStyle,
      fontFamily: '"JetBrains Mono", "SF Mono", Menlo, Monaco, Consolas, monospace',
      fontSize: snap.terminalFontSize,
      lineHeight: 1.28,
      scrollback: snap.terminalScrollback,
      theme: resolveXtermTheme(snap.resolvedTheme, snap.terminalThemeMode),
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(el);

    let webglAddon: WebglAddon | null = null;
    try {
      webglAddon = new WebglAddon();
      webglAddon.onContextLoss(() => {
        try {
          webglAddon?.dispose();
        } catch {
          // ignore disposal errors after context loss
        }
        webglAddon = null;
      });
      term.loadAddon(webglAddon);
    } catch {
      // Fallback to DOM renderer when WebGL2 is unavailable
    }

    // Register Cmd/Ctrl+Click link provider for URLs and file:line references
    const linkDisposable = term.registerLinkProvider({
      provideLinks(bufferLineNumber, callback) {
        const line = term.buffer.active.getLine(bufferLineNumber - 1)?.translateToString(true);
        if (!line) {
          callback(undefined);
          return;
        }
        const links: ILink[] = [];
        URL_REGEX.lastIndex = 0;
        let m: RegExpExecArray | null;
        while ((m = URL_REGEX.exec(line)) !== null) {
          const url = m[0].replace(/[.,;:!?]+$/, "");
          const startX = m.index + 1;
          links.push({
            text: url,
            range: {
              start: { x: startX, y: bufferLineNumber },
              end: { x: startX + url.length - 1, y: bufferLineNumber },
            },
            activate: (_ev, text) => {
              if (detached) {
                void orbitIpc.openExternalBrowserUrl(text);
              } else {
                orbitActions.openBrowserTab(text);
              }
            },
          });
        }
        if (!detached) {
          FILE_LINE_REGEX.lastIndex = 0;
          while ((m = FILE_LINE_REGEX.exec(line)) !== null) {
            const path = m[1];
            const lineNum = Number.parseInt(m[2], 10);
            if (!path || Number.isNaN(lineNum)) continue;
            const matchStart = m.index + m[0].indexOf(path);
            const fullText = `${path}:${m[2]}${m[3] ? `:${m[3]}` : ""}`;
            links.push({
              text: fullText,
              range: {
                start: { x: matchStart + 1, y: bufferLineNumber },
                end: { x: matchStart + fullText.length, y: bufferLineNumber },
              },
              activate: () => {
                const cur = orbitActions.getSnapshot();
                orbitActions.openFileInSidePanel(path, {
                  hostId,
                  cwd: cur.selectedWorkspaceCwd,
                  focusLine: lineNum,
                });
              },
            });
          }
        }
        callback(links.length > 0 ? links : undefined);
      },
    });

    // Scoped terminal-local shortcuts: Cmd+K (clear) and Cmd/Ctrl+F, Cmd/Ctrl+G, Esc (search)
    const isMac = /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent);
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      const key = ev.key.toLowerCase();
      if (ev.metaKey && !ev.shiftKey && !ev.altKey && !ev.ctrlKey && key === "k") {
        ev.preventDefault();
        ev.stopPropagation();
        term.clear();
        return false;
      }
      const action = resolveXtermSearchShortcut(ev, {
        isMac,
        terminalHasFocus: true,
        findOpen: findOpenRef.current,
        inSearchInput: false,
      });
      if (action === "open") {
        ev.preventDefault();
        ev.stopPropagation();
        shortcutActionsRef.current.openSearch();
        return false;
      }
      if (action === "next") {
        ev.preventDefault();
        ev.stopPropagation();
        shortcutActionsRef.current.stepSearch(1);
        return false;
      }
      if (action === "prev") {
        ev.preventDefault();
        ev.stopPropagation();
        shortcutActionsRef.current.stepSearch(-1);
        return false;
      }
      if (action === "close") {
        ev.preventDefault();
        ev.stopPropagation();
        shortcutActionsRef.current.closeSearch();
        return false;
      }
      return true;
    });

    termRef.current = term;
    fitRef.current = fitAddon;
    lastSizeRef.current = { cols: 0, rows: 0 };

    // Bounded frame-coalesced binary write pipeline: coalesces visible bursts into rAF,
    // drains via 16ms timeout when minimized/hidden (`document.hidden`), and flushes
    // immediately if buffered bytes hit 256 KiB so memory stays strictly bounded.
    let disposed = false;
    const writeBatcher = createXtermWriteBatcher(
      (bytes) => {
        if (!disposed) {
          term.write(bytes);
        }
      },
      {
        isDocumentHidden: () => typeof document !== "undefined" && document.hidden,
        requestAnimationFrame: (cb) => window.requestAnimationFrame(cb),
        cancelAnimationFrame: (id) => window.cancelAnimationFrame(id),
        setTimeout: (cb, ms) => window.setTimeout(cb, ms),
        clearTimeout: (id) => window.clearTimeout(id),
      },
    );

    const onVisibilityChange = () => {
      writeBatcher.onVisibilityChange();
    };
    document.addEventListener("visibilitychange", onVisibilityChange);

    const enqueueBinaryChunk = (chunk: Uint8Array) => {
      if (disposed || chunk.byteLength === 0) return;
      if (!detached) {
        orbitActions.recordTerminalOutputChunk(hostId, slot, chunk);
      }
      writeBatcher.enqueue(chunk);
    };

    let binarySubscriptionId: string | null = null;
    let unsubscribeFallback: (() => void) | null = null;

    if (isTauriRuntime()) {
      void orbitIpc
        .subscribeTerminalBinary(hostId, effectiveTerminalId, enqueueBinaryChunk, slot)
        .then((subId) => {
          if (disposed) {
            if (subId) {
              void orbitIpc.unsubscribeTerminalBinary(subId);
            }
          } else {
            binarySubscriptionId = subId;
          }
        })
        .catch(() => {
          if (!disposed) {
            unsubscribeFallback = orbitActions.subscribeTerminalSlot(
              hostId,
              slot,
              enqueueBinaryChunk,
            );
          }
        });
    } else {
      unsubscribeFallback = orbitActions.subscribeTerminalSlot(hostId, slot, enqueueBinaryChunk);
    }

    const dataDisposable = term.onData((data) => {
      if (effectiveTerminalId) {
        void orbitIpc.writeTerminalInput(hostId, effectiveTerminalId, data);
      } else {
        void orbitIpc.sendTerminalInputSlot(hostId, slot, data);
      }
    });

    const resizeObserver = new ResizeObserver(() => {
      if (el.clientWidth <= 0 || el.clientHeight <= 0) return;
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
      }
      resizeTimerRef.current = window.setTimeout(() => {
        resizeTimerRef.current = null;
        if (el.clientWidth > 0 && el.clientHeight > 0) {
          fitAddon.fit();
          syncRemoteSize(term);
        }
      }, 65);
    });
    resizeObserver.observe(el);

    return () => {
      disposed = true;
      document.removeEventListener("visibilitychange", onVisibilityChange);
      writeBatcher.dispose();

      if (isFocusedRef.current) {
        isFocusedRef.current = false;
        if (!detached) {
          orbitActions.setTerminalFocused(false);
        }
      }
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
      resizeObserver.disconnect();
      linkDisposable.dispose();
      dataDisposable.dispose();
      unsubscribeFallback?.();
      if (binarySubscriptionId) {
        const subToCancel = binarySubscriptionId;
        binarySubscriptionId = null;
        void orbitIpc.unsubscribeTerminalBinary(subToCancel);
      }
      try {
        webglAddon?.dispose();
      } catch {
        // ignore
      }
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [hostId, effectiveTerminalId, slot, detached, syncRemoteSize]);

  // Dynamically update live xterm palette & typography when preferences change
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = resolveXtermTheme(resolvedTheme, terminalThemeMode);
    term.options.fontSize = terminalFontSize;
    term.options.cursorStyle = terminalCursorStyle;
    term.options.scrollback = terminalScrollback;
    if (active && visible) {
      fitRef.current?.fit();
      syncRemoteSize(term);
    }
  }, [
    resolvedTheme,
    terminalThemeMode,
    terminalFontSize,
    terminalCursorStyle,
    terminalScrollback,
    active,
    visible,
    syncRemoteSize,
  ]);

  // Re-fit, sync remote PTY size, and focus when this persistent tab becomes visible/active;
  // also release terminal focus whenever hidden or deactivated (React does not fire blur on hide).
  useEffect(() => {
    if (!active || !visible) {
      if (isFocusedRef.current) {
        isFocusedRef.current = false;
        if (!detached) {
          orbitActions.setTerminalFocused(false);
        }
      }
      return;
    }
    const id = requestAnimationFrame(() => {
      fitRef.current?.fit();
      if (termRef.current) {
        syncRemoteSize(termRef.current);
        termRef.current.focus();
      }
    });
    return () => cancelAnimationFrame(id);
  }, [active, visible, detached, syncRemoteSize]);

  const handleWrapperKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      const wrapper = wrapperRef.current;
      const activeEl = document.activeElement;
      const terminalHasFocus = Boolean(wrapper && activeEl && wrapper.contains(activeEl));
      const inSearchInput = activeEl === findInputRef.current;
      const isMac = /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent);

      const action = resolveXtermSearchShortcut(
        {
          key: e.key,
          metaKey: e.metaKey,
          ctrlKey: e.ctrlKey,
          shiftKey: e.shiftKey,
          altKey: e.altKey,
          isComposing: e.nativeEvent.isComposing,
          defaultPrevented: e.defaultPrevented,
        },
        {
          isMac,
          terminalHasFocus,
          findOpen,
          inSearchInput,
        },
      );

      if (!action) return;
      e.preventDefault();
      e.stopPropagation();

      if (action === "open") {
        openSearch();
      } else if (action === "next") {
        stepSearch(1);
      } else if (action === "prev") {
        stepSearch(-1);
      } else if (action === "close") {
        closeSearch();
      }
    },
    [findOpen, openSearch, stepSearch, closeSearch],
  );

  const matchCountLabel =
    findQuery.length === 0
      ? ""
      : findMatches.length > 0
        ? `${findMatchIdx + 1} / ${findMatches.length}${searchTruncated ? "+" : ""}`
        : "0 / 0";

  return (
    <div
      ref={wrapperRef}
      onKeyDown={handleWrapperKeyDown}
      onFocusCapture={() => {
        isFocusedRef.current = true;
        if (!detached) {
          orbitActions.setTerminalFocused(true);
        }
      }}
      onBlurCapture={(e) => {
        const nextFocused = e.relatedTarget as Node | null;
        if (nextFocused && wrapperRef.current?.contains(nextFocused)) {
          return;
        }
        isFocusedRef.current = false;
        if (!detached) {
          orbitActions.setTerminalFocused(false);
        }
      }}
      className={cn(
        "relative h-full w-full overflow-hidden px-3.5 py-2.5",
        terminalThemeMode === "high_contrast"
          ? "bg-white dark:bg-[#0C0D12]"
          : "bg-surface-recessed",
        !active && "hidden",
      )}
    >
      {findOpen && (
        <div
          role="search"
          aria-label={t("terminal.findPlaceholder")}
          className={cn(
            "bg-elevated/95 floating-surface absolute top-2.5 right-3.5 z-20 flex items-center gap-1 rounded-lg border px-2 py-1 shadow-sm backdrop-blur-md",
            searchError
              ? "border-rose-500/60"
              : "border-black/[0.09] dark:border-white/[0.11]",
          )}
        >
          <Search className="text-muted-foreground h-3.5 w-3.5 shrink-0" />
          <input
            ref={findInputRef}
            type="text"
            value={findQuery}
            aria-invalid={searchError !== null}
            onChange={(e) => {
              const next = e.target.value;
              setFindQuery(next);
              runBufferSearch(next);
            }}
            placeholder={t("terminal.findPlaceholder")}
            className="text-foreground placeholder:text-muted-foreground w-40 bg-transparent text-[12px] outline-none"
          />

          <button
            type="button"
            aria-pressed={caseSensitive}
            title={t("terminal.matchCase")}
            onClick={() => {
              const next = !caseSensitive;
              setCaseSensitive(next);
              runBufferSearch(findQuery, { caseSensitive: next, regex: useRegex });
              findInputRef.current?.focus();
            }}
            className={cn(
              "inline-flex h-5 cursor-pointer items-center justify-center rounded px-1 font-mono text-[10.5px] font-medium transition-colors",
              caseSensitive
                ? "bg-signal-info/15 text-signal-info"
                : "text-muted-foreground hover:bg-accent hover:text-foreground",
            )}
          >
            Aa
          </button>

          <button
            type="button"
            aria-pressed={useRegex}
            title={t("terminal.useRegex")}
            onClick={() => {
              const next = !useRegex;
              setUseRegex(next);
              runBufferSearch(findQuery, { caseSensitive, regex: next });
              findInputRef.current?.focus();
            }}
            className={cn(
              "inline-flex h-5 cursor-pointer items-center justify-center rounded px-1 font-mono text-[10.5px] font-medium transition-colors",
              useRegex
                ? "bg-signal-info/15 text-signal-info"
                : "text-muted-foreground hover:bg-accent hover:text-foreground",
            )}
          >
            .*
          </button>

          {searchError ? (
            <span
              title={`${t("terminal.invalidRegex")}: ${searchError}`}
              className="inline-flex items-center px-1 text-rose-500"
            >
              <AlertCircle className="h-3.5 w-3.5" />
            </span>
          ) : (
            matchCountLabel && (
              <span className="text-muted-foreground min-w-[34px] px-0.5 text-right text-[11px] tabular-nums">
                {matchCountLabel}
              </span>
            )
          )}

          <div className="bg-border/70 mx-0.5 h-3.5 w-px shrink-0" />

          <button
            type="button"
            disabled={findMatches.length === 0}
            title={t("terminal.previousMatch")}
            aria-label={t("terminal.previousMatch")}
            onClick={() => stepSearch(-1)}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded transition-colors disabled:cursor-default disabled:opacity-40"
          >
            <ChevronUp className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            disabled={findMatches.length === 0}
            title={t("terminal.nextMatch")}
            aria-label={t("terminal.nextMatch")}
            onClick={() => stepSearch(1)}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded transition-colors disabled:cursor-default disabled:opacity-40"
          >
            <ChevronDown className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            title={t("terminal.closeSearch")}
            aria-label={t("terminal.closeSearch")}
            onClick={closeSearch}
            className="text-muted-foreground hover:bg-accent hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded transition-colors"
          >
            <X className="h-3.5 w-3.5" />
          </button>
        </div>
      )}
      <div ref={containerRef} className="h-full w-full overflow-hidden" />
    </div>
  );
}

export default XtermPane;
