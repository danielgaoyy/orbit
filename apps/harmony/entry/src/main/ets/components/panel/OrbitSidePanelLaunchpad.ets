import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitSidePanelKind,
  OrbitSidePanelTab,
  canonicalSideQuestionTabId
} from '../../model/stores/OrbitSidePanelStore';
import {
  OrbitTokens,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { resolveSidePanelKindSymbol } from './OrbitSidePanelTabBar';

interface OrbitLaunchpadTile {
  kind: OrbitSidePanelKind;
  title: string;
  subtitle: string;
  requiresAgent: boolean;
}

const LAUNCHPAD_TILES: OrbitLaunchpadTile[] = [
  { kind: 'review', title: 'Git Review', subtitle: 'Diffs & working tree', requiresAgent: false },
  { kind: 'files', title: 'Files', subtitle: 'Browse & edit code', requiresAgent: false },
  { kind: 'terminal', title: 'Terminal', subtitle: 'Interactive PTY slots', requiresAgent: false },
  { kind: 'board', title: 'Board & Peers', subtitle: 'BOARD.md & siblings', requiresAgent: false },
  { kind: 'side_question', title: 'Side Q&A', subtitle: '/btw without interrupting', requiresAgent: false },
  { kind: 'subagent', title: 'Subagent Crew', subtitle: 'Inspect, steer & archive', requiresAgent: true },
  { kind: 'briefing', title: 'Briefing', subtitle: 'Fleet decisions & status', requiresAgent: false },
  { kind: 'automations', title: 'Automations', subtitle: 'Schedules, timers & goals', requiresAgent: false }
];

/**
 * Side panel Launchpad rendered when `+` is tapped or no tab is active.
 * Strictly constructs tabs bound to its explicit `@Param` scope (`hostId`, `agentId`, `cwd`).
 */
@ComponentV2
export struct OrbitSidePanelLaunchpad {
  @Param boundHostId: string = '';
  @Param boundAgentId: string | null = null;
  @Param boundCwd: string | null = null;

  private readonly store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    if (this.boundHostId.length > 0) {
      this.store.ensureScopedHostDetailAsync(this.boundHostId);
    }
  }

  @Computed
  get visibleTiles(): OrbitLaunchpadTile[] {
    const hasAgent = this.boundAgentId !== null && this.boundAgentId.length > 0;
    const out: OrbitLaunchpadTile[] = [];
    for (let i = 0; i < LAUNCHPAD_TILES.length; i++) {
      if (!LAUNCHPAD_TILES[i].requiresAgent || hasAgent) {
        out.push(LAUNCHPAD_TILES[i]);
      }
    }
    return out;
  }

  @Computed
  get canReopenClosed(): boolean {
    const _rev = this.store.sidePanelStore.stateRevision;
    return _rev >= 0 && this.store.sidePanelStore.activeSnapshot.closedTabs.length > 0;
  }

  private launchTile(tile: OrbitLaunchpadTile): void {
    if (this.boundHostId.length === 0) {
      this.store.showToastResource($r('app.string.toast_pair_or_select_host_first'), true);
      return;
    }
    const scopeKey = this.store.sidePanelStore.scopeKeyFor(this.boundHostId, this.boundAgentId);
    const tabId = tile.kind === 'subagent'
      ? `crew:${scopeKey}`
      : (tile.kind === 'side_question'
        ? canonicalSideQuestionTabId(scopeKey)
        : `${tile.kind}:${scopeKey}`);
    const rawWsId = tile.kind === 'board'
      ? this.store.resolveScopedWorkspaceId(this.boundHostId, this.boundCwd ?? '', this.boundAgentId)
      : '';
    const wsId = rawWsId.length > 0 ? rawWsId : null;

    const tab: OrbitSidePanelTab = {
      tabId: tabId,
      kind: tile.kind,
      hostId: this.boundHostId,
      agentId: this.boundAgentId,
      cwd: this.boundCwd,
      path: null,
      parentHostId: this.boundHostId,
      title: tile.title,
      closable: true,
      parentAgentId: this.boundAgentId,
      workspaceId: wsId,
      terminalSlot: tile.kind === 'terminal' ? 1 : null
    };
    this.store.sidePanelStore.openTab(tab, { source: 'user', activate: true });
    this.store.triggerHapticPreset('selection');
  }

  build() {
    Scroll() {
      Column({ space: OrbitTokens.SPACE_MD }) {
        Column({ space: 2 }) {
          Text($r('app.string.side_panel_launchpad_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
          Text($r('app.string.side_panel_launchpad_subtitle'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }
        .width('100%')
        .alignItems(HorizontalAlign.Start)

        Flex({ wrap: FlexWrap.Wrap, justifyContent: FlexAlign.SpaceBetween }) {
          ForEach(
            this.visibleTiles,
            (tile: OrbitLaunchpadTile) => {
              Button({ type: ButtonType.Normal, stateEffect: true }) {
                Column({ space: 6 }) {
                  SymbolGlyph(resolveSidePanelKindSymbol(tile.kind))
                    .fontSize(18)
                    .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                    .fontColor([this.store.palette.iconPrimary])

                  Column({ space: 2 }) {
                    Text(tile.title)
                      .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                    Text(tile.subtitle)
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                  }
                  .width('100%')
                  .alignItems(HorizontalAlign.Start)
                }
                .width('100%')
                .alignItems(HorizontalAlign.Start)
              }
              .width('48%')
              .padding(OrbitTokens.SPACE_MD)
              .margin({ bottom: OrbitTokens.SPACE_SM })
              .borderRadius(OrbitTokens.RADIUS_MD)
              .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              .accessibilityText(`Open ${tile.title}`)
              .onClick(() => {
                this.launchTile(tile);
              })
            },
            (tile: OrbitLaunchpadTile): string => tile.kind
          )
        }
        .width('100%')

        if (this.canReopenClosed) {
          Button({ type: ButtonType.Normal, stateEffect: true }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              SymbolGlyph($r('sys.symbol.arrow_counterclockwise'))
                .fontSize(15)
                .fontColor([this.store.palette.iconSecondary])
              Text($r('app.string.side_panel_reopen_closed'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                .layoutWeight(1)
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
          .borderRadius(OrbitTokens.RADIUS_MD)
          .backgroundColor(this.store.palette.subtleSurface)
          .onClick(() => {
            this.store.sidePanelStore.reopenLast();
            this.store.triggerHapticPreset('selection');
          })
        }
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
      })
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
    .backgroundColor(this.store.palette.canvas)
  }
}
