import {
  ProviderModelSpecDto,
  ThinkingOptionSpecDto,
  formatEffortLabel,
  formatModeLabel,
  formatProviderLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';

const PROVIDERS: string[] = ['antigravity', 'codex', 'claude_code'];
const FALLBACK_EFFORTS: string[] = ['off', 'low', 'medium', 'high', 'xhigh', 'max', 'ultra'];
const SESSION_MODES: string[] = ['default', 'plan', 'accept_edits', 'full_access'];

@ComponentV2
export struct ModelPickerSheet {
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private activeModelSpec(): ProviderModelSpecDto | null {
    const models = this.store.modelsForProvider(this.store.composerProvider);
    for (let i = 0; i < models.length; i++) {
      if (
        (this.store.composerModel.length === 0 && i === 0) ||
        models[i].id === this.store.composerModel
      ) {
        return models[i];
      }
    }
    return models.length > 0 ? models[0] : null;
  }

  private availableEfforts(): string[] {
    const active = this.activeModelSpec();
    if (active !== null && active.thinking_options.length > 0) {
      const slugs: string[] = [];
      for (let j = 0; j < active.thinking_options.length; j++) {
        slugs.push(active.thinking_options[j].effort);
      }
      return slugs;
    }
    return FALLBACK_EFFORTS;
  }

  private activeEffortDescription(): string {
    const eff = this.store.composerEffort.trim().toLowerCase();
    const active = this.activeModelSpec();
    if (active !== null) {
      for (let i = 0; i < active.thinking_options.length; i++) {
        const opt: ThinkingOptionSpecDto = active.thinking_options[i];
        if (
          opt.effort.toLowerCase() === eff &&
          opt.description !== undefined &&
          opt.description !== null &&
          opt.description.trim().length > 0
        ) {
          return opt.description.trim();
        }
      }
    }
    if (eff === 'off') {
      return 'Instant direct replies';
    }
    if (eff === 'low') {
      return 'Light thinking budget';
    }
    if (eff === 'medium') {
      return 'Balanced speed and depth';
    }
    if (eff === 'xhigh' || eff === 'extra_high') {
      return 'Extended multi-step verification';
    }
    if (eff === 'max' || eff === 'ultra') {
      return 'Maximum deep reasoning budget';
    }
    return 'Thorough architecture & code reasoning';
  }

  private cleanModelDescription(spec: ProviderModelSpecDto): string {
    const raw = spec.description.trim();
    if (raw.length === 0) {
      return '';
    }
    const prefix = `${spec.label} · `;
    if (raw.startsWith(prefix)) {
      return raw.substring(prefix.length).trim();
    }
    const lowerPrefix = `${spec.label.toLowerCase()} · `;
    if (raw.toLowerCase().startsWith(lowerPrefix)) {
      return raw.substring(lowerPrefix.length).trim();
    }
    return raw;
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row() {
        Text('Model & Execution')
          .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
        Blank()
        Button('Done')
          .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
          .height(OrbitTokens.HIT_MIN)
          .onClick(() => {
            this.store.showModelPickerSheet = false;
          })
      }
      .width('100%')

      Scroll() {
        Column({ space: OrbitTokens.SPACE_LG }) {
          // 1. Provider Segmented Selector
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('PROVIDER')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            Row({ space: OrbitTokens.SPACE_SM }) {
              ForEach(
                PROVIDERS,
                (prov: string) => {
                  Button(prov === 'claude_code' ? 'Claude' : formatProviderLabel(prov))
                    .attributeModifier(
                      orbitButtonSubhead(
                        this.store.composerProvider === prov
                          ? this.store.palette.onAccent
                          : this.store.palette.fgPrimary,
                        this.store.composerProvider === prov
                          ? this.store.palette.accentPrimaryFill
                          : this.store.palette.subtleSurface
                      )
                    )
                    .height(OrbitTokens.HIT_MIN)
                    .borderRadius(OrbitTokens.CARD_RADIUS)
                    .layoutWeight(1)
                    .onClick(() => {
                      const models = this.store.modelsForProvider(prov);
                      const defaultId = models.length > 0 ? models[0].id : '';
                      const defaultEffort = models.length > 0 && models[0].default_effort !== null
                        ? models[0].default_effort ?? 'high'
                        : 'high';
                      this.store.applyProviderModelEffort(prov, defaultId, defaultEffort);
                    })
                },
                (prov: string) => prov
              )
            }
            .width('100%')
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')

          // 2. Reasoning Effort Strip (Immediately below PROVIDER & above MODEL)
          Column({ space: OrbitTokens.SPACE_SM }) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text('REASONING EFFORT')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
              Text(`· ${this.activeEffortDescription()}`)
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .maxLines(1)
                .textOverflow({ overflow: TextOverflow.Ellipsis })
                .layoutWeight(1)
            }
            .width('100%')

            Scroll() {
              Row({ space: OrbitTokens.SPACE_SM }) {
                ForEach(
                  this.availableEfforts(),
                  (eff: string) => {
                    Button(formatEffortLabel(eff))
                      .attributeModifier(
                        orbitButtonSubhead(
                          this.store.composerEffort === eff
                            ? this.store.palette.onAccent
                            : this.store.palette.fgPrimary,
                          this.store.composerEffort === eff
                            ? this.store.palette.accentPrimaryFill
                            : this.store.palette.subtleSurface
                        )
                      )
                      .height(38)
                      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
                      .borderRadius(OrbitTokens.PILL_RADIUS)
                      .onClick(() => {
                        this.store.applyProviderModelEffort(
                          this.store.composerProvider,
                          this.store.composerModel,
                          eff
                        );
                      })
                  },
                  (eff: string) => eff
                )
              }
            }
            .scrollable(ScrollDirection.Horizontal)
            .scrollBar(BarState.Off)
            .width('100%')
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')

          // 3. Model Catalog List
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('MODEL')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            ForEach(
              this.store.modelsForProvider(this.store.composerProvider),
              (spec: ProviderModelSpecDto) => {
                Row({ space: OrbitTokens.SPACE_MD }) {
                  Column({ space: 2 }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text(spec.label)
                        .attributeModifier(
                          orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium)
                        )
                      if (spec.is_default) {
                        Text('Default')
                          .attributeModifier(
                            orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium)
                          )
                      }
                    }
                    Text(spec.id)
                      .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                    if (this.cleanModelDescription(spec).length > 0) {
                      Text(this.cleanModelDescription(spec))
                        .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                        .maxLines(2)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                    }
                  }
                  .alignItems(HorizontalAlign.Start)
                  .layoutWeight(1)

                  if (
                    this.store.composerModel === spec.id ||
                    (this.store.composerModel.length === 0 && spec.is_default)
                  ) {
                    Text('✓')
                      .attributeModifier(
                        orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold)
                      )
                  }
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(
                  this.store.composerModel === spec.id ||
                    (this.store.composerModel.length === 0 && spec.is_default)
                    ? this.store.palette.accentPrimarySoft
                    : this.store.palette.elevated
                )
                .borderRadius(OrbitTokens.CARD_RADIUS)
                .border({
                  width: OrbitTokens.HAIRLINE,
                  color: this.store.palette.hairline
                })
                .onClick(() => {
                  const nextEffort = spec.default_effort !== null
                    ? spec.default_effort
                    : this.store.composerEffort;
                  this.store.applyProviderModelEffort(
                    this.store.composerProvider,
                    spec.id,
                    nextEffort
                  );
                })
              },
              (spec: ProviderModelSpecDto) => spec.id
            )
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')

          // 4. Permission Mode
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('PERMISSION MODE')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

            Flex({ wrap: FlexWrap.Wrap }) {
              ForEach(
                SESSION_MODES,
                (mode: string) => {
                  Button(formatModeLabel(mode))
                    .attributeModifier(
                      orbitButtonSubhead(
                        this.store.composerMode === mode
                          ? this.store.palette.onAccent
                          : this.store.palette.fgPrimary,
                        this.store.composerMode === mode
                          ? this.store.palette.accentPrimaryFill
                          : this.store.palette.subtleSurface
                      )
                    )
                    .height(OrbitTokens.HIT_MIN)
                    .borderRadius(OrbitTokens.PILL_RADIUS)
                    .margin({ right: OrbitTokens.SPACE_SM, bottom: OrbitTokens.SPACE_SM })
                    .onClick(() => {
                      this.store.setSessionMode(mode);
                    })
                },
                (mode: string) => mode
              )
            }
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')
        }
        .width('100%')
        .padding({ bottom: OrbitTokens.SPACE_XL })
      }
      .layoutWeight(1)
      .scrollBar(BarState.Off)
    }
    .width('100%')
    .height('100%')
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_LG,
      bottom: Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
    })
    .backgroundColor(this.store.palette.canvas)
  }
}
