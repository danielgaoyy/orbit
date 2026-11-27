import { OrbitTokens } from '../../theme/OrbitTokens';

export type OrbitBreakpoint = 'sm' | 'md' | 'lg';

export type OrbitModalRouteKind =
  | 'host_settings'
  | 'global_search'
  | 'archived'
  | 'export_transcript'
  | 'directory_browser'
  | 'work_picker'
  | 'pa_briefing'
  | 'image_viewer'
  | 'diagram_viewer'
  | 'worktree_handoff'
  | 'workspace_scripts'
  | 'create_workspace_v2'
  | 'rename_workspace';

export interface OrbitModalRoute {
  kind: OrbitModalRouteKind;
  hostId?: string;
  agentId?: string;
  workspaceId?: string;
  cwd?: string;
  title?: string;
  initialPath?: string;
  imageUri?: string;
  imageLabel?: string;
  diagramKind?: string;
  diagramLanguage?: string;
  diagramSource?: string;
  diagramSvg?: string;
}

export type OrbitShortcutAction =
  | 'none'
  | 'toggle_sidebar'
  | 'open_launchpad'
  | 'reopen_last_tab'
  | 'close_panel_tab'
  | 'global_search'
  | 'new_session';

export interface OrbitShortcutContext {
  breakpoint: OrbitBreakpoint;
  isModalOpen: boolean;
  isTextInputFocused: boolean;
  isTerminalFocused: boolean;
  isSidePanelVisible: boolean;
}

export const ORBIT_KEYCODE_K: number = 2027;
export const ORBIT_KEYCODE_N: number = 2030;
export const ORBIT_KEYCODE_T: number = 2036;
export const ORBIT_KEYCODE_W: number = 2039;
export const ORBIT_KEYCODE_BACKSLASH: number = 2061;
export const ORBIT_KEYCODE_SHIFT_LEFT: number = 2047;
export const ORBIT_KEYCODE_SHIFT_RIGHT: number = 2048;
export const ORBIT_KEYCODE_CTRL_LEFT: number = 2072;
export const ORBIT_KEYCODE_CTRL_RIGHT: number = 2073;
export const ORBIT_KEYCODE_ALT_LEFT: number = 2045;
export const ORBIT_KEYCODE_ALT_RIGHT: number = 2046;
export const ORBIT_KEYCODE_META_LEFT: number = 2076;
export const ORBIT_KEYCODE_META_RIGHT: number = 2077;

export function isLightboxModalKind(kind: OrbitModalRouteKind | undefined): boolean {
  return kind === 'image_viewer' || kind === 'diagram_viewer';
}

export type OrbitSheetLifecycleState = 'idle' | 'presenting' | 'dismissing';

/**
 * Serialized wait-for-disappear lifecycle host for a single ArkUI `bindSheet` slot (R3-P1-8).
 *
 * Guarantees that a replacement or restored sheet is NEVER bound (`boundOpen = true`) until the
 * previous sheet finishes exiting (`onDisappear`). Therefore:
 * - Delayed `onAppear` from an exiting sheet A while `lifecycle === 'dismissing'` is ignored and
 *   NEVER registers or corrupts incoming sheet B.
 * - Out-of-order `onDisappear` from A promotes the pending reopen cleanly instead of dismissing B.
 * - Only a swipe/user dismissal while `lifecycle === 'presenting'` for the active presentation ID
 *   returns `true` from `onDisappear()`.
 */
const ORBIT_COMPLETED_SHEET_ID_WINDOW: number = 32;

/**
 * O(1)-memory monotonic watermark + bounded out-of-order window for completed sheet presentation IDs (R5-N1).
 * Any `id <= watermark` is known completed without storing per-presentation entries; returning to `idle`
 * advances `watermark` and empties the window so 1,000+ sequential open/close cycles retain 0 entries.
 */
class OrbitBoundedCompletedIdSet {
  private watermark: number = 0;
  private readonly aboveWatermark: number[] = [];

  public get retainedCount(): number {
    return this.aboveWatermark.length;
  }

  public has(id: number): boolean {
    if (id <= 0) {
      return false;
    }
    if (id <= this.watermark) {
      return true;
    }
    return this.aboveWatermark.indexOf(id) >= 0;
  }

  public add(id: number): void {
    if (id <= 0 || id <= this.watermark) {
      return;
    }
    if (this.aboveWatermark.indexOf(id) < 0) {
      this.aboveWatermark.push(id);
    }
    this.compact();
  }

  public advanceIdleWatermark(maxIssuedId: number): void {
    if (maxIssuedId > this.watermark) {
      this.watermark = maxIssuedId;
    }
    this.aboveWatermark.length = 0;
  }

  public clear(): void {
    this.watermark = 0;
    this.aboveWatermark.length = 0;
  }

  private compact(): void {
    if (this.aboveWatermark.length > 1) {
      this.aboveWatermark.sort((a: number, b: number): number => a - b);
    }
    while (this.aboveWatermark.length > 0 && this.aboveWatermark[0] <= this.watermark + 1) {
      const head = this.aboveWatermark.shift();
      if (head !== undefined && head > this.watermark) {
        this.watermark = head;
      }
    }
    while (this.aboveWatermark.length > ORBIT_COMPLETED_SHEET_ID_WINDOW) {
      const evicted = this.aboveWatermark.shift();
      if (evicted !== undefined && evicted > this.watermark) {
        this.watermark = evicted;
      }
    }
  }
}

