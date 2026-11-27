import { webview } from '@kit.ArkWeb';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitModalRoute } from '../../model/stores/OrbitShellStore';
import {
  OrbitTokens,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitCopyIconButton } from '../common/OrbitCommonControls';
import { ContextPickerSheet } from '../composer/ContextPickerSheet';
import { ArchivedSessionsSheet } from '../sheets/ArchivedSessionsSheet';
import { DirectoryBrowserSheet } from '../sheets/DirectoryBrowserSheet';
import { ExportTranscriptSheet } from '../sheets/ExportTranscriptSheet';
import { GlobalSearchSheet } from '../sheets/GlobalSearchSheet';
import { HostSettingsSheet } from '../sheets/HostSettingsSheet';
import { PersonalRouterMeshSheet } from '../sheets/PersonalRouterMeshSheet';

function buildDiagramViewerHtml(svg: string, isDark: boolean): string {
  const fgHex = isDark ? '#F1F2F7' : '#14161D';
  return [
    '<!doctype html>',
    '<html><head><meta charset="utf-8">',
    '<meta name="viewport" content="width=device-width, initial-scale=1.0, maximum-scale=5.0, user-scalable=yes">',
    '<style>',
    `html, body { margin: 0; padding: 0; width: 100%; height: 100%; background: transparent; color: ${fgHex}; overflow: auto; font-family: sans-serif; }`,
    '.wrap { width: 100%; min-height: 100%; display: flex; align-items: center; justify-content: center; box-sizing: border-box; padding: 16px; }',
    'svg { display: block; max-width: 100%; max-height: 100%; width: auto; height: auto; }',
    '</style></head>',
    `<body><div class="wrap">${svg}</div></body></html>`
  ].join('\n');
}

@ComponentV2
struct OrbitImageViewerContent {
  @Param imageUri: string = '';
  @Param imageLabel: string = '';
  @Event onClose: () => void = () => {};

  @Local private zoomScale: number = 1;
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  build() {
    Stack({ alignContent: Alignment.TopEnd }) {
      // Full-bleed lightbox scrim extending cleanly behind system bars and camera cutouts
      Column()
        .width('100%')
        .height('100%')
        .backgroundColor(this.store.palette.scrim)
        .expandSafeArea(
          [SafeAreaType.SYSTEM, SafeAreaType.CUTOUT],
          [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM, SafeAreaEdge.START, SafeAreaEdge.END]
        )
        .onClick(() => {
          this.onClose();
        })

      Column({ space: OrbitTokens.SPACE_MD }) {
        if (this.imageUri.length > 0) {
          Image(this.imageUri)
            .objectFit(ImageFit.Contain)
            .width('100%')
            .layoutWeight(1)
            .scale({ x: this.zoomScale, y: this.zoomScale })
            .onClick(() => {
              this.zoomScale = this.zoomScale > 1 ? 1 : 2;
            })
        }
        if (this.imageLabel.length > 0) {
          Text(this.imageLabel)
            .attributeModifier(orbitTextMeta(this.store.palette.onAccent))
            .maxLines(2)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
      }
      .width('100%')
      .height('100%')
      .padding({
        left: OrbitTokens.SPACE_LG + this.store.safeLeftVp,
        right: OrbitTokens.SPACE_LG + this.store.safeRightVp,
        top: OrbitTokens.SPACE_XL + this.store.safeTopVp,
        bottom: OrbitTokens.SPACE_XL + this.store.safeBottomVp
      })
      .justifyContent(FlexAlign.Center)
      .alignItems(HorizontalAlign.Center)

      Button({ type: ButtonType.Circle, stateEffect: true }) {
        SymbolGlyph($r('sys.symbol.xmark'))
          .fontSize(16)
          .fontColor([this.store.palette.iconPrimary])
      }
      .width(OrbitTokens.HIT_COMPACT)
      .height(OrbitTokens.HIT_COMPACT)
      .padding(0)
      .margin({
        top: OrbitTokens.SPACE_MD + this.store.safeTopVp,
        right: OrbitTokens.SPACE_LG + this.store.safeRightVp
      })
      .backgroundColor(this.store.palette.overlay)
      .accessibilityText('Close image preview')
      .onClick(() => {
        this.onClose();
      })
    }
    .width('100%')
    .height('100%')
    .expandSafeArea(
      [SafeAreaType.SYSTEM, SafeAreaType.CUTOUT],
      [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM, SafeAreaEdge.START, SafeAreaEdge.END]
    )
  }
}

@ComponentV2
struct OrbitDiagramViewerContent {
  @Param title: string = 'Diagram';
  @Param diagramSource: string = '';
  @Param diagramSvg: string = '';
  @Event onClose: () => void = () => {};

