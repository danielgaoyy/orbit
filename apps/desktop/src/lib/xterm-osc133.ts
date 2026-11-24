export const MAX_OSC133_BLOCKS = 200;

export type Osc133MarkerKind = "A" | "B" | "C" | "D" | "P";

export interface Osc133Event {
  kind: Osc133MarkerKind;
  exitCode: number | null;
}

export interface Osc133CommandBlock {
  id: number;
  commandLine: string;
  promptRow: number;
  commandStartRow: number | null;
  commandStartCol: number | null;
  outputStartRow: number | null;
  endRow: number | null;
  exitCode: number | null;
  startedAt: number;
  finishedAt: number | null;
}

export interface Osc133CursorPos {
  /** 0-based absolute buffer row (`buffer.active.baseY + buffer.active.cursorY`) */
  row: number;
  /** 0-based column (`buffer.active.cursorX`) */
  col: number;
}

export interface Osc133Tracker {
  handleOsc(
    data: string,
    cursor: Osc133CursorPos,
    readLine?: (row: number) => string,
  ): Osc133CommandBlock[] | null;
  getBlocks(): readonly Osc133CommandBlock[];
  reset(): void;
}

export type TerminalPaneShortcutAction =
  | "split_horizontal"
  | "split_vertical"
  | "prev_command"
  | "next_command"
  | null;

export interface TerminalPaneShortcutEventLike {
  key: string;
  code?: string;
  metaKey: boolean;
  ctrlKey: boolean;
  shiftKey: boolean;
  altKey: boolean;
  isComposing?: boolean;
  defaultPrevented?: boolean;
}

export interface TerminalPaneShortcutContext {
  isMac: boolean;
  terminalHasFocus: boolean;
  findOpen: boolean;
  hasCommandBlocks: boolean;
  canSplit: boolean;
}

/**
 * Parse a FinalTerm / iTerm2 / VSCode OSC 133 payload (the string delivered by
 * `term.parser.registerOscHandler(133, cb)` after `\x1b]133;`).
 *
 * Supported sequences:
 * - `A` (prompt start)
 * - `B` (command input start)
 * - `C` (command execution / output start)
 * - `D` or `D;<exit_code>[;...]` (command finished)
 * - `P;...` (shell property metadata, ignored cleanly)
 */
export function parseOsc133Payload(rawData: string): Osc133Event | null {
  if (!rawData) return null;
  const parts = rawData.split(";");
  const code = parts[0]?.trim();
  if (code !== "A" && code !== "B" && code !== "C" && code !== "D" && code !== "P") {
    return null;
  }
  if (code !== "D") {
    return { kind: code, exitCode: null };
  }
  const rawExit = parts[1]?.trim() ?? "";
  if (rawExit.length > 0 && /^-?\d+$/.test(rawExit)) {
    const parsed = Number.parseInt(rawExit, 10);
    if (Number.isFinite(parsed)) {
      return { kind: "D", exitCode: parsed };
    }
  }
  return { kind: "D", exitCode: null };
}

function readCommandLineFromBuffer(
  startRow: number,
  startCol: number,
  endRow: number,
  readLine?: (row: number) => string,
): string {
  if (!readLine) return "";
  const safeStart = Math.max(0, startRow);
  const safeEnd = Math.max(safeStart, endRow);
  const maxSpan = Math.min(safeEnd, safeStart + 8);
  const segments: string[] = [];
  for (let r = safeStart; r <= maxSpan; r += 1) {
    const line = readLine(r) ?? "";
    const slice = r === safeStart && startCol > 0 ? line.slice(startCol) : line;
    const trimmed = slice.trim();
    if (trimmed.length > 0) {
      segments.push(trimmed);
    }
  }
  return segments.join(" ").trim();
}

/**
 * Stateful OSC 133 command-block tracker with a bounded ring buffer (`maxBlocks`, default 200).
 * Returns a fresh immutable snapshot array whenever tracked blocks mutate so callers can sync
 * React UI state without unnecessary renders on intermediate non-visual sequences.
 */
