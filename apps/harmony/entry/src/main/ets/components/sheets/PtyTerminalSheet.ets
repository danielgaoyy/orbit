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

interface PtyTerminalRenderSpan {
  renderKey: string;
  text: string;
  fgColor: ResourceColor;
  bgColor: ResourceColor | null;
  bold: boolean;
  underline: boolean;
}

interface PtyTerminalRenderRow {
  rowIndex: number;
  spans: PtyTerminalRenderSpan[];
}

interface PtyTerminalCell {
  text: string;
  width: number;
}

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

  @Computed
  get renderedRows(): PtyTerminalRenderRow[] {
    const screen = this.store.terminalScreen;
    const rows = this.store.terminalRows;
    const cursorVisible = screen !== null && screen.cursor_visible !== false && screen.cols > 0;
    const cursorRow = cursorVisible && screen !== null ? screen.cursor_row : -1;
    const cursorCol = cursorVisible && screen !== null && screen.cursor_col >= 0
      ? Math.min(screen.cursor_col, screen.cols - 1)
      : -1;
    const defaultFg = this.store.palette.terminalFg;
    const defaultBg = this.store.palette.terminalBg;
    const themeTag = this.store.palette.isDark ? 'd' : 'l';
    const out: PtyTerminalRenderRow[] = [];
    for (let i = 0; i < rows.length; i++) {
      const row = rows[i];
      out.push(
        this.buildRenderedRow(
          row,
          row.row_index === cursorRow ? cursorCol : -1,
          defaultFg,
          defaultBg,
          themeTag
        )
      );
    }
    return out;
  }

  private codePointCellWidth(cp: number): number {
    if (
      cp < 0x20 ||
      (cp >= 0x7F && cp < 0xA0) ||
      (cp >= 0x0300 && cp <= 0x036F) ||
      (cp >= 0x1160 && cp <= 0x11FF) ||
      (cp >= 0x1AB0 && cp <= 0x1AFF) ||
      (cp >= 0x1DC0 && cp <= 0x1DFF) ||
      (cp >= 0x200B && cp <= 0x200F) ||
      (cp >= 0x202A && cp <= 0x202E) ||
      (cp >= 0x2060 && cp <= 0x206F) ||
      (cp >= 0x20D0 && cp <= 0x20FF) ||
      (cp >= 0xD7B0 && cp <= 0xD7FF) ||
      (cp >= 0xFE00 && cp <= 0xFE0F) ||
      (cp >= 0xFE20 && cp <= 0xFE2F) ||
      cp === 0xFEFF ||
      (cp >= 0xE0100 && cp <= 0xE01EF)
    ) {
      return 0;
    }
    if (
      (cp >= 0x1100 && cp <= 0x115F) ||
      cp === 0x2329 ||
      cp === 0x232A ||
      (cp >= 0x2E80 && cp <= 0x303E) ||
      (cp >= 0x3040 && cp <= 0xA4CF) ||
      (cp >= 0xAC00 && cp <= 0xD7A3) ||
      (cp >= 0xF900 && cp <= 0xFAFF) ||
      (cp >= 0xFE10 && cp <= 0xFE19) ||
      (cp >= 0xFE30 && cp <= 0xFE6F) ||
      (cp >= 0xFF00 && cp <= 0xFF60) ||
      (cp >= 0xFFE0 && cp <= 0xFFE6) ||
      (cp >= 0x1F300 && cp <= 0x1FAFF) ||
      (cp >= 0x20000 && cp <= 0x3FFFD)
    ) {
      return 2;
    }
    return 1;
  }

  private splitTerminalCells(text: string): PtyTerminalCell[] {
    const out: PtyTerminalCell[] = [];
    let i = 0;
    while (i < text.length) {
      const cp = text.codePointAt(i);
      const step = cp !== undefined && cp > 0xFFFF ? 2 : 1;
      const chunk = text.substring(i, i + step);
      const width = cp !== undefined ? this.codePointCellWidth(cp) : 1;
      if (width === 0 && out.length > 0) {
        out[out.length - 1].text += chunk;
      } else {
        out.push({ text: chunk, width: width > 0 ? width : 1 });
      }
      i += step;
    }
    return out;
  }

  private joinCellRange(cells: PtyTerminalCell[], start: number, end: number): string {
    let text = '';
    for (let i = start; i < end; i++) {
      text += cells[i].text;
    }
    return text;
  }

  private appendStyledSpan(
    target: PtyTerminalRenderSpan[],
    rowIndex: number,
    text: string,
    rawFg: ResourceColor,
    rawFgTag: string,
    rawBg: ResourceColor | null,
    rawBgTag: string,
    defaultBg: ResourceColor,
    defaultBgTag: string,
    invertForCursor: boolean,
    bold: boolean,
    underline: boolean
  ): void {
    if (text.length === 0) {
      return;
    }
    // `orbit-client-core` already emits swapped `fg_hex`/`bg_hex` for `sp.inverse`;
    // only swap here when rendering the cursor overlay cell.
    const fgColor: ResourceColor = invertForCursor ? (rawBg !== null ? rawBg : defaultBg) : rawFg;
    const fgTag = invertForCursor ? (rawBgTag.length > 0 ? rawBgTag : defaultBgTag) : rawFgTag;
    const bgColor: ResourceColor | null = invertForCursor ? rawFg : rawBg;
    const bgTag = invertForCursor ? rawFgTag : rawBgTag;
    const idx = target.length;
    target.push({
      renderKey: `${rowIndex}|${idx}|${fgTag}|${bgTag}|${bold ? 1 : 0}|${underline ? 1 : 0}|${text}`,
      text: text,
      fgColor: fgColor,
      bgColor: bgColor,
      bold: bold,
      underline: underline
    });
  }

  private buildRenderedRow(
    row: TerminalRowDto,
    cursorCol: number,
    defaultFg: ResourceColor,
    defaultBg: ResourceColor,
    themeTag: string
  ): PtyTerminalRenderRow {
    const spans: PtyTerminalRenderSpan[] = [];
    const defaultFgTag = `dfg_${themeTag}`;
    const defaultBgTag = `dbg_${themeTag}`;
    let colOffset = 0;

    for (let s = 0; s < row.spans.length; s++) {
      const sp: TerminalSpanDto = row.spans[s];
      const hasFg = sp.fg_hex.length > 0;
      const rawFg: ResourceColor = hasFg ? sp.fg_hex : defaultFg;
      const rawFgTag = hasFg ? sp.fg_hex : defaultFgTag;
      const hasBg = sp.bg_hex !== null && sp.bg_hex.length > 0;
      const rawBg: ResourceColor | null = hasBg ? (sp.bg_hex as string) : null;
      const rawBgTag = hasBg ? (sp.bg_hex as string) : '';

      const cells = this.splitTerminalCells(sp.text);
      let spanCols = 0;
      let cursorCellIdx = -1;
      for (let c = 0; c < cells.length; c++) {
        const cellStart = colOffset + spanCols;
        const cellEnd = cellStart + cells[c].width;
        if (cursorCol >= cellStart && cursorCol < cellEnd) {
          cursorCellIdx = c;
        }
        spanCols += cells[c].width;
      }

      if (cursorCellIdx >= 0) {
        if (cursorCellIdx > 0) {
          this.appendStyledSpan(
            spans,
            row.row_index,
            this.joinCellRange(cells, 0, cursorCellIdx),
            rawFg,
            rawFgTag,
            rawBg,
            rawBgTag,
            defaultBg,
            defaultBgTag,
            false,
            sp.bold,
            sp.underline
          );
        }
        this.appendStyledSpan(
          spans,
          row.row_index,
          cells[cursorCellIdx].text,
          rawFg,
          rawFgTag,
          rawBg,
          rawBgTag,
          defaultBg,
          defaultBgTag,
          true,
          sp.bold,
          sp.underline
        );
        if (cursorCellIdx + 1 < cells.length) {
          this.appendStyledSpan(
            spans,
            row.row_index,
            this.joinCellRange(cells, cursorCellIdx + 1, cells.length),
            rawFg,
            rawFgTag,
            rawBg,
            rawBgTag,
            defaultBg,
            defaultBgTag,
            false,
            sp.bold,
            sp.underline
          );
        }
      } else {
        this.appendStyledSpan(
          spans,
          row.row_index,
          sp.text,
          rawFg,
          rawFgTag,
          rawBg,
          rawBgTag,
          defaultBg,
          defaultBgTag,
          false,
          sp.bold,
          sp.underline
        );
      }

      colOffset += spanCols;
    }

    if (cursorCol >= colOffset && cursorCol >= 0) {
      const padCount = cursorCol - colOffset;
      if (padCount > 0) {
        this.appendStyledSpan(
          spans,
          row.row_index,
          ' '.repeat(padCount),
          defaultFg,
          defaultFgTag,
          null,
          '',
          defaultBg,
          defaultBgTag,
          false,
          false,
          false
        );
      }
      this.appendStyledSpan(
        spans,
        row.row_index,
        ' ',
        defaultFg,
        defaultFgTag,
        null,
        '',
        defaultBg,
        defaultBgTag,
        true,
        false,
        false
      );
    } else if (spans.length === 0) {
      this.appendStyledSpan(
        spans,
        row.row_index,
        ' ',
        defaultFg,
        defaultFgTag,
        null,
        '',
        defaultBg,
        defaultBgTag,
        false,
        false,
        false
      );
    }

    return {
      rowIndex: row.row_index,
      spans: spans
    };
  }

  private inputPlaceholder(): string {
    if (this.store.ctrlSticky && this.store.altSticky) {
      return 'CTRL+ALT + key…';
    }
    if (this.store.ctrlSticky) {
      return 'CTRL + key…';
    }
    if (this.store.altSticky) {
      return 'ALT + key…';
    }
    return 'Send command or raw keys…';
  }

  private interceptArmedInsert(info: InsertValue): boolean {
    if (this.store.ctrlSticky || this.store.altSticky) {
      if (info.insertValue.length === 1) {
        this.store.sendTerminalKeys(info.insertValue);
        this.store.triggerHaptic(6);
        return false;
      }
      this.store.ctrlSticky = false;
      this.store.altSticky = false;
    }
    return true;
  }

  private submitShellInput(): void {
    if (this.store.ctrlSticky || this.store.altSticky) {
      this.store.sendTerminalKeys('\r');
      return;
    }
    const payload = this.shellInput;
    this.shellInput = '';
    this.store.sendTerminalKeys(`${payload}\r`);
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
              Repeat<PtyTerminalRenderRow>(this.renderedRows)
                .each((ri: RepeatItem<PtyTerminalRenderRow>) => {
                  ListItem() {
                    Text() {
                      ForEach(
                        ri.item.spans,
                        (sp: PtyTerminalRenderSpan) => {
                          if (sp.bgColor !== null) {
                            Span(sp.text)
                              .font(
                                orbitSpanMonoFont(
                                  sp.bold ? FontWeight.Bold : FontWeight.Normal
                                )
                              )
                              .fontColor(sp.fgColor)
                              .textBackgroundStyle({
                                color: sp.bgColor,
                                radius: 0
                              })
                              .decoration({
                                type: sp.underline
                                  ? TextDecorationType.Underline
                                  : TextDecorationType.None,
                                color: sp.fgColor
                              })
                          } else {
                            Span(sp.text)
                              .font(
                                orbitSpanMonoFont(
                                  sp.bold ? FontWeight.Bold : FontWeight.Normal
                                )
                              )
                              .fontColor(sp.fgColor)
                              .decoration({
                                type: sp.underline
                                  ? TextDecorationType.Underline
                                  : TextDecorationType.None,
                                color: sp.fgColor
                              })
                          }
                        },
                        (sp: PtyTerminalRenderSpan) => sp.renderKey
                      )
                    }
                    .copyOption(CopyOptions.LocalDevice)
                    .width('100%')
                  }
                })
                .key((r: PtyTerminalRenderRow) => r.rowIndex.toString())
                .virtualScroll({ totalCount: this.renderedRows.length })
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
            placeholder: this.inputPlaceholder(),
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
            .onWillInsert((info: InsertValue): boolean => {
              return this.interceptArmedInsert(info);
            })
            .onChange((val: string) => {
              this.shellInput = val;
            })
            .onSubmit(() => {
              this.submitShellInput();
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
            this.submitShellInput();
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
