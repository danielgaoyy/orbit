import { webview } from '@kit.ArkWeb';
import {
  HighlightedLineDto,
  RenderDiagramViewDto
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitTokens, orbitTextMeta, orbitTextMono } from '../../theme/OrbitTokens';
import { OrbitCopyIconButton } from '../common/OrbitCommonControls';
import { CodeHighlightBox } from './RichMarkdownView';

function buildHarmonyDiagramHtml(svg: string, isDark: boolean, zoomable: boolean): string {
  const fgHex = isDark ? '#F4F4F5' : '#18181B';
  const viewport = zoomable
    ? '<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=5.0, user-scalable=yes">'
    : '<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=1.0, user-scalable=no">';
  const overflowMode = zoomable ? 'auto' : 'hidden';
  const padPx = zoomable ? '16px' : '8px';
  return [
    '<!doctype html>',
    '<html><head><meta charset="utf-8">',
    viewport,
    '<style>',
    `html, body { margin: 0; padding: 0; width: 100%; height: 100%; background: transparent; color: ${fgHex}; overflow: ${overflowMode}; font-family: sans-serif; }`,
    `.wrap { width: 100%; min-height: 100%; display: flex; align-items: center; justify-content: center; box-sizing: border-box; padding: ${padPx}; }`,
    'svg { display: block; max-width: 100%; max-height: 100%; width: auto; height: auto; }',
    '</style></head>',
    `<body><div class="wrap">${svg}</div></body></html>`
  ].join('\n');
}

@ComponentV2
export struct DiagramBlockView {
  @Param diagramKind: string = 'mermaid';
  @Param diagramLanguage: string = '';
  @Param diagramSource: string = '';
  @Param diagramLines: HighlightedLineDto[] = [];

  @Local renderedView: RenderDiagramViewDto | null = null;
  @Local isLoading: boolean = true;
  @Local viewMode: string = 'diagram'; // 'diagram' | 'code'
  @Local showFullSheet: boolean = false;

