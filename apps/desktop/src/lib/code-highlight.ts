import type { HighlightedTokenView } from "@/lib/orbit-ipc";

export const DARK_PLAIN_TOKEN_FG = "#E2E8F0";
export const LIGHT_PLAIN_TOKEN_FG = "#24292F";

/**
 * Select the native Rust `syntect` token foreground color for the active appearance theme
 * (`base16-ocean.dark` in dark mode, `InspiredGitHub` in light mode).
 */
export function resolveTokenColor(
  token: Pick<HighlightedTokenView, "fg_hex" | "light_fg_hex">,
  isDark: boolean,
): string {
  return isDark ? token.fg_hex : (token.light_fg_hex ?? LIGHT_PLAIN_TOKEN_FG);
}