export function createOsc133Tracker(opts?: {
  maxBlocks?: number;
  now?: () => number;
}): Osc133Tracker {
  const maxBlocks = Math.max(1, opts?.maxBlocks ?? MAX_OSC133_BLOCKS);
  const now = opts?.now ?? (() => Date.now());
  let blocks: Osc133CommandBlock[] = [];
  let pendingPromptRow: number | null = null;
  let pendingCmdRow: number | null = null;
  let pendingCmdCol: number | null = null;
  let activeBlock: Osc133CommandBlock | null = null;
  let nextId = 1;

  const pushBlock = (block: Osc133CommandBlock): Osc133CommandBlock[] => {
    blocks = [...blocks, block];
    if (blocks.length > maxBlocks) {
      blocks = blocks.slice(blocks.length - maxBlocks);
    }
    return blocks;
  };

  const replaceActive = (updated: Osc133CommandBlock): Osc133CommandBlock[] => {
    const idx = blocks.findIndex((b) => b.id === updated.id);
    if (idx >= 0) {
      const next = blocks.slice();
      next[idx] = updated;
      blocks = next;
    } else {
      pushBlock(updated);
    }
    return blocks;
  };

  return {
    handleOsc(
      data: string,
      cursor: Osc133CursorPos,
      readLine?: (row: number) => string,
    ): Osc133CommandBlock[] | null {
      const ev = parseOsc133Payload(data);
      if (!ev || ev.kind === "P") return null;
      const row = Math.max(0, Math.floor(cursor.row));
      const col = Math.max(0, Math.floor(cursor.col));

      if (ev.kind === "A") {
        let changed = false;
        // If the previous command never emitted `133;D` before the next prompt, close it cleanly.
        if (activeBlock && activeBlock.finishedAt === null) {
          const closed: Osc133CommandBlock = {
            ...activeBlock,
            endRow: Math.max(activeBlock.outputStartRow ?? activeBlock.promptRow, row - 1),
            finishedAt: now(),
          };
          replaceActive(closed);
          activeBlock = null;
          changed = true;
        }
        pendingPromptRow = row;
        pendingCmdRow = null;
        pendingCmdCol = null;
        return changed ? blocks : null;
      }

      if (ev.kind === "B") {
        if (pendingPromptRow === null) {
          pendingPromptRow = row;
        }
        pendingCmdRow = row;
        pendingCmdCol = col;
        return null;
      }

      if (ev.kind === "C") {
        const promptRow = pendingPromptRow ?? row;
        const cmdStartRow = pendingCmdRow ?? promptRow;
        const cmdStartCol = pendingCmdCol ?? 0;
        // Capture command text typed between B and C (C typically fires right after Enter,
        // so the command text may sit on `cmdStartRow .. row` or `row - 1`).
        const cmdEndRow = row > cmdStartRow ? row - 1 : row;
        const commandLine = readCommandLineFromBuffer(
          cmdStartRow,
          cmdStartCol,
          cmdEndRow,
          readLine,
        );
        const created: Osc133CommandBlock = {
          id: nextId++,
          commandLine,
          promptRow,
          commandStartRow: cmdStartRow,
          commandStartCol: cmdStartCol,
          outputStartRow: row,
          endRow: null,
          exitCode: null,
          startedAt: now(),
          finishedAt: null,
        };
        activeBlock = created;
        pendingPromptRow = null;
        pendingCmdRow = null;
        pendingCmdCol = null;
        return pushBlock(created);
      }

      // `ev.kind === "D"`
      if (activeBlock) {
        const finished: Osc133CommandBlock = {
          ...activeBlock,
          endRow: Math.max(activeBlock.outputStartRow ?? activeBlock.promptRow, row),
          exitCode: ev.exitCode,
          finishedAt: now(),
        };
        activeBlock = null;
        return replaceActive(finished);
      }

      // Bare `133;D` after an empty prompt (e.g. pressing Enter with no `C`) — skip empty no-op
      // unless a prompt existed and exitCode is explicitly non-zero.
      if (pendingPromptRow !== null && ev.exitCode !== null && ev.exitCode !== 0) {
        const synthetic: Osc133CommandBlock = {
          id: nextId++,
          commandLine: readCommandLineFromBuffer(
            pendingCmdRow ?? pendingPromptRow,
            pendingCmdCol ?? 0,
            row,
            readLine,
          ),
          promptRow: pendingPromptRow,
          commandStartRow: pendingCmdRow,
          commandStartCol: pendingCmdCol,
          outputStartRow: row,
          endRow: row,
          exitCode: ev.exitCode,
          startedAt: now(),
          finishedAt: now(),
        };
        pendingPromptRow = null;
        pendingCmdRow = null;
        pendingCmdCol = null;
        return pushBlock(synthetic);
      }

      pendingPromptRow = null;
      pendingCmdRow = null;
      pendingCmdCol = null;
      return null;
    },

    getBlocks(): readonly Osc133CommandBlock[] {
      return blocks;
    },

    reset(): void {
      blocks = [];
      pendingPromptRow = null;
      pendingCmdRow = null;
      pendingCmdCol = null;
      activeBlock = null;
    },
  };
}

/**
 * Find the previous or next command block relative to an optional selected block ID or viewport row.
 */