export class OrbitSerializedSheetHost {
  public lifecycle: OrbitSheetLifecycleState = 'idle';
  public boundOpen: boolean = false;
  public activePresentationId: number = 0;
  public mountedPresentationId: number = 0;
  public presentationSeq: number = 0;
  public wantReopen: boolean = false;
  public activeHasAppeared: boolean = false;
  private readonly completedDisappearIds: OrbitBoundedCompletedIdSet = new OrbitBoundedCompletedIdSet();

  public get completedRecordCount(): number {
    return this.completedDisappearIds.retainedCount;
  }

  public requestOpen(): number {
    if (this.lifecycle === 'presenting') {
      return this.activePresentationId;
    }
    if (this.lifecycle === 'dismissing') {
      this.wantReopen = true;
      this.boundOpen = false;
      return this.activePresentationId;
    }
    this.presentationSeq += 1;
    this.activePresentationId = this.presentationSeq;
    this.mountedPresentationId = 0;
    this.lifecycle = 'presenting';
    this.boundOpen = true;
    this.wantReopen = false;
    this.activeHasAppeared = false;
    return this.activePresentationId;
  }

  public requestClose(): void {
    this.wantReopen = false;
    if (this.lifecycle === 'presenting') {
      if (this.mountedPresentationId > 0 && this.mountedPresentationId === this.activePresentationId) {
        this.lifecycle = 'dismissing';
        this.boundOpen = false;
      } else {
        // Never mounted natively yet: transition directly to idle without waiting for a phantom disappear.
        this.completedDisappearIds.advanceIdleWatermark(this.presentationSeq);
        this.lifecycle = 'idle';
        this.boundOpen = false;
        this.activePresentationId = 0;
        this.mountedPresentationId = 0;
        this.activeHasAppeared = false;
      }
    } else if (this.lifecycle === 'dismissing') {
      this.boundOpen = false;
    }
  }

  public onAppear(instanceId?: number): boolean {
    if (this.lifecycle !== 'presenting') {
      return false;
    }
    const targetId = (instanceId !== undefined && instanceId > 0)
      ? instanceId
      : this.activePresentationId;
    if (
      targetId <= 0 ||
      targetId !== this.activePresentationId ||
      this.completedDisappearIds.has(targetId)
    ) {
      return false;
    }
    this.activeHasAppeared = true;
    this.mountedPresentationId = targetId;
    return true;
  }

  /**
   * Processes the single authoritative instance-bound exit callback (`aboutToDisappear(capturedId)`).
   * Returns `true` ONLY when the currently mounted sheet was dismissed by the user (so the host
   * should mirror dismissal back into domain state). Duplicate or stale IDs are strictly ignored.
   */
  public onDisappear(instanceId?: number): boolean {
    const targetId = (instanceId !== undefined && instanceId > 0)
      ? instanceId
      : this.mountedPresentationId;
    if (
      targetId <= 0 ||
      this.completedDisappearIds.has(targetId) ||
      targetId !== this.activePresentationId
    ) {
      return false;
    }
    if (this.lifecycle === 'dismissing') {
      this.completedDisappearIds.add(targetId);
      this.mountedPresentationId = 0;
      if (this.wantReopen) {
        this.presentationSeq += 1;
        this.activePresentationId = this.presentationSeq;
        this.lifecycle = 'presenting';
        this.boundOpen = true;
        this.wantReopen = false;
        this.activeHasAppeared = false;
        return false;
      }
      this.completedDisappearIds.advanceIdleWatermark(this.presentationSeq);
      this.lifecycle = 'idle';
      this.boundOpen = false;
      this.activePresentationId = 0;
      this.activeHasAppeared = false;
      return false;
    }
    if (this.lifecycle === 'presenting') {
      if (targetId !== this.mountedPresentationId) {
        return false;
      }
      this.completedDisappearIds.advanceIdleWatermark(this.presentationSeq);
      this.lifecycle = 'idle';
      this.boundOpen = false;
      this.activePresentationId = 0;
      this.mountedPresentationId = 0;
      this.wantReopen = false;
      this.activeHasAppeared = false;
      return true;
    }
    return false;
  }

  public reset(): void {
    this.lifecycle = 'idle';
    this.boundOpen = false;
    this.activePresentationId = 0;
    this.mountedPresentationId = 0;
    this.wantReopen = false;
    this.activeHasAppeared = false;
    this.completedDisappearIds.clear();
  }
}

/**
 * Pure external-keyboard shortcut arbitrator supporting both `Ctrl` and `Meta` modifiers on `md`/`lg`,
 * with modal, terminal-focus, text-input-focus, and visible-panel guards.
 */
