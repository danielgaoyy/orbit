import { StageTurnDto, basenameOfPath } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitInputMono,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import { StageTurnView } from './StageTurnView';

interface StarterPromptChip {
  id: string;
  label: string;
  prompt: string;
}

const STARTER_PROMPTS: StarterPromptChip[] = [
  {
    id: 'arch',
    label: 'Map codebase architecture',
    prompt: 'Summarize the crate/module architecture and key entrypoints in this workspace.'
  },
  {
    id: 'review',
    label: 'Review working tree changes',
    prompt: 'Inspect uncommitted git changes, check for edge cases, and suggest improvements.'
  },
  {
    id: 'tests',
    label: 'Run tests & fix failures',
    prompt: 'Run the test suite, diagnose any failing tests or warnings, and fix them cleanly.'
  }
];

@ComponentV2
export struct StreamStage {
  @Local firstVisibleIndex: number = 0;
  @Local lastVisibleIndex: number = 0;
  @Local atBottomEdge: boolean = true;
  @Local manualPairUri: string = '';
  @Local isRefreshing: boolean = false;
  @Local store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private listScroller: Scroller = new Scroller();
  private isAutoAnchoringResize: boolean = false;

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
    setTimeout(() => {
      this.isRefreshing = false;
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

  private activeHostName(): string {
    const detail = this.store.currentHostDetail;
    if (detail !== null) {
      return detail.host.display_name;
    }
    for (let i = 0; i < this.store.hosts.length; i++) {
      if (this.store.hosts[i].host_id === this.store.selectedHostId) {
        return this.store.hosts[i].display_name;
      }
    }
    return 'No Host Connected';
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
        if (this.store.clipboardPairingUri.length > 0) {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Column({ space: 2 }) {
              Text('Pair from Clipboard')
                .attributeModifier(
                  orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold)
                )
              Text(this.store.clipboardPairingUri)
                .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
            }
            .alignItems(HorizontalAlign.Start)
            .layoutWeight(1)

            Button('Pair')
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .onClick(() => {
                this.store.pairFromClipboard();
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_MD)
          .backgroundColor(this.store.palette.accentPrimarySoft)
          .borderRadius(OrbitTokens.CARD_RADIUS)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.store.palette.accentPrimary
          })
          .alignItems(VerticalAlign.Center)
        }

        Button('Scan QR Code')
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .width('100%')
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.CARD_RADIUS)
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
            .borderRadius(OrbitTokens.CARD_RADIUS)
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
            .borderRadius(OrbitTokens.CARD_RADIUS)
            .onClick(() => {
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
    .layoutWeight(1)
    .padding({ left: OrbitTokens.SPACE_XL, right: OrbitTokens.SPACE_XL })
    .alignItems(HorizontalAlign.Center)
  }

  @Builder
  private renderNewSessionWelcome() {
    Column({ space: OrbitTokens.SPACE_LG }) {
      Blank()

      Column({ space: OrbitTokens.SPACE_SM }) {
        Text('New Session')
          .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))

        Row({ space: OrbitTokens.SPACE_SM }) {
          Text(this.activeHostName())
            .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary, FontWeight.Medium))
          Text('·')
            .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
          Text(basenameOfPath(this.store.activeCwd))
            .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Medium))
        }
        .height(OrbitTokens.HIT_MIN)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .backgroundColor(this.store.palette.subtleSurface)
        .borderRadius(OrbitTokens.CARD_RADIUS)
        .border({
          width: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline
        })
        .onClick(() => {
          this.store.sidebarOpen = true;
          this.store.browseDirectory(this.store.activeCwd === '~' ? null : this.store.activeCwd);
        })
      }
      .alignItems(HorizontalAlign.Center)
      .width('100%')

      Column({ space: OrbitTokens.SPACE_SM }) {
        ForEach(
          STARTER_PROMPTS,
          (chip: StarterPromptChip) => {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text(chip.label)
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Normal))
                .layoutWeight(1)
              Text('↗')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted))
            }
            .width('100%')
            .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            .backgroundColor(this.store.palette.elevated)
            .borderRadius(OrbitTokens.CARD_RADIUS)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.palette.hairline
            })
            .onClick(() => {
              this.store.composerText = chip.prompt;
              this.store.triggerHaptic(8);
            })
          },
          (chip: StarterPromptChip) => chip.id
        )
      }
      .constraintSize({ maxWidth: 520 })
      .width('100%')

      if (this.store.lastError.length > 0) {
        Text(this.store.lastError)
          .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
          .textAlign(TextAlign.Center)
      }

      Blank()
    }
    .width('100%')
    .height('100%')
    .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
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
      .layoutWeight(1)
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
                  StageTurnView({ turn: ri.item })
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
          Stack({ alignContent: Alignment.Center }) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              if (this.store.isActiveAgentRunning()) {
                Row()
                  .width(OrbitTokens.DOT_SIZE)
                  .height(OrbitTokens.DOT_SIZE)
                  .borderRadius(3)
                  .backgroundColor(this.store.palette.accentPrimary)
              }
              Text(this.store.isActiveAgentRunning() ? '↓ Streaming' : '↓ Latest')
                .attributeModifier(
                  orbitTextMeta(
                    this.store.isActiveAgentRunning()
                      ? this.store.palette.accentPrimary
                      : this.store.palette.fgPrimary,
                    FontWeight.Medium
                  )
                )
            }
            .height(32)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .backgroundColor(this.store.palette.elevated)
            .borderRadius(OrbitTokens.SPACE_LG)
            .border({
              width: OrbitTokens.HAIRLINE,
              color: this.store.isActiveAgentRunning()
                ? this.store.palette.accentPrimary
                : this.store.palette.hairline
            })
            .shadow({
              radius: 12,
              color: '#22000000',
              offsetX: 0,
              offsetY: 4
            })
            .alignItems(VerticalAlign.Center)
          }
          .height(OrbitTokens.HIT_MIN)
          .constraintSize({ minWidth: OrbitTokens.HIT_MIN })
          .margin({ bottom: OrbitTokens.SPACE_XS })
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
      }
      .width('100%')
      .layoutWeight(1)
      .onAreaChange((oldArea: Area, newArea: Area) => {
        const prevH = Number(oldArea.height);
        const nextH = Number(newArea.height);
        // When StickyDecisionDock or soft keyboard shrinks Stage height while anchored at bottom,
        // smoothly re-scroll so the latest Assistant reply stays snug right above StickyDecisionDock.
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
          setTimeout(() => {
            if (this.atBottomEdge && this.store.stageTurns.length > 0) {
              this.listScroller.scrollEdge(Edge.Bottom);
            }
            this.isAutoAnchoringResize = false;
          }, 80);
        }
      })
    }
  }
}
