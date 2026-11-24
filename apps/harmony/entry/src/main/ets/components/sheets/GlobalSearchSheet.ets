import {
  ConversationSearchHitDto,
  formatCompactRelativeTime
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { OrbitSheetHeader, dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

@ComponentV2
export struct GlobalSearchSheet {
  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  @Local private queryText: string = '';

  aboutToAppear(): void {
    this.queryText = this.store.globalSearchQuery;
  }

  private hitKey(hit: ConversationSearchHitDto, idx: number): string {
    return `${hit.host_id}_${hit.agent_id}_${hit.seq}_${idx}`;
  }

  build() {
    Column({ space: 0 }) {
      OrbitSheetHeader({
        title: $r('app.string.search_title'),
        subtitle: this.store.globalSearchRemoteQueried > 0
          ? `${this.store.globalSearchHits.length} hits · ${this.store.globalSearchRemoteQueried} remote host(s)`
          : `${this.store.hosts.length} paired host(s)`,
        onClose: () => {
          this.store.showGlobalSearchSheet = false;
        }
      })

      Column({ space: OrbitTokens.SPACE_SM }) {
        Search({ value: this.queryText, placeholder: $r('app.string.search_placeholder') })
          .width('100%')
          .height(OrbitTokens.HIT_MIN)
          .backgroundColor(this.store.palette.listCard)
          .placeholderColor(this.store.palette.fgMuted)
          .fontColor(this.store.palette.fgPrimary)
          .onChange((val: string) => {
            this.queryText = val;
            this.store.runLocalGlobalSearch(val);
          })
          .onSubmit((val: string) => {
            dismissHarmonyKeyboard(this.getUIContext());
            this.queryText = val;
            this.store.runRemoteGlobalSearch(val);
          })

        Row({ space: OrbitTokens.SPACE_SM }) {
          Button($r('app.string.search_remote_action'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .height(OrbitTokens.HIT_COMPACT)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .enabled(!this.store.globalSearchBusy && this.queryText.trim().length > 0)
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.runRemoteGlobalSearch(this.queryText);
            })

          if (this.store.globalSearchBusy) {
            LoadingProgress()
              .width(16)
              .height(16)
              .color(this.store.palette.accentPrimary)
          }

          Blank()

          if (this.store.globalSearchRemoteFailed > 0) {
            Text(`${this.store.globalSearchRemoteFailed} host(s) unreachable`)
              .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
          }
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        if (this.queryText.trim().length === 0) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Blank()
            SymbolGlyph($r('sys.symbol.magnifyingglass'))
              .fontSize(26)
              .fontColor([this.store.palette.fgMuted])
            Text($r('app.string.search_empty_hint'))
              .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
              .textAlign(TextAlign.Center)
            Blank()
          }
          .width('100%')
          .layoutWeight(1)
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          .alignItems(HorizontalAlign.Center)
        } else if (this.store.globalSearchHits.length === 0 && !this.store.globalSearchBusy) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Blank()
            Text($r('app.string.search_no_results'))
              .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
            Blank()
          }
          .width('100%')
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Center)
        } else {
          List({ space: OrbitTokens.SPACE_SM }) {
            ForEach(
              this.store.globalSearchHits,
              (hit: ConversationSearchHitDto) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Column({ space: OrbitTokens.SPACE_XS }) {
                      Row({ space: OrbitTokens.SPACE_XS }) {
                        Text(hit.title.trim().length > 0 ? hit.title.trim() : hit.project_name)
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .layoutWeight(1)

                        if (hit.updated_at_rfc3339.length > 0) {
                          Text(formatCompactRelativeTime(hit.updated_at_rfc3339))
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        }
                      }
                      .width('100%')

                      Row({ space: 6 }) {
                        Text(hit.host_name)
                          .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                          .padding({ left: 6, right: 6, top: 2, bottom: 2 })
                          .backgroundColor(this.store.palette.subtleSurface)
                          .borderRadius(OrbitTokens.RADIUS_XS)

                        if (hit.project_name.length > 0) {
                          Text(hit.project_name)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                        }

                        if (hit.is_remote) {
                          Text('Remote')
                            .attributeModifier(orbitTextMeta(this.store.palette.statusRelay, FontWeight.Medium))
                            .padding({ left: 6, right: 6, top: 2, bottom: 2 })
                            .backgroundColor(this.store.palette.statusRelaySoft)
                            .borderRadius(OrbitTokens.RADIUS_XS)
                        }

                        if (hit.is_unloaded) {
                          Text('Earlier history')
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        }
                      }
                      .width('100%')

                      if (hit.snippet.trim().length > 0) {
                        Text(hit.snippet.trim())
                          .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                          .maxLines(3)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                          .width('100%')
                      }
                    }
                    .width('100%')
                    .alignItems(HorizontalAlign.Start)
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_MD)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                  .onClick(() => {
                    dismissHarmonyKeyboard(this.getUIContext());
                    this.store.jumpToSearchHit(hit);
                  })
                }
              },
              (hit: ConversationSearchHitDto, idx: number) => this.hitKey(hit, idx)
            )
          }
          .width('100%')
          .layoutWeight(1)
          .scrollBar(BarState.Auto)
          .edgeEffect(EdgeEffect.Spring)
        }
      }
      .width('100%')
      .layoutWeight(1)
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_SM,
        bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
      })
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
