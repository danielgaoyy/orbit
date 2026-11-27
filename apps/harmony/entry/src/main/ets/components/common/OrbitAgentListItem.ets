import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { formatProviderLabel } from '../../model/OrbitTypes';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { OrbitTokens } from '../../theme/OrbitTokens';
import { OrbitProviderBrandIcon } from '../composer/ModelPickerSheet';
import { OrbitSubagentAvatar } from './OrbitSubagentAvatar';
import {
  OrbitAgentListItemVariant,
  isAgentListItemActiveStatus,
  resolveAgentListStatusResource
} from '../../model/OrbitAgentDisplayModel';

export {
  OrbitAgentListItemVariant,
  isAgentListItemActiveStatus,
  mapAgentCardToListStatus,
  mapBriefingStateToListStatus,
  mapCrewSubagentStatus,
  resolveAgentEffortDisplay,
  resolveAgentFallbackLabel,
  resolveAgentListStatusResource,
  resolveCompactAgentModelLabel,
  resolveCrewSubagentDepth,
  resolveCrewSubagentHostId
} from '../../model/OrbitAgentDisplayModel';

/**
 * Canonical strict two-line agent list row / card primitive shared across Sidebar conversations,
 * Personal Agent Briefing cards/rows, Subagent Crew, Workspace Board peers, and Archived sessions.
 *
 * Strict 2-line grammar (zero third-line prose):
 * - Line 1 (13 fp Medium): single-line truncated title + optional trailing stat/count/elapsed.
 * - Line 2 (11 fp): `[ProviderIcon 11vp][Model][Effort] · [Status]` (model truncatable, rest non-shrinking);
 *   degrades to `Agent · Status` when model is unknown/`default` or `depth >= 4`.
 */
@ReusableV2
@ComponentV2
export struct OrbitAgentListItem {
  @Param @Require title: string;
  @Param status: string = 'idle';
  @Param characterId: string = '';
  @Param avatarSeedKey: string = '';
  @Param avatarSizeVp: number = 0;
  @Param showStatusBadge: boolean = true;
  @Param leadingMode: string = 'avatar'; // 'avatar' | 'signal'

  @Param provider: string = '';
  @Param modelLabel: string = '';
  @Param effortLabel: string = '';
  @Param agentLabel: string = '';
  @Param depth: number = 0;
  @Param statusLabel: ResourceStr = '';
  @Param tone: string = 'default'; // 'default' | 'warning' | 'danger'
  @Param secondaryMeta: string = '';

  @Param titleTrailingText: ResourceStr = '';
  @Param diffAdditions: number = 0;
  @Param diffDeletions: number = 0;
  @Param isUnread: boolean = false;
  @Param hasArmedTimers: boolean = false;
  @Param isWorktree: boolean = false;

  @Param variant: OrbitAgentListItemVariant = 'row';
  @Param indentLevel: number = 0; // 0..5
  @Param isSelected: boolean = false;
  @Param interactive: boolean = true;

  @BuilderParam trailingSlot?: () => void;
  @Event onItemClick: () => void = () => {};

  private readonly store: OrbitHarmonyStore = orbitRuntime();

  private clampedIndent(): number {
    if (this.indentLevel <= 0) {
      return 0;
    }
    if (this.indentLevel >= 5) {
      return 5;
    }
    return Math.floor(this.indentLevel);
  }

  private resolvedTone(): string {
    if (this.tone === 'warning' || this.tone === 'danger') {
      return this.tone;
    }
    if (this.status === 'awaiting_permission') {
      return 'warning';
    }
    if (this.status === 'failed') {
      return 'danger';
    }
    return 'default';
  }

  private resolvedAvatarSize(): number {
    if (this.avatarSizeVp > 0) {
      return this.avatarSizeVp;
    }
    return this.clampedIndent() > 0 ? 20 : 24;
  }

  private effectiveSeed(): string {
    return this.avatarSeedKey.trim().length > 0 ? this.avatarSeedKey : this.title;
  }

  private shouldShowModel(): boolean {
    const clean = this.modelLabel.trim();
    return clean.length > 0 && clean.toLowerCase() !== 'default' && this.depth < 4;
  }

  private resolvedAgentFallbackLabel(): ResourceStr {
    if (this.agentLabel.trim().length > 0) {
      return this.agentLabel.trim();
    }
    if (this.provider.trim().length > 0) {
      return formatProviderLabel(this.provider);
    }
    return $r('app.string.briefing_agentFallback');
  }