export function resolveOrbitKeyboardShortcut(
  keyCode: number,
  hasCtrl: boolean,
  hasMeta: boolean,
  hasShift: boolean,
  hasAlt: boolean,
  ctx: OrbitShortcutContext
): OrbitShortcutAction {
  if (ctx.breakpoint === 'sm' || ctx.isModalOpen || hasAlt) {
    return 'none';
  }
  if (!hasCtrl && !hasMeta) {
    return 'none';
  }
  // When PTY terminal has focus, preserve raw Ctrl control bytes (^W, ^T, ^K, ^N) for the shell;
  // Meta (Cmd/Super) shortcuts still operate on the workspace shell.
  if (ctx.isTerminalFocused && hasCtrl && !hasMeta) {
    return 'none';
  }
  if (keyCode === ORBIT_KEYCODE_BACKSLASH && !hasShift) {
    return 'toggle_sidebar';
  }
  if (keyCode === ORBIT_KEYCODE_T && hasShift) {
    return 'reopen_last_tab';
  }
  if (keyCode === ORBIT_KEYCODE_T && !hasShift) {
    return 'open_launchpad';
  }
  if (keyCode === ORBIT_KEYCODE_W && !hasShift) {
    // NEVER close a tab when the side panel is hidden/collapsed or while Ctrl+W is used in text editing.
    if (!ctx.isSidePanelVisible) {
      return 'none';
    }
    if (ctx.isTextInputFocused && hasCtrl && !hasMeta) {
      return 'none';
    }
    return 'close_panel_tab';
  }
  if (keyCode === ORBIT_KEYCODE_K && !hasShift) {
    if (ctx.isTextInputFocused && hasCtrl && !hasMeta) {
      return 'none';
    }
    return 'global_search';
  }
  if (keyCode === ORBIT_KEYCODE_N && !hasShift) {
    return 'new_session';
  }
  return 'none';
}

export interface OrbitShellLayoutGeometry {
  breakpoint: OrbitBreakpoint;
  effectiveContentWidthVp: number;
  sidebarVisible: boolean;
  sidebarWidthVp: number;
  sidePanelInline: boolean;
  sidePanelWidthVp: number;
  stageWidthVp: number;
}

export interface OrbitShellWidthStorage {
  getSidebarWidthVp(): number;
  setSidebarWidthVp(widthVp: number): void;
  getSidePanelWidthVp(): number;
  setSidePanelWidthVp(widthVp: number): void;
}

export const ORBIT_BP_MD_MIN_VP: number = 680;
export const ORBIT_BP_LG_MIN_VP: number = 1024;
export const ORBIT_STAGE_MIN_WIDTH_VP: number = 360;

export const ORBIT_SIDEBAR_MD_WIDTH_VP: number = 260;
export const ORBIT_SIDEBAR_LG_DEFAULT_WIDTH_VP: number = 280;
export const ORBIT_SIDEBAR_LG_MIN_WIDTH_VP: number = 240;
export const ORBIT_SIDEBAR_LG_MAX_WIDTH_VP: number = 360;

export const ORBIT_PANEL_MD_MIN_WIDTH_VP: number = 320;
export const ORBIT_PANEL_MD_MAX_WIDTH_VP: number = 420;
export const ORBIT_PANEL_LG_MIN_WIDTH_VP: number = 320;
export const ORBIT_PANEL_LG_MAX_WIDTH_VP: number = 680;
export const ORBIT_PANEL_DEFAULT_WIDTH_VP: number = 360;

/**
 * Resolves the canonical 3-form-factor breakpoint:
 * - `sm`: `< 680 vp` (or folded posture)
 * - `md`: `680–1023 vp`
 * - `lg`: `>= 1024 vp`
 */
export function resolveOrbitBreakpoint(
  windowWidthVp: number,
  foldPosture: string = 'unknown'
): OrbitBreakpoint {
  if (foldPosture === 'folded') {
    return 'sm';
  }
  if (windowWidthVp < ORBIT_BP_MD_MIN_VP) {
    return 'sm';
  }
  if (windowWidthVp < ORBIT_BP_LG_MIN_VP) {
    return 'md';
  }
  return 'lg';
}

export function clampSidebarLgWidth(widthVp: number): number {
  if (!Number.isFinite(widthVp) || widthVp <= 0) {
    return ORBIT_SIDEBAR_LG_DEFAULT_WIDTH_VP;
  }
  return Math.max(
    ORBIT_SIDEBAR_LG_MIN_WIDTH_VP,
    Math.min(ORBIT_SIDEBAR_LG_MAX_WIDTH_VP, Math.round(widthVp))
  );
}

export function clampSidePanelWidthForBreakpoint(
  widthVp: number,
  breakpoint: OrbitBreakpoint,
  windowWidthVp: number,
  effectiveSidebarWidthVp: number
): number {
  const base = Number.isFinite(widthVp) && widthVp > 0 ? Math.round(widthVp) : ORBIT_PANEL_DEFAULT_WIDTH_VP;
  if (breakpoint === 'sm') {
    return Math.max(ORBIT_PANEL_MD_MIN_WIDTH_VP, Math.min(windowWidthVp, base));
  }
  const maxAllowedByStage = Math.max(
    ORBIT_PANEL_MD_MIN_WIDTH_VP,
    Math.floor(windowWidthVp - effectiveSidebarWidthVp - ORBIT_STAGE_MIN_WIDTH_VP)
  );
  if (breakpoint === 'md') {
    const hardMax = Math.min(ORBIT_PANEL_MD_MAX_WIDTH_VP, maxAllowedByStage);
    return Math.max(ORBIT_PANEL_MD_MIN_WIDTH_VP, Math.min(hardMax, base));
  }
  const hardMaxLg = Math.min(ORBIT_PANEL_LG_MAX_WIDTH_VP, maxAllowedByStage);
  return Math.max(ORBIT_PANEL_LG_MIN_WIDTH_VP, Math.min(hardMaxLg, base));
}

/**
 * Pure geometry solver deducting horizontal safe-area insets first, ensuring the primary Stage
 * reading column ALWAYS keeps `>= 360 vp` and any inline right panel keeps `>= 320 vp`.
 * If remaining effective content width cannot hold both minimums, `sidePanelInline` degrades
 * cleanly to `false` (compact bottom sheet presentation).
 */
