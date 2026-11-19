import { basenameOfPath, formatCompactTopBarConnectionLabel } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { FleetSidebar } from './FleetSidebar';
import { StreamStage } from '../stage/StreamStage';
import { StickyDecisionDock } from '../stage/StickyDecisionDock';
import { OmniComposer } from '../composer/OmniComposer';
import { ModelPickerSheet } from '../composer/ModelPickerSheet';
import { ContextPickerSheet } from '../composer/ContextPickerSheet';
import { GitDiffSheet } from '../sheets/GitDiffSheet';
import { PtyTerminalSheet } from '../sheets/PtyTerminalSheet';
import { HostSettingsSheet } from '../sheets/HostSettingsSheet';

@ComponentV2
export struct AdaptiveWorkspace {
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private isPhoneLayout(): boolean {
    return this.store.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN;
  }

  private canShowInlineDiff(): boolean {
    return this.store.canShowInlineDiffInspector();
  }

  private hasPairedHosts(): boolean {
    return this.store.hosts.length > 0;
  }

  private topBarTitle(): string {
    if (!this.hasPairedHosts()) {
      return 'Orbit';
    }
    if (this.store.currentAgent !== null && this.store.currentAgent.title.trim().length > 0) {
      return this.store.currentAgent.title.trim();
    }
    return 'New Session';
  }

  private topBarConnectionLabel(): string {
    const detail = this.store.currentHostDetail;
    if (detail !== null) {
      return formatCompactTopBarConnectionLabel(
        detail.host.connection_state,
        detail.host.via_ssh_tunnel
      );
    }
    for (let i = 0; i < this.store.hosts.length; i++) {
      const h = this.store.hosts[i];
      if (h.host_id === this.store.selectedHostId) {
        return formatCompactTopBarConnectionLabel(h.connection_state, h.via_ssh_tunnel);
      }
    }
    return 'Offline';
  }

  private topBarWorkspaceLabel(): string {
    if (this.store.activeCwd.length > 0 && this.store.activeCwd !== '~') {
      return basenameOfPath(this.store.activeCwd);
    }
    return '~';
  }

