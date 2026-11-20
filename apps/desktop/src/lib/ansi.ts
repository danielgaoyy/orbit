export interface AnsiSpanStyle {
  /** Standard ANSI 0-15 index (resolved per light/dark theme) or CSS color string for 256/truecolor */
  fg?: number | string;
  bg?: number | string;
  bold?: boolean;
  dim?: boolean;
  italic?: boolean;
  underline?: boolean;
  strike?: boolean;
}

export interface AnsiSpan {
  text: string;
  style: AnsiSpanStyle | null;
}

export interface ParsedAnsiLogLine {
  plain: string;
  /** `null` when the entire line has no ANSI styling (fast path renders plain text directly). */
  spans: AnsiSpan[] | null;
}

/**
 * Restrained 16-color terminal palettes tuned for crisp contrast in both light & dark modes
 * (indices 0-7 = normal, 8-15 = bright).
 */
const ANSI_16_LIGHT: readonly string[] = [
  "#27272a", // black
  "#dc2626", // red
  "#15803d", // green
  "#a16207", // yellow
  "#2563eb", // blue
  "#9333ea", // magenta
  "#0e7490", // cyan
  "#52525b", // white (muted slate on light canvas)
  "#71717a", // bright black (gray)
  "#ef4444", // bright red
  "#16a34a", // bright green
  "#ca8a04", // bright yellow
  "#3b82f6", // bright blue
  "#a855f7", // bright magenta
  "#0891b2", // bright cyan
  "#18181b", // bright white (strong ink on light canvas)
];

const ANSI_16_DARK: readonly string[] = [
  "#52525b", // black
  "#f87171", // red
  "#4ade80", // green
  "#facc15", // yellow
  "#60a5fa", // blue
  "#c084fc", // magenta
  "#22d3ee", // cyan
  "#d4d4d8", // white
  "#71717a", // bright black (gray)
  "#fca5a5", // bright red
  "#86efac", // bright green
  "#fde047", // bright yellow
  "#93c5fd", // bright blue
  "#d8b4fe", // bright magenta
  "#67e8f9", // bright cyan
  "#fafafa", // bright white
];

const XTERM_CUBE = [0, 95, 135, 175, 215, 255] as const;

function xterm256ToColor(code: number): number | string | undefined {
  if (code < 0 || code > 255) return undefined;
  if (code < 16) return code;
  if (code < 232) {
    const idx = code - 16;
    const r = XTERM_CUBE[Math.floor(idx / 36)];
    const g = XTERM_CUBE[Math.floor((idx % 36) / 6)];
    const b = XTERM_CUBE[idx % 6];
    return `rgb(${r},${g},${b})`;
  }
  const gray = 8 + (code - 232) * 10;
  return `rgb(${gray},${gray},${gray})`;
}

export function resolveAnsiColor(
  color: number | string | undefined,
  isDark: boolean,
): string | undefined {
  if (color === undefined) return undefined;
  if (typeof color === "string") return color;
  return isDark ? ANSI_16_DARK[color] : ANSI_16_LIGHT[color];
}

function isEmptyStyle(s: AnsiSpanStyle): boolean {
  return (
    s.fg === undefined &&
    s.bg === undefined &&
    !s.bold &&
    !s.dim &&
    !s.italic &&
    !s.underline &&
    !s.strike
  );
}

function cloneStyle(s: AnsiSpanStyle): AnsiSpanStyle {
  const out: AnsiSpanStyle = {};
  if (s.fg !== undefined) out.fg = s.fg;
  if (s.bg !== undefined) out.bg = s.bg;
  if (s.bold) out.bold = true;
  if (s.dim) out.dim = true;
  if (s.italic) out.italic = true;
  if (s.underline) out.underline = true;
  if (s.strike) out.strike = true;
  return out;
}

function applySgrParams(paramsRaw: string, style: AnsiSpanStyle): AnsiSpanStyle {
  if (paramsRaw.length === 0) return {};
  const parts = paramsRaw.split(";");
  let next = cloneStyle(style);
  for (let i = 0; i < parts.length; i += 1) {
    const code = parts[i].length === 0 ? 0 : Number.parseInt(parts[i], 10);
    if (Number.isNaN(code)) continue;
    if (code === 0) {
      next = {};
    } else if (code === 1) {
      next.bold = true;
    } else if (code === 2) {
      next.dim = true;
    } else if (code === 3) {
      next.italic = true;
    } else if (code === 4) {
      next.underline = true;
    } else if (code === 9) {
      next.strike = true;
    } else if (code === 22) {
      delete next.bold;
      delete next.dim;
    } else if (code === 23) {
      delete next.italic;
    } else if (code === 24) {
      delete next.underline;
    } else if (code === 29) {
      delete next.strike;
    } else if (code >= 30 && code <= 37) {
      next.fg = code - 30;
    } else if (code === 39) {
      delete next.fg;
    } else if (code >= 40 && code <= 47) {
      next.bg = code - 40;
    } else if (code === 49) {
      delete next.bg;
    } else if (code >= 90 && code <= 97) {
      next.fg = code - 90 + 8;
    } else if (code >= 100 && code <= 107) {
      next.bg = code - 100 + 8;
    } else if (code === 38 || code === 48) {
      const isFg = code === 38;
      const mode = Number.parseInt(parts[i + 1] ?? "", 10);
      if (mode === 5 && i + 2 < parts.length) {
        const idx = Number.parseInt(parts[i + 2], 10);
        const resolved = xterm256ToColor(idx);
        if (resolved !== undefined) {
          if (isFg) next.fg = resolved;
          else next.bg = resolved;
        }
        i += 2;
      } else if (mode === 2 && i + 4 < parts.length) {
        const r = Math.max(0, Math.min(255, Number.parseInt(parts[i + 2], 10) || 0));
        const g = Math.max(0, Math.min(255, Number.parseInt(parts[i + 3], 10) || 0));
        const b = Math.max(0, Math.min(255, Number.parseInt(parts[i + 4], 10) || 0));
        const rgb = `rgb(${r},${g},${b})`;
        if (isFg) next.fg = rgb;
        else next.bg = rgb;
        i += 4;
      }
    }
  }
  return next;
}

