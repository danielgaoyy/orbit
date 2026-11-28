import {
  HighlightedLineDto,
  RenderDiagramViewDto
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitTextMeta,
  orbitTextMono
} from '../../theme/OrbitTokens';
import { OrbitCopyIconButton } from '../common/OrbitCommonControls';
import { CodeHighlightBox } from './RichMarkdownView';

/**
 * Lightweight native diagram block rendered inside the virtualized conversation list.
 *
 * Architectural invariant (§8.2 P5 / `apps/harmony/AGENTS.md`):
 * NEVER instantiates ArkWeb (`<Web>`) inside `List` / `Repeat` conversation items.
 * Interactive SVG rendering lives exclusively in the single on-demand full-screen Lightbox
 * (`OrbitDiagramLightboxContent` inside `OrbitModalContentRouter`) with `javaScriptAccess(false)`.
 */
@ComponentV2
export struct DiagramBlockView {
  @Param diagramKind: string = 'mermaid';
  @Param diagramLanguage: string = '';
  @Param diagramSource: string = '';
  @Param diagramLines: HighlightedLineDto[] = [];

  @Local renderedView: RenderDiagramViewDto | null = null;
  @Local isLoading: boolean = true;
  @Local viewMode: string = 'diagram'; // 'diagram' | 'code'

  private requestSeq: number = 0;

  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  aboutToAppear(): void {
    void this.requestRender();
  }

  @Monitor('diagramKind', 'diagramSource', 'store.palette')
  onDiagramInputsChanged(): void {
    void this.requestRender();
  }

  private async requestRender(): Promise<void> {
    this.requestSeq++;
    const curSeq = this.requestSeq;
    this.isLoading = true;
    const view = await this.store.renderDiagram(
      this.diagramKind,
      this.diagramSource,
      this.store.palette.isDark
    );
    if (curSeq !== this.requestSeq) {
      return;
    }
    this.renderedView = view;
    this.isLoading = false;
  }

  @Computed
  get kindBaseTitle(): string {
    if (this.diagramKind === 'graphviz') {
      return 'Graphviz';
    }
    if (this.diagramKind === 'svg') {
      return 'SVG';
    }
    return 'Mermaid';
  }

  @Computed
  get headerLabel(): string {
    const typeLabel = this.renderedView?.diagram_type_label ?? '';
    if (typeLabel.length > 0 && typeLabel !== this.kindBaseTitle) {
      return `${this.kindBaseTitle} · ${typeLabel}`;
    }
    return this.kindBaseTitle;
  }

  @Computed
  get hasValidSvg(): boolean {
    return (
      this.renderedView !== null &&
      this.renderedView.error === null &&
      this.renderedView.svg !== null &&
      this.renderedView.svg.length > 0
    );
  }

  @Computed
  get previewSnippet(): string {
    const lines = this.diagramSource.split('\n');
    const kept: string[] = [];
    for (let i = 0; i < lines.length && kept.length < 4; i++) {
      const trimmed = lines[i].trim();
      if (trimmed.length > 0) {
        kept.push(trimmed);
      }
    }
    return kept.join('\n');
  }

  private openLightbox(): void {
    this.store.triggerHapticPreset('action');
    this.store.openDiagramViewerModal(
      this.diagramKind,
      this.diagramLanguage,
      this.diagramSource,
      this.renderedView?.svg ?? '',
      this.headerLabel
    );
  }

