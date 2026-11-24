import { TimelineItemDto } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { OrbitSheetHeader } from '../common/OrbitCommonControls';
import { RichMarkdownView } from '../stage/RichMarkdownView';

@ComponentV2
export struct SubagentTimelineSheet {
  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private itemKey(item: TimelineItemDto, idx: number): string {
    return `${item.item_type}_${item.seq}_${item.call_id ?? item.item_id ?? idx}`;
  }

  private itemSummary(item: TimelineItemDto): string {
    if ((item.text ?? '').trim().length > 0) {
      return (item.text ?? '').trim();
    }
    if ((item.summary_title ?? '').trim().length > 0) {
      return (item.summary_title ?? '').trim();
    }
    if (item.detail !== undefined) {
      if ((item.detail.command ?? '').trim().length > 0) {
        return `$ ${(item.detail.command ?? '').trim()}`;
      }
      if ((item.detail.path ?? '').trim().length > 0) {
        return (item.detail.path ?? '').trim();
      }
      if ((item.detail.tool_name ?? '').trim().length > 0) {
        return (item.detail.tool_name ?? '').trim();
      }
    }
    return item.item_type;
  }

  build() {
    Column({ space: 0 }) {
      OrbitSheetHeader({
        title: this.store.inspectedSubagentTitle.length > 0
          ? this.store.inspectedSubagentTitle
          : 'Subagent',
        subtitle: this.store.inspectedSubagentId.length > 0
          ? `${this.store.inspectedSubagentItems.length} events · ${this.store.inspectedSubagentId}`
          : '',
        onClose: () => {
          this.store.showSubagentSheet = false;
        }
      })

      if (this.store.inspectedSubagentItems.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.person_2'))
            .fontSize(26)
            .fontColor([this.store.palette.fgMuted])
          Text('No subagent timeline events loaded yet.')
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: OrbitTokens.SPACE_SM }) {
          ForEach(
            this.store.inspectedSubagentItems,
            (item: TimelineItemDto) => {
              ListItem() {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    Text(item.item_type.replace(/_/g, ' '))
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
                    Blank()
                    Text(`#${item.seq}`)
                      .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  }
                  .width('100%')

                  if (item.item_type === 'assistant_message') {
                    RichMarkdownView({
                      blocks: item.blocks ?? [],
                      fallbackText: item.text ?? ''
                    })
                  } else {
                    Text(this.itemSummary(item))
                      .attributeModifier(
                        item.item_type === 'tool_call'
                          ? orbitTextMono(this.store.palette.fgPrimary)
                          : orbitTextBody(this.store.palette.fgPrimary)
                      )
                      .width('100%')
                      .copyOption(CopyOptions.LocalDevice)
                  }
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
              }
            },
            (item: TimelineItemDto, idx: number) => this.itemKey(item, idx)
          )
        }
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: OrbitTokens.SPACE_LG,
          right: OrbitTokens.SPACE_LG,
          top: OrbitTokens.SPACE_SM,
          bottom: OrbitTokens.SPACE_LG + this.store.safeBottomVp
        })
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