export function findAdjacentOsc133Block(
  blocks: readonly Osc133CommandBlock[],
  currentViewportRow: number,
  direction: "prev" | "next",
  selectedBlockId?: number | null,
): Osc133CommandBlock | null {
  if (blocks.length === 0) return null;

  if (selectedBlockId !== undefined && selectedBlockId !== null) {
    const curIdx = blocks.findIndex((b) => b.id === selectedBlockId);
    if (curIdx >= 0) {
      const nextIdx = direction === "prev" ? curIdx - 1 : curIdx + 1;
      if (nextIdx >= 0 && nextIdx < blocks.length) {
        return blocks[nextIdx];
      }
      return blocks[curIdx];
    }
  }

  if (direction === "prev") {
    for (let i = blocks.length - 1; i >= 0; i -= 1) {
      if (blocks[i].promptRow < currentViewportRow) {
        return blocks[i];
      }
    }
    return blocks[0];
  }

  for (let i = 0; i < blocks.length; i += 1) {
    if (blocks[i].promptRow > currentViewportRow) {
      return blocks[i];
    }
  }
  return blocks[blocks.length - 1];
}

/**
 * Extract the clean text output of a tracked OSC 133 command block from an xterm buffer.
 */
export function extractOsc133BlockOutput(
  block: Osc133CommandBlock,
  readLine: (row: number) => string,
  bufferLength: number,
  maxLines = 200,
): string {
  if (bufferLength <= 0) return "";
  const start = Math.max(0, block.outputStartRow ?? block.promptRow);
  const rawEnd = block.endRow !== null ? Math.min(block.endRow, bufferLength - 1) : bufferLength - 1;
  if (start > rawEnd) return "";

  const lines: string[] = [];
  for (let row = start; row <= rawEnd; row += 1) {
    lines.push((readLine(row) ?? "").trimEnd());
  }

  // Trim leading/trailing blank rows introduced by the shell newline before prompt redraw
  while (lines.length > 0 && lines[0].length === 0) {
    lines.shift();
  }
  while (lines.length > 0 && lines[lines.length - 1].length === 0) {
    lines.pop();
  }

  // If the first captured row is identical to the echoed command line, drop that duplicate echo
  if (
    lines.length > 1 &&
    block.commandLine &&
    lines[0].trim() === block.commandLine.trim()
  ) {
    lines.shift();
  }

  if (lines.length > maxLines) {
    return lines.slice(-maxLines).join("\n");
  }
  return lines.join("\n");
}

/**
 * Pure keyboard shortcut resolver for focused terminal panes:
 * - `Cmd+Shift+D` (macOS) / `Ctrl+Shift+D` (Win/Linux): Split Right (`split_horizontal`)
 * - `Cmd+Alt+D` (macOS) / `Ctrl+Alt+D` (Win/Linux): Split Down (`split_vertical`)
 * - `Cmd+ArrowUp` / `Cmd+ArrowDown` (macOS) or `Ctrl+Alt+ArrowUp` / `Ctrl+Alt+ArrowDown` (Win/Linux):
 *   Jump between OSC 133 command blocks.
 */
export function resolveTerminalPaneShortcut(
  event: TerminalPaneShortcutEventLike,
  ctx: TerminalPaneShortcutContext,
): TerminalPaneShortcutAction {
  if (event.defaultPrevented || event.isComposing || !ctx.terminalHasFocus || ctx.findOpen) {
    return null;
  }
  const mod = ctx.isMac ? event.metaKey : event.ctrlKey;
  const otherMod = ctx.isMac ? event.ctrlKey : event.metaKey;
  const keyLower = event.key.toLowerCase();
  const isDKey = keyLower === "d" || event.code === "KeyD";

  if (ctx.canSplit && mod && !otherMod && isDKey) {
    if (event.shiftKey && !event.altKey) {
      return "split_horizontal";
    }
    if (event.altKey && !event.shiftKey) {
      return "split_vertical";
    }
  }

  if (ctx.hasCommandBlocks) {
    const jumpMod = ctx.isMac
      ? event.metaKey && !event.ctrlKey && !event.altKey && !event.shiftKey
      : event.ctrlKey && event.altKey && !event.metaKey && !event.shiftKey;
    if (jumpMod && event.key === "ArrowUp") {
      return "prev_command";
    }
    if (jumpMod && event.key === "ArrowDown") {
      return "next_command";
    }
  }

  return null;
}

/**
 * Clamp and redistribute split ratios across `paneCount` panes so every pane stays >= `minRatio`
 * and all ratios sum to `1`.
 */
export function adjustSplitPaneRatios(
  ratios: readonly number[],
  dividerIndex: number,
  deltaRatio: number,
  minRatio = 0.18,
): number[] {
  if (ratios.length < 2 || dividerIndex < 0 || dividerIndex >= ratios.length - 1) {
    return [...ratios];
  }
  const next = ratios.slice();
  const left = next[dividerIndex];
  const right = next[dividerIndex + 1];
  const pairSum = left + right;
  const effectiveMin = Math.min(minRatio, pairSum / 2);
  const nextLeft = Math.min(pairSum - effectiveMin, Math.max(effectiveMin, left + deltaRatio));
  next[dividerIndex] = nextLeft;
  next[dividerIndex + 1] = pairSum - nextLeft;
  return next;
}
