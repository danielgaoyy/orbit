import { SymbolGlyphModifier } from '@kit.ArkUI';
import {
  HostCardDto,
  LaunchpadProjectDto,
  StageTurnDto,
  basenameOfPath,
  cwdPathsEquivalent,
  formatCompactTopBarConnectionLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitInputMono,
  orbitSurface,
  orbitTextBody,
  orbitTextMeta,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import {
  dismissHarmonyKeyboard,
  resolveHostStatusColor,
  resolveSelectedHost
} from '../common/OrbitCommonControls';
import { StageTurnView } from './StageTurnView';

@ComponentV2
export struct StreamStage {
  @Local firstVisibleIndex: number = 0;
  @Local lastVisibleIndex: number = 0;
  @Local atBottomEdge: boolean = true;
  @Local manualPairUri: string = '';
  @Local isRefreshing: boolean = false;

  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  private listScroller: Scroller = new Scroller();
  private isAutoAnchoringResize: boolean = false;
  private refreshTimerId: number = -1;
  private resizeTimerId: number = -1;

  aboutToDisappear(): void {
    if (this.refreshTimerId !== -1) {
      clearTimeout(this.refreshTimerId);
      this.refreshTimerId = -1;
    }
    if (this.resizeTimerId !== -1) {
      clearTimeout(this.resizeTimerId);
      this.resizeTimerId = -1;
    }
  }

  @Computed
  get activeHostCard(): HostCardDto | null {
    const resolved = resolveSelectedHost(this.store);
    if (resolved !== null) {
      return resolved;
    }
    return this.store.hosts.length > 0 ? this.store.hosts[0] : null;
  }

  @Computed
  get launchpadProjects(): LaunchpadProjectDto[] {
    return this.store.launchpadProjects();
  }

  @Computed
  get activeProjectLabel(): string {
    const projects = this.launchpadProjects;
    for (let i = 0; i < projects.length; i++) {
      if (cwdPathsEquivalent(projects[i].path, this.store.activeCwd)) {
        return projects[i].name;
      }
    }
    if (this.store.activeCwd.trim().length > 0 && this.store.activeCwd !== '~') {
      return basenameOfPath(this.store.activeCwd);
    }
    return '~';
  }

  private showJumpToLatest(): boolean {
    const totalItems = this.store.stageTurns.length;
    if (totalItems === 0) {
      return false;
    }
    if (this.lastVisibleIndex < totalItems - 1) {
      return true;
    }
    return !this.atBottomEdge;
  }

  private handlePullRefresh(): void {
    this.isRefreshing = true;
    this.store.pullRefreshStage();
    if (this.refreshTimerId !== -1) {
      clearTimeout(this.refreshTimerId);
    }
    this.refreshTimerId = setTimeout(() => {
      this.isRefreshing = false;
      this.refreshTimerId = -1;
    }, 450);
  }

  @Monitor('store.stageRevision')
  onStageUpdated(_mon: IMonitor): void {
    const totalItems = this.store.stageTurns.length;
    if (this.atBottomEdge && totalItems > 0) {
      this.lastVisibleIndex = totalItems - 1;
      this.listScroller.scrollEdge(Edge.Bottom);
    }
  }

  @Monitor('store.selectedAgentId')
  onSelectedAgentChanged(_mon: IMonitor): void {
    this.firstVisibleIndex = 0;
    this.lastVisibleIndex = Math.max(0, this.store.stageTurns.length - 1);
    this.atBottomEdge = true;
  }

  private isHostOnline(host: HostCardDto): boolean {
    return host.connection_state === 'online_direct' || host.connection_state === 'online_relay';
  }

  private hostSubtitle(host: HostCardDto): string {
    const route = formatCompactTopBarConnectionLabel(host.connection_state, host.via_ssh_tunnel);
    if (host.last_ping_ms !== null && host.last_ping_ms !== undefined) {
      return `${route} · ${host.last_ping_ms}ms`;
    }
    return route;
  }

  private isDraftSession(): boolean {
    return this.store.isNewSessionDraft || this.store.selectedAgentId.length === 0;
  }

  @Builder
  private renderPairHostWelcome() {
    Column({ space: OrbitTokens.SPACE_LG }) {
      Blank()

      Column({ space: OrbitTokens.SPACE_SM }) {
        Text('Pair Your First Host')
          .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
        Text(
          'Connect to orbit daemon on your workstation, Linux devbox, or Mac mini over LAN, SSH, or E2EE Noise Relay.'
        )
          .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          .textAlign(TextAlign.Center)
      }
      .alignItems(HorizontalAlign.Center)
      .width('100%')

      Column({ space: OrbitTokens.SPACE_MD }) {
        Button('Scan QR Code')
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .width('100%')
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .onClick(() => {
            this.store.scanQrAndPair(this.getUIContext().getHostContext());
          })

        Row({ space: OrbitTokens.SPACE_SM }) {
          TextInput({ placeholder: 'orbit://pair/...', text: this.manualPairUri })
            .attributeModifier(
              orbitInputMono(
                this.store.palette.fgPrimary,
                this.store.palette.fgMuted,
                this.store.palette.code
              )
            )
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline
            })
            .layoutWeight(1)
            .onChange((val: string) => {
              this.manualPairUri = val;
            })

          Button('Pair Host')
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
            )
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              if (this.manualPairUri.trim().length > 0) {
                this.store.pairHost(this.manualPairUri.trim());
                this.manualPairUri = '';
              }
            })
        }
        .width('100%')

        Button('Discover Local Daemon')
          .attributeModifier(orbitButtonSubhead(this.store.palette.fgSecondary, Color.Transparent))
          .height(OrbitTokens.HIT_MIN)
          .onClick(() => {
            this.store.discoverAndPairLocalDaemon();
          })
      }
      .constraintSize({ maxWidth: 460 })
      .width('100%')

      if (this.store.lastError.length > 0) {
        Text(this.store.lastError)
          .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
          .textAlign(TextAlign.Center)
      } else if (this.store.statusNote.length > 0) {
        Text(this.store.statusNote)
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          .textAlign(TextAlign.Center)
      }

      Blank()
    }
    .width('100%')
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_XL, right: OrbitTokens.SPACE_XL })
    .alignItems(HorizontalAlign.Center)
  }

  @Builder
  private renderProjectPickerMenu() {
    Menu() {
      MenuItemGroup({ header: 'Projects' }) {
        ForEach(
          this.launchpadProjects,
          (proj: LaunchpadProjectDto) => {
            MenuItem({
              symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder'))
                .fontColor([this.store.palette.iconPrimary]),
              content: proj.name,
              labelInfo: (proj.branch ?? '').length > 0 ? (proj.branch ?? undefined) : undefined,
              symbolEndIcon: cwdPathsEquivalent(proj.path, this.store.activeCwd)
                ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
                : undefined
            })
              .labelFontColor(this.store.palette.fgMuted)
              .onClick(() => {
                this.store.setActiveCwd(proj.path);
              })
          },
          (proj: LaunchpadProjectDto) => proj.id
        )
      }

      MenuItemGroup() {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.folder_badge_plus'))
            .fontColor([this.store.palette.accentPrimary]),
          content: 'Browse Directories…'
        })
          .contentFontColor(this.store.palette.accentPrimary)
          .onClick(() => {
            this.store.browseDirectory(
              this.store.activeCwd === '~' ? null : this.store.activeCwd
            );
          })
      }
    }
  }

  @Builder
  private renderHostPickerMenu() {
    Menu() {
      MenuItemGroup({ header: 'Hosts' }) {
        ForEach(
          this.store.hosts,
          (host: HostCardDto) => {
            MenuItem({
              symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.dot_circle'))
                .fontColor([resolveHostStatusColor(host, this.store)]),
              content: host.display_name,
              labelInfo: this.hostSubtitle(host),
              symbolEndIcon: host.host_id === this.store.selectedHostId
                ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
                : undefined
            })
              .labelFontColor(this.store.palette.fgMuted)
              .onClick(() => {
                this.store.selectHost(host.host_id);
                this.store.triggerHaptic(8);
              })
          },
          (host: HostCardDto) => `stage_host_${host.host_id}`
        )
      }
    }
  }

  @Builder
  private renderExecutionModeMenu() {
    Menu() {
      MenuItemGroup({ header: 'Execution Environment' }) {
        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.pc'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Local',
          labelInfo: 'Current directory',
          symbolEndIcon: !this.store.draftUseWorktree
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setDraftUseWorktree(false);
          })

        MenuItem({
          symbolStartIcon: new SymbolGlyphModifier($r('sys.symbol.arrow_triangle_divide'))
            .fontColor([this.store.palette.iconPrimary]),
          content: 'Worktree',
          labelInfo: 'Isolated git worktree',
          symbolEndIcon: this.store.draftUseWorktree
            ? new SymbolGlyphModifier($r('sys.symbol.checkmark')).fontColor([this.store.palette.accentPrimary])
            : undefined
        })
          .labelFontColor(this.store.palette.fgMuted)
          .onClick(() => {
            this.store.setDraftUseWorktree(true);
          })
      }
    }
  }

  @Builder
  private renderNewSessionWelcome() {
    Column({ space: 0 }) {
      Blank()

      Text('What would you like to build?')
        .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
        .textAlign(TextAlign.Center)
        .maxLines(2)
        .width('100%')

      // Contextual Readiness Pills: Native HarmonyOS 7 Capsule Buttons + Native Menus
      Flex({
        wrap: FlexWrap.Wrap,
        justifyContent: FlexAlign.Center,
        alignItems: ItemAlign.Center
      }) {
        Button({ type: ButtonType.Capsule, stateEffect: true }) {
          Row({ space: 6 }) {
            SymbolGlyph($r('sys.symbol.folder'))
              .fontSize(14)
              .fontColor([this.store.palette.iconSecondary])
            Text(this.activeProjectLabel)
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .constraintSize({ maxWidth: 160 })
            SymbolGlyph($r('sys.symbol.chevron_down'))
              .fontSize(11)
              .fontColor([this.store.palette.fgMuted])
          }
          .alignItems(VerticalAlign.Center)
        }
        .height(OrbitTokens.HIT_COMPACT)
        .padding({ left: 14, right: 12 })
        .margin({ left: 4, right: 4, top: 4, bottom: 4 })
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText(`Project: ${this.activeProjectLabel}`)
        .bindMenu(this.renderProjectPickerMenu())

        if (this.store.hosts.length > 1 && this.activeHostCard !== null) {
          Button({ type: ButtonType.Capsule, stateEffect: true }) {
            Row({ space: 6 }) {
              Row()
                .width(7)
                .height(7)
                .borderRadius(3.5)
                .backgroundColor(resolveHostStatusColor(this.activeHostCard, this.store))
              Text(this.activeHostCard.display_name)
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .constraintSize({ maxWidth: 140 })
              SymbolGlyph($r('sys.symbol.chevron_down'))
                .fontSize(11)
                .fontColor([this.store.palette.fgMuted])
            }
            .alignItems(VerticalAlign.Center)
          }
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: 14, right: 12 })
          .margin({ left: 4, right: 4, top: 4, bottom: 4 })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText(`Host: ${this.activeHostCard.display_name}`)
          .bindMenu(this.renderHostPickerMenu())
        }

        if (this.isDraftSession()) {
          Button({ type: ButtonType.Capsule, stateEffect: true }) {
            Row({ space: 6 }) {
              SymbolGlyph(
                this.store.draftUseWorktree
                  ? $r('sys.symbol.arrow_triangle_divide')
                  : $r('sys.symbol.pc')
              )
                .fontSize(14)
                .fontColor([this.store.palette.iconSecondary])
              Text(this.store.draftUseWorktree ? 'Worktree' : 'Local')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .maxLines(1)
              SymbolGlyph($r('sys.symbol.chevron_down'))
                .fontSize(11)
                .fontColor([this.store.palette.fgMuted])
            }
            .alignItems(VerticalAlign.Center)
          }
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: 14, right: 12 })
          .margin({ left: 4, right: 4, top: 4, bottom: 4 })
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText(`Execution environment: ${this.store.draftUseWorktree ? 'Worktree' : 'Local'}`)
          .bindMenu(this.renderExecutionModeMenu())
        }
      }
      .width('100%')
      .margin({ top: OrbitTokens.SPACE_MD })

      if (this.activeHostCard !== null && !this.isHostOnline(this.activeHostCard)) {
        Row({ space: 6 }) {
          Row()
            .width(6)
            .height(6)
            .borderRadius(3)
            .backgroundColor(resolveHostStatusColor(this.activeHostCard, this.store))
          Text(
            `${formatCompactTopBarConnectionLabel(
              this.activeHostCard.connection_state,
              this.activeHostCard.via_ssh_tunnel
            )} · ${this.activeHostCard.display_name}`
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .margin({ top: OrbitTokens.SPACE_SM })
        .alignItems(VerticalAlign.Center)
      } else if (this.store.activeGitBranch().length > 0) {
        Row({ space: 4 }) {
          SymbolGlyph($r('sys.symbol.arrow_triangle_divide'))
            .fontSize(12)
            .fontColor([this.store.palette.fgMuted])
          Text(
            this.store.draftUseWorktree
              ? `Worktree from ${this.store.activeGitBranch()}`
              : this.store.activeGitBranch()
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .margin({ top: OrbitTokens.SPACE_SM })
        .alignItems(VerticalAlign.Center)
      }

      if (this.store.lastError.length > 0) {
        Text(this.store.lastError)
          .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
          .textAlign(TextAlign.Center)
          .margin({ top: OrbitTokens.SPACE_MD })
      }

      Blank()
    }
    .width('100%')
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, bottom: OrbitTokens.SPACE_XL })
    .alignItems(HorizontalAlign.Center)
  }

  build() {
    if (this.store.hosts.length === 0) {
      this.renderPairHostWelcome()
    } else if (this.store.stageTurns.length === 0) {
      Refresh({ refreshing: $$this.isRefreshing }) {
        this.renderNewSessionWelcome()
      }
      .onRefreshing(() => {
        this.handlePullRefresh();
      })
      .width('100%')
      .height('100%')
    } else {
      Stack({ alignContent: Alignment.Bottom }) {
        Refresh({ refreshing: $$this.isRefreshing }) {
          List({
            space: OrbitTokens.SPACE_XL,
            initialIndex: Math.max(0, this.store.stageTurns.length - 1),
            scroller: this.listScroller
          }) {
            Repeat<StageTurnDto>(this.store.stageTurns)
              .each((ri: RepeatItem<StageTurnDto>) => {
                ListItem() {
                  StageTurnView({
                    turn: ri.item,
                    isTurnActive: ri.item.has_active_item ||
                      (this.store.isActiveAgentRunning() && ri.index === this.store.stageTurns.length - 1)
                  })
                }
              })
              .key((t: StageTurnDto) => t.key)
              .virtualScroll()
          }
          .width('100%')
          .height('100%')
          .padding({
            left: OrbitTokens.SPACE_LG,
            right: OrbitTokens.SPACE_LG,
            top: OrbitTokens.SPACE_LG,
            bottom: OrbitTokens.SPACE_XL
          })
          .scrollBar(BarState.Auto)
          .edgeEffect(EdgeEffect.Spring)
          .onScrollStart(() => {
            if (this.store.keyboardHeightVp > 0) {
              dismissHarmonyKeyboard(this.getUIContext());
            }
          })
          .onScrollIndex((firstIndex: number, endIndex: number) => {
            const totalItems = this.store.stageTurns.length;
            this.firstVisibleIndex = firstIndex;
            this.lastVisibleIndex = endIndex;
            if (!this.isAutoAnchoringResize && totalItems > 0 && endIndex < totalItems - 1) {
              this.atBottomEdge = false;
            }
          })
          .onReachEnd(() => {
            this.atBottomEdge = true;
            this.lastVisibleIndex = Math.max(0, this.store.stageTurns.length - 1);
          })
          .onScrollFrameBegin((offset: number, _state: ScrollState) => {
            if (offset < -2 && this.store.stageTurns.length > 0) {
              this.atBottomEdge = false;
            }
            return { offsetRemain: offset };
          })
        }
        .onRefreshing(() => {
          this.handlePullRefresh();
        })
        .width('100%')
        .height('100%')

        if (this.showJumpToLatest()) {
          Badge({
            count: this.store.isActiveAgentRunning() ? 1 : 0,
            style: {
              badgeColor: this.store.palette.accentPrimaryFill,
              badgeSize: 8
            },
            position: BadgePosition.RightTop
          }) {
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.arrow_down'))
                .fontSize(18)
                .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                .fontColor([this.store.palette.iconPrimary])
            }
            .width(OrbitTokens.HIT_COMPACT)
            .height(OrbitTokens.HIT_COMPACT)
            .padding(0)
            .attributeModifier(
              orbitSurface(
                this.store.palette.listCard,
                this.store.palette.hairline,
                OrbitTokens.HAIRLINE,
                ShadowStyle.OUTER_FLOATING_SM
              )
            )
            .accessibilityText(
              this.store.isActiveAgentRunning()
                ? 'Scroll to latest streaming output'
                : 'Scroll to latest message'
            )
            .onClick(() => {
              const lastIdx = Math.max(0, this.store.stageTurns.length - 1);
              this.atBottomEdge = true;
              this.lastVisibleIndex = lastIdx;
              this.listScroller.scrollToIndex(lastIdx, true, ScrollAlign.END);
              this.listScroller.scrollEdge(Edge.Bottom);
              this.store.triggerHaptic(8);
            })
          }
          .margin({ bottom: OrbitTokens.SPACE_SM })
        }
      }
      .width('100%')
      .height('100%')
      .onAreaChange((oldArea: Area, newArea: Area) => {
        const prevH = Number(oldArea.height);
        const nextH = Number(newArea.height);
        if (
          prevH > 0 &&
          nextH > 0 &&
          nextH < prevH - 4 &&
          this.atBottomEdge &&
          this.store.stageTurns.length > 0
        ) {
          const lastIdx = Math.max(0, this.store.stageTurns.length - 1);
          this.isAutoAnchoringResize = true;
          this.lastVisibleIndex = lastIdx;
          this.listScroller.scrollToIndex(lastIdx, true, ScrollAlign.END);
          this.listScroller.scrollEdge(Edge.Bottom);
          if (this.resizeTimerId !== -1) {
            clearTimeout(this.resizeTimerId);
          }
          this.resizeTimerId = setTimeout(() => {
            if (this.atBottomEdge && this.store.stageTurns.length > 0) {
              this.listScroller.scrollEdge(Edge.Bottom);
            }
            this.isAutoAnchoringResize = false;
            this.resizeTimerId = -1;
          }, 80);
        }
      })
    }
  }
}