  private inlineWebCtrl: webview.WebviewController = new webview.WebviewController();
  private fullWebCtrl: webview.WebviewController = new webview.WebviewController();
  private inlineAttached: boolean = false;
  private fullAttached: boolean = false;
  private lastInlineHtml: string = '';
  private lastFullHtml: string = '';
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
    this.syncInlineWebHtml();
  }

  private syncInlineWebHtml(): void {
    if (!this.inlineAttached || this.renderedView === null || this.renderedView.svg === null) {
      return;
    }
    const nextHtml = buildHarmonyDiagramHtml(
      this.renderedView.svg,
      this.store.palette.isDark,
      false
    );
    if (nextHtml === this.lastInlineHtml) {
      return;
    }
    this.lastInlineHtml = nextHtml;
    try {
      this.inlineWebCtrl.loadData(nextHtml, 'text/html', 'UTF-8');
    } catch (_err) {
      // Controller may detach during fast scroll recycling
    }
  }

  private syncFullWebHtml(): void {
    if (!this.fullAttached || this.renderedView === null || this.renderedView.svg === null) {
      return;
    }
    const nextHtml = buildHarmonyDiagramHtml(
      this.renderedView.svg,
      this.store.palette.isDark,
      true
    );
    if (nextHtml === this.lastFullHtml) {
      return;
    }
    this.lastFullHtml = nextHtml;
    try {
      this.fullWebCtrl.loadData(nextHtml, 'text/html', 'UTF-8');
    } catch (_err) {
      // Ignore transient lifecycle errors
    }
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
  get inlineCardHeightVp(): number {
    if (this.renderedView === null || this.renderedView.height <= 0 || this.renderedView.width <= 0) {
      return 180;
    }
    const ratio = this.renderedView.width / Math.max(1, this.renderedView.height);
    const est = Math.round(320 / Math.max(0.45, Math.min(2.8, ratio)));
    return Math.max(130, Math.min(400, est));
  }

  @Builder
  private renderFullSheetBuilder() {
    Column() {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text(this.headerLabel)
          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
          .layoutWeight(1)

        OrbitCopyIconButton({
          content: this.diagramSource,
          toastLabel: 'Copied diagram source',
          accessibilityLabel: 'Copy diagram source',
          buttonSize: OrbitTokens.HIT_COMPACT
        })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(16)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Close diagram')
        .onClick(() => {
          this.showFullSheet = false;
        })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_SM
      })
      .border({
        width: { bottom: OrbitTokens.HAIRLINE },
        color: this.store.palette.hairline
      })

      Web({ src: '', controller: this.fullWebCtrl })
        .javaScriptAccess(false)
        .zoomAccess(true)
        .backgroundColor(Color.Transparent)
        .layoutWeight(1)
        .width('100%')
        .onControllerAttached(() => {
          this.fullAttached = true;
          this.lastFullHtml = '';
          this.syncFullWebHtml();
        })
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }

  build() {
    Column() {
      Row({ space: OrbitTokens.SPACE_XS }) {
        SymbolGlyph($r('sys.symbol.code'))
          .fontSize(14)
          .fontColor([this.store.palette.iconSecondary])

        Text(this.headerLabel)
          .attributeModifier(orbitTextMono(this.store.palette.fgMuted, FontWeight.Medium))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        if (this.hasValidSvg) {
          Row({ space: 2 }) {
            Button('Diagram', { type: ButtonType.Normal, stateEffect: true })
              .height(26)
              .padding({ left: 8, right: 8 })
              .fontSize(11)
              .fontColor(
                this.viewMode === 'diagram'
                  ? this.store.palette.fgPrimary
                  : this.store.palette.fgMuted
              )
              .backgroundColor(
                this.viewMode === 'diagram' ? this.store.palette.elevated : Color.Transparent
              )
              .borderRadius(6)
              .onClick(() => {
                this.viewMode = 'diagram';
              })

            Button('Code', { type: ButtonType.Normal, stateEffect: true })
              .height(26)
              .padding({ left: 8, right: 8 })
              .fontSize(11)
              .fontColor(
                this.viewMode === 'code'
                  ? this.store.palette.fgPrimary
                  : this.store.palette.fgMuted
              )
              .backgroundColor(
                this.viewMode === 'code' ? this.store.palette.elevated : Color.Transparent
              )
              .borderRadius(6)
              .onClick(() => {
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
          .accessibilityText('Expand diagram')
          .onClick(() => {
            this.showFullSheet = true;
            this.store.triggerHaptic(6);
          })
        } else if (this.renderedView !== null && this.renderedView.error !== null) {
          Text('Code view')
            .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Medium))
            .padding({ left: 6, right: 6 })
        }

        OrbitCopyIconButton({
          content: this.diagramSource,
          toastLabel: 'Copied diagram source',
          accessibilityLabel: 'Copy diagram source',
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

      if (this.viewMode === 'diagram' && this.hasValidSvg) {
        Stack({ alignContent: Alignment.Center }) {
          Web({ src: '', controller: this.inlineWebCtrl })
            .javaScriptAccess(false)
            .zoomAccess(false)
            .backgroundColor(Color.Transparent)
            .width('100%')
            .height(this.inlineCardHeightVp)
            .onControllerAttached(() => {
              this.inlineAttached = true;
              this.lastInlineHtml = '';
              this.syncInlineWebHtml();
            })
        }
        .width('100%')
        .height(this.inlineCardHeightVp)
        .onClick(() => {
          this.showFullSheet = true;
        })
      } else if (this.isLoading && this.viewMode === 'diagram') {
        Row({ space: OrbitTokens.SPACE_SM }) {
          LoadingProgress()
            .width(18)
            .height(18)
            .color(this.store.palette.accentPrimary)
          Text(`Rendering ${this.kindBaseTitle}…`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }
        .width('100%')
        .height(96)
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
    .bindSheet($$this.showFullSheet, this.renderFullSheetBuilder, {
      detents: [SheetSize.LARGE],
      dragBar: true,
      backgroundColor: this.store.palette.canvas
    })
  }
}
