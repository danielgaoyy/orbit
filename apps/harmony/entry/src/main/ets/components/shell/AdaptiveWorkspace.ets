import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  DividerShowType,
  HdsNavigation,
  HdsNavigationTitleMode,
  ScrollEffectType
} from '@kit.UIDesignKit';
import { HostCardDto } from '../../model/OrbitTypes';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitLogWarn } from '../../model/OrbitLogger';
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
import { PersonalAgentBriefingPanel } from '../personal-agent/PersonalAgentBriefingPanel';
import { PersonalAgentSetupCard } from '../personal-agent/PersonalAgentSetupCard';
import { StreamStage } from '../stage/StreamStage';
import { PlanReviewCard } from '../stage/PlanReviewCard';
import { StickyDecisionDock } from '../stage/StickyDecisionDock';
import { OmniComposer } from '../composer/OmniComposer';
import { ContextPickerSheet } from '../composer/ContextPickerSheet';
import { GitDiffSheet } from '../sheets/GitDiffSheet';
import { OrbitTerminalView } from '../sheets/OrbitTerminalView';
import { HostSettingsSheet } from '../sheets/HostSettingsSheet';
import { DirectoryBrowserSheet } from '../sheets/DirectoryBrowserSheet';
import { AutomationsSheet } from '../sheets/AutomationsSheet';
import { WorkspaceBoardSheet } from '../sheets/WorkspaceBoardSheet';
import { SideQuestionSheet } from '../sheets/SideQuestionSheet';
import { ArchivedSessionsSheet } from '../sheets/ArchivedSessionsSheet';
import { FileExplorerSheet } from '../sheets/FileExplorerSheet';
import { GlobalSearchSheet } from '../sheets/GlobalSearchSheet';
import { PersonalRouterMeshSheet } from '../sheets/PersonalRouterMeshSheet';
import { ExportTranscriptSheet } from '../sheets/ExportTranscriptSheet';
import { SubagentTimelineSheet } from '../sheets/SubagentTimelineSheet';

const BRIEFING_RAIL_WIDTH_VP: number = 304;

@ComponentV2
export struct AdaptiveWorkspace {
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  private mainNavStack: NavPathStack = new NavPathStack();

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

  private isPhoneLayout(): boolean {
    if (this.store.foldPosture === 'folded') {
      return true;
    }
    if (this.store.foldPosture === 'expanded' && this.store.windowWidthVp >= 540) {
      return false;
    }
    return this.store.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN;
  }

  private isExpandedDesktopLayout(): boolean {
    return this.store.windowWidthVp >= OrbitTokens.BP_EXPANDED_MIN;
  }

