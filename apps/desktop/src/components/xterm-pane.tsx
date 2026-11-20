import { useCallback, useEffect, useRef, useState } from "react";
import { type ILink, type ITheme, Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { ChevronDown, ChevronUp, Search, X } from "lucide-react";
import { orbitIpc } from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const DARK_XTERM_THEME: ITheme = {
  background: "#0B0E14",
  foreground: "#E2E8F0",
  cursor: "#93C5FD",
  selectionBackground: "rgba(147, 197, 253, 0.24)",
};

const LIGHT_XTERM_THEME: ITheme = {
  background: "#F7F8FA",
  foreground: "#1E293B",
  cursor: "#2563EB",
  selectionBackground: "rgba(37, 99, 235, 0.18)",
  black: "#1E293B",
  red: "#DC2626",
  green: "#16A34A",
  yellow: "#CA8A04",
  blue: "#2563EB",
  magenta: "#9333EA",
  cyan: "#0891B2",
  white: "#64748B",
};

const URL_REGEX = /https?:\/\/[^\s"'<>()[\]]+/g;
const FILE_LINE_REGEX = /(?:^|[\s("'`])([A-Za-z0-9_./-]+\.[A-Za-z0-9]{1,8}):(\d+)(?::(\d+))?/g;

export function XtermPane({
  hostId,
  slot,
  active,
  visible,
}: {
  hostId: string;
  slot: number;
  active: boolean;
  visible: boolean;
}) {
  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme);
  const terminalFontSize = useOrbitSelector((s) => s.terminalFontSize);
  const terminalCursorStyle = useOrbitSelector((s) => s.terminalCursorStyle);
  const terminalScrollback = useOrbitSelector((s) => s.terminalScrollback);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const lastSizeRef = useRef<{ cols: number; rows: number }>({ cols: 0, rows: 0 });
  const resizeTimerRef = useRef<number | null>(null);

  const [findOpen, setFindOpen] = useState(false);
  const [findQuery, setFindQuery] = useState("");
  const [findMatchIdx, setFindMatchIdx] = useState(0);
  const [findMatches, setFindMatches] = useState<number[]>([]);
  const findInputRef = useRef<HTMLInputElement | null>(null);

  const syncRemoteSize = useCallback(
    (term: Terminal) => {
      if (term.cols <= 0 || term.rows <= 0) return;
      if (lastSizeRef.current.cols === term.cols && lastSizeRef.current.rows === term.rows) {
        return;
      }
      lastSizeRef.current = { cols: term.cols, rows: term.rows };
      void orbitIpc.resizeTerminalSlot(hostId, slot, term.cols, term.rows);
    },
    [hostId, slot],
  );

  const runBufferSearch = useCallback((query: string) => {
    const term = termRef.current;
    const q = query.trim().toLowerCase();
    if (!term || !q) {
      setFindMatches([]);
      setFindMatchIdx(0);
      return;
    }
    const buf = term.buffer.active;
    const hits: number[] = [];
    for (let y = 0; y < buf.length; y += 1) {
      const line = buf.getLine(y)?.translateToString(true);
      if (line && line.toLowerCase().includes(q)) {
        hits.push(y);
      }
    }
    setFindMatches(hits);
    if (hits.length > 0) {
      const lastIdx = hits.length - 1;
      setFindMatchIdx(lastIdx);
      term.scrollToLine(Math.max(0, hits[lastIdx] - 3));
    } else {
      setFindMatchIdx(0);
    }
  }, []);

  const stepSearch = useCallback(
    (delta: number) => {
      const term = termRef.current;
      if (!term || findMatches.length === 0) return;
      const next = (findMatchIdx + delta + findMatches.length) % findMatches.length;
      setFindMatchIdx(next);
      term.scrollToLine(Math.max(0, findMatches[next] - 3));
    },
    [findMatches, findMatchIdx],
  );

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const snap = orbitActions.getSnapshot();
    const initialDark = snap.resolvedTheme === "dark";
    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: snap.terminalCursorStyle,
      fontFamily: '"JetBrains Mono", "SF Mono", Menlo, Monaco, Consolas, monospace',
      fontSize: snap.terminalFontSize,
      lineHeight: 1.28,
      scrollback: snap.terminalScrollback,
      theme: initialDark ? DARK_XTERM_THEME : LIGHT_XTERM_THEME,
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
              orbitActions.openBrowserTab(text);
            },
          });
        }
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
        callback(links.length > 0 ? links : undefined);
      },
    });

    // Handle terminal-local Cmd+K (clear) and Cmd/Ctrl+F (search)
    const isMac = /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent);
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      const isMod = isMac ? ev.metaKey : ev.ctrlKey;
      const key = ev.key.toLowerCase();
      if (ev.metaKey && !ev.shiftKey && !ev.altKey && key === "k") {
        ev.preventDefault();
        term.clear();
        return false;
      }
      if (isMod && !ev.shiftKey && !ev.altKey && key === "f") {
        ev.preventDefault();
        setFindOpen(true);
        requestAnimationFrame(() => findInputRef.current?.focus());
        return false;
      }
      return true;
    });

    termRef.current = term;
    fitRef.current = fitAddon;
    lastSizeRef.current = { cols: 0, rows: 0 };

    const unsubscribeStream = orbitActions.subscribeTerminalSlot(hostId, slot, (bytes) => {
      term.write(bytes);
    });

    const dataDisposable = term.onData((data) => {
      void orbitIpc.sendTerminalInputSlot(hostId, slot, data);
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
      if (resizeTimerRef.current !== null) {
        window.clearTimeout(resizeTimerRef.current);
        resizeTimerRef.current = null;
      }
      resizeObserver.disconnect();
      linkDisposable.dispose();
      dataDisposable.dispose();
      unsubscribeStream();
      try {
        webglAddon?.dispose();
      } catch {
        // ignore
      }
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [hostId, slot, syncRemoteSize]);

  // Dynamically update live xterm palette & typography when preferences change
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = resolvedTheme === "dark" ? DARK_XTERM_THEME : LIGHT_XTERM_THEME;
    term.options.fontSize = terminalFontSize;
    term.options.cursorStyle = terminalCursorStyle;
    term.options.scrollback = terminalScrollback;
    if (active && visible) {
      fitRef.current?.fit();
      syncRemoteSize(term);
    }
  }, [resolvedTheme, terminalFontSize, terminalCursorStyle, terminalScrollback, active, visible, syncRemoteSize]);

  // Re-fit, sync remote PTY size, and focus when this persistent tab becomes visible/active
  useEffect(() => {
    if (!active || !visible) return;
    const id = requestAnimationFrame(() => {
      fitRef.current?.fit();
      if (termRef.current) {
        syncRemoteSize(termRef.current);
        termRef.current.focus();
      }
    });
    return () => cancelAnimationFrame(id);
  }, [active, visible, syncRemoteSize]);

  return (
    <div
      onFocusCapture={() => orbitActions.setTerminalFocused(true)}
      onBlurCapture={() => orbitActions.setTerminalFocused(false)}
      className={cn(
        "relative h-full w-full overflow-hidden bg-[#F7F8FA] px-2.5 py-1.5 dark:bg-[#0B0E14]",
        !active && "hidden",
      )}
    >
      {findOpen && (
        <div className="border-border bg-elevated/95 absolute top-2 right-3 z-20 flex items-center gap-1 rounded-lg border px-2 py-1 shadow-md backdrop-blur-sm">
          <Search className="text-muted-foreground h-3.5 w-3.5 shrink-0" />
          <input
            ref={findInputRef}
            type="text"
            value={findQuery}
            onChange={(e) => {
              setFindQuery(e.target.value);
              runBufferSearch(e.target.value);
            }}
            onKeyDown={(e) => {
              if (e.key === "Escape") {
                e.preventDefault();
                setFindOpen(false);
                termRef.current?.focus();
              } else if (e.key === "Enter") {
                e.preventDefault();
                stepSearch(e.shiftKey ? -1 : 1);
              }
            }}
            placeholder="Find in terminal…"
            className="text-foreground placeholder:text-muted-foreground w-40 bg-transparent text-[12px] outline-none"
          />
          <span className="text-muted-foreground tabular-nums text-[11px]">
            {findMatches.length > 0 ? `${findMatchIdx + 1}/${findMatches.length}` : "0"}
          </span>
          <button
            type="button"
            title="Previous match"
            onClick={() => stepSearch(-1)}
            className="text-muted-foreground hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded"
          >
            <ChevronUp className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            title="Next match"
            onClick={() => stepSearch(1)}
            className="text-muted-foreground hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded"
          >
            <ChevronDown className="h-3.5 w-3.5" />
          </button>
          <button
            type="button"
            title="Close search"
            onClick={() => {
              setFindOpen(false);
              termRef.current?.focus();
            }}
            className="text-muted-foreground hover:text-foreground inline-flex h-5 w-5 cursor-pointer items-center justify-center rounded"
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