  private webCtrl: webview.WebviewController = new webview.WebviewController();
  private attached: boolean = false;
  private lastHtml: string = '';
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  private syncWebHtml(): void {
    if (!this.attached || this.diagramSvg.length === 0) {
      return;
    }
    const html = buildDiagramViewerHtml(this.diagramSvg, this.store.palette.isDark);
    if (html === this.lastHtml) {
      return;
    }
    this.lastHtml = html;
    try {
      this.webCtrl.loadData(html, 'text/html', 'UTF-8');
    } catch (_err) {
      // Ignore transient lifecycle errors
    }
  }

  build() {
    Column({ space: 0 }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text(this.title.length > 0 ? this.title : 'Diagram')
          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        if (this.diagramSource.length > 0) {
          OrbitCopyIconButton({
            content: this.diagramSource,
            toastLabel: 'Copied diagram source',
            accessibilityLabel: 'Copy diagram source',
            buttonSize: OrbitTokens.HIT_COMPACT
          })
        }

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(16)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Close diagram viewer')
        .onClick(() => {
          this.onClose();
        })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_LG + this.store.safeLeftVp,
        right: OrbitTokens.SPACE_MD + this.store.safeRightVp,
        top: OrbitTokens.SPACE_SM + this.store.safeTopVp,
        bottom: OrbitTokens.SPACE_SM
      })
      .border({
        width: { bottom: OrbitTokens.HAIRLINE },
        color: this.store.palette.hairline
      })

      if (this.diagramSvg.length > 0) {
        Web({ src: '', controller: this.webCtrl })
          .javaScriptAccess(false)
          .zoomAccess(true)
          .backgroundColor(Color.Transparent)
          .layoutWeight(1)
          .width('100%')
          .onControllerAttached(() => {
            this.attached = true;
            this.lastHtml = '';
            this.syncWebHtml();
          })
      } else {
        Scroll() {
          Text(this.diagramSource)
            .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
            .copyOption(CopyOptions.LocalDevice)
            .width('100%')
            .padding(OrbitTokens.SPACE_LG)
        }
        .width('100%')
        .layoutWeight(1)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
    .expandSafeArea(
      [SafeAreaType.SYSTEM, SafeAreaType.CUTOUT],
      [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM, SafeAreaEdge.START, SafeAreaEdge.END]
    )
  }
}

/**
 * Single Modal Content Router rendered inside AdaptiveWorkspace's sole `bindSheet`.
 * Handles all 8 `OrbitModalRouteKind` destinations without stacked sheet anchors.
 */
@ComponentV2
export struct OrbitModalContentRouter {
  @Param route: OrbitModalRoute | null = null;
  @Param presentationId: number = 0;
  @Event onClose: (presentationId: number) => void = (_id: number) => {};
  @Event onNativeAppear: (presentationId: number) => void = (_id: number) => {};
  @Event onNativeDisappear: (presentationId: number) => void = (_id: number) => {};

  private capturedPresentationId: number = 0;
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    this.capturedPresentationId = this.presentationId;
    if (this.capturedPresentationId > 0) {
      this.onNativeAppear(this.capturedPresentationId);
    }
  }

  aboutToDisappear(): void {
    const id = this.capturedPresentationId > 0 ? this.capturedPresentationId : this.presentationId;
    if (id > 0) {
      this.onNativeDisappear(id);
    }
  }

  private effectivePresentationId(): number {
    return this.capturedPresentationId > 0 ? this.capturedPresentationId : this.presentationId;
  }

  build() {
    Column() {
      if (this.route === null) {
        Column() {
          Text('')
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
        }
        .width('100%')
        .height('100%')
      } else if (this.route.kind === 'host_settings') {
        if (this.route.initialPath === 'router_mesh') {
          PersonalRouterMeshSheet()
        } else {
          HostSettingsSheet()
        }
      } else if (this.route.kind === 'global_search') {
        GlobalSearchSheet()
      } else if (this.route.kind === 'archived') {
        ArchivedSessionsSheet()
      } else if (this.route.kind === 'export_transcript') {
        ExportTranscriptSheet()
      } else if (this.route.kind === 'directory_browser') {
        DirectoryBrowserSheet()
      } else if (this.route.kind === 'work_picker') {
        ContextPickerSheet()
      } else if (this.route.kind === 'image_viewer') {
        OrbitImageViewerContent({
          imageUri: this.route.imageUri ?? '',
          imageLabel: this.route.imageLabel ?? '',
          onClose: () => {
            const id = this.presentationId > 0 ? this.presentationId : this.effectivePresentationId();
            this.onClose(id);
          }
        })
      } else {
        OrbitDiagramViewerContent({
          title: this.route.title ?? 'Diagram',
          diagramSource: this.route.diagramSource ?? '',
          diagramSvg: this.route.diagramSvg ?? '',
          onClose: () => {
            const id = this.presentationId > 0 ? this.presentationId : this.effectivePresentationId();
            this.onClose(id);
          }
        })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
    .expandSafeArea(
      [SafeAreaType.SYSTEM, SafeAreaType.CUTOUT],
      [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM, SafeAreaEdge.START, SafeAreaEdge.END]
    )
  }
}