function collapseCarriageReturn(line: string): string {
  if (!line.includes("\r")) return line;
  const parts = line.split("\r").filter((p) => p.length > 0);
  return parts.length > 0 ? parts[parts.length - 1] : "";
}

function parseSingleAnsiLine(
  line: string,
  state: { style: AnsiSpanStyle },
): ParsedAnsiLogLine {
  const hasEsc = line.includes("\x1b");
  const activeStyleEmpty = isEmptyStyle(state.style);

  // Zero-allocation fast path for unstyled plain lines
  if (!hasEsc && activeStyleEmpty) {
    return { plain: line, spans: null };
  }
  if (!hasEsc) {
    return {
      plain: line,
      spans: [{ text: line, style: cloneStyle(state.style) }],
    };
  }

  const spans: AnsiSpan[] = [];
  let plain = "";
  let hasStyledSpan = false;
  let i = 0;
  const len = line.length;

  const pushText = (chunk: string) => {
    if (!chunk) return;
    plain += chunk;
    const styled = !isEmptyStyle(state.style);
    if (styled) hasStyledSpan = true;
    const prev = spans.length > 0 ? spans[spans.length - 1] : null;
    if (!styled && prev && prev.style === null) {
      prev.text += chunk;
    } else {
      spans.push({
        text: chunk,
        style: styled ? cloneStyle(state.style) : null,
      });
    }
  };

  while (i < len) {
    const escIdx = line.indexOf("\x1b", i);
    if (escIdx === -1) {
      pushText(line.slice(i));
      break;
    }
    if (escIdx > i) {
      pushText(line.slice(i, escIdx));
    }

    const nextCh = line.charCodeAt(escIdx + 1);
    if (nextCh === 0x5b /* '[' CSI */) {
      let j = escIdx + 2;
      while (j < len) {
        const c = line.charCodeAt(j);
        if (c >= 0x40 && c <= 0x7e) break;
        j += 1;
      }
      if (j < len) {
        if (line.charCodeAt(j) === 0x6d /* 'm' SGR */) {
          state.style = applySgrParams(line.slice(escIdx + 2, j), state.style);
        }
        i = j + 1;
        continue;
      }
      break;
    }

    if (nextCh === 0x5d /* ']' OSC */) {
      let j = escIdx + 2;
      while (j < len) {
        const c = line.charCodeAt(j);
        if (c === 0x07 /* BEL */) {
          j += 1;
          break;
        }
        if (c === 0x1b && line.charCodeAt(j + 1) === 0x5c /* ST */) {
          j += 2;
          break;
        }
        j += 1;
      }
      i = j;
      continue;
    }

    // Skip single-character ESC sequence
    i = escIdx + 2;
  }

  return {
    plain,
    spans: hasStyledSpan ? spans : null,
  };
}

/**
 * Parse raw terminal/task log output into normalized lines with native ANSI SGR styling.
 * Keeps O(N) fast path with zero regex / zero span allocation when no ESC sequences exist.
 */
export function parseAnsiLogLines(raw: string): ParsedAnsiLogLine[] {
  const rawLines = raw.split("\n");
  const hasAnyEsc = raw.includes("\x1b");
  const state: { style: AnsiSpanStyle } = { style: {} };

  const parsed: ParsedAnsiLogLine[] = new Array(rawLines.length);
  for (let i = 0; i < rawLines.length; i += 1) {
    const frame = collapseCarriageReturn(rawLines[i]);
    parsed[i] = hasAnyEsc
      ? parseSingleAnsiLine(frame, state)
      : { plain: frame, spans: null };
  }

  // Trim leading and trailing blank lines
  let start = 0;
  while (start < parsed.length && parsed[start].plain.trim().length === 0) {
    start += 1;
  }
  let end = parsed.length;
  while (end > start && parsed[end - 1].plain.trim().length === 0) {
    end -= 1;
  }

  // Collapse runs of multiple empty lines to at most one spacer
  const out: ParsedAnsiLogLine[] = [];
  let prevBlank = false;
  for (let i = start; i < end; i += 1) {
    const isBlank = parsed[i].plain.trim().length === 0;
    if (isBlank && prevBlank) continue;
    out.push(isBlank ? { plain: "", spans: null } : parsed[i]);
    prevBlank = isBlank;
  }
  return out;
}