export function computeOrbitShellLayout(
  windowWidthVp: number,
  foldPosture: string,
  sidebarRequestedOpen: boolean,
  sidebarLgPrefWidthVp: number,
  sidePanelOpen: boolean,
  sidePanelPrefWidthVp: number,
  horizontalInsetVp: number = 0
): OrbitShellLayoutGeometry {
  const bp = resolveOrbitBreakpoint(windowWidthVp, foldPosture);
  const safeInset = Number.isFinite(horizontalInsetVp) && horizontalInsetVp > 0
    ? Math.round(horizontalInsetVp)
    : 0;
  const effectiveW = Math.max(0, Math.floor(windowWidthVp) - safeInset);

  if (bp === 'sm') {
    const drawerW = Math.min(320, Math.max(280, Math.floor(windowWidthVp * 0.82)));
    return {
      breakpoint: 'sm',
      effectiveContentWidthVp: effectiveW,
      sidebarVisible: sidebarRequestedOpen,
      sidebarWidthVp: drawerW,
      sidePanelInline: false,
      sidePanelWidthVp: windowWidthVp,
      stageWidthVp: effectiveW
    };
  }

  if (bp === 'md') {
    // In md (680-1023 vp), opening the right panel automatically yields the left sidebar
    // so the main Stage always remains >= 360 vp. If horizontal safe insets shrink effective
    // content width below 360 + 320 = 680 vp, degrade inline right panel to sheet presentation.
    const canHostInlinePanel = effectiveW >= ORBIT_STAGE_MIN_WIDTH_VP + ORBIT_PANEL_MD_MIN_WIDTH_VP;
    const panelInline = sidePanelOpen && canHostInlinePanel;
    const sidebarVisible = panelInline ? false : (sidebarRequestedOpen && effectiveW >= ORBIT_STAGE_MIN_WIDTH_VP + ORBIT_SIDEBAR_MD_WIDTH_VP);
    const sidebarW = sidebarVisible ? ORBIT_SIDEBAR_MD_WIDTH_VP : 0;
    const panelW = panelInline
      ? clampSidePanelWidthForBreakpoint(sidePanelPrefWidthVp, 'md', effectiveW, sidebarW)
      : 0;
    const stageW = Math.max(ORBIT_STAGE_MIN_WIDTH_VP, effectiveW - sidebarW - panelW);
    return {
      breakpoint: 'md',
      effectiveContentWidthVp: effectiveW,
      sidebarVisible: sidebarVisible,
      sidebarWidthVp: ORBIT_SIDEBAR_MD_WIDTH_VP,
      sidePanelInline: panelInline,
      sidePanelWidthVp: panelW,
      stageWidthVp: stageW
    };
  }

  // lg (>= 1024 vp): three columns side-by-side within effective content width (post safe insets).
  const canHostInlinePanelLg = effectiveW >= ORBIT_STAGE_MIN_WIDTH_VP + ORBIT_PANEL_LG_MIN_WIDTH_VP;
  const panelInlineLg = sidePanelOpen && canHostInlinePanelLg;
  let sidebarVisibleLg = sidebarRequestedOpen;
  let sidebarW = sidebarVisibleLg ? clampSidebarLgWidth(sidebarLgPrefWidthVp) : 0;
  let panelW = panelInlineLg
    ? Math.max(ORBIT_PANEL_LG_MIN_WIDTH_VP, Math.min(ORBIT_PANEL_LG_MAX_WIDTH_VP, Math.round(sidePanelPrefWidthVp)))
    : 0;

  if (sidebarVisibleLg && panelInlineLg) {
    const budget = Math.max(0, effectiveW - ORBIT_STAGE_MIN_WIDTH_VP);
    if (budget < ORBIT_SIDEBAR_LG_MIN_WIDTH_VP + ORBIT_PANEL_LG_MIN_WIDTH_VP) {
      // Safe insets squeezed effective width below three-pane minimum: yield sidebar to protect Stage >= 360 vp.
      sidebarVisibleLg = false;
      sidebarW = 0;
      panelW = clampSidePanelWidthForBreakpoint(panelW, 'lg', effectiveW, 0);
    } else if (sidebarW + panelW > budget) {
      // Shrink panel first down to min (320 vp), then sidebar down to min (240 vp).
      panelW = Math.max(ORBIT_PANEL_LG_MIN_WIDTH_VP, budget - sidebarW);
      if (sidebarW + panelW > budget) {
        sidebarW = Math.max(ORBIT_SIDEBAR_LG_MIN_WIDTH_VP, budget - panelW);
      }
    }
  } else if (panelInlineLg) {
    panelW = clampSidePanelWidthForBreakpoint(panelW, 'lg', effectiveW, 0);
  }

  const stageW = Math.max(ORBIT_STAGE_MIN_WIDTH_VP, effectiveW - sidebarW - panelW);
  return {
    breakpoint: 'lg',
    effectiveContentWidthVp: effectiveW,
    sidebarVisible: sidebarVisibleLg,
    sidebarWidthVp: sidebarW > 0 ? sidebarW : clampSidebarLgWidth(sidebarLgPrefWidthVp),
    sidePanelInline: panelInlineLg,
    sidePanelWidthVp: panelW,
    stageWidthVp: stageW
  };
}

/**
 * Shell UI domain store:
 * - Single Modal Router (`modalRoute: OrbitModalRoute | null`) eliminating 15 stacked sheets
 * - 3-form-factor breakpoint state (`sm` / `md` / `lg`) & `md` sidebar auto-yield
 * - Persisted `lg` sidebar width (240-360 vp) and right panel width (320-680 vp)
 * - 2-in-1 free-window title-button right avoidance width
 * - Reduced-motion & fold posture flags
 */
