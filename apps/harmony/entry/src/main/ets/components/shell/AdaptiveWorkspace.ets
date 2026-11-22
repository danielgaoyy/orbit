import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  DividerShowType,
  HdsNavigation,
  HdsNavigationTitleMode,
  ScrollEffectType
} from '@kit.UIDesignKit';
import { HostCardDto, basenameOfPath, formatCompactTopBarConnectionLabel } from '../../model/OrbitTypes';
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
import { FleetSidebar } from './FleetSidebar';
import { StreamStage } from '../stage/StreamStage';
import { StickyDecisionDock } from '../stage/StickyDecisionDock';
import { OmniComposer } from '../composer/OmniComposer';
import { ContextPickerSheet } from '../composer/ContextPickerSheet';
import { GitDiffSheet } from '../sheets/GitDiffSheet';
import { PtyTerminalSheet } from '../sheets/PtyTerminalSheet';
import { HostSettingsSheet } from '../sheets/HostSettingsSheet';
import { DirectoryBrowserSheet } from '../sheets/DirectoryBrowserSheet';
import { AutomationsSheet } from '../sheets/AutomationsSheet';
import { WorkspaceBoardSheet } from '../sheets/WorkspaceBoardSheet';
import { SideQuestionSheet } from '../sheets/SideQuestionSheet';
import { ArchivedSessionsSheet } from '../sheets/ArchivedSessionsSheet';