  private topStatusDotColor(): string {
    const ag = this.store.currentAgent;
    if (ag !== null) {
      if (ag.is_awaiting_permission) {
        return this.store.palette.statusWarning;
      }
      if (ag.is_running) {
        return this.store.palette.accentPrimary;
      }
      if (ag.status.state === 'error') {
        return this.store.palette.statusDanger;
      }
    }
    const detail = this.store.currentHostDetail;
    if (detail !== null) {
      if (detail.host.connection_state === 'online_direct') {
        return this.store.palette.statusDirect;
      }
      if (detail.host.connection_state === 'online_relay') {
        return this.store.palette.statusRelay;
      }
    }
    return this.store.palette.fgMuted;
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
  private buildModelPickerSheet() {
    ModelPickerSheet()
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
  private renderTopBar() {
    Column() {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Stack({ alignContent: Alignment.TopEnd }) {
          Row() {
            Text(this.isPhoneLayout() ? '☰' : '◧')
              .attributeModifier(
                orbitTextSubhead(
                  !this.isPhoneLayout() && this.store.isSidebarCollapsed
                    ? this.store.palette.fgSecondary
                    : this.store.palette.fgPrimary,
                  FontWeight.Bold
                )
              )
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .justifyContent(FlexAlign.Center)

          if (this.store.totalPendingApprovals > 0) {
            Row()
              .width(OrbitTokens.DOT_SIZE)
              .height(OrbitTokens.DOT_SIZE)
              .borderRadius(3)
              .backgroundColor(this.store.palette.statusWarning)
              .margin({ top: 10, right: 10 })
          }
        }
        .width(OrbitTokens.HIT_MIN)
        .height(OrbitTokens.HIT_MIN)
        .accessibilityText(
          this.isPhoneLayout()
            ? (this.store.sidebarOpen ? 'Close navigation drawer' : 'Open navigation drawer')
            : (this.store.isSidebarCollapsed ? 'Expand fleet sidebar' : 'Collapse fleet sidebar')
        )
        .onClick(() => {
          this.store.toggleSidebar();
        })

        if (this.hasPairedHosts()) {
          Column({ space: 2 }) {
            Text(this.topBarTitle())
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .width('100%')

            Row({ space: OrbitTokens.SPACE_XS }) {
              Row()
                .width(OrbitTokens.DOT_SIZE)
                .height(OrbitTokens.DOT_SIZE)
                .borderRadius(3)
                .backgroundColor(this.topStatusDotColor())

              Text(`${this.topBarConnectionLabel()} · ${this.topBarWorkspaceLabel()}`)
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
        } else {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text(this.topBarTitle())
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .layoutWeight(1)
          }
          .layoutWeight(1)
          .height(OrbitTokens.HIT_MIN)
          .alignItems(VerticalAlign.Center)
        }

        if (this.hasPairedHosts()) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            if (this.store.totalDiffAdditions() > 0 || this.store.totalDiffDeletions() > 0) {
              Text(`+${this.store.totalDiffAdditions()}`)
                .attributeModifier(
                  orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium)
                )
              Text(`-${this.store.totalDiffDeletions()}`)
                .attributeModifier(
                  orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium)
                )
            } else {
              Text('Diff')
                .attributeModifier(
                  orbitTextMeta(
                    this.canShowInlineDiff() && this.store.showInlineDiffInspector
                      ? this.store.palette.accentPrimary
                      : this.store.palette.fgSecondary,
                    FontWeight.Medium
                  )
                )
            }
          }
          .height(OrbitTokens.HIT_MIN)
          .constraintSize({ minWidth: OrbitTokens.HIT_MIN })
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
          .backgroundColor(
            this.canShowInlineDiff() && this.store.showInlineDiffInspector
              ? this.store.palette.accentPrimarySoft
              : Color.Transparent
          )
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .justifyContent(FlexAlign.Center)
          .alignItems(VerticalAlign.Center)
          .accessibilityText('Inspect Git working tree diff')
          .onClick(() => {
            this.store.toggleTopBarGitDiff();
          })

          Row() {
            Text('>_')
              .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Bold))
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .justifyContent(FlexAlign.Center)
          .accessibilityText('Open PTY terminal')
          .onClick(() => {
            this.store.openPtyTerminal();
          })

          Row() {
            Text('+')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .justifyContent(FlexAlign.Center)
          .accessibilityText('Start new session')
          .onClick(() => {
            this.store.startNewSessionDraft();
          })
        } else {
          Row() {
            Text('Settings')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary, FontWeight.Medium))
          }
          .height(OrbitTokens.HIT_MIN)
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
          .alignItems(VerticalAlign.Center)
          .onClick(() => {
            this.store.openHostSettings();
          })
        }
      }
      .width('100%')
      .height(OrbitTokens.TOP_BAR_HEIGHT)
      .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS })
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .backgroundColor(this.store.palette.canvas)
    .border({
      width: { bottom: OrbitTokens.HAIRLINE },
      color: this.store.palette.hairline
    })
  }

  @Builder
  private renderToastBanner() {
    if (this.store.toastMessage.length > 0) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(OrbitTokens.DOT_SIZE)
          .height(OrbitTokens.DOT_SIZE)
          .borderRadius(3)
          .backgroundColor(
            this.store.toastIsError
              ? this.store.palette.statusDanger
              : this.store.palette.statusDirect
          )

        Text(this.store.toastMessage)
          .attributeModifier(
            orbitTextMeta(
              this.store.toastIsError
                ? this.store.palette.statusDanger
                : this.store.palette.fgPrimary,
              FontWeight.Medium
            )
          )
          .maxLines(2)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .flexShrink(1)
      }
      .constraintSize({ maxWidth: 420 })
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_SM
      })
      .margin({
        top: 56,
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG
      })
      .backgroundColor(
        this.store.toastIsError
          ? this.store.palette.statusDangerSoft
          : this.store.palette.elevated
      )
      .borderRadius(OrbitTokens.COMPOSER_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.toastIsError
          ? this.store.palette.statusDanger
          : this.store.palette.hairline
      })
      .shadow({
        radius: 12,
        color: '#24000000',
        offsetX: 0,
        offsetY: 4
      })
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.store.dismissToast();
      })
    }
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
          Column() {
            this.renderTopBar()

            Row() {
              Column() {
                StreamStage()
                if (this.hasPairedHosts()) {
                  StickyDecisionDock()
                  OmniComposer()
                }
              }
              .layoutWeight(1)
              .height('100%')
              .backgroundColor(this.store.palette.canvas)

              // Side-by-side GitDiffInspector (360vp):
              // Enabled on Mate XT 3-Screen / Landscape MatePad (>= 1024vp) OR whenever
              // (viewportWidth - effectiveSidebarWidth) >= 780vp (e.g. Zen Stage with collapsed sidebar).
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
                  width: { left: 1 },
                  color: this.store.palette.hairline
                })
              }
            }
            .width('100%')
            .layoutWeight(1)
          }
          .width('100%')
          .height('100%')

          this.renderToastBanner()
        }
        .width('100%')
        .height('100%')
        .padding({ top: this.store.safeTopVp })
        .backgroundColor(this.store.palette.canvas)
      }
      .showControlButton(false)
      .showSideBar(
        this.isPhoneLayout()
          ? this.store.sidebarOpen
          : !this.store.isSidebarCollapsed
      )
      .sideBarWidth(OrbitTokens.SIDEBAR_FOLD_WIDTH)
      .minSideBarWidth(220)
      .maxSideBarWidth(320)
      .divider({
        strokeWidth: 1,
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

      Column()
        .width(0)
        .height(0)
        .bindSheet(this.store.showGitDiffSheet, () => {
          this.buildGitDiffSheet();
        }, {
          detents: [SheetSize.LARGE, SheetSize.MEDIUM],
          dragBar: true,
          showClose: false,
          backgroundColor: this.store.palette.canvas,
          onDisappear: () => {
            this.store.showGitDiffSheet = false;
          }
        })

      Column()
        .width(0)
        .height(0)
        .bindSheet(this.store.showPtySheet, () => {
          this.buildPtyTerminalSheet();
        }, {
          detents: [SheetSize.LARGE, SheetSize.MEDIUM],
          dragBar: true,
          showClose: false,
          backgroundColor: this.store.palette.canvas,
          onDisappear: () => {
            this.store.showPtySheet = false;
          }
        })

      Column()
        .width(0)
        .height(0)
        .bindSheet(this.store.showModelPickerSheet, () => {
          this.buildModelPickerSheet();
        }, {
          detents: [SheetSize.MEDIUM, SheetSize.LARGE],
          dragBar: true,
          showClose: false,
          backgroundColor: this.store.palette.canvas,
          onDisappear: () => {
            this.store.showModelPickerSheet = false;
          }
        })

      Column()
        .width(0)
        .height(0)
        .bindSheet(this.store.showContextPickerSheet, () => {
          this.buildContextPickerSheet();
        }, {
          detents: [SheetSize.MEDIUM, SheetSize.LARGE],
          dragBar: true,
          showClose: false,
          backgroundColor: this.store.palette.canvas,
          onDisappear: () => {
            this.store.showContextPickerSheet = false;
          }
        })

      Column()
        .width(0)
        .height(0)
        .bindSheet(this.store.showHostSettingsSheet, () => {
          this.buildHostSettingsSheet();
        }, {
          detents: [SheetSize.LARGE, SheetSize.MEDIUM],
          dragBar: true,
          showClose: false,
          backgroundColor: this.store.palette.canvas,
          onDisappear: () => {
            this.store.showHostSettingsSheet = false;
          }
        })
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
