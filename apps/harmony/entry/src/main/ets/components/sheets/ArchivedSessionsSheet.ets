import {
  AgentCardDto,
  basenameOfPath,
  formatCompactRelativeTime,
  formatProviderLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitTextMeta,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';

@ComponentV2
export struct ArchivedSessionsSheet {
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get archivedList(): AgentCardDto[] {
    return this.store.archivedAgents();
  }

  private confirmPermanentDelete(agent: AgentCardDto): void {
    const label = agent.title.trim().length > 0 ? agent.title.trim() : agent.provider_label;
    this.getUIContext().showAlertDialog({
      title: `Delete "${label}"?`,
      message: 'This permanently deletes the archived session transcript from the host.',
      autoCancel: true,
      primaryButton: {
        value: 'Cancel',
        fontColor: this.store.palette.fgSecondary,
        action: () => {
          // Dismissed
        }
      },
      secondaryButton: {
        value: 'Delete',
        fontColor: this.store.palette.statusDanger,
        action: () => {
          this.store.deleteSessionPermanently(agent.agent_id);
        }
      }
    });
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text('Archived Sessions')
            .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
          Text(`${this.archivedList.length} archived on current host`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)
      }
      .width('100%')

      if (this.archivedList.length === 0) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Text('No archived sessions')
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
          Text('Archived conversations appear here so you can restore or delete them anytime.')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .textAlign(TextAlign.Center)
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_XL)
        .backgroundColor(this.store.palette.listCard)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: OrbitTokens.SPACE_SM }) {
          ForEach(
            this.archivedList,
            (ag: AgentCardDto) => {
              ListItem() {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Column({ space: 3 }) {
                    Text(ag.title.trim().length > 0 ? ag.title : ag.provider_label)
                      .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })

                    Text(
                      `${basenameOfPath(ag.cwd)} · ${formatProviderLabel(ag.provider)}${formatCompactRelativeTime(ag.updated_at_rfc3339).length > 0 ? ` · ${formatCompactRelativeTime(ag.updated_at_rfc3339)}` : ''}`
                    )
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)

                  Button('Restore', { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                    )
                    .height(34)
                    .padding({ left: 12, right: 12 })
                    .onClick(() => {
                      this.store.unarchiveSession(ag.agent_id);
                    })

                  Button({ type: ButtonType.Circle, stateEffect: true }) {
                    SymbolGlyph($r('sys.symbol.trash'))
                      .fontSize(15)
                      .fontColor([this.store.palette.statusDanger])
                  }
                  .width(34)
                  .height(34)
                  .padding(0)
                  .backgroundColor(Color.Transparent)
                  .accessibilityText('Delete session permanently')
                  .onClick(() => {
                    this.confirmPermanentDelete(ag);
                  })
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_LG)
                .alignItems(VerticalAlign.Center)
              }
            },
            (ag: AgentCardDto): string => `arch_${ag.agent_id}_${ag.updated_at_rfc3339}`
          )
        }
        .width('100%')
        .layoutWeight(1)
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .height('100%')
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_LG,
      bottom: Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
    })
    .backgroundColor(this.store.palette.drawer)
  }
}
