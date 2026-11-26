import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { type ILink, Terminal } from "@xterm/xterm";
import { FitAddon } from "@xterm/addon-fit";
import { WebglAddon } from "@xterm/addon-webgl";
import {
  AlertCircle,
  Check,
  ChevronDown,
  ChevronUp,
  Copy,
  CornerUpLeft,
  Search,
  X,
} from "lucide-react";
import { SimpleTooltip } from "@/components/ui/overlays";
import { useI18n } from "@/lib/i18n";
import { modSymbol } from "@/lib/keybindings";
import { isMacDesktop, isTauriRuntime, orbitIpc } from "@/lib/orbit-ipc";
import { matchDesktopZoomAction, resolveTypography } from "@/lib/typography";
import {
  type Osc133CommandBlock,
  type Osc133Tracker,
  createOsc133Tracker,
  extractOsc133BlockOutput,
  findAdjacentOsc133Block,
  resolveTerminalPaneShortcut,
} from "@/lib/xterm-osc133";
import {
  type XtermSearchMatch,
  resolveXtermSearchShortcut,
  revealXtermMatch,
  searchXtermBuffer,
  stepXtermMatchIndex,
} from "@/lib/xterm-search";
import { resolveXtermTheme } from "@/lib/xterm-theme";
import { createXtermWriteBatcher } from "@/lib/xterm-write-batcher";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

import type { TerminalSplitDirection } from "@/state/orbit-store";