  build() {
    Column() {
      Row({ space: OrbitTokens.SPACE_XS }) {
        SymbolGlyph($r('sys.symbol.code'))
          .fontSize(14)
          .fontColor([this.store.palette.iconSecondary])

        Text(this.headerLabel)
          .attributeModifier(orbitTextMono(this.store.palette.fgMuted, FontWeight.Medium))
          .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        if (this.hasValidSvg) {
          Row({ space: 2 }) {
            Button($r('app.string.diagram_tab_preview'), { type: ButtonType.Normal, stateEffect: true })
              .constraintSize({ minHeight: 26 })
              .padding({ left: 8, right: 8 })
              .fontSize(11)
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .fontColor(
                this.viewMode === 'diagram'
                  ? this.store.palette.fgPrimary
                  : this.store.palette.fgMuted
              )
              .backgroundColor(
                this.viewMode === 'diagram' ? this.store.palette.elevated : Color.Transparent
              )
              .borderRadius(6)
              .accessibilityText($r('app.string.diagram_tab_preview'))
              .onClick(() => {
                this.store.triggerHapticPreset('selection');
                this.viewMode = 'diagram';
              })

            Button($r('app.string.diagram_tab_code'), { type: ButtonType.Normal, stateEffect: true })
              .constraintSize({ minHeight: 26 })
              .padding({ left: 8, right: 8 })
              .fontSize(11)
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .fontColor(
                this.viewMode === 'code'
                  ? this.store.palette.fgPrimary
                  : this.store.palette.fgMuted
              )
              .backgroundColor(
                this.viewMode === 'code' ? this.store.palette.elevated : Color.Transparent
              )
              .borderRadius(6)
              .accessibilityText($r('app.string.diagram_tab_code'))
              .onClick(() => {
                this.store.triggerHapticPreset('selection');
                this.viewMode = 'code';
              })
          }
          .padding(2)
          .borderRadius(8)
          .backgroundColor(this.store.palette.subtleSurface)

          Button({ type: ButtonType.Normal, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.arrow_up_left_and_arrow_down_right'))
              .fontSize(14)
              .fontColor([this.store.palette.iconSecondary])
          }
          .width(OrbitTokens.HIT_COMPACT)
          .height(OrbitTokens.HIT_COMPACT)
          .backgroundColor(Color.Transparent)
          .accessibilityText($r('app.string.diagram_expand_a11y'))
          .onClick(() => {
            this.openLightbox();
          })
        } else if (this.renderedView !== null && this.renderedView.error !== null) {
          Text($r('app.string.diagram_code_view'))
            .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Medium))
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
            .padding({ left: 6, right: 6 })
        }

        OrbitCopyIconButton({
          content: this.diagramSource,
          accessibilityLabel: $r('app.string.diagram_copy_source_a11y'),
          buttonSize: OrbitTokens.HIT_COMPACT
        })
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_XS })
      .alignItems(VerticalAlign.Center)
      .border({
        width: { bottom: OrbitTokens.HAIRLINE },
        color: this.store.palette.hairline
      })

      if (this.viewMode === 'diagram' && this.hasValidSvg && this.renderedView !== null) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Row({ space: OrbitTokens.SPACE_SM }) {
            SymbolGlyph($r('sys.symbol.picture'))
              .fontSize(18)
              .fontColor([this.store.palette.accentPrimary])
            Text(
              $r(
                'app.string.diagram_ready_summary',
                this.headerLabel,
                Math.max(0, Math.round(this.renderedView.width)),
                Math.max(0, Math.round(this.renderedView.height))
              )
            )
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .layoutWeight(1)
              .maxLines(2)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)

          if (this.previewSnippet.length > 0) {
            Text(this.previewSnippet)
              .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
              .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
              .maxLines(4)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .width('100%')
              .padding(OrbitTokens.SPACE_SM)
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.RADIUS_SM)
          }

          Row() {
            Button($r('app.string.diagram_open_fullscreen'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
              )
              .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
              .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .accessibilityText($r('app.string.diagram_expand_a11y'))
              .onClick(() => {
                this.openLightbox();
              })
          }
          .width('100%')
          .justifyContent(FlexAlign.End)
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_MD)
        .alignItems(HorizontalAlign.Start)
        .onClick(() => {
          this.openLightbox();
        })
      } else if (this.isLoading && this.viewMode === 'diagram') {
        Row({ space: OrbitTokens.SPACE_SM }) {
          LoadingProgress()
            .width(18)
            .height(18)
            .color(this.store.palette.accentPrimary)
          Text($r('app.string.diagram_rendering', this.kindBaseTitle))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxFontScale(OrbitTokens.MAX_FONT_SCALE_CHROME)
        }
        .width('100%')
        .constraintSize({ minHeight: 88 })
        .justifyContent(FlexAlign.Center)
      } else {
        Column() {
          CodeHighlightBox({
            language: this.diagramLanguage,
            rawCode: this.diagramSource,
            lines: this.diagramLines
          })
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_XS)
      }
    }
    .width('100%')
    .backgroundColor(this.store.palette.code)
    .borderRadius(OrbitTokens.RADIUS_MD)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.palette.hairline
    })
    .clip(true)
  }
}