  private resolvedStatusResource(): ResourceStr {
    if (typeof this.statusLabel === 'string') {
      if (this.statusLabel.trim().length > 0) {
        return this.statusLabel;
      }
      return resolveAgentListStatusResource(this.status);
    }
    return this.statusLabel;
  }

  private statusTextColor(): ResourceColor {
    const t = this.resolvedTone();
    if (t === 'warning') {
      return this.store.palette.statusWarning;
    }
    if (t === 'danger') {
      return this.store.palette.statusDanger;
    }
    return this.store.palette.fgMuted;
  }

  private hasDiffStat(): boolean {
    return this.diffAdditions > 0 || this.diffDeletions > 0;
  }

  private hasTrailingLabel(): boolean {
    if (typeof this.titleTrailingText === 'string') {
      return this.titleTrailingText.trim().length > 0;
    }
    return true;
  }

  private hasTitleTrailing(): boolean {
    return this.hasDiffStat() || this.hasTrailingLabel();
  }

  private rowBackgroundColor(): ResourceColor {
    if (this.resolvedTone() === 'warning') {
      return this.store.palette.statusWarningSoft;
    }
    if (this.isSelected) {
      return this.store.palette.subtleSurface;
    }
    return Color.Transparent;
  }

  @Builder
  private renderLeadingVisual() {
    if (this.leadingMode === 'signal') {
      Row() {
        if (this.status === 'awaiting_permission') {
          SymbolGlyph($r('sys.symbol.exclamationmark_triangle_fill'))
            .fontSize(13)
            .fontColor([this.store.palette.statusWarning])
        } else if (this.status === 'failed') {
          SymbolGlyph($r('sys.symbol.exclamationmark_circle_fill'))
            .fontSize(13)
            .fontColor([this.store.palette.statusDanger])
        } else if (this.status === 'running') {
          LoadingProgress()
            .width(14)
            .height(14)
            .color(this.store.palette.iconSecondary)
        } else if (this.isUnread) {
          Row()
            .width(7)
            .height(7)
            .borderRadius(3.5)
            .backgroundColor(this.store.palette.unreadBadge)
        } else if (this.hasArmedTimers) {
          SymbolGlyph($r('sys.symbol.clock'))
            .fontSize(12)
            .fontColor([this.store.palette.statusInfo])
        } else if (this.isWorktree) {
          SymbolGlyph($r('sys.symbol.arrow_triangle_divide'))
            .fontSize(12)
            .fontColor([this.store.palette.fgFaint])
        }
      }
      .width(18)
      .justifyContent(FlexAlign.Center)
      .alignItems(VerticalAlign.Center)
      .flexShrink(0)
    } else {
      OrbitSubagentAvatar({
        characterId: this.characterId,
        seedKey: this.effectiveSeed(),
        status: this.status,
        avatarSize: this.resolvedAvatarSize(),
        showStatusBadge: this.showStatusBadge && !isAgentListItemActiveStatus(this.status)
      })
    }
  }

