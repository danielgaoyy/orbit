export const ZOOM_MIN_PERCENT = 80;
export const ZOOM_MAX_PERCENT = 150;
export const ZOOM_STEP_PERCENT = 10;
export const ZOOM_DEFAULT_PERCENT = 100;
export const ZOOM_STEPS = [80, 90, 100, 110, 120, 130, 140, 150] as const;

export const READ_SIZES = [13, 14, 15, 16, 17, 18, 20, 22] as const;
export const CODE_SIZES = [11, 12, 13, 14, 15, 16, 18] as const;
export const DEFAULT_READ_SIZE = 15;
export const DEFAULT_CODE_SIZE = 13;

export const TYPOGRAPHY_SNAPSHOT_STORAGE_KEY = "orbit.desktop.typography.css.v1";
export const ZOOM_STORAGE_KEY = "orbit.desktop.zoom.v1";

/** Exact default body font stack from `src/index.css`. */
export const ORBIT_UI_DEFAULT_STACK =
  '-apple-system, BlinkMacSystemFont, "SF Pro Text", "PingFang SC", "Hiragino Sans GB", "Inter", "Segoe UI", Roboto, sans-serif';

/** Exact default CodeMirror / code monospace stack from `src/lib/codemirror-theme.ts`. */
export const ORBIT_CODE_DEFAULT_STACK =
  '"JetBrains Mono", "SF Mono", ui-monospace, SFMono-Regular, Menlo, Monaco, Consolas, monospace';

/** Exact default xterm.js font stack from `src/components/xterm-pane.tsx` (kept separate per D6=A). */
export const ORBIT_TERMINAL_DEFAULT_STACK =
  '"JetBrains Mono", "SF Mono", Menlo, Monaco, Consolas, monospace';

export const ORBIT_CODE_CJK_FALLBACK =
  '"PingFang SC", "Hiragino Sans", "Microsoft YaHei UI", "Noto Sans CJK SC"';

export const ORBIT_SERIF_STACK =
  'ui-serif, "New York", Georgia, "Songti SC", "Hiragino Mincho ProN", "Noto Serif CJK SC", serif';

export type FontSlot = "ui" | "content" | "code";

export interface FontPreset {
  readonly id: string;
  readonly kind: FontSlot;
  readonly family: string | null;
}

export const UI_FONT_PRESETS: readonly FontPreset[] = [
  { id: "default", kind: "ui", family: null },
  { id: "inter", kind: "ui", family: "Inter" },
  { id: "helvetica-neue", kind: "ui", family: "Helvetica Neue" },
  { id: "segoe-ui", kind: "ui", family: "Segoe UI" },
  { id: "ibm-plex-sans", kind: "ui", family: "IBM Plex Sans" },
] as const;

export const CONTENT_FONT_PRESETS: readonly FontPreset[] = [
  { id: "match-ui", kind: "content", family: null },
  { id: "inter", kind: "content", family: "Inter" },
  { id: "system-serif", kind: "content", family: null },
] as const;

export const CODE_FONT_PRESETS: readonly FontPreset[] = [
  { id: "default", kind: "code", family: null },
  { id: "sf-mono", kind: "code", family: "SF Mono" },
  { id: "menlo", kind: "code", family: "Menlo" },
  { id: "jetbrains-mono", kind: "code", family: "JetBrains Mono" },
  { id: "fira-code", kind: "code", family: "Fira Code" },
  { id: "cascadia-code", kind: "code", family: "Cascadia Code" },
  { id: "consolas", kind: "code", family: "Consolas" },
  { id: "ibm-plex-mono", kind: "code", family: "IBM Plex Mono" },
] as const;

export const FONT_PRESETS_BY_SLOT: Readonly<Record<FontSlot, readonly FontPreset[]>> = {
  ui: UI_FONT_PRESETS,
  content: CONTENT_FONT_PRESETS,
  code: CODE_FONT_PRESETS,
};

export const DEFAULT_FONT_ID_BY_SLOT: Readonly<Record<FontSlot, string>> = {
  ui: "default",
  content: "match-ui",
  code: "default",
};