@ObservedV2
export class OrbitShellStore {
  @Trace public windowWidthVp: number = 390;
  @Trace public windowHeightVp: number = 844;
  @Trace public safeLeftVp: number = 0;
  @Trace public safeRightVp: number = 0;
  @Trace public isFoldableDevice: boolean = false;
  @Trace public foldPosture: string = 'unknown';
  @Trace public breakpoint: OrbitBreakpoint = 'sm';
  @Trace public reducedMotion: boolean = false;

  // External keyboard focus arbitration
  @Trace public isTextInputFocused: boolean = false;
  @Trace public isTerminalFocused: boolean = false;

  // Sidebar state + md auto-yield bookkeeping
  @Trace public sidebarOpen: boolean = false;
  @Trace public isSidebarCollapsed: boolean = false;
  @Trace public sidebarAutoYieldedForSidePanel: boolean = false;
  @Trace public sidebarWidthLgVp: number = ORBIT_SIDEBAR_LG_DEFAULT_WIDTH_VP;
  @Trace public sidePanelWidthVp: number = ORBIT_PANEL_DEFAULT_WIDTH_VP;

  // Single Modal Route (replaces 15 bindSheet booleans in AdaptiveWorkspace)
  // Standard sheet modals bind to `isSheetModalPresented` through a serialized wait-for-disappear
  // state machine (`idle -> presenting -> dismissing -> presenting`) so an exiting sheet A's delayed
  // `onAppear` or `onDisappear` can NEVER race with or dismiss an incoming sheet B (R3-P1-8).
  // Full-screen Lightbox modals (`image_viewer` / `diagram_viewer`) mount via `isLightboxPresented`.
  @Trace public modalRoute: OrbitModalRoute | null = null;
  @Trace public isSheetModalPresented: boolean = false;
  @Trace public isLightboxPresented: boolean = false;
  @Trace public modalPresentationId: number = 0;
  @Trace public activeSheetPresentationId: number = 0;
  @Trace public activeLightboxPresentationId: number = 0;
  @Trace public mountedSheetPresentationId: number = 0;
  @Trace public sheetLifecycleState: OrbitSheetLifecycleState = 'idle';
  @Trace public pendingModalRoute: OrbitModalRoute | null = null;
  private pendingPresentationId: number = 0;
  private activeSheetAppeared: boolean = false;
  private readonly completedSheetDisappearIds: OrbitBoundedCompletedIdSet = new OrbitBoundedCompletedIdSet();

  public get completedSheetRecordCount(): number {
    return this.completedSheetDisappearIds.retainedCount;
  }

  // 2-in-1 Free-Window Title Bar Avoidance (vp)
  @Trace public isFreeWindowDecorHidden: boolean = false;
  @Trace public titleButtonRightAvoidVp: number = 0;

  private widthStorage: OrbitShellWidthStorage | null = null;

  public get isModalSheetPresented(): boolean {
    return this.isSheetModalPresented || this.isLightboxPresented || this.pendingModalRoute !== null;
  }

  public set isModalSheetPresented(val: boolean) {
    if (!val) {
      this.closeModal();
    }
  }

  public get effectiveModalRoute(): OrbitModalRoute | null {
    return this.pendingModalRoute !== null ? this.pendingModalRoute : this.modalRoute;
  }

  public setReducedMotion(enabled: boolean): void {
    if (this.reducedMotion !== enabled) {
      this.reducedMotion = enabled;
    }
  }

  public setTextInputFocused(focused: boolean): void {
    this.isTextInputFocused = focused;
  }

  public setTerminalFocused(focused: boolean): void {
    this.isTerminalFocused = focused;
  }

  public updateHorizontalInsets(leftVp: number, rightVp: number): void {
    const cleanL = Number.isFinite(leftVp) && leftVp > 0 ? Math.round(leftVp) : 0;
    const cleanR = Number.isFinite(rightVp) && rightVp > 0 ? Math.round(rightVp) : 0;
    if (this.safeLeftVp !== cleanL) {
      this.safeLeftVp = cleanL;
    }
    if (this.safeRightVp !== cleanR) {
      this.safeRightVp = cleanR;
    }
  }

  public bindWidthStorage(storage: OrbitShellWidthStorage | null): void {
    this.widthStorage = storage;
    if (storage !== null) {
      const savedSidebar = storage.getSidebarWidthVp();
      if (savedSidebar > 0) {
        this.sidebarWidthLgVp = clampSidebarLgWidth(savedSidebar);
      }
      const savedPanel = storage.getSidePanelWidthVp();
      if (savedPanel > 0) {
        this.sidePanelWidthVp = Math.max(
          ORBIT_PANEL_LG_MIN_WIDTH_VP,
          Math.min(ORBIT_PANEL_LG_MAX_WIDTH_VP, Math.round(savedPanel))
        );
      }
    }
  }

  public get isHalfFoldedHover(): boolean {
    return this.isFoldableDevice && this.foldPosture === 'half_folded';
  }