  @Builder
  private renderTwoLineContent() {
    Row({ space: OrbitTokens.SPACE_SM }) {
      this.renderLeadingVisual()

      Column({ space: 2 }) {
        // Line 1: 13 fp Medium single-line title + optional compact trailing stat/count/elapsed
        Row({ space: 6 }) {
          Text(this.title)
            .fontSize(OrbitTokens.FONT_SUBHEAD)
            .fontWeight(FontWeight.Medium)
            .fontColor(this.store.palette.fgPrimary)
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)

          if (this.hasTitleTrailing()) {
            Row({ space: 4 }) {
              if (this.diffAdditions > 0) {
                Text(`+${this.diffAdditions}`)
                  .fontSize(OrbitTokens.FONT_META)
                  .fontFamily(OrbitTokens.FAMILY_MONO)
                  .fontColor(this.store.palette.statusDirect)
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .maxLines(1)
              }
              if (this.diffDeletions > 0) {
                Text(`-${this.diffDeletions}`)
                  .fontSize(OrbitTokens.FONT_META)
                  .fontFamily(OrbitTokens.FAMILY_MONO)
                  .fontColor(this.store.palette.statusDanger)
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .maxLines(1)
              }
              if (this.hasTrailingLabel()) {
                Text(this.titleTrailingText)
                  .fontSize(OrbitTokens.FONT_META)
                  .fontFamily(OrbitTokens.FAMILY_MONO)
                  .fontColor(this.store.palette.fgFaint)
                  .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                  .maxLines(1)
              }
            }
            .flexShrink(0)
            .alignItems(VerticalAlign.Center)
          }
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        // Line 2: 11 fp [ProviderIcon 11vp][Model][Effort] · [Status] (· optional secondaryMeta)
        Row({ space: 4 }) {
          if (this.provider.trim().length > 0) {
            OrbitProviderBrandIcon({
              provider: this.provider,
              iconSize: 11
            })
              .flexShrink(0)
          }

          if (this.shouldShowModel()) {
            Text(this.modelLabel.trim())
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgMuted)
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .flexShrink(1)

            if (this.effortLabel.trim().length > 0) {
              Text(this.effortLabel.trim())
                .fontSize(OrbitTokens.FONT_META)
                .fontColor(this.store.palette.fgFaint)
                .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
                .maxLines(1)
                .flexShrink(0)
            }
          } else {
            Text(this.resolvedAgentFallbackLabel())
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgMuted)
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .maxLines(1)
              .flexShrink(0)
          }

          Text('·') // i18n-ignore
            .fontSize(OrbitTokens.FONT_META)
            .fontColor(this.store.palette.fgFaint)
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .flexShrink(0)

          Text(this.resolvedStatusResource())
            .fontSize(OrbitTokens.FONT_META)
            .fontWeight(this.resolvedTone() === 'warning' ? FontWeight.Medium : FontWeight.Normal)
            .fontColor(this.statusTextColor())
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .maxLines(1)
            .flexShrink(0)

          if (this.secondaryMeta.trim().length > 0) {
            Text('·') // i18n-ignore
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgFaint)
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .flexShrink(0)

            Text(this.secondaryMeta.trim())
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgMuted)
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .flexShrink(1)
          }
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)
      }
      .alignItems(HorizontalAlign.Start)
      .layoutWeight(1)
    }
    .layoutWeight(1)
    .alignItems(VerticalAlign.Center)
  }

  build() {
    if (this.variant === 'card') {
      Row({ space: 6 }) {
        Row() {
          this.renderTwoLineContent()
        }
        .layoutWeight(1)
        .alignItems(VerticalAlign.Center)
        .onClick(() => {
          if (this.interactive) {
            this.onItemClick();
          }
        })

        if (this.trailingSlot !== undefined) {
          Row({ space: 4 }) {
            this.trailingSlot()
          }
          .flexShrink(0)
          .alignItems(VerticalAlign.Center)
        }
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: 8, bottom: 8 })
      .borderRadius(OrbitTokens.RADIUS_LG)
      .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
      .border(
        this.resolvedTone() === 'danger'
          ? { width: 1, color: this.store.palette.statusDanger }
          : (this.resolvedTone() === 'warning'
            ? { width: 1, color: this.store.palette.statusWarning }
            : { width: OrbitTokens.HAIRLINE, color: this.store.palette.borderDefault })
      )
      .alignItems(VerticalAlign.Center)
    } else {
      Row() {
        Row({ space: 6 }) {
          Row() {
            this.renderTwoLineContent()
          }
          .layoutWeight(1)
          .alignItems(VerticalAlign.Center)
          .onClick(() => {
            if (this.interactive) {
              this.onItemClick();
            }
          })

          if (this.trailingSlot !== undefined) {
            Row({ space: 4 }) {
              this.trailingSlot()
            }
            .flexShrink(0)
            .alignItems(VerticalAlign.Center)
          }
        }
        .width('100%')
        .constraintSize({ minHeight: 42 })
        .padding({
          left: this.clampedIndent() > 0 ? 10 : 8,
          right: 8,
          top: 6,
          bottom: 6
        })
        .backgroundColor(this.rowBackgroundColor())
        .borderRadius(this.clampedIndent() > 0 ? OrbitTokens.RADIUS_XS : OrbitTokens.RADIUS_MD)
        .border(
          this.clampedIndent() > 0
            ? { width: { left: 1 }, color: this.store.palette.hairline }
            : { width: 0, color: Color.Transparent }
        )
        .alignItems(VerticalAlign.Center)
      }
      .width('100%')
      .padding({ left: this.clampedIndent() * 12 })
    }
  }
}