const READ_SIZE_SET: ReadonlySet<number> = new Set(READ_SIZES);
const CODE_SIZE_SET: ReadonlySet<number> = new Set(CODE_SIZES);

export function normalizeConversationFontSize(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && READ_SIZE_SET.has(value)
    ? value
    : DEFAULT_READ_SIZE;
}

export function normalizeCodeFontSize(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) && CODE_SIZE_SET.has(value)
    ? value
    : DEFAULT_CODE_SIZE;
}

export function normalizeFontId(slot: FontSlot, value: unknown): string {
  if (typeof value !== "string") return DEFAULT_FONT_ID_BY_SLOT[slot];
  if (value === "custom") return "custom";
  const presets = FONT_PRESETS_BY_SLOT[slot];
  return presets.some((p) => p.id === value) ? value : DEFAULT_FONT_ID_BY_SLOT[slot];
}

/**
 * Strips CSS/HTML-breaking punctuation, control characters, and Unicode line separators,
 * collapses whitespace, and caps length at 64 characters. Returns `null` when empty.
 */
export function sanitizeFontName(raw: unknown): string | null {
  if (typeof raw !== "string") return null;
  const cleaned = raw
    .replace(/["'\\;{}<>]/g, "")
    // Strip ASCII/C0/C1 control characters and Unicode line/paragraph separators
    // eslint-disable-next-line no-control-regex
    .replace(/[\u0000-\u001F\u007F-\u009F\u2028\u2029]/g, " ")
    .replace(/\s+/g, " ")
    .trim()
    .slice(0, 64)
    .trim();
  return cleaned.length > 0 ? cleaned : null;
}

const fontInstalledCache = new Map<string, boolean>();
const FONT_PROBE_TEXT = "mmmmmmmmmmlli10OO";
const FONT_PROBE_FALLBACKS = ["monospace", "serif", "sans-serif"] as const;

let sharedProbeCanvas: HTMLCanvasElement | null = null;

function getFontProbeContext(): CanvasRenderingContext2D | null {
  if (typeof document === "undefined") return null;
  if (!sharedProbeCanvas) {
    sharedProbeCanvas = document.createElement("canvas");
  }
  try {
    return sharedProbeCanvas.getContext("2d");
  } catch {
    return null;
  }
}

/**
 * Detects whether a local font family is installed via Canvas 2D glyph width comparison
 * against generic `monospace`, `serif`, and `sans-serif` fallbacks.
 * Works consistently across WebKit/Tauri and Web browsers where `document.fonts.check` is unreliable.
 */
export function isFontInstalled(rawFamily: string): boolean {
  const family = sanitizeFontName(rawFamily);
  if (!family) return false;

  const cached = fontInstalledCache.get(family);
  if (cached !== undefined) return cached;

  const ctx = getFontProbeContext();
  if (!ctx) return true;

  let installed = false;
  for (const fallback of FONT_PROBE_FALLBACKS) {
    ctx.font = `72px ${fallback}`;
    const baselineWidth = ctx.measureText(FONT_PROBE_TEXT).width;
    ctx.font = `72px "${family}", ${fallback}`;
    const testWidth = ctx.measureText(FONT_PROBE_TEXT).width;
    if (Math.abs(testWidth - baselineWidth) > 0.01) {
      installed = true;
      break;
    }
  }

  fontInstalledCache.set(family, installed);
  return installed;
}

export function resolveFontPreviewStack(slot: FontSlot, presetId: string, custom?: string | null): string {
  if (slot === "ui") {
    if (presetId === "default") return ORBIT_UI_DEFAULT_STACK;
    if (presetId === "custom") {
      const clean = sanitizeFontName(custom);
      return clean ? `"${clean}", ${ORBIT_UI_DEFAULT_STACK}` : ORBIT_UI_DEFAULT_STACK;
    }
    const preset = UI_FONT_PRESETS.find((p) => p.id === presetId);
    return preset?.family ? `"${preset.family}", ${ORBIT_UI_DEFAULT_STACK}` : ORBIT_UI_DEFAULT_STACK;
  }

  if (slot === "content") {
    if (presetId === "match-ui") return "var(--orbit-font-ui)";
    if (presetId === "system-serif") return ORBIT_SERIF_STACK;
    if (presetId === "custom") {
      const clean = sanitizeFontName(custom);
      return clean ? `"${clean}", ${ORBIT_UI_DEFAULT_STACK}` : "var(--orbit-font-ui)";
    }
    const preset = CONTENT_FONT_PRESETS.find((p) => p.id === presetId);
    return preset?.family ? `"${preset.family}", ${ORBIT_UI_DEFAULT_STACK}` : "var(--orbit-font-ui)";
  }

  if (presetId === "default") return ORBIT_CODE_DEFAULT_STACK;
  if (presetId === "custom") {
    const clean = sanitizeFontName(custom);
    return clean
      ? `"${clean}", ${ORBIT_CODE_CJK_FALLBACK}, ${ORBIT_CODE_DEFAULT_STACK}`
      : ORBIT_CODE_DEFAULT_STACK;
  }
  const preset = CODE_FONT_PRESETS.find((p) => p.id === presetId);
  return preset?.family
    ? `"${preset.family}", ${ORBIT_CODE_CJK_FALLBACK}, ${ORBIT_CODE_DEFAULT_STACK}`
    : ORBIT_CODE_DEFAULT_STACK;
}

export interface TypographyPreferencesInput {
  conversationFontSize?: number;
  codeFontSize?: number;
  uiFontId?: string;
  contentFontId?: string;
  codeFontId?: string;
  uiFontCustom?: string | null;
  contentFontCustom?: string | null;
  codeFontCustom?: string | null;
}

export interface CodeEditorMetrics {
  readonly fontPx: number;
  readonly linePx: number;
  readonly scale: number;
  readonly family: string;
}

export interface ResolvedTypography {
  readonly cssVars: Record<`--orbit-${string}`, string>;
  readonly readSizePx: number;
  readonly codeFontPx: number;
  readonly codeLinePx: number;
  readonly readScale: number;
  readonly codeScale: number;
  readonly codeMetrics: CodeEditorMetrics;
  readonly terminal: {
    readonly fontFamily: string;
    readonly fontSize: number;
    readonly lineHeight: number;
  };
  readonly typographyKey: string;
  readonly isDefault: boolean;
}

export const DEFAULT_CODE_METRICS: CodeEditorMetrics = {
  fontPx: DEFAULT_CODE_SIZE,
  linePx: 20,
  scale: 1,
  family: ORBIT_CODE_DEFAULT_STACK,
};

function resolveSlotKeyToken(slot: FontSlot, id: string, custom: string | null): string {
  if (id === "custom") {
    return custom ? `custom:${custom}` : DEFAULT_FONT_ID_BY_SLOT[slot];
  }
  return id;
}

export function resolveTypography(prefs: TypographyPreferencesInput = {}): ResolvedTypography {
  const readSize = normalizeConversationFontSize(prefs.conversationFontSize);
  const codeSize = normalizeCodeFontSize(prefs.codeFontSize);

  const uiId = normalizeFontId("ui", prefs.uiFontId);
  const contentId = normalizeFontId("content", prefs.contentFontId);
  const codeId = normalizeFontId("code", prefs.codeFontId);

  const uiCustom = sanitizeFontName(prefs.uiFontCustom);
  const contentCustom = sanitizeFontName(prefs.contentFontCustom);
  const codeCustom = sanitizeFontName(prefs.codeFontCustom);

  const uiKey = resolveSlotKeyToken("ui", uiId, uiCustom);
  const contentKey = resolveSlotKeyToken("content", contentId, contentCustom);
  const codeKey = resolveSlotKeyToken("code", codeId, codeCustom);

  const r = readSize / 15;
  const c = codeSize / 13;
  const codeLinePx = Math.round(20 * c);

  const uiFontStack = resolveFontPreviewStack("ui", uiId, uiCustom);
  const contentFontStack = resolveFontPreviewStack("content", contentId, contentCustom);
  const codeFontStack = resolveFontPreviewStack("code", codeId, codeCustom);

  let terminalFontFamily = ORBIT_TERMINAL_DEFAULT_STACK;
  if (codeId === "custom" && codeCustom) {
    terminalFontFamily = `"${codeCustom}", ${ORBIT_CODE_CJK_FALLBACK}, ${ORBIT_TERMINAL_DEFAULT_STACK}`;
  } else if (codeId !== "default") {
    const preset = CODE_FONT_PRESETS.find((p) => p.id === codeId);
    if (preset?.family) {
      terminalFontFamily = `"${preset.family}", ${ORBIT_CODE_CJK_FALLBACK}, ${ORBIT_TERMINAL_DEFAULT_STACK}`;
    }
  }

  const cssVars: Record<`--orbit-${string}`, string> = {
    "--orbit-read-body": `${readSize}px`,
    "--orbit-read-leading": "1.65",
    "--orbit-read-compact": `${Math.round(13 * r)}px`,
    "--orbit-read-compact-leading": "1.55",
    "--orbit-read-h1": `${Math.round(20 * r)}px`,
    "--orbit-read-h2": `${Math.round(17 * r)}px`,
    "--orbit-read-h3": `${Math.round(15 * r)}px`,
    "--orbit-read-table": `${Math.round(13 * r)}px`,
    "--orbit-read-th": `${Math.round(12 * r)}px`,
    "--orbit-composer-size": `${readSize}px`,
    "--orbit-composer-leading": `${Math.round(22 * r)}px`,
    "--orbit-composer-max-h": `${Math.round(200 * r)}px`,
    "--orbit-code-size": `${codeSize}px`,
    "--orbit-code-pane-size": `${Math.round(12 * c)}px`,
    "--orbit-code-line": `${codeLinePx}px`,
    "--orbit-code-block-leading": "1.6",
    "--orbit-font-ui": uiFontStack,
    "--orbit-font-content": contentFontStack,
    "--orbit-font-code": codeFontStack,
  };
  if (codeKey !== "default") {
    cssVars["--orbit-font-code-surface"] = codeFontStack;
  }

  const typographyKey = `r${readSize}:c${codeSize}:${uiKey}:${contentKey}:${codeKey}`;
  const isDefault =
    readSize === DEFAULT_READ_SIZE &&
    codeSize === DEFAULT_CODE_SIZE &&
    uiKey === "default" &&
    contentKey === "match-ui" &&
    codeKey === "default";

  return {
    cssVars,
    readSizePx: readSize,
    codeFontPx: codeSize,
    codeLinePx,
    readScale: r,
    codeScale: c,
    codeMetrics: {
      fontPx: codeSize,
      linePx: codeLinePx,
      scale: c,
      family: codeFontStack,
    },
    terminal: {
      fontFamily: terminalFontFamily,
      fontSize: codeSize,
      lineHeight: 1.28,
    },
    typographyKey,
    isDefault,
  };
}

const lastAppliedTypographyVars = new Map<string, string>();

const OPTIONAL_TYPOGRAPHY_VARS = ["--orbit-font-code-surface"] as const;

/**
 * Synchronously writes all typography CSS variables in a single frame (never deferred to rAF).
 * At default settings, clears inline `--orbit-*` typography properties and removes the
 * localStorage flash-guard snapshot so `:root` defaults apply untouched.
 */
export function applyTypography(resolved: ResolvedTypography): void {
  if (typeof document === "undefined") return;
  const style = document.documentElement.style;

  if (resolved.isDefault) {
    const keysToClear = new Set<string>([
      ...Object.keys(resolved.cssVars),
      ...lastAppliedTypographyVars.keys(),
      ...OPTIONAL_TYPOGRAPHY_VARS,
    ]);
    for (const key of keysToClear) {
      if (lastAppliedTypographyVars.has(key) || style.getPropertyValue(key)) {
        style.removeProperty(key);
      }
    }
    lastAppliedTypographyVars.clear();
    if (typeof window !== "undefined") {
      try {
        window.localStorage.removeItem(TYPOGRAPHY_SNAPSHOT_STORAGE_KEY);
      } catch {
        // Ignore storage errors
      }
    }
    return;
  }

  for (const key of OPTIONAL_TYPOGRAPHY_VARS) {
    if (!(key in resolved.cssVars)) {
      if (lastAppliedTypographyVars.has(key) || style.getPropertyValue(key)) {
        style.removeProperty(key);
      }
      lastAppliedTypographyVars.delete(key);
    }
  }
  for (const key of Array.from(lastAppliedTypographyVars.keys())) {
    if (!(key in resolved.cssVars)) {
      style.removeProperty(key);
      lastAppliedTypographyVars.delete(key);
    }
  }

  for (const [key, value] of Object.entries(resolved.cssVars)) {
    if (lastAppliedTypographyVars.get(key) !== value || style.getPropertyValue(key) !== value) {
      style.setProperty(key, value);
      lastAppliedTypographyVars.set(key, value);
    }
  }

  if (typeof window !== "undefined") {
    try {
      window.localStorage.setItem(
        TYPOGRAPHY_SNAPSHOT_STORAGE_KEY,
        JSON.stringify(resolved.cssVars),
      );
    } catch {
      // Ignore storage quota errors
    }
  }
}

export function normalizeZoomPercent(raw: number): number {
  if (!Number.isFinite(raw)) return ZOOM_DEFAULT_PERCENT;
  const snapped = Math.round(raw / ZOOM_STEP_PERCENT) * ZOOM_STEP_PERCENT;
  return Math.max(ZOOM_MIN_PERCENT, Math.min(ZOOM_MAX_PERCENT, snapped));
}

/**
 * Updates `--orbit-zoom` on `:root` (for macOS titlebar compensation only) and persists
 * the non-default zoom snapshot for cold-start flash prevention.
 */
export function applyZoomCssVar(zoomPercent: number): void {
  const normalized = normalizeZoomPercent(zoomPercent);
  if (typeof document !== "undefined") {
    const style = document.documentElement.style;
    if (normalized === ZOOM_DEFAULT_PERCENT) {
      if (style.getPropertyValue("--orbit-zoom")) {
        style.removeProperty("--orbit-zoom");
      }
    } else {
      const cssVal = String(normalized / 100);
      if (style.getPropertyValue("--orbit-zoom") !== cssVal) {
        style.setProperty("--orbit-zoom", cssVal);
      }
    }
  }

  if (typeof window !== "undefined") {
    try {
      if (normalized === ZOOM_DEFAULT_PERCENT) {
        window.localStorage.removeItem(ZOOM_STORAGE_KEY);
      } else {
        window.localStorage.setItem(ZOOM_STORAGE_KEY, String(normalized));
      }
    } catch {
      // Ignore storage quota errors
    }
  }
}

export type DesktopZoomAction = "in" | "out" | "reset";

export type ZoomKeyEventLike = Pick<
  KeyboardEvent,
  "key" | "code" | "metaKey" | "ctrlKey" | "altKey" | "shiftKey"
> & { isComposing?: boolean };

export function matchDesktopZoomAction(
  e: ZoomKeyEventLike,
  isMac: boolean,
): DesktopZoomAction | null {
  if (e.isComposing) return null;
  const modOk = isMac ? e.metaKey && !e.ctrlKey : e.ctrlKey && !e.metaKey;
  if (!modOk || e.altKey) return null;

  if (e.key === "=" || e.key === "+" || e.code === "Equal" || e.code === "NumpadAdd") {
    return "in";
  }
  if (!e.shiftKey && (e.key === "-" || e.code === "Minus" || e.code === "NumpadSubtract")) {
    return "out";
  }
  if (!e.shiftKey && (e.key === "0" || e.code === "Digit0" || e.code === "Numpad0")) {
    return "reset";
  }
  return null;
}