  public openModal(route: OrbitModalRoute): number {
    this.modalPresentationId += 1;
    const presentationId = this.modalPresentationId;
    if (this.breakpoint === 'sm') {
      this.sidebarOpen = false;
    }
    const lightbox = isLightboxModalKind(route.kind);
    if (lightbox) {
      this.pendingModalRoute = null;
      this.pendingPresentationId = 0;
      if (this.sheetLifecycleState === 'presenting') {
        if (this.mountedSheetPresentationId > 0 && this.mountedSheetPresentationId === this.activeSheetPresentationId) {
          this.isSheetModalPresented = false;
          this.sheetLifecycleState = 'dismissing';
        } else {
          if (this.activeSheetPresentationId > 0) {
            this.completedSheetDisappearIds.add(this.activeSheetPresentationId);
          }
          this.isSheetModalPresented = false;
          this.activeSheetPresentationId = 0;
          this.mountedSheetPresentationId = 0;
          this.activeSheetAppeared = false;
          this.sheetLifecycleState = 'idle';
        }
      } else {
        this.isSheetModalPresented = false;
      }
      this.modalRoute = route;
      this.activeLightboxPresentationId = presentationId;
      this.isLightboxPresented = true;
      return presentationId;
    }

    this.isLightboxPresented = false;
    this.activeLightboxPresentationId = 0;

    if (this.sheetLifecycleState === 'idle') {
      this.modalRoute = route;
      this.activeSheetPresentationId = presentationId;
      this.mountedSheetPresentationId = 0;
      this.activeSheetAppeared = false;
      this.pendingModalRoute = null;
      this.pendingPresentationId = 0;
      this.sheetLifecycleState = 'presenting';
      this.isSheetModalPresented = true;
      return presentationId;
    }

    // If the previous sheet NEVER mounted natively yet (e.g. consecutive open(A) -> open(B) before first render),
    // replace A directly with B instead of waiting on a phantom disappear from an unmounted A (R3-P1-8).
    if (
      this.sheetLifecycleState === 'presenting' &&
      (this.mountedSheetPresentationId <= 0 || this.mountedSheetPresentationId !== this.activeSheetPresentationId)
    ) {
      if (this.activeSheetPresentationId > 0) {
        this.completedSheetDisappearIds.add(this.activeSheetPresentationId);
      }
      this.modalRoute = route;
      this.activeSheetPresentationId = presentationId;
      this.mountedSheetPresentationId = 0;
      this.activeSheetAppeared = false;
      this.pendingModalRoute = null;
      this.pendingPresentationId = 0;
      this.sheetLifecycleState = 'presenting';
      this.isSheetModalPresented = true;
      return presentationId;
    }

    // Previous sheet IS mounted natively or actively dismissing: serialize by closing A and holding
    // B in `pendingModalRoute` until A's native `onModalSheetDisappear(idA)` confirms exit (R3-P1-8).
    this.pendingModalRoute = route;
    this.pendingPresentationId = presentationId;
    this.sheetLifecycleState = 'dismissing';
    this.isSheetModalPresented = false;
    return presentationId;
  }

  public closeModal(): void {
    this.modalPresentationId += 1;
    this.pendingModalRoute = null;
    this.pendingPresentationId = 0;
    this.isLightboxPresented = false;
    this.activeLightboxPresentationId = 0;
    if (this.sheetLifecycleState === 'presenting') {
      if (this.mountedSheetPresentationId > 0 && this.mountedSheetPresentationId === this.activeSheetPresentationId) {
        this.sheetLifecycleState = 'dismissing';
        this.isSheetModalPresented = false;
      } else {
        this.completedSheetDisappearIds.advanceIdleWatermark(this.modalPresentationId);
        this.activeSheetPresentationId = 0;
        this.mountedSheetPresentationId = 0;
        this.activeSheetAppeared = false;
        this.sheetLifecycleState = 'idle';
        this.isSheetModalPresented = false;
      }
    } else {
      this.isSheetModalPresented = false;
    }
    this.modalRoute = null;
  }

  /**
   * Records native sheet mount (`OrbitModalContentRouter.aboutToAppear` or `bindSheet.onAppear`)
   * strictly for the currently presenting sheet instance. Delayed or superseded IDs are ignored.
   */
  public onModalSheetAppear(instanceId?: number): boolean {
    if (this.sheetLifecycleState !== 'presenting') {
      return false;
    }
    const targetId = (instanceId !== undefined && instanceId > 0)
      ? instanceId
      : this.activeSheetPresentationId;
    if (
      targetId <= 0 ||
      targetId !== this.activeSheetPresentationId ||
      this.completedSheetDisappearIds.has(targetId)
    ) {
      return false;
    }
    this.activeSheetAppeared = true;
    this.mountedSheetPresentationId = targetId;
    return true;
  }

