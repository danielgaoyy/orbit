import type { ITheme } from "@xterm/xterm";
import type { TerminalThemeMode } from "@/state/store-types";

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

export function resolveXtermTheme(resolvedTheme: "light" | "dark", mode: TerminalThemeMode): ITheme {
  if (mode === "high_contrast") {
    return resolvedTheme === "dark"
      ? DARK_HIGH_CONTRAST_XTERM_THEME
      : LIGHT_HIGH_CONTRAST_XTERM_THEME;
  }
  return resolvedTheme === "dark"
    ? DARK_HARMONIZED_XTERM_THEME
    : LIGHT_HARMONIZED_XTERM_THEME;
}
