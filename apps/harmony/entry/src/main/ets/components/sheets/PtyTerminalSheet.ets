import {
  DevKeyItem,
  TerminalRowDto,
  TerminalSpanDto,
  TerminalSummaryDto,
  basenameOfPath
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonMono,
  orbitButtonSubhead,
  orbitInputMono,
  orbitSpanMonoFont,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';

const DEV_KEYBAR_ITEMS: DevKeyItem[] = [
  { id: 'esc', label: 'ESC', sequence: '\x1b', isModifier: false, a11yLabel: 'Escape key' },
  { id: 'tab', label: 'TAB', sequence: '\t', isModifier: false, a11yLabel: 'Tab key' },
  { id: 'ctrl', label: 'CTRL', sequence: '', isModifier: true, a11yLabel: 'Sticky Control modifier' },
  { id: 'alt', label: 'ALT', sequence: '', isModifier: true, a11yLabel: 'Sticky Alt modifier' },
  { id: 'ctrl_c', label: '^C', sequence: '\x03', isModifier: false, a11yLabel: 'Interrupt Control C' },
  { id: 'ctrl_d', label: '^D', sequence: '\x04', isModifier: false, a11yLabel: 'End of input Control D' },
  { id: 'ctrl_l', label: '^L', sequence: '\x0c', isModifier: false, a11yLabel: 'Clear screen Control L' },
  { id: 'up', label: '↑', sequence: '\x1b[A', isModifier: false, a11yLabel: 'Up arrow' },
  { id: 'down', label: '↓', sequence: '\x1b[B', isModifier: false, a11yLabel: 'Down arrow' },
  { id: 'left', label: '←', sequence: '\x1b[D', isModifier: false, a11yLabel: 'Left arrow' },
  { id: 'right', label: '→', sequence: '\x1b[C', isModifier: false, a11yLabel: 'Right arrow' },
  { id: 'pipe', label: '|', sequence: '|', isModifier: false, a11yLabel: 'Pipe character' },
  { id: 'slash', label: '/', sequence: '/', isModifier: false, a11yLabel: 'Slash character' },
  { id: 'dash', label: '-', sequence: '-', isModifier: false, a11yLabel: 'Dash character' },
  { id: 'tilde', label: '~', sequence: '~', isModifier: false, a11yLabel: 'Tilde character' }
];

@ComponentV2
export struct PtyTerminalSheet {
  @Local shellInput: string = '';
  @Local copiedFeedback: boolean = false;
  @Local sheetHeightVp: number = 0;
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private isCompactLandscape(): boolean {
    if (this.sheetHeightVp > 0 && this.sheetHeightVp < 430) {
      return true;
    }
    return this.store.windowHeightVp > 0 && this.store.windowHeightVp < 430;
  }

  private terminals(): TerminalSummaryDto[] {
    const detail = this.store.currentHostDetail;
    return detail !== null ? detail.terminals : [];
  }

  private activeTerminalId(): string {
    const detail = this.store.currentHostDetail;
    return detail !== null && detail.active_terminal_id !== null ? detail.active_terminal_id : '';
  }

  private isKeyActiveModifier(item: DevKeyItem): boolean {
    if (item.id === 'ctrl') {
      return this.store.ctrlSticky;
    }
    if (item.id === 'alt') {
      return this.store.altSticky;
    }
    return false;
  }

  private handleDevKeyTap(item: DevKeyItem): void {
    if (item.id === 'ctrl') {
      this.store.ctrlSticky = !this.store.ctrlSticky;
      this.store.triggerHaptic(8);
      return;
    }
    if (item.id === 'alt') {
      this.store.altSticky = !this.store.altSticky;
      this.store.triggerHaptic(8);
      return;
    }
    this.store.sendTerminalKeys(item.sequence);
    this.store.triggerHaptic(6);
  }

  build() {
    Column({ space: this.isCompactLandscape() ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_XS }) {
        if (this.isCompactLandscape()) {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text('PTY')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            Text(
              `${basenameOfPath(this.store.activeCwd)} · ${this.store.terminalCols}×${this.store.terminalGridRows}`
            )
              .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .layoutWeight(1)
          }
          .alignItems(VerticalAlign.Center)
          .layoutWeight(1)
        } else {
          Column({ space: 2 }) {
            Text('PTY Terminal')
              .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
            Text(
              `${basenameOfPath(this.store.activeCwd)} · ${this.store.terminalCols}×${this.store.terminalGridRows}`
            )
              .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
          .alignItems(HorizontalAlign.Start)
          .layoutWeight(1)
        }

        Button(this.copiedFeedback ? 'Copied' : 'Copy')
          .attributeModifier(
            orbitButtonMeta(
              this.copiedFeedback ? this.store.palette.statusDirect : this.store.palette.fgPrimary,
              this.copiedFeedback
                ? this.store.palette.statusDirectSoft
                : this.store.palette.subtleSurface
            )
          )
          .height(OrbitTokens.HIT_MIN)
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .accessibilityText('Copy terminal output')
          .onClick(() => {
            this.store.copyTerminalSnapshot();
            this.copiedFeedback = true;
            setTimeout(() => {
              this.copiedFeedback = false;
            }, 1600);
          })

        Button('Quote')
          .attributeModifier(
            orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
          )
          .height(OrbitTokens.HIT_MIN)
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .accessibilityText('Quote terminal output into composer')
          .onClick(() => {
            this.store.quoteTerminalIntoComposer();
          })

        Button('+ New')
          .attributeModifier(
            orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
          )
          .height(OrbitTokens.HIT_MIN)
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .accessibilityText('Spawn new terminal session')
          .onClick(() => {
            this.store.startNewTerminal();
          })

        Button('Done')
          .attributeModifier(orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent))
          .height(OrbitTokens.HIT_MIN)
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_XS })
          .onClick(() => {
            this.store.showPtySheet = false;
          })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      if (this.terminals().length > 0) {
        Scroll() {
          Row({ space: this.isCompactLandscape() ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_SM }) {
            ForEach(
              this.terminals(),
              (term: TerminalSummaryDto, tIdx: number) => {
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Row() {
                    Text(term.title.length > 0 ? term.title : `Shell ${tIdx + 1}`)
                      .attributeModifier(
                        orbitTextMono(
                          this.activeTerminalId() === term.terminal_id
                            ? this.store.palette.accentPrimary
                            : this.store.palette.fgSecondary,
                          FontWeight.Medium
                        )
                      )
                  }
                  .height(this.isCompactLandscape() ? 36 : OrbitTokens.HIT_MIN)
                  .padding({
                    left: this.isCompactLandscape() ? OrbitTokens.SPACE_SM : OrbitTokens.SPACE_MD,
                    right: OrbitTokens.SPACE_XS
                  })
                  .alignItems(VerticalAlign.Center)
                  .onClick(() => {
                    this.store.attachTerminal(term.terminal_id, 1);
                  })

                  Stack({ alignContent: Alignment.Center }) {
                    Text('×')
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
                  }
                  .width(OrbitTokens.HIT_MIN)
                  .height(this.isCompactLandscape() ? 36 : OrbitTokens.HIT_MIN)
                  .accessibilityText(
                    `Kill terminal ${term.title.length > 0 ? term.title : `Shell ${tIdx + 1}`}`
                  )
                  .onClick(() => {
                    this.store.killTerminal(term.terminal_id);
                  })
                }
                .constraintSize({ minHeight: this.isCompactLandscape() ? 36 : OrbitTokens.HIT_MIN })
                .backgroundColor(
                  this.activeTerminalId() === term.terminal_id
                    ? this.store.palette.accentPrimarySoft
                    : this.store.palette.subtleSurface
                )
                .borderRadius(OrbitTokens.PILL_RADIUS)
                .alignItems(VerticalAlign.Center)
              },
              (term: TerminalSummaryDto) => term.terminal_id
            )
          }
        }
        .scrollable(ScrollDirection.Horizontal)
        .scrollBar(BarState.Off)
        .width('100%')
      }

      Column() {
        if (this.store.terminalRows.length === 0) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Blank()
            Text('orbit shim detached PTY ready')
              .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            Button('Spawn Shell Session')
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.CARD_RADIUS)
              .onClick(() => {
                this.store.startNewTerminal();
              })
            Blank()
          }
          .width('100%')
          .height('100%')
          .alignItems(HorizontalAlign.Center)
        } else {
          List({ space: 0 }) {
            Repeat<TerminalRowDto>(this.store.terminalRows)
              .each((ri: RepeatItem<TerminalRowDto>) => {
                ListItem() {
                  Text() {
                    if (ri.item.spans.length === 0) {
                      Span(' ')
                        .font(orbitSpanMonoFont())
                        .fontColor('#EFEFF2')
                    } else {
                      ForEach(
                        ri.item.spans,
                        (sp: TerminalSpanDto) => {
                          if (sp.bg_hex !== null && sp.bg_hex.length > 0) {
                            Span(sp.text)
                              .font(
                                orbitSpanMonoFont(
                                  sp.bold ? FontWeight.Bold : FontWeight.Normal
                                )
                              )
                              .fontColor(sp.fg_hex)
                              .textBackgroundStyle({
                                color: sp.bg_hex,
                                radius: 0
                              })
                              .decoration({
                                type: sp.underline
                                  ? TextDecorationType.Underline
                                  : TextDecorationType.None,
                                color: sp.fg_hex
                              })
                          } else {
                            Span(sp.text)
                              .font(
                                orbitSpanMonoFont(
                                  sp.bold ? FontWeight.Bold : FontWeight.Normal
                                )
                              )
                              .fontColor(sp.fg_hex)
                              .decoration({
                                type: sp.underline
                                  ? TextDecorationType.Underline
                                  : TextDecorationType.None,
                                color: sp.fg_hex
                              })
                          }
                        },
                        (_sp: TerminalSpanDto, sIdx: number) => `${ri.item.row_index}_${sIdx}`
                      )
                    }
                  }
                  .lineHeight(OrbitTokens.LH_META)
                  .width('100%')
                }
              })
              .key((r: TerminalRowDto) => r.row_index.toString())
              .virtualScroll()
          }
          .width('100%')
          .height('100%')
          .scrollBar(BarState.Off)
        }
      }
      .width('100%')
      .layoutWeight(1)
      .padding(this.isCompactLandscape() ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_SM)
      .backgroundColor('#0A0B0E')
      .borderRadius(OrbitTokens.CARD_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
      .onAreaChange((_oldArea: Area, newArea: Area) => {
        const w = Number(newArea.width);
        const h = Number(newArea.height);
        if (w > 120 && h > 64) {
          const pad = this.isCompactLandscape() ? 8 : 16;
          const cols = Math.floor((w - pad) / 7.4);
          const rows = Math.floor((h - pad) / 16.0);
          this.store.resizeTerminalIfChanged(cols, rows);
        }
      })

      Scroll() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          ForEach(
            DEV_KEYBAR_ITEMS,
            (keyItem: DevKeyItem) => {
              Button(keyItem.label)
                .attributeModifier(
                  orbitButtonMono(
                    this.isKeyActiveModifier(keyItem)
                      ? this.store.palette.onAccent
                      : this.store.palette.fgPrimary,
                    this.isKeyActiveModifier(keyItem)
                      ? this.store.palette.accentPrimaryFill
                      : this.store.palette.subtleSurface
                  )
                )
                .height(OrbitTokens.HIT_MIN)
                .constraintSize({ minWidth: OrbitTokens.HIT_MIN })
                .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                .borderRadius(OrbitTokens.PILL_RADIUS)
                .accessibilityText(keyItem.a11yLabel)
                .onClick(() => {
                  this.handleDevKeyTap(keyItem);
                })
            },
            (keyItem: DevKeyItem) => keyItem.id
          )
        }
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')

      Row({ space: OrbitTokens.SPACE_SM }) {
        TextInput({
          placeholder: 'Send command or raw keys…',
          text: this.shellInput
        })
          .attributeModifier(
            orbitInputMono(
              this.store.palette.fgPrimary,
              this.store.palette.fgMuted,
              this.store.palette.code
            )
          )
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.CARD_RADIUS)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline
          })
          .layoutWeight(1)
          .onChange((val: string) => {
            this.shellInput = val;
          })
          .onSubmit(() => {
            this.store.sendTerminalKeys(`${this.shellInput}\r`);
            this.shellInput = '';
          })

        Button('Send ↵')
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.CARD_RADIUS)
          .onClick(() => {
            this.store.sendTerminalKeys(`${this.shellInput}\r`);
            this.shellInput = '';
          })
      }
      .width('100%')
    }
    .width('100%')
    .height('100%')
    .padding({
      left: this.isCompactLandscape() ? OrbitTokens.SPACE_MD : OrbitTokens.SPACE_LG,
      right: this.isCompactLandscape() ? OrbitTokens.SPACE_MD : OrbitTokens.SPACE_LG,
      top: this.isCompactLandscape() ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_LG,
      bottom: Math.max(
        this.isCompactLandscape() ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_MD,
        this.store.keyboardHeightVp > 0 ? this.store.keyboardHeightVp : this.store.safeBottomVp
      )
    })
    .backgroundColor(this.store.palette.canvas)
    .onAreaChange((_oldArea: Area, newArea: Area) => {
      const h = Number(newArea.height);
      if (h > 0) {
        this.sheetHeightVp = h;
      }
    })
  }
}