  /**
   * Consumes native sheet teardown (`OrbitModalContentRouter.aboutToDisappear` or `bindSheet.onDisappear`)
   * strictly matched against the instance's immutable presentation ID (R3-P1-8):
   * - Duplicate or stale presentation IDs are always no-ops (NEVER disturb a newly promoted sheet B).
   * - If `dismissing`: finishes tearing down A and promotes any queued `pendingModalRoute` (B),
   *   returning `false` so B stays open.
   * - If `presenting`: verifies `targetId === mountedSheetPresentationId` and clears the route on
   *   native user swipe dismissal (returning `true`).
   */
  public onModalSheetDisappear(instanceId?: number): boolean {
    const targetId = (instanceId !== undefined && instanceId > 0)
      ? instanceId
      : this.mountedSheetPresentationId;
    if (
      targetId <= 0 ||
      this.completedSheetDisappearIds.has(targetId) ||
      targetId !== this.activeSheetPresentationId
    ) {
      return false;
    }
    if (this.sheetLifecycleState === 'dismissing') {
      this.completedSheetDisappearIds.add(targetId);
      this.mountedSheetPresentationId = 0;
      if (this.pendingModalRoute !== null && !this.isLightboxPresented) {
        const nextRoute = this.pendingModalRoute;
        const nextId = this.pendingPresentationId;
        this.pendingModalRoute = null;
        this.pendingPresentationId = 0;
        this.modalRoute = nextRoute;
        this.activeSheetPresentationId = nextId;
        this.activeSheetAppeared = false;
        this.sheetLifecycleState = 'presenting';
        this.isSheetModalPresented = true;
        return false;
      }
      this.completedSheetDisappearIds.advanceIdleWatermark(this.modalPresentationId);
      this.activeSheetPresentationId = 0;
      this.activeSheetAppeared = false;
      this.sheetLifecycleState = 'idle';
      this.isSheetModalPresented = false;
      if (!this.isLightboxPresented) {
        this.modalRoute = null;
      }
      return false;
    }
    if (this.sheetLifecycleState === 'presenting') {
      if (targetId !== this.mountedSheetPresentationId) {
        return false;
      }
      this.completedSheetDisappearIds.advanceIdleWatermark(this.modalPresentationId);
      this.isSheetModalPresented = false;
      this.activeSheetPresentationId = 0;
      this.mountedSheetPresentationId = 0;
      this.activeSheetAppeared = false;
      this.sheetLifecycleState = 'idle';
      if (!this.isLightboxPresented) {
        this.modalRoute = null;
      }
      return true;
    }
    return false;
  }

  /**
   * Requests closing a modal strictly by its immutable presentation ID without pretending native
   * `onDisappear` has already completed (R3-P1-8):
   * - For a Lightbox: ONLY matches `activeLightboxPresentationId` (an old Lightbox close callback can
   *   NEVER dismiss a newer standard Sheet).
   * - For a mounted Sheet: transitions `presenting -> dismissing` (`isSheetModalPresented = false`)
   *   while retaining `activeSheetPresentationId` & `mountedSheetPresentationId` so a subsequent
   *   `openModal(B)` serializes properly until native `onModalSheetDisappear(idA)` completes!
   * - For an unmounted Sheet: transitions directly to `idle` and marks the ID completed.
   */
  public closeModalByPresentationId(presentationId: number): boolean {
    if (presentationId <= 0) {
      return false;
    }
    if (this.isLightboxPresented && presentationId === this.activeLightboxPresentationId) {
      this.isLightboxPresented = false;
      this.activeLightboxPresentationId = 0;
      if (this.sheetLifecycleState !== 'presenting') {
        this.modalRoute = null;
      }
      return true;
    }
    if (
      this.sheetLifecycleState !== 'presenting' ||
      presentationId !== this.activeSheetPresentationId
    ) {
      return false;
    }
    this.pendingModalRoute = null;
    this.pendingPresentationId = 0;
    this.isSheetModalPresented = false;
    if (this.mountedSheetPresentationId > 0 && this.mountedSheetPresentationId === presentationId) {
      this.sheetLifecycleState = 'dismissing';
      this.modalRoute = null;
      return true;
    }
    this.completedSheetDisappearIds.advanceIdleWatermark(this.modalPresentationId);
    this.activeSheetPresentationId = 0;
    this.mountedSheetPresentationId = 0;
    this.activeSheetAppeared = false;
    this.sheetLifecycleState = 'idle';
    this.modalRoute = null;
    return true;
  }

  public isModalKindOpen(kind: OrbitModalRouteKind): boolean {
    if (this.pendingModalRoute !== null) {
      return this.pendingModalRoute.kind === kind;
    }
    return this.isModalSheetPresented && this.modalRoute !== null && this.modalRoute.kind === kind;
  }

  public updateFoldPosture(isFoldable: boolean, posture: string, sidePanelOpen: boolean): void {
    this.isFoldableDevice = isFoldable;
    this.foldPosture = posture;
    const prevBp = this.breakpoint;
    const nextBp = resolveOrbitBreakpoint(this.windowWidthVp, this.foldPosture);
    this.breakpoint = nextBp;
    if (posture === 'folded') {
      this.sidebarOpen = false;
      this.sidebarAutoYieldedForSidePanel = false;
    } else if (posture === 'expanded' && nextBp !== 'sm') {
      if (nextBp === 'md' && sidePanelOpen) {
        this.isSidebarCollapsed = true;
        this.sidebarOpen = false;
        this.sidebarAutoYieldedForSidePanel = true;
      } else if (!this.isSidebarCollapsed) {
        this.sidebarOpen = true;
      }
    } else if (prevBp !== nextBp) {
      this.reconcileBreakpointTransition(prevBp, nextBp, sidePanelOpen);
    }
  }

  public updateWindowSize(widthVp: number, heightVp: number, sidePanelOpen: boolean): void {
    if (heightVp > 0) {
      this.windowHeightVp = heightVp;
    }
    if (widthVp > 0) {
      const prevBp = this.breakpoint;
      this.windowWidthVp = widthVp;
      const nextBp = resolveOrbitBreakpoint(widthVp, this.foldPosture);
      this.breakpoint = nextBp;
      if (prevBp !== nextBp) {
        this.reconcileBreakpointTransition(prevBp, nextBp, sidePanelOpen);
      }
    }
  }

