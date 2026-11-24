import { ExportedTranscriptDto } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { OrbitSheetHeader } from '../common/OrbitCommonControls';

@ComponentV2
export struct ExportTranscriptSheet {
  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  @Local private selectedFormat: string = 'markdown';
  @Local private includeThinking: boolean = false;
  @Local private includeToolCalls: boolean = true;
  @Local private includeTimestamps: boolean = true;
  @Local private previewResult: ExportedTranscriptDto | null = null;

  aboutToAppear(): void {
    this.regeneratePreview();
  }

  private regeneratePreview(): void {
    this.previewResult = this.store.exportCurrentSessionTranscript(
      this.selectedFormat,
      this.includeThinking,
      this.includeToolCalls,
      this.includeTimestamps
    );
  }

  private formatSizeLabel(bytes: number): string {
    if (bytes < 1024) {
      return `${bytes} B`;
    }
    return `${(bytes / 1024).toFixed(1)} KB`;
  }

  build() {
    Column({ space: 0 }) {
      OrbitSheetHeader({
        title: $r('app.string.export_title'),
        subtitle: this.previewResult !== null
          ? `${this.previewResult.suggested_filename} · ${this.previewResult.turn_count} turns · ${this.formatSizeLabel(this.previewResult.byte_len)}`
          : '',
        onClose: () => {
          this.store.showExportTranscriptSheet = false;
        }
      })

      Column({ space: OrbitTokens.SPACE_MD }) {
        // Format selector row
        Row({ space: OrbitTokens.SPACE_XS }) {
          Button($r('app.string.export_format_md'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(
                this.selectedFormat === 'markdown'
                  ? this.store.palette.onAccent
                  : this.store.palette.fgPrimary,
                this.selectedFormat === 'markdown'
                  ? this.store.palette.accentPrimaryFill
                  : this.store.palette.listCard
              )
            )
            .height(OrbitTokens.HIT_COMPACT)
            .layoutWeight(1)
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .onClick(() => {
              this.selectedFormat = 'markdown';
              this.regeneratePreview();
            })

          Button($r('app.string.export_format_json'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(
                this.selectedFormat === 'json'
                  ? this.store.palette.onAccent
                  : this.store.palette.fgPrimary,
                this.selectedFormat === 'json'
                  ? this.store.palette.accentPrimaryFill
                  : this.store.palette.listCard
              )
            )
            .height(OrbitTokens.HIT_COMPACT)
            .layoutWeight(1)
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .onClick(() => {
              this.selectedFormat = 'json';
              this.regeneratePreview();
            })

          Button($r('app.string.export_format_html'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(
                this.selectedFormat === 'html'
                  ? this.store.palette.onAccent
                  : this.store.palette.fgPrimary,
                this.selectedFormat === 'html'
                  ? this.store.palette.accentPrimaryFill
                  : this.store.palette.listCard
              )
            )
            .height(OrbitTokens.HIT_COMPACT)
            .layoutWeight(1)
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .onClick(() => {
              this.selectedFormat = 'html';
              this.regeneratePreview();
            })
        }
        .width('100%')

        // Option toggles
        Column({ space: OrbitTokens.SPACE_SM }) {
          Row() {
            Text($r('app.string.export_include_thinking'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
              .layoutWeight(1)
            Toggle({ type: ToggleType.Switch, isOn: this.includeThinking })
              .selectedColor(this.store.palette.accentPrimaryFill)
              .onChange((val: boolean) => {
                this.includeThinking = val;
                this.regeneratePreview();
              })
          }
          .width('100%')

          Divider().strokeWidth(OrbitTokens.HAIRLINE).color(this.store.palette.hairline)

          Row() {
            Text($r('app.string.export_include_tools'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
              .layoutWeight(1)
            Toggle({ type: ToggleType.Switch, isOn: this.includeToolCalls })
              .selectedColor(this.store.palette.accentPrimaryFill)
              .onChange((val: boolean) => {
                this.includeToolCalls = val;
                this.regeneratePreview();
              })
          }
          .width('100%')

          Divider().strokeWidth(OrbitTokens.HAIRLINE).color(this.store.palette.hairline)

          Row() {
            Text($r('app.string.export_include_timestamps'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary))
              .layoutWeight(1)
            Toggle({ type: ToggleType.Switch, isOn: this.includeTimestamps })
              .selectedColor(this.store.palette.accentPrimaryFill)
              .onChange((val: boolean) => {
                this.includeTimestamps = val;
                this.regeneratePreview();
              })
          }
          .width('100%')
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_MD)
        .borderRadius(OrbitTokens.RADIUS_MD)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))

        if (this.previewResult !== null) {
          Scroll() {
            Text(
              this.previewResult.content.length > 4000
                ? `${this.previewResult.content.substring(0, 4000)}\n…`
                : this.previewResult.content
            )
              .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
              .copyOption(CopyOptions.LocalDevice)
              .width('100%')
              .padding(OrbitTokens.SPACE_SM)
          }
          .width('100%')
          .layoutWeight(1)
          .backgroundColor(this.store.palette.code)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
          .scrollBar(BarState.Auto)
        } else {
          Column() {
            Blank()
            Text('Open a conversation to export its transcript.')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            Blank()
          }
          .width('100%')
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Center)
        }

        Button($r('app.string.export_copy_action'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .width('100%')
          .height(OrbitTokens.HIT_MIN)
          .enabled(this.previewResult !== null && this.previewResult.content.length > 0)
          .onClick(() => {
            if (this.previewResult !== null) {
              this.store.copyToClipboard(
                this.previewResult.content,
                `Copied ${this.previewResult.suggested_filename}`
              );
              this.store.showExportTranscriptSheet = false;
            }
          })
      }
      .width('100%')
      .layoutWeight(1)
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
      })
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
