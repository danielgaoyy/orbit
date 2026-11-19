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
  orbitButtonSubhead,
  orbitInputMono,
  orbitSpanMonoFont,
  orbitTextMono
} from '../../theme/OrbitTokens';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

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

  private copiedTimerId: number = -1;
  private navPathStack: NavPathStack = new NavPathStack();
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  aboutToDisappear(): void {
    if (this.copiedTimerId !== -1) {
      clearTimeout(this.copiedTimerId);
      this.copiedTimerId = -1;
    }
  }

  @Computed
  get isCompactLandscape(): boolean {
    if (this.sheetHeightVp > 0 && this.sheetHeightVp < 430) {
      return true;
    }
    return this.store.windowHeightVp > 0 && this.store.windowHeightVp < 430;
  }

  @Computed
  get terminals(): TerminalSummaryDto[] {
    const detail = this.store.currentHostDetail;
    return detail !== null ? detail.terminals : [];
  }

  @Computed
  get activeTerminalId(): string {
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

  @Builder
  private renderTerminalMenus() {
    Row({ space: OrbitTokens.SPACE_XS }) {
      Button({ type: ButtonType.Circle, stateEffect: true }) {
        SymbolGlyph(
          this.copiedFeedback ? $r('sys.symbol.checkmark') : $r('sys.symbol.square_and_square')
        )
          .fontSize(17)
          .renderingStrategy(SymbolRenderingStrategy.SINGLE)
          .fontColor([
            this.copiedFeedback ? this.store.palette.statusDirect : this.store.palette.iconPrimary
          ])
      }
      .width(OrbitTokens.HIT_COMPACT)
      .height(OrbitTokens.HIT_COMPACT)
      .padding(0)
      .backgroundColor(
        this.copiedFeedback
          ? this.store.palette.statusDirectSoft
          : this.store.palette.subtleSurface
      )
      .accessibilityText('Copy terminal output')
      .onClick(() => {
        this.store.copyTerminalSnapshot();
        this.copiedFeedback = true;
        if (this.copiedTimerId !== -1) {
          clearTimeout(this.copiedTimerId);
        }
        this.copiedTimerId = setTimeout(() => {
          this.copiedFeedback = false;
          this.copiedTimerId = -1;
        }, 1600);
      })

      Button('Quote', { type: ButtonType.Capsule, stateEffect: true })
        .attributeModifier(
          orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
        )
        .height(OrbitTokens.HIT_COMPACT)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .accessibilityText('Quote terminal output into composer')
        .onClick(() => {
          dismissHarmonyKeyboard(this.getUIContext());
          this.store.quoteTerminalIntoComposer();
        })

      Button({ type: ButtonType.Circle, stateEffect: true }) {
        SymbolGlyph($r('sys.symbol.plus'))
          .fontSize(18)
          .renderingStrategy(SymbolRenderingStrategy.SINGLE)
          .fontColor([this.store.palette.iconPrimary])
      }
      .width(OrbitTokens.HIT_COMPACT)
      .height(OrbitTokens.HIT_COMPACT)
      .padding(0)
      .backgroundColor(this.store.palette.subtleSurface)
      .accessibilityText('Spawn new terminal session')
      .onClick(() => {
        this.store.startNewTerminal();
      })
    }
    .padding({ right: OrbitTokens.SPACE_MD })
    .alignItems(VerticalAlign.Center)
  }

  build() {
    Navigation(this.navPathStack) {
      Column({ space: this.isCompactLandscape ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_SM }) {
        if (this.terminals.length > 0) {
          Scroll() {
            Row({ space: this.isCompactLandscape ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_SM }) {
              ForEach(
                this.terminals,
                (term: TerminalSummaryDto, tIdx: number) => {
                  Row({ space: 2 }) {
                    Button({ type: ButtonType.Normal, stateEffect: true }) {
                      Text(term.title.length > 0 ? term.title : `Shell ${tIdx + 1}`)
                        .attributeModifier(
                          orbitTextMono(
                            this.activeTerminalId === term.terminal_id
                              ? this.store.palette.accentPrimary
                              : this.store.palette.fgSecondary,
                            FontWeight.Medium
                          )
                        )
                    }
                    .height(OrbitTokens.HIT_COMPACT)
                    .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_XS })
                    .backgroundColor(Color.Transparent)
                    .onClick(() => {
                      this.store.attachTerminal(term.terminal_id, 1);
                    })

                    Button({ type: ButtonType.Circle, stateEffect: true }) {
                      SymbolGlyph($r('sys.symbol.xmark'))
                        .fontSize(12)
                        .renderingStrategy(SymbolRenderingStrategy.SINGLE)
                        .fontColor([this.store.palette.iconSecondary])
                    }
                    .width(OrbitTokens.HIT_COMPACT)
                    .height(OrbitTokens.HIT_COMPACT)
                    .padding(0)
                    .backgroundColor(Color.Transparent)
                    .accessibilityText(
                      `Kill terminal ${term.title.length > 0 ? term.title : `Shell ${tIdx + 1}`}`
                    )
                    .onClick(() => {
                      this.store.killTerminal(term.terminal_id);
                    })
                  }
                  .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                  .backgroundColor(
                    this.activeTerminalId === term.terminal_id
                      ? this.store.palette.accentPrimarySoft
                      : this.store.palette.listCard
                  )
                  .borderRadius(OrbitTokens.RADIUS_MD)
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
                .borderRadius(OrbitTokens.RADIUS_MD)
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
                          .fontColor(this.store.palette.terminalFg)
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
                    .copyOption(CopyOptions.LocalDevice)
                    .width('100%')
                  }
                })
                .key((r: TerminalRowDto) => r.row_index.toString())
                .virtualScroll()
            }
            .width('100%')
            .height('100%')
            .scrollBar(BarState.Off)
            .onScrollStart(() => {
              dismissHarmonyKeyboard(this.getUIContext());
            })
          }
        }
        .width('100%')
        .layoutWeight(1)
        .padding(this.isCompactLandscape ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_SM)
        .backgroundColor(this.store.palette.terminalBg)
        .borderRadius(OrbitTokens.RADIUS_LG)
        .border({
          width: OrbitTokens.HAIRLINE,
          color: this.store.palette.hairline
        })
        .onAreaChange((_oldArea: Area, newArea: Area) => {
          const w = Number(newArea.width);
          const h = Number(newArea.height);
          if (w > 120 && h > 64) {
            const pad = this.isCompactLandscape ? 8 : 16;
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
                    orbitButtonMeta(
                      this.isKeyActiveModifier(keyItem)
                        ? this.store.palette.onAccent
                        : this.store.palette.fgPrimary,
                      this.isKeyActiveModifier(keyItem)
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.listCard
                    )
                  )
                  .fontFamily(OrbitTokens.FAMILY_MONO)
                  .height(OrbitTokens.HIT_COMPACT)
                  .constraintSize({ minWidth: OrbitTokens.HIT_COMPACT })
                  .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                  .borderRadius(OrbitTokens.RADIUS_SM)
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
                this.store.palette.listCard
              )
            )
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .layoutWeight(1)
            .onChange((val: string) => {
              this.shellInput = val;
            })
            .onSubmit(() => {
              this.store.sendTerminalKeys(`${this.shellInput}\r`);
              this.shellInput = '';
            })

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.paperplane_fill'))
              .fontSize(18)
              .renderingStrategy(SymbolRenderingStrategy.SINGLE)
              .fontColor([this.store.palette.onAccent])
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .padding(0)
          .backgroundColor(this.store.palette.accentPrimaryFill)
          .accessibilityText('Send terminal command')
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
        left: this.isCompactLandscape ? OrbitTokens.SPACE_MD : OrbitTokens.SPACE_LG,
        right: this.isCompactLandscape ? OrbitTokens.SPACE_MD : OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_XS,
        bottom: this.store.keyboardHeightVp > 0
          ? (this.isCompactLandscape ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_SM)
          : Math.max(
            this.isCompactLandscape ? OrbitTokens.SPACE_XS : OrbitTokens.SPACE_MD,
            this.store.safeBottomVp
          )
      })
      .onAreaChange((_oldArea: Area, newArea: Area) => {
        const h = Number(newArea.height);
        if (h > 0) {
          this.sheetHeightVp = h;
        }
      })
    }
    .title({
      main: 'PTY Terminal',
      sub: `${basenameOfPath(this.store.activeCwd)} · ${this.store.terminalCols}×${this.store.terminalGridRows}`
    })
    .titleMode(NavigationTitleMode.Mini)
    .hideBackButton(true)
    .menus(this.renderTerminalMenus())
    .mode(NavigationMode.Stack)
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
  }
}
