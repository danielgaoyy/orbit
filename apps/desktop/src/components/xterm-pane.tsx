import { useEffect, useRef } from "react";
import { type ITheme, Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
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

    try {
      const webglAddon = new WebglAddon();
      term.loadAddon(webglAddon);
    } catch {
      // Fallback to standard canvas renderer when WebGL2 is unavailable
    }

    termRef.current = term;
    fitRef.current = fitAddon;

    const unsubscribeStream = orbitActions.subscribeTerminalSlot(hostId, slot, (bytes) => {
      term.write(bytes);
    });

    const dataDisposable = term.onData((data) => {
      void orbitIpc.sendTerminalInputSlot(hostId, slot, data);
    });

    const resizeObserver = new ResizeObserver(() => {
      if (el.clientWidth > 0 && el.clientHeight > 0) {
        fitAddon.fit();
        if (term.cols > 0 && term.rows > 0) {
          void orbitIpc.resizeTerminalSlot(hostId, slot, term.cols, term.rows);
        }
      }
    });
    resizeObserver.observe(el);

    return () => {
      resizeObserver.disconnect();
      dataDisposable.dispose();
      unsubscribeStream();
      term.dispose();
      termRef.current = null;
      fitRef.current = null;
    };
  }, [hostId, slot]);

  // Dynamically update live xterm palette & typography when preferences change
  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    term.options.theme = resolvedTheme === "dark" ? DARK_XTERM_THEME : LIGHT_XTERM_THEME;
    term.options.fontSize = terminalFontSize;
    term.options.cursorStyle = terminalCursorStyle;
    term.options.scrollback = terminalScrollback;
    fitRef.current?.fit();
  }, [resolvedTheme, terminalFontSize, terminalCursorStyle, terminalScrollback]);

  // Re-fit and focus when this persistent tab becomes visible/active
  useEffect(() => {
    if (!active || !visible) return;
    const id = requestAnimationFrame(() => {
      fitRef.current?.fit();
      termRef.current?.focus();
    });
    return () => cancelAnimationFrame(id);
  }, [active, visible]);

  return (
    <div
      onFocusCapture={() => orbitActions.setTerminalFocused(true)}
      onBlurCapture={() => orbitActions.setTerminalFocused(false)}
      className={cn(
        "h-full w-full overflow-hidden bg-[#F7F8FA] px-2.5 py-1.5 dark:bg-[#0B0E14]",
        !active && "hidden",
      )}
    >
      <div ref={containerRef} className="h-full w-full overflow-hidden" />
    </div>
  );
}

export default XtermPane;
