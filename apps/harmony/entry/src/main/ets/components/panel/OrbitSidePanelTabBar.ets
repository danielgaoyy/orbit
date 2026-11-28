import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitSidePanelKind,
  OrbitSidePanelTab
} from '../../model/stores/OrbitSidePanelStore';
import {
  OrbitTokens,
  orbitTextMeta
} from '../../theme/OrbitTokens';

export function resolveSidePanelKindSymbol(kind: OrbitSidePanelKind): Resource {
  if (kind === 'review') {
    return $r('sys.symbol.arrow_left_arrow_right');
  }
  if (kind === 'files') {
    return $r('sys.symbol.folder');
  }
  if (kind === 'file') {
    return $r('sys.symbol.doc_text');
  }
  if (kind === 'terminal') {
    return $r('sys.symbol.code_square');
  }
  if (kind === 'board') {
    return $r('sys.symbol.square_grid_2x2');
  }
  if (kind === 'side_question') {
    return $r('sys.symbol.message');
  }
  if (kind === 'subagent') {
    return $r('sys.symbol.person_2');
  }
  if (kind === 'peer') {
    return $r('sys.symbol.person');
  }
  if (kind === 'briefing') {
    return $r('sys.symbol.house');
  }
  return $r('sys.symbol.clock');
}

/**
 * Horizontal capsule tab strip + `+` Launchpad trigger + panel dismiss control.
 * Used identically across sm bottom Sheet and md/lg inline right panel.
 */
@ComponentV2
export struct OrbitSidePanelTabBar {
  @Param isCompactSheet: boolean = false;
  @Event onClosePanel: () => void = () => {};

  private readonly store: OrbitHarmonyStore = orbitRuntime();

  @Computed
  get tabs(): OrbitSidePanelTab[] {
    return this.store.sidePanelStore.activeSnapshot.tabs;
  }

  @Computed
  get activeTabId(): string {
    return this.store.sidePanelStore.activeSnapshot.activeTabId;
  }

  private isTabUnseen(tabId: string): boolean {
    return this.store.sidePanelStore.activeSnapshot.unseenTabIds.indexOf(tabId) >= 0;
  }

  build() {
    Column({ space: 0 }) {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Scroll() {
          Row({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              this.tabs,
              (tab: OrbitSidePanelTab) => {
                Row({ space: 5 }) {
                  SymbolGlyph(resolveSidePanelKindSymbol(tab.kind))
                    .fontSize(12)
                    .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                    .fontColor([
                      this.activeTabId === tab.tabId
                        ? this.store.palette.iconPrimary
                        : this.store.palette.iconSecondary
                    ])

                  Text(tab.title)
                    .attributeModifier(
                      orbitTextMeta(
                        this.activeTabId === tab.tabId
                          ? this.store.palette.fgPrimary
                          : this.store.palette.fgSecondary,
                        this.activeTabId === tab.tabId ? FontWeight.Medium : FontWeight.Normal
                      )
                    )
                    .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .constraintSize({ maxWidth: 118 })

                  if (this.isTabUnseen(tab.tabId)) {
                    Row()
                      .width(OrbitTokens.DOT_SIZE)
                      .height(OrbitTokens.DOT_SIZE)
                      .borderRadius(3)
                      .backgroundColor(this.store.palette.unreadBadge)
                  }

                  if (tab.closable) {
                    Button({ type: ButtonType.Circle, stateEffect: true }) {
                      SymbolGlyph($r('sys.symbol.xmark'))
                        .fontSize(10)
                        .fontColor([this.store.palette.fgMuted])
                    }
                    .width(20)
                    .height(20)
                    .padding(0)
                    .backgroundColor(Color.Transparent)
                    .accessibilityText($r('app.string.a11y_close_tab', tab.title))
                    .onClick(() => {
                      this.store.sidePanelStore.close(tab.tabId);
                      this.store.triggerHaptic('selection');
                    })
                  }
                }
                .constraintSize({ minHeight: 30 })
                .padding({ left: 9, right: tab.closable ? 4 : 9, top: 4, bottom: 4 })
                .borderRadius(OrbitTokens.RADIUS_XL)
                .backgroundColor(
                  this.activeTabId === tab.tabId
                    ? this.store.palette.elevated
                    : this.store.palette.subtleSurface
                )
                .border({
                  width: OrbitTokens.HAIRLINE,
                  color: this.activeTabId === tab.tabId
                    ? this.store.palette.borderDefault
                    : Color.Transparent
                })
                .alignItems(VerticalAlign.Center)
                .onClick(() => {
                  this.store.sidePanelStore.activate(tab.tabId);
                  this.store.triggerHaptic('selection');
                })
              },
              (tab: OrbitSidePanelTab): string => tab.tabId
            )

            // `+` Launchpad button
            Button({ type: ButtonType.Circle, stateEffect: true }) {
              SymbolGlyph($r('sys.symbol.plus'))
                .fontSize(14)
                .fontColor([
                  this.activeTabId.length === 0
                    ? this.store.palette.onAccent
                    : this.store.palette.iconPrimary
                ])
            }
            .width(30)
            .height(30)
            .padding(0)
            .backgroundColor(
              this.activeTabId.length === 0
                ? this.store.palette.accentPrimaryFill
                : this.store.palette.subtleSurface
            )
            .accessibilityText($r('app.string.a11y_open_panel_launchpad'))
            .onClick(() => {
              this.store.sidePanelStore.openLaunchpad();
              this.store.triggerHaptic('selection');
            })
          }
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_XS })
          .alignItems(VerticalAlign.Center)
        }
        .scrollable(ScrollDirection.Horizontal)
        .scrollBar(BarState.Off)
        .layoutWeight(1)
        .constraintSize({ minHeight: 40 })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(14)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .margin({ right: OrbitTokens.SPACE_SM })
        .accessibilityText($r('app.string.a11y_close_side_panel'))
        .onClick(() => {
          this.onClosePanel();
        })
      }
      .width('100%')
      .constraintSize({ minHeight: this.isCompactSheet ? 48 : 44 })
      .alignItems(VerticalAlign.Center)
      .backgroundColor(this.store.palette.recessed)

      Divider()
        .strokeWidth(OrbitTokens.HAIRLINE)
        .color(this.store.palette.hairline)
        .width('100%')
    }
    .width('100%')
  }
}
