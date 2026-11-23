import type { CSSProperties } from "react";

/**
 * Single-source-of-truth geometry rules for Orbit's three-pane desktop shell:
 *   [ Left Sidebar ] | [ Main Conversation Stage ] | [ Right Side Panel ]
 *
 * High-performance hybrid strategy (VS Code / Linear / Apple SplitView):
 * 1. During continuous window resizing, layout is computed 100% natively in WebKit C++
 *    via CSS custom properties (`clamp` / `min` / `max` / `calc`) with ZERO per-frame
 *    React re-renders and ZERO double-layout thrashing.
 * 2. On spacious viewports (`center >= 440px`), both the left navigation rail and the
 *    right side panel hold their user-chosen widths steady so only the center stage
 *    absorbs window changes (zero unnecessary reflow inside Diff / Code / Terminal).
 * 3. Once the center stage reaches its 440px comfort floor, the right side panel yields
 *    smoothly to a 50/50 peer split (down to 320px) so the main conversation is never
 *    crushed, and the left sidebar gently yields toward 240px only if the window gets tight.
 */

export const SIDEBAR_DEFAULT_WIDTH_PX = 280;
export const SIDEBAR_MIN_WIDTH_PX = 240;
export const SIDEBAR_MAX_WIDTH_PX = 420;

export const SIDE_PANEL_DEFAULT_WIDTH_PX = 480;
export const SIDE_PANEL_MIN_WIDTH_PX = 320;
export const SIDE_PANEL_MAX_WIDTH_PX = 1400;

export const STAGE_MIN_WIDTH_PX = 360;
export const STAGE_COMFORT_WIDTH_PX = 440;

const MIN_WORK_PAIR_PX = STAGE_MIN_WIDTH_PX + SIDE_PANEL_MIN_WIDTH_PX; // 680px

function clamp(value: number, min: number, max: number): number {
  if (max < min) return min;
  return Math.min(max, Math.max(min, value));
}

export function clampSidebarPrefWidth(width: number): number {
  return Math.round(clamp(width, SIDEBAR_MIN_WIDTH_PX, SIDEBAR_MAX_WIDTH_PX));
}

export function clampSidePanelPrefWidth(width: number): number {
  return Math.round(clamp(width, SIDE_PANEL_MIN_WIDTH_PX, SIDE_PANEL_MAX_WIDTH_PX));
}

/**
 * Generates pure-CSS custom properties expressing the EXACT same geometry constraints as
 * `resolveWorkspacePaneWidths`, allowing WebKit to resize panes in a single native C++
 * layout pass at 120Hz with zero React state updates.
 */
export function buildWorkspaceShellCssVars(params: {
  sidebarOpen: boolean;
  sidebarPrefWidth: number;
  sidePanelOpen: boolean;
  sidePanelPrefWidth: number;
}): CSSProperties {
  const leftPref = clampSidebarPrefWidth(params.sidebarPrefWidth);
  const rightPref = clampSidePanelPrefWidth(params.sidePanelPrefWidth);
  const minWork = params.sidePanelOpen ? MIN_WORK_PAIR_PX : STAGE_MIN_WIDTH_PX;

  const leftW = `clamp(${SIDEBAR_MIN_WIDTH_PX}px, ${leftPref}px, max(${SIDEBAR_MIN_WIDTH_PX}px, calc(100vw - ${minWork}px)))`;
  const leftActiveW = params.sidebarOpen ? "var(--orbit-left-w)" : "0px";
  const rightAvail = "calc(100vw - var(--orbit-left-active-w))";
  const rightMin = `min(${SIDE_PANEL_MIN_WIDTH_PX}px, calc(var(--orbit-right-avail) * 0.45))`;
  const rightDesired = `min(${rightPref}px, max(calc(var(--orbit-right-avail) * 0.5), calc(var(--orbit-right-avail) - ${STAGE_COMFORT_WIDTH_PX}px)))`;
  const rightMax = `max(var(--orbit-right-min), min(${SIDE_PANEL_MAX_WIDTH_PX}px, calc(var(--orbit-right-avail) - ${STAGE_MIN_WIDTH_PX}px)))`;
  const rightW = `clamp(var(--orbit-right-min), ${rightDesired}, ${rightMax})`;

  return {
    "--orbit-left-w": leftW,
    "--orbit-left-active-w": leftActiveW,
    "--orbit-right-avail": rightAvail,
    "--orbit-right-min": rightMin,
    "--orbit-right-w": rightW,
  } as CSSProperties;
}

/**
 * Exact JS mirror of `buildWorkspaceShellCssVars` — used ONLY for discrete breakpoint
 * detection and panel open/close toggle keyframe snapshots (never for per-frame resize renders).
 */
export function resolveWorkspacePaneWidths(params: {
  viewportWidth: number;
  sidebarOpen: boolean;
  sidebarPrefWidth: number;
  sidePanelOpen: boolean;
  sidePanelPrefWidth: number;
}): {
  leftWidthPx: number;
  rightWidthPx: number;
  centerWidthPx: number;
} {
  const V = Math.max(320, Math.round(params.viewportWidth));
  const leftPref = clampSidebarPrefWidth(params.sidebarPrefWidth);
  const rightPref = clampSidePanelPrefWidth(params.sidePanelPrefWidth);

  let leftWidthPx = 0;
  if (params.sidebarOpen) {
    const minWorkSpace = params.sidePanelOpen ? MIN_WORK_PAIR_PX : STAGE_MIN_WIDTH_PX;
    const maxLeft = Math.max(SIDEBAR_MIN_WIDTH_PX, V - minWorkSpace);
    leftWidthPx = Math.round(clamp(leftPref, SIDEBAR_MIN_WIDTH_PX, Math.min(SIDEBAR_MAX_WIDTH_PX, maxLeft)));
  }

  let rightWidthPx = 0;
  const avail = Math.max(0, V - leftWidthPx);
  if (params.sidePanelOpen && avail > 0) {
    const rightMin = Math.min(SIDE_PANEL_MIN_WIDTH_PX, Math.floor(avail * 0.45));
    const comfortBudget = Math.max(Math.round(avail * 0.5), avail - STAGE_COMFORT_WIDTH_PX);
    const desired = Math.min(rightPref, comfortBudget);
    const rightMax = Math.max(
      rightMin,
      Math.min(SIDE_PANEL_MAX_WIDTH_PX, avail - STAGE_MIN_WIDTH_PX),
    );
    rightWidthPx = Math.round(clamp(desired, rightMin, rightMax));
  }

  const centerWidthPx = Math.max(0, V - leftWidthPx - rightWidthPx);
  return { leftWidthPx, rightWidthPx, centerWidthPx };
}

/**
 * Computes the exact 50/50 split width for the right side panel given the current viewport
 * and active left sidebar width (used when double-clicking the center/right divider).
 */
export function computeEqualizedSidePanelWidth(
  viewportWidth: number,
  activeLeftWidthPx: number,
): number {
  const remaining = Math.max(
    SIDE_PANEL_MIN_WIDTH_PX,
    Math.round(viewportWidth - activeLeftWidthPx),
  );
  return clampSidePanelPrefWidth(Math.round(remaining / 2));
}