  private isHalfFoldedHover(): boolean {
    return this.store.isFoldableDevice && this.store.foldPosture === 'half_folded';
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

  private resolvedSideBarWidth(): number {
    if (this.isPhoneLayout()) {
      return Math.min(320, Math.max(280, Math.floor(this.store.windowWidthVp * 0.82)));
    }
    return OrbitTokens.SIDEBAR_FOLD_WIDTH;
  }

  private canShowInlineDiff(): boolean {
    return this.store.canShowInlineDiffInspector() && !this.shouldDockBriefingRail();
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
    return this.hasPairedHosts() && !this.store.showPersonalAgentSetup && this.store.currentAgent !== null;
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

  private buildFullHeightSheetOptions(
    onClose: () => void,
    bgColor?: ResourceColor
  ): SheetOptions {
    return {
      height: '100%',
      detents: ['100%'],
      preferType: SheetType.BOTTOM,
      dragBar: true,
      showClose: false,
      backgroundColor: bgColor !== undefined ? bgColor : this.store.palette.canvas,
      onDisappear: onClose
    };
  }

  private buildSheetOptions(
    detents: [(SheetSize | Length), (SheetSize | Length)?, (SheetSize | Length)?],
    onClose: () => void,
    bgColor?: ResourceColor
  ): SheetOptions {
    return {
      detents: detents,
      dragBar: true,
      showClose: false,
      backgroundColor: bgColor !== undefined ? bgColor : this.store.palette.drawer,
      onDisappear: onClose
    };
  }

  @Builder
  private buildGitDiffSheet() {
    GitDiffSheet({ isEmbeddedPane: false })
  }

  @Builder
  private buildPtyTerminalSheet() {
    OrbitTerminalView()
  }

  @Builder
  private buildContextPickerSheet() {
    ContextPickerSheet()
  }

  @Builder
  private buildHostSettingsSheet() {
    HostSettingsSheet()
  }

  @Builder
  private buildDirectoryBrowserSheet() {
    DirectoryBrowserSheet()
  }

  @Builder
  private buildAutomationsSheet() {
    AutomationsSheet()
  }

  @Builder
  private buildWorkspaceBoardSheet() {
    WorkspaceBoardSheet()
  }

  @Builder
  private buildSideQuestionSheet() {
    SideQuestionSheet()
  }

  @Builder
  private buildArchivedSessionsSheet() {
    ArchivedSessionsSheet()
  }

  @Builder
  private buildFileExplorerSheet() {
    FileExplorerSheet()
  }

  @Builder
  private buildGlobalSearchSheet() {
    GlobalSearchSheet()
  }

  @Builder
  private buildPersonalRouterMeshSheet() {
    PersonalRouterMeshSheet()
  }

  @Builder
  private buildExportTranscriptSheet() {
    ExportTranscriptSheet()
  }

  @Builder
  private buildSubagentTimelineSheet() {
    SubagentTimelineSheet()
  }

  @Builder
  private buildBriefingSheet() {
    PersonalAgentBriefingPanel({
      embedded: false,
      onClose: () => {
        this.store.closeBriefingSheet();
      }
    })
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
          content: this.canShowInlineDiff() && this.store.showInlineDiffInspector
            ? $r('app.string.sidebar_hideDiffReview')
            : $r('app.string.nav_git_review'),
          labelInfo: this.diffFileSubtitle()
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.toggleTopBarGitDiff();
          })

        if (this.canShowInlineDiff() && this.store.showInlineDiffInspector) {
          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_up_left_and_arrow_down_right'))
              .fontColor([this.store.palette.iconPrimary]),
            content: $r('app.string.sidebar_fullDiff')
          })
            .onClick(() => {
              this.store.openFullGitDiffSheet();
            })
        }

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
        count: (this.isPhoneLayout() || this.store.isSidebarCollapsed) ? this.store.totalPendingApprovals : 0,
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
              !this.isPhoneLayout() && this.store.isSidebarCollapsed
                ? this.store.palette.iconSecondary
                : this.store.palette.iconPrimary
            ])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText(
          this.isPhoneLayout()
            ? (this.store.sidebarOpen
              ? $r('app.string.sidebar_closeDrawerAria')
              : $r('app.string.sidebar_openDrawerAria'))
            : (this.store.isSidebarCollapsed
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
        Row({ space: OrbitTokens.SPACE_SM }) {
          // Briefing toggle button (on phone/compact opens full-height Sheet; on wide toggles 304vp rail)
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
    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
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
        this.isPhoneLayout()
          ? SideBarContainerType.Overlay
          : SideBarContainerType.Embed
      ) {
        FleetSidebar()

        Stack({ alignContent: Alignment.Top }) {
          HdsNavigation(this.mainNavStack) {
            Row() {
              if (this.isShowingPaSetup()) {
                Column() {
                  PersonalAgentSetupCard()
                }
                .layoutWeight(1)
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
                .enableAnimation(!this.store.reducedMotion)
                .autoHalfFold(true)
                .alignContent(Alignment.Bottom)
                .layoutWeight(1)
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
                .height('100%')
                .alignItems(HorizontalAlign.Center)
                .backgroundColor(this.store.palette.canvas)
              }

              // Wide (>= 1024vp) persistent 304vp Personal Agent Briefing rail
              if (this.shouldDockBriefingRail()) {
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
              } else if (
                this.canShowInlineDiff() &&
                this.hasPairedHosts() &&
                this.store.showInlineDiffInspector
              ) {
                Column() {
                  GitDiffSheet({ isEmbeddedPane: true })
                }
                .width(OrbitTokens.DIFF_PANE_EXPANDED_WIDTH)
                .height('100%')
                .border({
                  width: { left: OrbitTokens.HAIRLINE },
                  color: this.store.palette.hairline
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

          if (this.isPhoneLayout() && this.store.sidebarOpen) {
            Column()
              .width('100%')
              .height('100%')
              .backgroundColor(this.store.palette.scrim)
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                this.store.sidebarOpen = false;
              })
          }
        }
        .width('100%')
        .height('100%')
        .backgroundColor(this.store.palette.canvas)
      }
      .showControlButton(false)
      .showSideBar(
        this.isPhoneLayout()
          ? this.store.sidebarOpen
          : !this.store.isSidebarCollapsed
      )
      .sideBarWidth(this.resolvedSideBarWidth())
      .minSideBarWidth(240)
      .maxSideBarWidth(340)
      .divider({
        strokeWidth: this.isPhoneLayout() ? 0 : OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
      .onChange((isOpen: boolean) => {
        if (this.isPhoneLayout()) {
          this.store.sidebarOpen = isOpen;
        } else {
          this.store.isSidebarCollapsed = !isOpen;
          this.store.sidebarOpen = isOpen;
        }
      })
      .width('100%')
      .height('100%')

      // Compact (< 1024vp) full-height Personal Agent Briefing Sheet (mutually exclusive with >=1024vp docked rail)
      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet(
          $$this.store.showBriefingSheet,
          this.buildBriefingSheet(),
          this.buildFullHeightSheetOptions(
            () => {
              this.store.closeBriefingSheet();
            },
            this.store.palette.canvas
          )
        )

      // Contextual tool & settings sheets (converged in PR-4 single modal router)
      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showGitDiffSheet, this.buildGitDiffSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.closeGitInspector();
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showPtySheet, this.buildPtyTerminalSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showPtySheet = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showContextPickerSheet, this.buildContextPickerSheet(), this.buildSheetOptions(
          [SheetSize.MEDIUM, SheetSize.LARGE],
          () => {
            this.store.showContextPickerSheet = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showHostSettingsSheet, this.buildHostSettingsSheet(), this.buildSheetOptions(
          [SheetSize.LARGE],
          () => {
            this.store.showHostSettingsSheet = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showDirBrowser, this.buildDirectoryBrowserSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showDirBrowser = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showAutomationsSheet, this.buildAutomationsSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showAutomationsSheet = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showWorkspaceBoardSheet, this.buildWorkspaceBoardSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showWorkspaceBoardSheet = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showSideQuestionSheet, this.buildSideQuestionSheet(), this.buildSheetOptions(
          [SheetSize.MEDIUM, SheetSize.LARGE],
          () => {
            this.store.showSideQuestionSheet = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showArchivedSessionsSheet, this.buildArchivedSessionsSheet(), this.buildSheetOptions(
          [SheetSize.MEDIUM, SheetSize.LARGE],
          () => {
            this.store.showArchivedSessionsSheet = false;
          },
          this.store.palette.drawer
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showFileExplorerSheet, this.buildFileExplorerSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showFileExplorerSheet = false;
          },
          this.store.palette.canvas
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showGlobalSearchSheet, this.buildGlobalSearchSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showGlobalSearchSheet = false;
          },
          this.store.palette.canvas
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showRouterMeshSheet, this.buildPersonalRouterMeshSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showRouterMeshSheet = false;
          },
          this.store.palette.canvas
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showExportTranscriptSheet, this.buildExportTranscriptSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showExportTranscriptSheet = false;
          },
          this.store.palette.canvas
        ))

      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showSubagentSheet, this.buildSubagentTimelineSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showSubagentSheet = false;
          },
          this.store.palette.canvas
        ))
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      const w = Number(newArea.width);
      const h = Number(newArea.height);
      if (w > 0 || h > 0) {
        this.store.updateWindowSize(w, h);
      }
    })
  }
}
