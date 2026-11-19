import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2ItemOptions,
  SegmentButtonV2Items
} from '@kit.ArkUI';
import {
  ProviderModelSpecDto,
  ThinkingOptionSpecDto,
  formatEffortLabel,
  formatProviderLabel,
  formatShortEffortLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitSegmentSelectedBg,
  orbitSegmentSelectedFg,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';

@ComponentV2
export struct OrbitProviderBrandIcon {
  @Param provider: string = 'antigravity';
  @Param iconSize: number = 16;

  private providerResource(): Resource {
    if (this.provider === 'codex') {
      return $r('app.media.provider_codex');
    }
    if (this.provider === 'claude_code' || this.provider === 'claude') {
      return $r('app.media.provider_claude');
    }
    return $r('app.media.provider_antigravity');
  }

  build() {
    Image(this.providerResource())
      .width(this.iconSize)
      .height(this.iconSize)
      .objectFit(ImageFit.Contain)
      .interpolation(ImageInterpolation.High)
      .draggable(false)
  }
}

@ComponentV2
export struct ModelPickerSheet {
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get isNewSession(): boolean {
    return this.store.isNewSessionDraft || this.store.selectedAgentId.length === 0;
  }

  @Computed
  get detectedProviders(): string[] {
    const detail = this.store.currentHostDetail;
    if (detail !== null && detail.host.available_providers.length > 0) {
      const unique: string[] = [];
      for (let i = 0; i < detail.host.available_providers.length; i++) {
        const p = detail.host.available_providers[i];
        if (!unique.includes(p)) {
          unique.push(p);
        }
      }
      if (unique.length > 0) {
        return unique;
      }
    }
    return ['antigravity', 'codex', 'claude_code'];
  }

  @Computed
  get showAgentSwitcher(): boolean {
    return this.isNewSession && this.detectedProviders.length > 1;
  }

  @Computed
  get providerModels(): ProviderModelSpecDto[] {
    return this.store.modelsForProvider(this.store.composerProvider);
  }

  @Computed
  get activeModelSpec(): ProviderModelSpecDto | null {
    const models = this.providerModels;
    for (let i = 0; i < models.length; i++) {
      if (models[i].id === this.store.composerModel) {
        return models[i];
      }
    }
    for (let i = 0; i < models.length; i++) {
      if (models[i].is_default) {
        return models[i];
      }
    }
    return models.length > 0 ? models[0] : null;
  }

  @Computed
  get availableEfforts(): ThinkingOptionSpecDto[] {
    const active = this.activeModelSpec;
    if (active !== null && active.thinking_options.length > 0) {
      return active.thinking_options;
    }
    return [];
  }

  @Computed
  get activeProviderIndex(): number {
    const idx = this.detectedProviders.indexOf(this.store.composerProvider);
    return idx >= 0 ? idx : 0;
  }

  @Computed
  get providerSegmentItems(): SegmentButtonV2Items {
    const provs = this.detectedProviders;
    const opts: SegmentButtonV2ItemOptions[] = [];
    for (let i = 0; i < provs.length; i++) {
      const prov = provs[i];
      opts.push({
        text: prov === 'antigravity' ? 'Antigravity' : formatProviderLabel(prov)
      });
    }
    return new SegmentButtonV2Items(opts);
  }

  @Computed
  get activeEffortSlug(): string {
    const active = this.activeModelSpec;
    if (active !== null) {
      return this.resolvedEffortSlugForRow(active).trim().toLowerCase();
    }
    const efforts = this.availableEfforts;
    if (efforts.length === 0) {
      return '';
    }
    return efforts[0].effort.trim().toLowerCase();
  }

  @Computed
  get activeEffortIndex(): number {
    const slug = this.activeEffortSlug;
    const efforts = this.availableEfforts;
    for (let i = 0; i < efforts.length; i++) {
      if (efforts[i].effort.trim().toLowerCase() === slug) {
        return i;
      }
    }
    return 0;
  }

  @Computed
  get effortSegmentItems(): SegmentButtonV2Items {
    const efforts = this.availableEfforts;
    const count = efforts.length;
    const opts: SegmentButtonV2ItemOptions[] = [];
    for (let i = 0; i < count; i++) {
      const opt = efforts[i];
      opts.push({
        text: count >= 4
          ? formatShortEffortLabel(opt.effort)
          : (opt.label.trim().length > 0 ? opt.label.trim() : formatEffortLabel(opt.effort))
      });
    }
    return new SegmentButtonV2Items(opts);
  }

  @Computed
  get activeEffortDescription(): string {
    const eff = this.activeEffortSlug;
    const opts = this.availableEfforts;
    for (let i = 0; i < opts.length; i++) {
      if (
        opts[i].effort.toLowerCase() === eff &&
        opts[i].description !== undefined &&
        opts[i].description !== null &&
        opts[i].description!.trim().length > 0
      ) {
        return opts[i].description!.trim();
      }
    }
    if (eff === 'off') {
      return 'Direct replies without extended reasoning';
    }
    if (eff === 'low') {
      return 'Fast lightweight reasoning';
    }
    if (eff === 'medium') {
      return 'Balanced speed and reasoning depth';
    }
    if (eff === 'xhigh' || eff === 'extra_high') {
      return 'Deep verification across files';
    }
    if (eff === 'max' || eff === 'ultra') {
      return 'Maximum deep reasoning budget';
    }
    return eff.length > 0 ? formatEffortLabel(eff) : '';
  }

  private isSelectedModel(spec: ProviderModelSpecDto): boolean {
    const active = this.activeModelSpec;
    if (active !== null) {
      return active.id === spec.id;
    }
    return this.store.composerModel === spec.id;
  }

  private cleanModelLabel(spec: ProviderModelSpecDto): string {
    if (this.store.composerProvider === 'claude_code' && spec.label.startsWith('Claude ')) {
      const stripped = spec.label.substring('Claude '.length).trim();
      if (stripped.length > 0) {
        return stripped;
      }
    }
    return spec.label;
  }

  private resolvedEffortSlugForRow(spec: ProviderModelSpecDto): string {
    if (spec.thinking_options.length === 0) {
      return '';
    }
    const isActive = this.isSelectedModel(spec);
    const catalog = this.store.catalogForProvider(this.store.composerProvider);
    const candidates: string[] = [];
    if (isActive && this.store.composerEffort.trim().length > 0) {
      candidates.push(this.store.composerEffort.trim().toLowerCase());
    }
    if (spec.default_effort !== undefined && spec.default_effort !== null && spec.default_effort.trim().length > 0) {
      candidates.push(spec.default_effort.trim().toLowerCase());
    }
    if (
      catalog !== null &&
      catalog.configured_default_effort !== undefined &&
      catalog.configured_default_effort !== null &&
      catalog.configured_default_effort.trim().length > 0
    ) {
      candidates.push(catalog.configured_default_effort.trim().toLowerCase());
    }
    for (let c = 0; c < candidates.length; c++) {
      for (let i = 0; i < spec.thinking_options.length; i++) {
        if (spec.thinking_options[i].effort.trim().toLowerCase() === candidates[c]) {
          return spec.thinking_options[i].effort;
        }
      }
    }
    for (let i = 0; i < spec.thinking_options.length; i++) {
      if (spec.thinking_options[i].is_default) {
        return spec.thinking_options[i].effort;
      }
    }
    return spec.thinking_options[0].effort;
  }

  private resolvedEffortForRow(spec: ProviderModelSpecDto): string {
    const slug = this.resolvedEffortSlugForRow(spec);
    return slug.length > 0 ? formatEffortLabel(slug) : '';
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      // Top Native HarmonyOS 7 CapsuleSegmentButtonV2 Provider Switcher
      if (this.showAgentSwitcher) {
        Row() {
          CapsuleSegmentButtonV2({
            items: this.providerSegmentItems,
            selectedIndex: this.activeProviderIndex,
            onItemClicked: (idx: number) => {
              const provs = this.detectedProviders;
              if (idx >= 0 && idx < provs.length) {
                this.store.switchDraftProvider(provs[idx]);
              }
            },
            itemFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
            itemSelectedFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
            itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
            itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
            itemMaxFontScale: 1.3,
            itemMinHeight: LengthMetrics.vp(36),
            itemPadding: {
              top: LengthMetrics.vp(4),
              bottom: LengthMetrics.vp(4),
              start: LengthMetrics.vp(4),
              end: LengthMetrics.vp(4)
            }
          })
            .width('100%')
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }

      // Provider Header Row + Native Refresh Circle Button
      Row({ space: 6 }) {
        OrbitProviderBrandIcon({ provider: this.store.composerProvider, iconSize: 15 })
        Text(formatProviderLabel(this.store.composerProvider))
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
          .maxLines(1)
        Blank()
        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(14)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconSecondary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(Color.Transparent)
        .accessibilityText('Refresh Model Catalog')
        .onClick(() => {
          this.store.refreshModelsAndQuotas(true);
        })
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      // Native Model List
      Scroll() {
        Column({ space: 2 }) {
          ForEach(
            this.providerModels,
            (spec: ProviderModelSpecDto) => {
              Button({ type: ButtonType.Normal, stateEffect: true }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Row({ space: 6 }) {
                    Text(this.cleanModelLabel(spec))
                      .attributeModifier(
                        orbitTextSubhead(
                          this.store.palette.fgPrimary,
                          this.isSelectedModel(spec) ? FontWeight.Medium : FontWeight.Normal
                        )
                      )
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                      .flexShrink(1)

                    if (this.resolvedEffortForRow(spec).length > 0) {
                      Text(this.resolvedEffortForRow(spec))
                        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                        .maxLines(1)
                        .flexShrink(0)
                    }
                  }
                  .layoutWeight(1)
                  .alignItems(VerticalAlign.Center)

                  if (this.isSelectedModel(spec)) {
                    SymbolGlyph($r('sys.symbol.checkmark'))
                      .fontSize(15)
                      .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                      .fontColor([this.store.palette.accentPrimary])
                  }
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
              .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .borderRadius(OrbitTokens.RADIUS_MD)
              .backgroundColor(
                this.isSelectedModel(spec) ? this.store.palette.subtleSurface : Color.Transparent
              )
              .onClick(() => {
                if (this.isSelectedModel(spec)) {
                  this.store.triggerHaptic(6);
                  this.store.showModelPickerSheet = false;
                  return;
                }
                const targetEffort = this.resolvedEffortSlugForRow(spec);
                this.store.applyProviderModelEffort(
                  this.store.composerProvider,
                  spec.id,
                  targetEffort
                );
                if (spec.thinking_options.length === 0) {
                  this.store.showModelPickerSheet = false;
                }
              })
            },
            (spec: ProviderModelSpecDto) => `${this.store.composerProvider}_${spec.id}`
          )
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }
      .width('100%')
      .constraintSize({ maxHeight: 268 })
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)

      // Pinned Bottom Native HarmonyOS 7 CapsuleSegmentButtonV2 Thinking Switcher
      if (this.availableEfforts.length > 0) {
        Divider()
          .strokeWidth(OrbitTokens.HAIRLINE)
          .color(this.store.palette.hairline)
          .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })

        Column({ space: 6 }) {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text('Thinking')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
            Blank()
            if (this.activeEffortDescription.length > 0) {
              Text(this.activeEffortDescription)
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .constraintSize({ maxWidth: '68%' })
            }
          }
          .width('100%')
          .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS })

          CapsuleSegmentButtonV2({
            items: this.effortSegmentItems,
            selectedIndex: this.activeEffortIndex,
            onItemClicked: (idx: number) => {
              const opts = this.availableEfforts;
              if (idx >= 0 && idx < opts.length) {
                this.store.selectEffort(opts[idx].effort);
              }
            },
            itemFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
            itemSelectedFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
            itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
            itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
            itemMaxFontScale: 1.25,
            itemMinHeight: LengthMetrics.vp(36),
            itemPadding: {
              top: LengthMetrics.vp(4),
              bottom: LengthMetrics.vp(4),
              start: LengthMetrics.vp(2),
              end: LengthMetrics.vp(2)
            }
          })
            .width('100%')
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }
    }
    .width(304)
    .padding({ top: OrbitTokens.SPACE_MD, bottom: OrbitTokens.SPACE_MD })
  }
}
