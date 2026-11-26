import {
  AgentCardDto,
  basenameOfPath,
  formatCompactRelativeTime,
  formatProviderLabel
} from '../../model/OrbitTypes';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitTextMeta,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import {
  OrbitAgentListItem,
  mapAgentCardToListStatus,
  resolveAgentEffortDisplay,
  resolveCompactAgentModelLabel
} from '../common/OrbitAgentListItem';

@ComponentV2
export struct ArchivedSessionsSheet {
  private readonly store: OrbitHarmonyStore = orbitRuntime();

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

  @Builder
  private renderArchivedActions(ag: AgentCardDto) {
    Row({ space: OrbitTokens.SPACE_XS }) {
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
    .alignItems(VerticalAlign.Center)
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
                OrbitAgentListItem({
                  variant: 'card',
                  title: ag.title.trim().length > 0 ? ag.title : ag.provider_label,
                  status: mapAgentCardToListStatus(ag),
                  avatarSeedKey: ag.agent_id,
                  provider: ag.provider,
                  modelLabel: resolveCompactAgentModelLabel(
                    ag,
                    this.store.currentHostDetail?.provider_catalogs ?? []
                  ),
                  effortLabel: resolveAgentEffortDisplay(ag),
                  agentLabel: formatProviderLabel(ag.provider),
                  secondaryMeta: basenameOfPath(ag.cwd),
                  titleTrailingText: formatCompactRelativeTime(ag.updated_at_rfc3339),
                  interactive: false,
                  trailingSlot: (): void => {
                    this.renderArchivedActions(ag);
                  }
                })
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