@ComponentV2
export struct AdaptiveWorkspace {
  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
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
    return this.store.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN;
  }

  private resolvedSideBarWidth(): number {
    if (this.isPhoneLayout()) {
      return Math.min(320, Math.max(280, Math.floor(this.store.windowWidthVp * 0.82)));
    }
    return OrbitTokens.SIDEBAR_FOLD_WIDTH;
  }

  private canShowInlineDiff(): boolean {
    return this.store.canShowInlineDiffInspector();
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

  private hasActiveConversation(): boolean {
    return this.hasPairedHosts() && this.store.currentAgent !== null;
  }

  private isDraftTitle(): boolean {
    return this.hasPairedHosts() && this.store.currentAgent === null;
  }

  private isConnectionHealthy(): boolean {
    const host = this.selectedHost();
    if (host === null) {
      return false;
    }
    return host.connection_state === 'online_direct' || host.connection_state === 'online_relay';
  }

  private topBarTitle(): string {
    if (!this.hasPairedHosts()) {
      return 'Orbit';
    }
    if (this.store.currentAgent !== null && this.store.currentAgent.title.trim().length > 0) {
      return this.store.currentAgent.title.trim();
    }
    return 'New Conversation';
  }

  private topBarConnectionLabel(): string {
    const host = this.selectedHost();
    if (host !== null) {
      return formatCompactTopBarConnectionLabel(host.connection_state, host.via_ssh_tunnel);
    }
    return 'Offline';
  }

  private topBarWorkspaceLabel(): string {
    if (this.store.activeCwd.length > 0 && this.store.activeCwd !== '~') {
      return basenameOfPath(this.store.activeCwd);
    }
    return '~';
  }

  private topStatusDotColor(): ResourceColor {
    return resolveHostStatusColor(this.selectedHost(), this.store);
  }

  private topBarSubtitle(): string {
    const host = this.selectedHost();
    if (host === null) {
      return 'No Host Paired';
    }
    const repo = this.topBarWorkspaceLabel();
    const rawHost = host.display_name.trim();
    const hostName = rawHost.length > 0 ? rawHost : host.host_id;
    const isMultiHost = this.store.hosts.length > 1;
    if (!this.isConnectionHealthy()) {
      const target = isMultiHost ? hostName : repo;
      return `${this.topBarConnectionLabel()} · ${target}`;
    }
    if (isMultiHost) {
      return `${hostName} / ${repo}`;
    }
    const branch = this.store.resolveWorkspaceBranch(this.store.activeCwd);
    if (branch.length > 0) {
      return `${repo} · ${branch}`;
    }
    return repo;
  }

  private diffFileSubtitle(): string | undefined {
    const count = this.store.totalDiffFileCount();
    if (count <= 0) {
      return undefined;
    }
    return count === 1 ? '1 file' : `${count} files`;
  }

  private terminalSubtitle(): string | undefined {
    const count = this.store.activeTerminalCount();
    return count > 0 ? `${count} active` : undefined;
  }

  private openFolderFromHeader(): void {
    dismissHarmonyKeyboard(this.getUIContext());
    this.store.browseDirectory(this.store.activeCwd === '~' ? null : this.store.activeCwd);
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
    PtyTerminalSheet()
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
  private renderTitleMenu() {
    Menu() {
      MenuItem({
        symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
          .fontColor([this.store.palette.iconPrimary]),
        content: 'Open Folder'
      })
        .onClick(() => {
          this.openFolderFromHeader();
        })

      MenuItem({
        symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
          .fontColor([this.store.palette.iconPrimary]),
        content: 'Copy Path'
      })
        .onClick(() => {
          this.store.copyToClipboard(this.store.activeCwd, 'Copied path');
        })
    }
  }

  @Builder
  private renderMoreMenu() {
    Menu() {
      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_left_arrow_right'))
            .fontColor([this.store.palette.iconPrimary]),
          content: this.canShowInlineDiff() && this.store.showInlineDiffInspector
            ? 'Hide Diff Review'
            : 'Diff Review',
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
            content: 'Full Diff'
          })
            .onClick(() => {
              this.store.openFullGitDiffSheet();
            })
        }

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.code_square'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Terminal',
          labelInfo: this.terminalSubtitle()
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.openPtyTerminal();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.doc_text'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Board & Peers'
        })
          .onClick(() => {
            this.store.openWorkspaceBoard();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.message'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Side Question (/btw)'
        })
          .onClick(() => {
            this.store.openSideQuestionSheet();
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.clock'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Automations'
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
            content: 'Fork Conversation'
          })
            .onClick(() => {
              this.store.forkCurrentSession(false);
            })

          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_triangle_divide'))
              .fontColor([this.store.palette.iconPrimary]),
            content: 'Fork in Worktree'
          })
            .onClick(() => {
              this.store.forkCurrentSession(true);
            })

          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.square_and_square'))
              .fontColor([this.store.palette.iconPrimary]),
            content: 'Export Markdown'
          })
            .onClick(() => {
              this.store.exportCurrentSessionMarkdown();
            })

          MenuItem({
            symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.archivebox'))
              .fontColor([this.store.palette.statusDanger]),
            content: 'Archive'
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
          content: 'Archived Sessions'
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
            ? (this.store.sidebarOpen ? 'Close navigation drawer' : 'Open navigation drawer')
            : (this.store.isSidebarCollapsed ? 'Expand fleet sidebar' : 'Collapse fleet sidebar')
        )
        .onClick(() => {
          dismissHarmonyKeyboard(this.getUIContext());
          this.store.toggleSidebar();
        })
      }

      if (this.hasPairedHosts() && this.isDraftTitle()) {
        Blank()
          .layoutWeight(1)
      } else if (this.hasPairedHosts()) {
        Column({ space: 2 }) {
          Row({ space: 4 }) {
            Text(this.topBarTitle())
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

            Text(this.topBarSubtitle())
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Normal))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .flexShrink(1)
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

          Text('No Host Paired')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Normal))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .width('100%')
        }
        .layoutWeight(1)
        .justifyContent(FlexAlign.Center)
        .alignItems(HorizontalAlign.Start)
        .accessibilityText('Pair Host')
        .onClick(() => {
          this.store.scanQrAndPair(this.getUIContext().getHostContext());
        })
      }

      if (this.hasPairedHosts()) {
        Row({ space: OrbitTokens.SPACE_SM }) {
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
          .accessibilityText('More actions')
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
            .accessibilityText('New Conversation')
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
        .accessibilityText('Settings')
        .onClick(() => {
          this.store.openHostSettings();
        })
      }
    }
    .width('100%')
    .height(OrbitTokens.TOP_BAR_HEIGHT)
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
                  StickyDecisionDock()
                  OmniComposer()
                }
              }
              .layoutWeight(1)
              .height('100%')
              .backgroundColor(this.store.palette.canvas)

              if (
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

      // Transparent pass-through bindSheet anchors with `$$` two-way binding
      Column()
        .width('100%')
        .height('100%')
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.store.showGitDiffSheet, this.buildGitDiffSheet(), this.buildSheetOptions(
          [SheetSize.LARGE, SheetSize.MEDIUM],
          () => {
            this.store.showGitDiffSheet = false;
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
