import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  DividerShowType,
  HdsNavigation,
  HdsNavigationTitleMode,
  ScrollEffectType
} from '@kit.UIDesignKit';
import { HostCardDto } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { orbitLogWarn } from '../../model/OrbitLogger';
import {
  ORBIT_KEYCODE_ALT_LEFT,
  ORBIT_KEYCODE_ALT_RIGHT,
  ORBIT_KEYCODE_BACKSLASH,
  ORBIT_KEYCODE_CTRL_LEFT,
  ORBIT_KEYCODE_CTRL_RIGHT,
  ORBIT_KEYCODE_K,
  ORBIT_KEYCODE_META_LEFT,
  ORBIT_KEYCODE_META_RIGHT,
  ORBIT_KEYCODE_N,
  ORBIT_KEYCODE_SHIFT_LEFT,
  ORBIT_KEYCODE_SHIFT_RIGHT,
  ORBIT_KEYCODE_T,
  ORBIT_KEYCODE_W,
  OrbitShellLayoutGeometry,
  OrbitShellStore,
  OrbitShortcutAction,
  OrbitShortcutContext,
  resolveOrbitKeyboardShortcut
} from '../../model/stores/OrbitShellStore';
import {
  OrbitTokens,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import {
  dismissHarmonyKeyboard,
  resolveHostStatusColor,
  resolveSelectedHost
} from '../common/OrbitCommonControls';
import {
  TopBarDisplayModel,
  isWorkspaceCwdUnset,
  resolveConnectionStateResource,
  resolveTopBarDisplayModel
} from './TopBarDisplayModel';
import { FleetSidebar } from './FleetSidebar';
import { OrbitModalContentRouter } from './OrbitModalContentRouter';
import { OrbitSidePanelContainer } from '../panel/OrbitSidePanelContainer';
import { SubagentCrewMiniDock } from '../panel/SubagentCrewMiniDock';
import { PersonalAgentBriefingPanel } from '../personal-agent/PersonalAgentBriefingPanel';
import { PersonalAgentSetupCard } from '../personal-agent/PersonalAgentSetupCard';
import { StreamStage } from '../stage/StreamStage';
import { PlanReviewCard } from '../stage/PlanReviewCard';
import { StickyDecisionDock } from '../stage/StickyDecisionDock';
import { OmniComposer } from '../composer/OmniComposer';

const BRIEFING_RAIL_WIDTH_VP: number = 304;

/**
 * Three-form-factor responsive workspace shell + single-modal router:
 * - `sm` (`< 680 vp`): Overlay sidebar drawer + bottom-Sheet side panel (with unread dot on auto open)
 * - `md` (`680–1023 vp`): Embed sidebar (260 vp, auto-yields when right panel opens) + inline right panel (320–420 vp), Stage >= 360 vp
 * - `lg` (`>= 1024 vp`): Three panes side-by-side with persisted draggable sidebar (240–360 vp) and right panel (320–680 vp), Stage >= 360 vp
 * - Half-folded hover: `FolderStack` (upper Stage, lower Composer, honoring `reducedMotion`)
 * - Single `bindSheet` for `shellStore.modalRoute` + single unified `OrbitSidePanelContainer`
 * - External keyboard shortcuts on `md`/`lg`
 */
@ComponentV2
export struct AdaptiveWorkspace {
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private shellStore: OrbitShellStore = orbitRuntime().shellStore;
  private mainNavStack: NavPathStack = new NavPathStack();
  @Local private isDraggingSidebarWidth: boolean = false;
  private sidebarDragStartWidthVp: number = 280;
  private pressedCtrl: boolean = false;
  private pressedMeta: boolean = false;
  private pressedShift: boolean = false;
  private pressedAlt: boolean = false;
  private lastShortcutSig: string = '';
  private lastShortcutTs: number = 0;

  @Monitor('store.toastRevision')
  onToastRevisionChanged(_mon: IMonitor): void {
    const msg = this.store.toastMessage.trim();
    if (msg.length === 0) {
      return;
    }
    try {
      this.getUIContext().getPromptAction().openToast({
        message: msg,
        duration: 2000
      }).catch((err: Object) => {
        orbitLogWarn('OrbitToast', 'openToast failed', err);
      });
    } catch (err) {
      orbitLogWarn('OrbitToast', 'PromptAction unavailable', err as Object);
    }
  }

  @Computed
  get shellLayout(): OrbitShellLayoutGeometry {
    return this.store.shellStore.resolveLayout(this.store.sidePanelStore.isOpen);
  }

  private isCompactLayout(): boolean {
    return this.store.shellStore.breakpoint === 'sm';
  }

  private isExpandedDesktopLayout(): boolean {
    return this.store.windowWidthVp >= OrbitTokens.BP_EXPANDED_MIN;
  }

  private isHalfFoldedHover(): boolean {
    return this.store.shellStore.isHalfFoldedHover;
  }

  private isShowingPaSetup(): boolean {
    return this.hasPairedHosts() && this.store.showPersonalAgentSetup;
  }

  private isInPersonalAgentSession(): boolean {
    return this.hasPairedHosts() && !this.store.showPersonalAgentSetup && this.store.isCurrentSessionPersonalAgent();
  }

  private shouldDockBriefingRail(): boolean {
    return this.isExpandedDesktopLayout() &&
      this.isInPersonalAgentSession() &&
      this.store.showBriefingPanel;
  }

  private hasPairedHosts(): boolean {
    return this.store.hosts.length > 0;
  }

  private selectedHost(): HostCardDto | null {
    const resolved = resolveSelectedHost(this.store);
    if (resolved !== null) {
      return resolved;
    }
    return this.store.hosts.length > 0 ? this.store.hosts[0] : null;
  }

  private selectedHostDisplayName(): string {
    const host = this.selectedHost();
    if (host === null) {
      return '';
    }
    const rawHost = host.display_name.trim();
    return rawHost.length > 0 ? rawHost : host.host_id;
  }

  private hasActiveConversation(): boolean {
    return this.hasPairedHosts() &&
      !this.store.showPersonalAgentSetup &&
      this.store.currentAgent !== null &&
      !this.store.isNewSessionDraft;
  }

  private isDraftTitle(): boolean {
    return this.hasPairedHosts() && !this.store.showPersonalAgentSetup && this.store.currentAgent === null;
  }

  private isWorkspaceCwdUnset(): boolean {
    return isWorkspaceCwdUnset(this.store.activeCwd);
  }

  private isConnectionHealthy(): boolean {
    const host = this.selectedHost();
    if (host === null) {
      return false;
    }
    return host.connection_state === 'online_direct' || host.connection_state === 'online_relay';
  }

  private currentTopBarModel(): TopBarDisplayModel {
    const host = this.selectedHost();
    return resolveTopBarDisplayModel({
      hasPairedHosts: this.hasPairedHosts(),
      isShowingPaSetup: this.isShowingPaSetup(),
      isPersonalAgentSession: this.isInPersonalAgentSession(),
      isDraft: this.isDraftTitle(),
      currentAgentTitle: this.store.currentAgent?.title ?? '',
      activeCwd: this.store.activeCwd,
      hostName: this.selectedHostDisplayName(),
      hostCount: this.store.hosts.length,
      isConnectionHealthy: this.isConnectionHealthy(),
      connectionState: host?.connection_state ?? '',
      viaSshTunnel: host?.via_ssh_tunnel === true,
      gitBranch: this.store.resolveWorkspaceBranch(this.store.activeCwd)
    });
  }

  private topBarTitleResource(): ResourceStr {
    const model = this.currentTopBarModel();
    if (model.titleKind === 'orbit') {
      return 'Orbit'; // i18n-ignore
    }
    if (model.titleKind === 'pa') {
      return $r('app.string.sidebar_personalRouter');
    }
    if (model.titleKind === 'text' && model.titleText.length > 0) {
      return model.titleText;
    }
    return $r('app.string.sidebar_newConversation');
  }

  private topStatusDotColor(): ResourceColor {
    return resolveHostStatusColor(this.selectedHost(), this.store);
  }

  private topBarSubtitleResource(): ResourceStr {
    const model = this.currentTopBarModel();
    if (model.subtitleKind === 'no_hosts') {
      return $r('app.string.sidebar_noHostsTitle');
    }
    if (model.subtitleKind === 'hosts_count') {
      return $r('app.string.sidebar_hostsCount', this.store.hosts.length);
    }
    if (model.subtitleKind === 'select_project') {
      return $r('app.string.sidebar_selectProject');
    }
    if (model.subtitleKind === 'new_conv') {
      return $r('app.string.sidebar_newConversation');
    }
    if (model.subtitleKind === 'host_select_project') {
      return $r('app.string.sidebar_hostSelectProject', model.subtitleHostName);
    }
    return model.subtitleText;
  }

  private isTopBarConnTargetSubtitle(): boolean {
    return this.currentTopBarModel().subtitleKind === 'conn_target';
  }

  private topBarConnStateResource(): Resource {
    const host = this.selectedHost();
    return resolveConnectionStateResource(host?.connection_state ?? '', host?.via_ssh_tunnel === true);
  }

  private topBarConnTargetText(): string {
    return this.currentTopBarModel().subtitleTarget;
  }

  private diffFileSubtitle(): ResourceStr | undefined {
    const count = this.store.totalDiffFileCount();
    if (count <= 0) {
      return undefined;
    }
    return count === 1
      ? $r('app.string.sidebar_filesCountOne')
      : $r('app.string.sidebar_filesCountMany', count);
  }

  private terminalSubtitle(): ResourceStr | undefined {
    const count = this.store.activeTerminalCount();
    return count > 0 ? $r('app.string.sidebar_activeCount', count) : undefined;
  }

  private paNeedsYouCount(): number {
    return this.store.briefing !== null ? this.store.briefing.counts.needs_you : 0;
  }

  private openFolderFromHeader(): void {
    dismissHarmonyKeyboard(this.getUIContext());
    this.store.browseDirectory(this.isWorkspaceCwdUnset() ? null : this.store.activeCwd);
  }

  private modalSheetDetents(): [(SheetSize | Length), (SheetSize | Length)?, (SheetSize | Length)?] {
    const kind = this.store.shellStore.modalRoute?.kind;
    if (
      kind === 'host_settings' ||
      kind === 'pa_briefing' ||
      kind === 'image_viewer' ||
      kind === 'diagram_viewer'
    ) {
      return [SheetSize.LARGE];
    }
    if (kind === 'work_picker' || kind === 'archived') {
      return [SheetSize.MEDIUM, SheetSize.LARGE];
    }
    return [SheetSize.LARGE, SheetSize.MEDIUM];
  }

  private modalSheetBgColor(): ResourceColor {
    const kind = this.store.shellStore.modalRoute?.kind;
    if (kind === 'image_viewer') {
      return this.store.palette.scrim;
    }
    if (
      kind === 'global_search' ||
      kind === 'export_transcript' ||
      kind === 'pa_briefing' ||
      kind === 'diagram_viewer'
    ) {
      return this.store.palette.canvas;
    }
    return this.store.palette.drawer;
  }

  private closeCurrentPanelTabShortcut(): void {
    if (!this.store.isSidePanelActuallyVisible()) {
      return;
    }
    const activeId = this.store.sidePanelStore.activeTabId;
    if (activeId.length > 0) {
      this.store.sidePanelStore.close(activeId);
    } else if (this.store.sidePanelStore.isOpen) {
      this.store.sidePanelStore.dismissPanel();
    }
  }

  private buildShortcutContext(): OrbitShortcutContext {
    return {
      breakpoint: this.store.shellStore.breakpoint,
      isModalOpen: this.store.shellStore.isModalSheetPresented,
      isTextInputFocused: this.store.shellStore.isTextInputFocused,
      isTerminalFocused: this.store.shellStore.isTerminalFocused,
      isSidePanelVisible: this.store.isSidePanelActuallyVisible()
    };
  }

  private executeShortcutAction(action: OrbitShortcutAction): boolean {
    if (action === 'none') {
      return false;
    }
    const now = Date.now();
    if (this.lastShortcutSig === action && now - this.lastShortcutTs < 45) {
      return true;
    }
    this.lastShortcutSig = action;
    this.lastShortcutTs = now;
    if (action === 'toggle_sidebar') {
      this.store.toggleSidebar();
      return true;
    }
    if (action === 'open_launchpad') {
      this.store.sidePanelStore.openLaunchpad();
      return true;
    }
    if (action === 'reopen_last_tab') {
      this.store.sidePanelStore.reopenLast();
      return true;
    }
    if (action === 'close_panel_tab') {
      this.closeCurrentPanelTabShortcut();
      return true;
    }
    if (action === 'global_search') {
      this.store.openGlobalSearch();
      return true;
    }
    if (action === 'new_session') {
      this.store.startNewSessionDraft();
      return true;
    }
    return false;
  }

  private dispatchAnchoredCtrlShortcut(keyCode: number, hasShift: boolean): void {
    const action = resolveOrbitKeyboardShortcut(
      keyCode,
      true,
      false,
      hasShift,
      false,
      this.buildShortcutContext()
    );
    this.executeShortcutAction(action);
  }

  private queryModifierState(event: KeyEvent, modName: string, fallback: boolean): boolean {
    if (event.getModifierKeyState !== undefined) {
      try {
        return event.getModifierKeyState([modName]);
      } catch (_err) {
        return fallback;
      }
    }
    return fallback;
  }

  private handleWorkspaceKeyEvent(event: KeyEvent): void {
    const kc = event.keyCode;
    const isDown = event.type === KeyType.Down;
    if (kc === ORBIT_KEYCODE_CTRL_LEFT || kc === ORBIT_KEYCODE_CTRL_RIGHT) {
      this.pressedCtrl = isDown;
      return;
    }
    if (kc === ORBIT_KEYCODE_META_LEFT || kc === ORBIT_KEYCODE_META_RIGHT) {
      this.pressedMeta = isDown;
      return;
    }
    if (kc === ORBIT_KEYCODE_SHIFT_LEFT || kc === ORBIT_KEYCODE_SHIFT_RIGHT) {
      this.pressedShift = isDown;
      return;
    }
    if (kc === ORBIT_KEYCODE_ALT_LEFT || kc === ORBIT_KEYCODE_ALT_RIGHT) {
      this.pressedAlt = isDown;
      return;
    }
    if (!isDown) {
      return;
    }

    const hasCtrl = this.queryModifierState(event, 'Ctrl', this.pressedCtrl);
    const hasShift = this.queryModifierState(event, 'Shift', this.pressedShift);
    const hasAlt = this.queryModifierState(event, 'Alt', this.pressedAlt);
    const hasMeta = event.metaKey === 1 || this.pressedMeta;

    const action = resolveOrbitKeyboardShortcut(
      kc,
      hasCtrl,
      hasMeta,
      hasShift,
      hasAlt,
      this.buildShortcutContext()
    );
    if (this.executeShortcutAction(action)) {
      event.stopPropagation();
    }
  }

  private sheetPresentationIds(): number[] {
    return this.shellStore.activeSheetPresentationId > 0
      ? [this.shellStore.activeSheetPresentationId]
      : [];
  }

  private lightboxPresentationIds(): number[] {
    return this.shellStore.activeLightboxPresentationId > 0
      ? [this.shellStore.activeLightboxPresentationId]
      : [];
  }

  @Builder
  private buildSingleModalSheet() {
    ForEach(
      this.sheetPresentationIds(),
      (sheetId: number) => {
        OrbitModalContentRouter({
          route: this.shellStore.modalRoute,
          presentationId: sheetId,
          onClose: (id: number) => {
            this.shellStore.closeModalByPresentationId(id);
          },
          onNativeAppear: (id: number) => {
            this.shellStore.onModalSheetAppear(id);
          },
          onNativeDisappear: (id: number) => {
            this.shellStore.onModalSheetDisappear(id);
          }
        })
      },
      (sheetId: number): string => `sheet_${sheetId}`
    )
  }

  @Builder
  private renderKeyboardShortcutAnchors() {
    // Dedicated one-shortcut-per-node host active on md/lg external keyboards for Ctrl combos
    // (paired with root `onKeyEvent` handling both Meta and Ctrl with full focus/modal/panel guards).
    if (!this.isCompactLayout()) {
      Row() {
        Blank()
          .width(0)
          .height(0)
          .keyboardShortcut('\\', [ModifierKey.CTRL], () => {
            this.dispatchAnchoredCtrlShortcut(ORBIT_KEYCODE_BACKSLASH, false);
          })

        Blank()
          .width(0)
          .height(0)
          .keyboardShortcut('t', [ModifierKey.CTRL], () => {
            this.dispatchAnchoredCtrlShortcut(ORBIT_KEYCODE_T, false);
          })

        Blank()
          .width(0)
          .height(0)
          .keyboardShortcut('t', [ModifierKey.CTRL, ModifierKey.SHIFT], () => {
            this.dispatchAnchoredCtrlShortcut(ORBIT_KEYCODE_T, true);
          })

        Blank()
          .width(0)
          .height(0)
          .keyboardShortcut('w', [ModifierKey.CTRL], () => {
            this.dispatchAnchoredCtrlShortcut(ORBIT_KEYCODE_W, false);
          })

        Blank()
          .width(0)
          .height(0)
          .keyboardShortcut('k', [ModifierKey.CTRL], () => {
            this.dispatchAnchoredCtrlShortcut(ORBIT_KEYCODE_K, false);
          })

        Blank()
          .width(0)
          .height(0)
          .keyboardShortcut('n', [ModifierKey.CTRL], () => {
            this.dispatchAnchoredCtrlShortcut(ORBIT_KEYCODE_N, false);
          })
      }
      .width(0)
      .height(0)
      .hitTestBehavior(HitTestMode.None)
    }
  }

  @Builder
  private renderTitleMenu() {
    Menu() {
      MenuItem({
        symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
          .fontColor([this.store.palette.iconPrimary]),
        content: $r('app.string.sidebar_openFolder')
      })
        .onClick(() => {
          this.openFolderFromHeader();
        })

      MenuItem({
        symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
          .fontColor([this.store.palette.iconPrimary]),
        content: $r('app.string.sidebar_copyPath')
      })
        .onClick(() => {
          this.store.copyToClipboard(this.store.activeCwd, '');
        })
    }
  }

  @Builder
  private renderMoreMenu() {
    Menu() {
      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.person_2'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.sidebar_personalRouter')
        })
          .onClick(() => {
            this.store.openPersonalAgentHome();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.magnifyingglass'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_search_fleet')
        })
          .onClick(() => {
            this.store.openGlobalSearch();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_file_explorer')
        })
          .onClick(() => {
            this.store.openFileExplorer();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_git_review'),
          labelInfo: this.diffFileSubtitle()
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.toggleTopBarGitDiff();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_terminal'),
          labelInfo: this.terminalSubtitle()
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.openPtyTerminal();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.doc_text'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_workspace_board')
        })
          .onClick(() => {
            this.store.openWorkspaceBoard();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.message'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_side_question')
        })
          .onClick(() => {
            this.store.openSideQuestionSheet();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.clock'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_automations')
        })
          .onClick(() => {
            this.store.openAutomationsSheet();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.house'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.briefing_ariaLabel')
        })
          .onClick(() => {
            this.store.openBriefingPanelTab();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.link'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_personal_router')
        })
          .onClick(() => {
            this.store.openRouterMeshSheet();
          })
      }

      if (this.hasActiveConversation()) {
        MenuItemGroup() {
          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_triangle_divide'))
              .fontColor([this.store.palette.iconPrimary]),
            content: $r('app.string.topbar_forkConversation')
          })
            .onClick(() => {
              this.store.forkCurrentSession(false);
            })

          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_triangle_divide'))
              .fontColor([this.store.palette.iconPrimary]),
            content: $r('app.string.topbar_forkInWorktree')
          })
            .onClick(() => {
              this.store.forkCurrentSession(true);
            })

          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.upload'))
              .fontColor([this.store.palette.iconPrimary]),
            content: $r('app.string.nav_export_transcript')
          })
            .onClick(() => {
              this.store.openExportTranscriptSheet();
            })

          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.archivebox'))
              .fontColor([this.store.palette.statusDanger]),
            content: $r('app.string.common_archive')
          })
            .contentFontColor(this.store.palette.statusDanger)
            .onClick(() => {
              if (this.store.currentAgent !== null) {
                this.store.archiveSession(this.store.currentAgent.agent_id);
              }
            })
        }
      }

      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.archivebox'))
            .fontColor([this.store.palette.iconPrimary]),
          content: $r('app.string.nav_archived_sessions')
        })
          .onClick(() => {
            this.store.openArchivedSessions();
          })
      }
    }
  }

  @Builder
  private renderHdsTitleBarContent() {
    Row({ space: OrbitTokens.SPACE_SM }) {
      Badge({
        count: (this.isCompactLayout() || this.store.shellStore.isSidebarCollapsed)
          ? this.store.totalPendingApprovals
          : 0,
        style: {
          badgeColor: this.store.palette.statusWarning,
          badgeSize: 8
        },
        position: BadgePosition.RightTop
      }) {
        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.open_sidebar'))
            .fontSize(20)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([
              !this.isCompactLayout() && this.store.shellStore.isSidebarCollapsed
                ? this.store.palette.iconSecondary
                : this.store.palette.iconPrimary
            ])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText(
          this.isCompactLayout()
            ? (this.store.shellStore.sidebarOpen
              ? $r('app.string.sidebar_closeDrawerAria')
              : $r('app.string.sidebar_openDrawerAria'))
            : (this.store.shellStore.isSidebarCollapsed
              ? $r('app.string.sidebar_expandSidebarAria')
              : $r('app.string.sidebar_collapseSidebarAria'))
        )
        .onClick(() => {
          dismissHarmonyKeyboard(this.getUIContext());
          this.store.toggleSidebar();
        })
      }

      if (this.hasPairedHosts() && this.isShowingPaSetup()) {
        Column({ space: 2 }) {
          Text($r('app.string.sidebar_personalRouter'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
          Text($r('app.string.sidebar_hostsCount', this.store.hosts.length))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
        .alignItems(HorizontalAlign.Start)
      } else if (this.hasPairedHosts() && this.isDraftTitle()) {
        Column({ space: 2 }) {
          Text(this.topBarTitleResource())
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text(this.topBarSubtitleResource())
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
        .alignItems(HorizontalAlign.Start)
        .onClick(() => {
          this.openFolderFromHeader();
        })
      } else if (this.hasPairedHosts()) {
        Column({ space: 2 }) {
          Row({ space: 4 }) {
            Text(this.topBarTitleResource())
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .flexShrink(1)

            SymbolGlyph($r('sys.symbol.chevron_down'))
              .fontSize(12)
              .fontColor([this.store.palette.fgMuted])
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)

          Row({ space: OrbitTokens.SPACE_XS }) {
            Row()
              .width(OrbitTokens.DOT_SIZE)
              .height(OrbitTokens.DOT_SIZE)
              .borderRadius(3)
              .backgroundColor(this.topStatusDotColor())

            if (this.isTopBarConnTargetSubtitle()) {
              Text(this.topBarConnStateResource())
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Normal))
                .maxLines(1)
                .flexShrink(0)
              if (this.topBarConnTargetText().length > 0) {
                Text('·') // i18n-ignore
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Normal))
                  .flexShrink(0)
                Text(this.topBarConnTargetText())
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Normal))
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .flexShrink(1)
              }
            } else {
              Text(this.topBarSubtitleResource())
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Normal))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .flexShrink(1)
            }
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
        .alignItems(HorizontalAlign.Start)
        .bindMenu(this.renderTitleMenu())
      } else {
        Column({ space: 2 }) {
          Text('Orbit')
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')

          Text($r('app.string.sidebar_noHostsTitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Normal))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
        .alignItems(HorizontalAlign.Start)
        .accessibilityText($r('app.string.sidebar_pairHostAria'))
        .onClick(() => {
          this.store.scanQrAndPair(this.getUIContext().getHostContext());
        })
      }

      if (this.hasPairedHosts()) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          // Briefing toggle button (on compact opens Briefing via single modal router; on wide toggles 304vp rail)
          if (this.isInPersonalAgentSession()) {
            Badge({
              count: this.paNeedsYouCount(),
              style: {
                badgeColor: this.store.palette.statusWarning,
                badgeSize: 8
              },
              position: BadgePosition.RightTop
            }) {
              Button({ type: ButtonType.Circle, stateEffect: true }) {
                Image($r('app.media.orbit_agent_bot'))
                  .width(18)
                  .height(18)
                  .objectFit(ImageFit.Contain)
                  .fillColor($r('app.color.icon_primary'))
                  .draggable(false)
              }
              .width(OrbitTokens.HIT_COMPACT)
              .height(OrbitTokens.HIT_COMPACT)
              .padding(0)
              .backgroundColor(
                (this.isExpandedDesktopLayout() ? this.store.showBriefingPanel : this.store.showBriefingSheet)
                  ? this.store.palette.accentPrimarySoft
                  : this.store.palette.subtleSurface
              )
              .accessibilityText(
                (this.isExpandedDesktopLayout() ? this.store.showBriefingPanel : this.store.showBriefingSheet)
                  ? $r('app.string.briefing_hideBriefing')
                  : $r('app.string.briefing_showBriefing')
              )
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                if (this.isExpandedDesktopLayout()) {
                  this.store.toggleBriefingPanel();
                } else {
                  this.store.openBriefingSheet();
                }
              })
            }
          }

          if (this.hasActiveConversation()) {
            SubagentCrewMiniDock({
              hostId: this.store.selectedHostId,
              rootAgentId: this.store.selectedAgentId
            })
          }

          // Side Panel toggle with unseen-tab red dot indicator (critical for sm auto-open)
          Badge({
            count: this.store.sidePanelStore.activeSnapshot.unseenTabIds.length,
            style: {
              badgeColor: this.store.palette.unreadBadge,
              badgeSize: 8
            },
            position: BadgePosition.RightTop
          }) {
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.square_grid_2x2'))
                .fontSize(18)
                .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                .fontColor([
                  this.store.sidePanelStore.isOpen
                    ? this.store.palette.accentPrimary
                    : this.store.palette.iconPrimary
                ])
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .padding(0)
            .backgroundColor(
              this.store.sidePanelStore.isOpen
                ? this.store.palette.accentPrimarySoft
                : this.store.palette.subtleSurface
            )
            .accessibilityText(
              this.store.sidePanelStore.isOpen
                ? $r('app.string.sidebar_collapseSidebarAria')
                : $r('app.string.sidebar_expandSidebarAria')
            )
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.sidePanelStore.togglePanel();
              this.store.triggerHaptic(8);
            })
          }

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.more'))
              .fontSize(20)
              .renderingStrategy(SymbolRenderingStrategy.SINGLE)
              .fontColor([this.store.palette.iconPrimary])
          }
          .width(OrbitTokens.HIT_COMPACT)
          .height(OrbitTokens.HIT_COMPACT)
          .padding(0)
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText($r('app.string.sidebar_moreActionsAria'))
          .bindMenu(this.renderMoreMenu())

          if (this.hasActiveConversation()) {
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.square_and_pencil'))
                .fontSize(19)
                .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                .fontColor([this.store.palette.iconPrimary])
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .padding(0)
            .backgroundColor(this.store.palette.subtleSurface)
            .accessibilityText($r('app.string.sidebar_newConversation'))
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.startNewSessionDraft();
            })
          }
        }
        .alignItems(VerticalAlign.Center)
      } else {
        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.gearshape'))
            .fontSize(20)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.sidebar_settings'))
        .onClick(() => {
          this.store.openHostSettings();
        })
      }
    }
    .width('100%')
    .constraintSize({ minHeight: OrbitTokens.TOP_BAR_HEIGHT })
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: this.store.shellStore.resolvedTopBarRightPaddingVp()
    })
    .alignItems(VerticalAlign.Center)
    .parallelGesture(
      TapGesture({ count: 1 }).onAction(() => {
        if (this.store.keyboardHeightVp > 0) {
          dismissHarmonyKeyboard(this.getUIContext());
        }
      })
    )
  }

  build() {
    Stack() {
      SideBarContainer(
        this.isCompactLayout()
          ? SideBarContainerType.Overlay
          : SideBarContainerType.Embed
      ) {
        // Left Pane: FleetSidebar + lg draggable right-edge resize sash (240–360 vp persisted)
        Row({ space: 0 }) {
          Column() {
            FleetSidebar()
          }
          .layoutWeight(1)
          .height('100%')

          if (this.store.shellStore.breakpoint === 'lg') {
            Stack({ alignContent: Alignment.Center }) {
              Column()
                .width(this.isDraggingSidebarWidth ? 2 : OrbitTokens.HAIRLINE)
                .height('100%')
                .backgroundColor(
                  this.isDraggingSidebarWidth
                    ? this.store.palette.borderStrong
                    : this.store.palette.hairline
                )
            }
            .width(6)
            .height('100%')
            .backgroundColor(Color.Transparent)
            .accessibilityText($r('app.string.sidebar_expandSidebarAria'))
            .gesture(
              PanGesture({ direction: PanDirection.Horizontal, distance: 2 })
                .onActionStart(() => {
                  this.isDraggingSidebarWidth = true;
                  this.sidebarDragStartWidthVp = this.store.shellStore.sidebarWidthLgVp;
                })
                .onActionUpdate((event: GestureEvent) => {
                  this.store.shellStore.setSidebarWidthLgVp(
                    this.sidebarDragStartWidthVp + event.offsetX,
                    false
                  );
                })
                .onActionEnd(() => {
                  this.isDraggingSidebarWidth = false;
                  this.store.shellStore.setSidebarWidthLgVp(
                    this.store.shellStore.sidebarWidthLgVp,
                    true
                  );
                })
                .onActionCancel(() => {
                  this.isDraggingSidebarWidth = false;
                })
            )
          }
        }
        .width('100%')
        .height('100%')

        // Center + Right Pane Host
        Stack({ alignContent: Alignment.Top }) {
          HdsNavigation(this.mainNavStack) {
            Row({ space: 0 }) {
              if (this.isShowingPaSetup()) {
                Column() {
                  PersonalAgentSetupCard()
                }
                .layoutWeight(1)
                .constraintSize({ minWidth: this.isCompactLayout() ? 0 : 360 })
                .height('100%')
                .backgroundColor(this.store.palette.canvas)
              } else if (this.isHalfFoldedHover()) {
                FolderStack({ upperItems: ['orbit_stage_upper'] }) {
                  Column() {
                    StreamStage()
                  }
                  .id('orbit_stage_upper')
                  .width('100%')
                  .height('100%')
                  .parallelGesture(
                    TapGesture({ count: 1 }).onAction(() => {
                      if (this.store.keyboardHeightVp > 0) {
                        dismissHarmonyKeyboard(this.getUIContext());
                      }
                    })
                  )

                  Column() {
                    if (this.hasPairedHosts()) {
                      PlanReviewCard()
                      StickyDecisionDock()
                      OmniComposer()
                    }
                  }
                  .width('100%')
                  .alignItems(HorizontalAlign.Center)
                }
                .enableAnimation(!this.store.shellStore.reducedMotion)
                .autoHalfFold(true)
                .alignContent(Alignment.Bottom)
                .layoutWeight(1)
                .constraintSize({ minWidth: this.isCompactLayout() ? 0 : 360 })
                .height('100%')
                .backgroundColor(this.store.palette.canvas)
              } else {
                Column() {
                  Column() {
                    StreamStage()
                  }
                  .width('100%')
                  .layoutWeight(1)
                  .parallelGesture(
                    TapGesture({ count: 1 }).onAction(() => {
                      if (this.store.keyboardHeightVp > 0) {
                        dismissHarmonyKeyboard(this.getUIContext());
                      }
                    })
                  )

                  if (this.hasPairedHosts()) {
                    PlanReviewCard()
                    StickyDecisionDock()
                    OmniComposer()
                  }
                }
                .layoutWeight(1)
                .constraintSize({ minWidth: this.isCompactLayout() ? 0 : 360 })
                .height('100%')
                .alignItems(HorizontalAlign.Center)
                .backgroundColor(this.store.palette.canvas)
              }

              // Wide (>= 1024vp) persistent 304vp Personal Agent Briefing rail (yields when side panel opens)
              if (this.shouldDockBriefingRail() && !this.store.sidePanelStore.isOpen) {
                Column() {
                  PersonalAgentBriefingPanel({
                    embedded: true,
                    onClose: () => {
                      this.store.closeBriefingPanel();
                    }
                  })
                }
                .width(BRIEFING_RAIL_WIDTH_VP)
                .height('100%')
                .border({
                  width: { left: OrbitTokens.HAIRLINE },
                  color: this.store.palette.hairline
                })
              }

              // Single unified Side Panel Container (sm or tight insets: bottom Sheet; md/lg: inline right pane)
              if (this.hasPairedHosts()) {
                OrbitSidePanelContainer({
                  inlineWidthVp: this.shellLayout.sidePanelWidthVp,
                  sidePanelInline: this.shellLayout.sidePanelInline,
                  launchpadHostId: this.store.selectedHostId,
                  launchpadAgentId: this.store.isNewSessionDraft
                    ? null
                    : (this.store.selectedAgentId.length > 0 ? this.store.selectedAgentId : null),
                  launchpadCwd: this.store.activeCwd
                })
              }
            }
            .width('100%')
            .height('100%')
            .padding({
              left: this.store.safeLeftVp,
              right: this.store.safeRightVp
            })
          }
          .titleMode(HdsNavigationTitleMode.MINI)
          .mode(NavigationMode.Stack)
          .hideBackButton(true)
          .titleBar({
            avoidLayoutSafeArea: true,
            enableComponentSafeArea: true,
            style: {
              scrollEffectOpts: {
                enableScrollEffect: true,
                scrollEffectType: ScrollEffectType.COMMON_BLUR
              },
              originalStyle: {
                backgroundStyle: {
                  backgroundColor: this.store.palette.canvas
                }
              }
            },
            content: {
              stackBuilder: (): void => {
                this.renderHdsTitleBarContent();
              },
              divider: {
                showType: this.isDraftTitle() ? DividerShowType.OFF : DividerShowType.AUTO
              }
            }
          })
          .width('100%')
          .height('100%')
          .backgroundColor(this.store.palette.canvas)

          // Compact Overlay Drawer Scrim — extends edge-to-edge across SYSTEM + CUTOUT safe areas
          if (this.isCompactLayout() && this.store.shellStore.sidebarOpen) {
            Column()
              .width('100%')
              .height('100%')
              .backgroundColor(this.store.palette.scrim)
              .expandSafeArea(
                [SafeAreaType.SYSTEM, SafeAreaType.CUTOUT],
                [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM, SafeAreaEdge.START, SafeAreaEdge.END]
              )
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                this.store.shellStore.sidebarOpen = false;
              })
          }
        }
        .width('100%')
        .height('100%')
        .backgroundColor(this.store.palette.canvas)
      }
      .showControlButton(false)
      .showSideBar(this.shellLayout.sidebarVisible)
      .sideBarWidth(this.shellLayout.sidebarWidthVp)
      .minSideBarWidth(this.store.shellStore.resolvedMinSideBarWidth())
      .maxSideBarWidth(this.store.shellStore.resolvedMaxSideBarWidth())
      .divider({
        strokeWidth: this.isCompactLayout() || this.store.shellStore.breakpoint === 'lg' ? 0 : OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
      .onChange((isOpen: boolean) => {
        if (this.isCompactLayout()) {
          this.store.shellStore.sidebarOpen = isOpen;
        } else {
          this.store.shellStore.isSidebarCollapsed = !isOpen;
          this.store.shellStore.sidebarOpen = isOpen;
        }
      })
      .width('100%')
      .height('100%')

      // External keyboard shortcut host (md / lg)
      this.renderKeyboardShortcutAnchors()

      // Sole bindSheet in AdaptiveWorkspace: Standard Single Modal Sheet Router.
      // Exit confirmation is routed EXCLUSIVELY through the mounted `OrbitModalContentRouter`'s
      // immutable `presentationId` (`aboutToAppear` / `aboutToDisappear`) — NO secondary mutable
      // `bindSheet.onDisappear` callback reading live state (R3-P1-8).
      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.shellStore.isSheetModalPresented, this.buildSingleModalSheet(), {
          detents: this.modalSheetDetents(),
          dragBar: true,
          showClose: false,
          backgroundColor: this.modalSheetBgColor()
        })

      // True Full-Screen Edge-to-Edge Lightbox Host for `image_viewer` & `diagram_viewer` (P1-9 + R3-P1-8).
      // Keyed by `activeLightboxPresentationId` via `ForEach` so consecutive image->diagram swaps
      // mount a fresh `OrbitModalContentRouter` capturing the new immutable `presentationId`.
      if (this.store.shellStore.isLightboxPresented && this.store.shellStore.modalRoute !== null) {
        Column() {
          ForEach(
            this.lightboxPresentationIds(),
            (lbId: number) => {
              OrbitModalContentRouter({
                route: this.store.shellStore.modalRoute,
                presentationId: lbId,
                onClose: (id: number) => {
                  this.shellStore.closeModalByPresentationId(id);
                }
              })
            },
            (lbId: number): string => `lb_${lbId}`
          )
        }
        .width('100%')
        .height('100%')
        .zIndex(999)
        .backgroundColor(
          this.store.shellStore.modalRoute.kind === 'image_viewer'
            ? this.store.palette.scrim
            : this.store.palette.canvas
        )
        .expandSafeArea(
          [SafeAreaType.SYSTEM, SafeAreaType.CUTOUT],
          [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM, SafeAreaEdge.START, SafeAreaEdge.END]
        )
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
    .onKeyEvent((event: KeyEvent) => {
      this.handleWorkspaceKeyEvent(event);
    })
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      const w = Number(newArea.width);
      const h = Number(newArea.height);
      if (w > 0 || h > 0) {
        this.store.updateWindowSize(w, h);
      }
    })
  }
}
