import { useEffect, useRef } from "react";
import { Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import { orbitIpc } from "@/lib/orbit-ipc";

export interface XtermStreamChunk {
  seq: number;
  bytes: Uint8Array;
}

export function XtermPane({
  hostId,
  cwd,
  chunks,
}: {
  hostId: string;
  cwd: string;
  chunks: XtermStreamChunk[];
}) {
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const lastSeqRef = useRef<number>(0);

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: "block",
      fontFamily: '"JetBrains Mono", "SF Mono", Menlo, Monaco, Consolas, monospace',
      fontSize: 13,
      lineHeight: 1.25,
      theme: {
        background: "#090D16",
        foreground: "#E2E8F0",
        cursor: "#60A5FA",
        selectionBackground: "rgba(96, 165, 250, 0.28)",
      },
    });
    const fitAddon = new FitAddon();
    term.loadAddon(fitAddon);
    term.open(el);

    try {
      const webglAddon = new WebglAddon();
      term.loadAddon(webglAddon);
    } catch {
      // Graceful canvas fallback when WebGL2 context is unavailable (e.g. headless Linux VM)
    }

    fitAddon.fit();
    termRef.current = term;
    lastSeqRef.current = 0;

    void orbitIpc.startTerminal(hostId, cwd, term.cols || 100, term.rows || 30);

    const dataDisposable = term.onData((data) => {
      void orbitIpc.sendTerminalInput(hostId, data);
    });

    const resizeObserver = new ResizeObserver(() => {
      fitAddon.fit();
      if (term.cols > 0 && term.rows > 0) {
        void orbitIpc.resizeTerminal(hostId, term.cols, term.rows);
      }
    });
    resizeObserver.observe(el);

    return () => {
      resizeObserver.disconnect();
      dataDisposable.dispose();
      term.dispose();
      termRef.current = null;
    };
  }, [hostId, cwd]);

  useEffect(() => {
    const term = termRef.current;
    if (!term) return;
    for (const chunk of chunks) {
      if (chunk.seq > lastSeqRef.current) {
        term.write(chunk.bytes);
        lastSeqRef.current = chunk.seq;
      }
    }
  }, [chunks]);

  return (
    <div className="flex h-full flex-col overflow-hidden rounded-xl border border-white/10 bg-[#090D16] p-2">
      <div ref={containerRef} className="h-full w-full flex-1 overflow-hidden" />
    </div>
  );
}