  private reconcileBreakpointTransition(
    prevBp: OrbitBreakpoint,
    nextBp: OrbitBreakpoint,
    sidePanelOpen: boolean
  ): void {
    if (nextBp === 'sm') {
      this.sidebarOpen = false;
      this.sidebarAutoYieldedForSidePanel = false;
      return;
    }
    if (nextBp === 'md') {
      if (sidePanelOpen) {
        if (!this.isSidebarCollapsed) {
          this.sidebarAutoYieldedForSidePanel = true;
        }
        this.isSidebarCollapsed = true;
        this.sidebarOpen = false;
      } else if (prevBp === 'sm') {
        this.sidebarOpen = !this.isSidebarCollapsed;
      }
      return;
    }
    // nextBp === 'lg'
    if (this.sidebarAutoYieldedForSidePanel) {
      this.sidebarAutoYieldedForSidePanel = false;
      this.isSidebarCollapsed = false;
    }
    this.sidebarOpen = !this.isSidebarCollapsed;
  }

  /**
   * Reconciles `md` (680-1023 vp) left sidebar yield when the right side panel opens or closes.
   * Opening right panel in `md` auto-collapses left sidebar so Stage stays >= 360 vp;
   * closing right panel restores left sidebar if it was auto-yielded.
   */
  public onSidePanelOpenChanged(nextPanelOpen: boolean): void {
    if (this.breakpoint !== 'md') {
      if (!nextPanelOpen) {
        this.sidebarAutoYieldedForSidePanel = false;
      }
      return;
    }
    if (nextPanelOpen) {
      if (!this.isSidebarCollapsed) {
        this.sidebarAutoYieldedForSidePanel = true;
        this.isSidebarCollapsed = true;
        this.sidebarOpen = false;
      }
    } else if (this.sidebarAutoYieldedForSidePanel) {
      this.sidebarAutoYieldedForSidePanel = false;
      this.isSidebarCollapsed = false;
      this.sidebarOpen = true;
    }
  }

  public toggleSidebar(sidePanelOpen: boolean, dismissSidePanel?: () => void): void {
    if (this.breakpoint === 'sm') {
      this.sidebarOpen = !this.sidebarOpen;
      return;
    }
    const willExpand = this.isSidebarCollapsed;
    this.isSidebarCollapsed = !willExpand;
    this.sidebarOpen = willExpand;
    this.sidebarAutoYieldedForSidePanel = false;
    // In md, expanding the left sidebar yields the right panel so Stage stays >= 360 vp.
    if (willExpand && this.breakpoint === 'md' && sidePanelOpen && dismissSidePanel !== undefined) {
      dismissSidePanel();
    }
  }

  public setSidebarWidthLgVp(widthVp: number, persist: boolean = true): void {
    const clamped = clampSidebarLgWidth(widthVp);
    this.sidebarWidthLgVp = clamped;
    if (persist && this.widthStorage !== null) {
      this.widthStorage.setSidebarWidthVp(clamped);
    }
  }

  public setSidePanelWidthVp(widthVp: number, persist: boolean = true): void {
    const sidebarW = this.breakpoint === 'lg' && !this.isSidebarCollapsed ? this.sidebarWidthLgVp : 0;
    const clamped = clampSidePanelWidthForBreakpoint(
      widthVp,
      this.breakpoint === 'sm' ? 'lg' : this.breakpoint,
      this.windowWidthVp,
      sidebarW
    );
    this.sidePanelWidthVp = clamped;
    if (persist && this.widthStorage !== null) {
      this.widthStorage.setSidePanelWidthVp(clamped);
    }
  }

  public updateTitleButtonAvoidWidth(rightAvoidVp: number, decorHidden: boolean): void {
    this.isFreeWindowDecorHidden = decorHidden;
    this.titleButtonRightAvoidVp = decorHidden ? Math.max(0, Math.round(rightAvoidVp)) : 0;
  }

  public resolveLayout(sidePanelOpen: boolean): OrbitShellLayoutGeometry {
    const sidebarRequested = this.breakpoint === 'sm'
      ? this.sidebarOpen
      : !this.isSidebarCollapsed;
    return computeOrbitShellLayout(
      this.windowWidthVp,
      this.foldPosture,
      sidebarRequested,
      this.sidebarWidthLgVp,
      sidePanelOpen,
      this.sidePanelWidthVp,
      this.safeLeftVp + this.safeRightVp
    );
  }

  public resolvedSidebarWidthVp(): number {
    if (this.breakpoint === 'sm') {
      return Math.min(320, Math.max(280, Math.floor(this.windowWidthVp * 0.82)));
    }
    if (this.breakpoint === 'md') {
      return ORBIT_SIDEBAR_MD_WIDTH_VP;
    }
    return clampSidebarLgWidth(this.sidebarWidthLgVp);
  }

  public resolvedMinSideBarWidth(): number {
    return this.breakpoint === 'lg' ? ORBIT_SIDEBAR_LG_MIN_WIDTH_VP : ORBIT_SIDEBAR_MD_WIDTH_VP;
  }

  public resolvedMaxSideBarWidth(): number {
    if (this.breakpoint === 'sm') {
      return 320;
    }
    return this.breakpoint === 'lg' ? ORBIT_SIDEBAR_LG_MAX_WIDTH_VP : ORBIT_SIDEBAR_MD_WIDTH_VP;
  }

  public resolvedTopBarRightPaddingVp(): number {
    return OrbitTokens.SPACE_LG + (this.isFreeWindowDecorHidden ? this.titleButtonRightAvoidVp : 0);
  }
}