const URL_REGEX = /https?:\/\/[^\s"'<>()[\]]+/g;
const FILE_LINE_REGEX = /(?:^|[\s("'`])([A-Za-z0-9_./-]+\.[A-Za-z0-9]{1,8}):(\d+)(?::(\d+))?/g;

export function XtermPane({
  hostId,
  terminalId,
  slot = 1,
  active,
  focused,
  visible,
  detached = false,
  paneTitle,
  isSplit = false,
  onPaneFocus,
  onRequestSplit,
  onRequestClosePane,
}: {
  hostId: string;
  terminalId?: string | null;
  slot?: number;
  active: boolean;
  focused?: boolean;
  visible: boolean;
  detached?: boolean;
  paneTitle?: string;
  isSplit?: boolean;
  onPaneFocus?: () => void;
  onRequestSplit?: (direction: TerminalSplitDirection) => void;
  onRequestClosePane?: () => void;
}) {
  const { t } = useI18n();
  const mod = modSymbol();
  const isFocusedPane = focused ?? active;
  const resolvedTheme = useOrbitSelector((s) => s.resolvedTheme);
  const codeFontSize = useOrbitSelector((s) => s.codeFontSize);
  const codeFontId = useOrbitSelector((s) => s.codeFontId);
  const codeFontCustom = useOrbitSelector((s) => s.codeFontCustom);
  const terminalCursorStyle = useOrbitSelector((s) => s.terminalCursorStyle);
  const terminalScrollback = useOrbitSelector((s) => s.terminalScrollback);
  const terminalThemeMode = useOrbitSelector((s) => s.terminalThemeMode);
  const terminalTypography = useMemo(
    () => resolveTypography({ codeFontSize, codeFontId, codeFontCustom }).terminal,
    [codeFontSize, codeFontId, codeFontCustom],
  );

  const wrapperRef = useRef<HTMLDivElement | null>(null);
  const containerRef = useRef<HTMLDivElement | null>(null);
  const termRef = useRef<Terminal | null>(null);
  const fitRef = useRef<FitAddon | null>(null);
  const webglAddonRef = useRef<WebglAddon | null>(null);
  const osc133TrackerRef = useRef<Osc133Tracker | null>(null);
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

  const [oscBlocks, setOscBlocks] = useState<Osc133CommandBlock[]>([]);
  const [activeOscBlockId, setActiveOscBlockId] = useState<number | null>(null);
  const [copiedBlockOutput, setCopiedBlockOutput] = useState(false);
  const copiedBlockTimerRef = useRef<number | null>(null);

  const findInputRef = useRef<HTMLInputElement | null>(null);
  const findOpenRef = useRef(false);
  findOpenRef.current = findOpen;
  const oscBlocksRef = useRef<Osc133CommandBlock[]>(oscBlocks);
  oscBlocksRef.current = oscBlocks;
  const activeOscBlockIdRef = useRef<number | null>(activeOscBlockId);
  activeOscBlockIdRef.current = activeOscBlockId;
  const onRequestSplitRef = useRef(onRequestSplit);
  onRequestSplitRef.current = onRequestSplit;
  const onPaneFocusRef = useRef(onPaneFocus);
  onPaneFocusRef.current = onPaneFocus;

  const effectiveTerminalId = terminalId?.trim() ?? "";

  useEffect(() => {
    return () => {
      if (copiedBlockTimerRef.current !== null) {
        window.clearTimeout(copiedBlockTimerRef.current);
      }
    };
  }, []);

  const selectedOscBlock = useMemo(() => {
    if (oscBlocks.length === 0) return null;
    if (activeOscBlockId !== null) {
      const found = oscBlocks.find((b) => b.id === activeOscBlockId);
      if (found) return found;
    }
    return oscBlocks[oscBlocks.length - 1] ?? null;
  }, [oscBlocks, activeOscBlockId]);

  const selectedOscBlockIndex = useMemo(() => {
    if (!selectedOscBlock) return -1;
    return oscBlocks.findIndex((b) => b.id === selectedOscBlock.id);
  }, [oscBlocks, selectedOscBlock]);

  const jumpOscBlock = useCallback((direction: "prev" | "next") => {
    const term = termRef.current;
    const blocks = oscBlocksRef.current;
    if (!term || blocks.length === 0) return;
    const target = findAdjacentOsc133Block(
      blocks,
      term.buffer.active.viewportY,
      direction,
      activeOscBlockIdRef.current,
    );
    if (!target) return;
    setActiveOscBlockId(target.id);
    term.scrollToLine(Math.max(0, target.promptRow - 1));
  }, []);

  const readSelectedOscBlockOutput = useCallback((): {
    block: Osc133CommandBlock;
    output: string;
  } | null => {
    const term = termRef.current;
    const blocks = oscBlocksRef.current;
    if (!term || blocks.length === 0) return null;
    const target =
      (activeOscBlockIdRef.current !== null
        ? blocks.find((b) => b.id === activeOscBlockIdRef.current)
        : null) ?? blocks[blocks.length - 1];
    if (!target) return null;
    const buf = term.buffer.active;
    const output = extractOsc133BlockOutput(
      target,
      (row) => buf.getLine(row)?.translateToString(true) ?? "",
      buf.length,
    );
    return { block: target, output };
  }, []);

  const copyOscBlockOutput = useCallback(() => {
    const res = readSelectedOscBlockOutput();
    if (!res || !res.output) {
      orbitActions.showToast(t("terminal.noBlockOutput"));
      return;
    }
    void navigator.clipboard
      ?.writeText(res.output)
      .then(() => {
        setCopiedBlockOutput(true);
        if (copiedBlockTimerRef.current !== null) {
          window.clearTimeout(copiedBlockTimerRef.current);
        }
        copiedBlockTimerRef.current = window.setTimeout(() => setCopiedBlockOutput(false), 1400);
      })
      .catch(() => {});
  }, [readSelectedOscBlockOutput, t]);

  const sendOscBlockToComposer = useCallback(() => {
    if (detached) return;
    const res = readSelectedOscBlockOutput();
    if (!res || !res.output) {
      orbitActions.showToast(t("terminal.noBlockOutput"));
      return;
    }
    const lineCount = res.output.split("\n").length;
    const cmdLabel = res.block.commandLine
      ? `$ ${res.block.commandLine.slice(0, 40)}${res.block.commandLine.length > 40 ? "…" : ""}`
      : paneTitle || t("sidePanel.actionTerminal");
    const content = res.block.commandLine
      ? `$ ${res.block.commandLine}\n${res.output}`
      : res.output;
    orbitActions.addContextMention({
      id: `term_osc_${res.block.id}_${Date.now()}`,
      kind: "terminal_output",
      label: `${cmdLabel} · ${lineCount}L`,
      lineCount,
      content,
    });
    orbitActions.focusComposer();
    orbitActions.showToast(t("terminal.sentOutputToComposer"));
  }, [detached, paneTitle, readSelectedOscBlockOutput, t]);

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
    jumpOscBlock,
  });
  shortcutActionsRef.current = {
    openSearch,
    closeSearch,
    stepSearch,
    jumpOscBlock,
  };

  useEffect(() => {
    const el = containerRef.current;
    if (!el) return;

    const snap = orbitActions.getSnapshot();
    const initialTermTypography = resolveTypography(snap).terminal;
    const term = new Terminal({
      cursorBlink: true,
      cursorStyle: snap.terminalCursorStyle,
      fontFamily: initialTermTypography.fontFamily,
      fontSize: initialTermTypography.fontSize,
      lineHeight: initialTermTypography.lineHeight,
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
        webglAddonRef.current = null;
      });
      term.loadAddon(webglAddon);
      webglAddonRef.current = webglAddon;
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

    // Register FinalTerm / iTerm2 / VSCode OSC 133 shell-integration command-block handler
    const osc133Tracker = createOsc133Tracker({ maxBlocks: 200 });
    osc133TrackerRef.current = osc133Tracker;
    const osc133Disposable = term.parser.registerOscHandler(133, (data) => {
      if (disposed) return true;
      const buf = term.buffer.active;
      const updated = osc133Tracker.handleOsc(
        data,
        { row: buf.baseY + buf.cursorY, col: buf.cursorX },
        (row) => buf.getLine(row)?.translateToString(true) ?? "",
      );
      if (updated) {
        setOscBlocks(updated);
        const latest = updated[updated.length - 1] ?? null;
        setActiveOscBlockId(latest ? latest.id : null);
      }
      return true;
    });

    // Scoped terminal-local shortcuts: Cmd+K (clear), Cmd+Shift/Alt+D (split), Cmd+Up/Down (OSC 133),
    // and Cmd/Ctrl+F, Cmd/Ctrl+G, Esc (search)
    const isMac = isMacDesktop();
    term.attachCustomKeyEventHandler((ev) => {
      if (ev.type !== "keydown") return true;
      if (isTauriRuntime() && matchDesktopZoomAction(ev, isMac)) {
        return false;
      }
      const key = ev.key.toLowerCase();
      if (ev.metaKey && !ev.shiftKey && !ev.altKey && !ev.ctrlKey && key === "k") {
        ev.preventDefault();
        ev.stopPropagation();
        term.clear();
        osc133Tracker.reset();
        setOscBlocks([]);
        setActiveOscBlockId(null);
        return false;
      }
      const paneAction = resolveTerminalPaneShortcut(
        {
          key: ev.key,
          code: ev.code,
          metaKey: ev.metaKey,
          ctrlKey: ev.ctrlKey,
          shiftKey: ev.shiftKey,
          altKey: ev.altKey,
          isComposing: ev.isComposing,
          defaultPrevented: ev.defaultPrevented,
        },
        {
          isMac,
          terminalHasFocus: true,
          findOpen: findOpenRef.current,
          hasCommandBlocks: oscBlocksRef.current.length > 0,
          canSplit: !detached && Boolean(onRequestSplitRef.current),
        },
      );
      if (paneAction === "split_horizontal") {
        ev.preventDefault();
        ev.stopPropagation();
        onRequestSplitRef.current?.("horizontal");
        return false;
      }
      if (paneAction === "split_vertical") {
        ev.preventDefault();
        ev.stopPropagation();
        onRequestSplitRef.current?.("vertical");
        return false;
      }
      if (paneAction === "prev_command") {
        ev.preventDefault();
        ev.stopPropagation();
        shortcutActionsRef.current.jumpOscBlock("prev");
        return false;
      }
      if (paneAction === "next_command") {
        ev.preventDefault();
        ev.stopPropagation();
        shortcutActionsRef.current.jumpOscBlock("next");
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
      osc133Disposable.dispose();
      osc133TrackerRef.current = null;
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
      webglAddonRef.current = null;
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
    term.options.cursorStyle = terminalCursorStyle;
    term.options.scrollback = terminalScrollback;

    const fontChanged =
      term.options.fontFamily !== terminalTypography.fontFamily ||
      term.options.fontSize !== terminalTypography.fontSize;

    if (!fontChanged) {
      if (active && visible) {
        fitRef.current?.fit();
        syncRemoteSize(term);
      }
      return;
    }

    let cancelled = false;
    const applyFontAndRefit = () => {
      if (cancelled || termRef.current !== term) return;
      term.options.fontFamily = terminalTypography.fontFamily;
      term.options.fontSize = terminalTypography.fontSize;
      try {
        webglAddonRef.current?.clearTextureAtlas();
      } catch {
        // Ignore if WebGL addon is unavailable or already disposed
      }
      if (active && visible) {
        fitRef.current?.fit();
        syncRemoteSize(term);
      }
    };

    const fontsReady = typeof document !== "undefined" ? document.fonts?.ready : undefined;
    if (fontsReady) {
      void fontsReady.then(applyFontAndRefit, applyFontAndRefit);
    } else {
      applyFontAndRefit();
    }
    return () => {
      cancelled = true;
    };
  }, [
    resolvedTheme,
    terminalThemeMode,
    terminalTypography.fontFamily,
    terminalTypography.fontSize,
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
        if (isFocusedPane) {
          termRef.current.focus();
        }
      }
    });
    return () => cancelAnimationFrame(id);
  }, [active, isFocusedPane, visible, detached, syncRemoteSize]);

  const handleWrapperKeyDown = useCallback(
    (e: React.KeyboardEvent<HTMLDivElement>) => {
      const wrapper = wrapperRef.current;
      const activeEl = document.activeElement;
      const terminalHasFocus = Boolean(wrapper && activeEl && wrapper.contains(activeEl));
      const inSearchInput = activeEl === findInputRef.current;
      const isMac = /Mac|iPhone|iPad|iPod/i.test(navigator.platform || navigator.userAgent);

      if (!inSearchInput) {
        const paneAction = resolveTerminalPaneShortcut(
          {
            key: e.key,
            code: e.code,
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
            hasCommandBlocks: oscBlocks.length > 0,
            canSplit: !detached && Boolean(onRequestSplit),
          },
        );
        if (paneAction) {
          e.preventDefault();
          e.stopPropagation();
          if (paneAction === "split_horizontal") {
            onRequestSplit?.("horizontal");
          } else if (paneAction === "split_vertical") {
            onRequestSplit?.("vertical");
          } else if (paneAction === "prev_command") {
            jumpOscBlock("prev");
          } else if (paneAction === "next_command") {
            jumpOscBlock("next");
          }
          return;
        }
      }

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
    [
      findOpen,
      openSearch,
      stepSearch,
      closeSearch,
      oscBlocks.length,
      detached,
      onRequestSplit,
      jumpOscBlock,
    ],
  );

  const matchCountLabel =
    findQuery.length === 0
      ? ""
      : findMatches.length > 0
        ? `${findMatchIdx + 1} / ${findMatches.length}${searchTruncated ? "+" : ""}`
        : "0 / 0";

  const showMicroHeader = isSplit || oscBlocks.length > 0;
  const jumpPrevShortcut = mod === "⌘" ? "⌘↑" : "Ctrl+Alt+↑";
  const jumpNextShortcut = mod === "⌘" ? "⌘↓" : "Ctrl+Alt+↓";

  return (
    <div
      ref={wrapperRef}
      onKeyDown={handleWrapperKeyDown}
      onMouseDown={() => {
        if (!isFocusedPane) {
          onPaneFocusRef.current?.();
        }
      }}
      onFocusCapture={() => {
        isFocusedRef.current = true;
        if (!isFocusedPane) {
          onPaneFocusRef.current?.();
        }
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
        "relative flex h-full w-full flex-col overflow-hidden",
        terminalThemeMode === "high_contrast"
          ? "bg-white dark:bg-[#0C0D12]"
          : "bg-surface-recessed",
        isSplit && (isFocusedPane ? "ring-border-strong ring-1 ring-inset" : "opacity-[0.94]"),
        !active && "hidden",
      )}
    >
      {showMicroHeader && (
        <div className="border-border-subtle bg-surface-base/95 flex h-[26px] shrink-0 items-center justify-between gap-2 border-b px-2 text-[11px] select-none">
          <div className="flex min-w-0 items-center gap-1.5">
            {isSplit && (
              <>
                <span
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    isFocusedPane ? "bg-signal-success" : "bg-ink-faint",
                  )}
                />
                {paneTitle && (
                  <span className="text-ink-secondary max-w-36 truncate font-mono font-medium">
                    {paneTitle}
                  </span>
                )}
              </>
            )}

            {selectedOscBlock && (
              <div className="flex min-w-0 items-center gap-1.5">
                {isSplit && <span className="text-ink-faint">·</span>}
                <span
                  aria-label={
                    selectedOscBlock.exitCode === null
                      ? t("terminal.blockRunning")
                      : selectedOscBlock.exitCode === 0
                        ? t("terminal.blockExitOk")
                        : t("terminal.blockExitFailed", { code: selectedOscBlock.exitCode })
                  }
                  className={cn(
                    "size-1.5 shrink-0 rounded-full",
                    selectedOscBlock.exitCode === null
                      ? "bg-ink-muted"
                      : selectedOscBlock.exitCode === 0
                        ? "bg-signal-success"
                        : "bg-signal-danger",
                  )}
                />
                {selectedOscBlock.commandLine && (
                  <span
                    title={selectedOscBlock.commandLine}
                    className="text-ink-muted max-w-44 truncate font-mono"
                  >
                    {selectedOscBlock.commandLine}
                  </span>
                )}
                {selectedOscBlock.exitCode !== null && selectedOscBlock.exitCode !== 0 && (
                  <span className="text-signal-danger shrink-0 font-mono tabular-nums">
                    exit {selectedOscBlock.exitCode}
                  </span>
                )}
              </div>
            )}
          </div>

          <div className="flex shrink-0 items-center gap-0.5">
            {oscBlocks.length > 0 && (
              <>
                <span className="text-ink-faint mr-0.5 font-mono tabular-nums">
                  {selectedOscBlockIndex + 1}/{oscBlocks.length}
                </span>
                <SimpleTooltip label={t("terminal.prevCommand")} shortcut={jumpPrevShortcut}>
                  <button
                    type="button"
                    aria-label={t("terminal.prevCommand")}
                    disabled={selectedOscBlockIndex <= 0}
                    onClick={() => jumpOscBlock("prev")}
                    className="text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-5 cursor-pointer items-center justify-center rounded transition-colors disabled:cursor-default disabled:opacity-35"
                  >
                    <ChevronUp className="size-3.5" />
                  </button>
                </SimpleTooltip>
                <SimpleTooltip label={t("terminal.nextCommand")} shortcut={jumpNextShortcut}>
                  <button
                    type="button"
                    aria-label={t("terminal.nextCommand")}
                    disabled={selectedOscBlockIndex >= oscBlocks.length - 1}
                    onClick={() => jumpOscBlock("next")}
                    className="text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-5 cursor-pointer items-center justify-center rounded transition-colors disabled:cursor-default disabled:opacity-35"
                  >
                    <ChevronDown className="size-3.5" />
                  </button>
                </SimpleTooltip>
                <SimpleTooltip label={t("terminal.copyLastOutput")}>
                  <button
                    type="button"
                    aria-label={t("terminal.copyLastOutput")}
                    onClick={copyOscBlockOutput}
                    className="text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-5 cursor-pointer items-center justify-center rounded transition-colors"
                  >
                    {copiedBlockOutput ? (
                      <Check className="text-signal-success size-3" />
                    ) : (
                      <Copy className="size-3" />
                    )}
                  </button>
                </SimpleTooltip>
                {!detached && (
                  <SimpleTooltip label={t("terminal.sendOutputToComposer")}>
                    <button
                      type="button"
                      aria-label={t("terminal.sendOutputToComposer")}
                      onClick={sendOscBlockToComposer}
                      className="text-ink-muted hover:bg-accent hover:text-ink-primary inline-flex size-5 cursor-pointer items-center justify-center rounded transition-colors"
                    >
                      <CornerUpLeft className="size-3" />
                    </button>
                  </SimpleTooltip>
                )}
              </>
            )}

            {isSplit && onRequestClosePane && (
              <SimpleTooltip label={t("terminal.closePane")}>
                <button
                  type="button"
                  aria-label={t("terminal.closePane")}
                  onClick={(e) => {
                    e.stopPropagation();
                    onRequestClosePane();
                  }}
                  className="text-ink-muted hover:bg-accent hover:text-ink-primary ml-0.5 inline-flex size-5 cursor-pointer items-center justify-center rounded transition-colors"
                >
                  <X className="size-3" />
                </button>
              </SimpleTooltip>
            )}
          </div>
        </div>
      )}

      <div className="relative min-h-0 flex-1 overflow-hidden px-3.5 py-2.5">
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
    </div>
  );
}

export default XtermPane;
