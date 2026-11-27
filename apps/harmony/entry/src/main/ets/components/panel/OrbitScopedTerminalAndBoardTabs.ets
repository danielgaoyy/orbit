import {
  AgentCardDto,
  DevKeyItem,
  TerminalRowDto,
  TerminalScreenDto,
  TerminalSpanDto,
  TerminalSummaryDto,
  WorkspaceEventRecordDto,
  basenameOfPath,
  formatAgentStatusLabel,
  formatCompactRelativeTime,
  formatProviderLabel
} from '../../model/OrbitTypes';
import {
  OrbitHarmonyStore,
  OrbitScopedBoardData
} from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitFleetStore } from '../../model/stores/OrbitFleetStore';
import {
  OrbitSidePanelTab,
  encodeSidePanelKeyTuple
} from '../../model/stores/OrbitSidePanelStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitInputSubhead,
  orbitSpanMonoFont,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

interface OrbitScopedTerminalRenderSpan {
  renderKey: string;
  text: string;
  fgColor: ResourceColor;
  bgColor: ResourceColor | null;
  bold: boolean;
  underline: boolean;
}

interface OrbitScopedTerminalRenderRow {
  rowKey: string;
  rowIndex: number;
  spans: OrbitScopedTerminalRenderSpan[];
}

interface OrbitScopedTerminalCell {
  text: string;
  width: number;
}

const SCOPED_TERMINAL_KEYBAR: DevKeyItem[] = [
  { id: 'esc', label: 'ESC', sequence: '\x1b', isModifier: false, a11yLabel: 'Escape key' },
  { id: 'tab', label: 'TAB', sequence: '\t', isModifier: false, a11yLabel: 'Tab key' },
  { id: 'ctrl', label: 'CTRL', sequence: '', isModifier: true, a11yLabel: 'Sticky Control modifier' },
  { id: 'alt', label: 'ALT', sequence: '', isModifier: true, a11yLabel: 'Sticky Alt modifier' },
  { id: 'ctrl_c', label: '^C', sequence: '\x03', isModifier: false, a11yLabel: 'Interrupt Control C' },
  { id: 'ctrl_d', label: '^D', sequence: '\x04', isModifier: false, a11yLabel: 'End of input Control D' },
  { id: 'ctrl_z', label: '^Z', sequence: '\x1a', isModifier: false, a11yLabel: 'Suspend Control Z' },
  { id: 'ctrl_l', label: '^L', sequence: '\x0c', isModifier: false, a11yLabel: 'Clear screen Control L' },
  { id: 'bksp', label: '⌫', sequence: '\x7f', isModifier: false, a11yLabel: 'Backspace raw byte' },
  { id: 'up', label: '↑', sequence: '\x1b[A', isModifier: false, a11yLabel: 'Up arrow' },
  { id: 'down', label: '↓', sequence: '\x1b[B', isModifier: false, a11yLabel: 'Down arrow' },
  { id: 'left', label: '←', sequence: '\x1b[D', isModifier: false, a11yLabel: 'Left arrow' },
  { id: 'right', label: '→', sequence: '\x1b[C', isModifier: false, a11yLabel: 'Right arrow' },
  { id: 'pipe', label: '|', sequence: '|', isModifier: false, a11yLabel: 'Pipe character' },
  { id: 'slash', label: '/', sequence: '/', isModifier: false, a11yLabel: 'Slash character' },
  { id: 'dash', label: '-', sequence: '-', isModifier: false, a11yLabel: 'Dash character' },
  { id: 'tilde', label: '~', sequence: '~', isModifier: false, a11yLabel: 'Tilde character' }
];

function codePointCellWidth(cp: number): number {
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

function splitTerminalCells(text: string): OrbitScopedTerminalCell[] {
  const out: OrbitScopedTerminalCell[] = [];
  let i = 0;
  while (i < text.length) {
    const cp = text.codePointAt(i);
    const step = cp !== undefined && cp > 0xFFFF ? 2 : 1;
    const chunk = text.substring(i, i + step);
    const cellW = cp !== undefined ? codePointCellWidth(cp) : 1;
    if (cellW === 0 && out.length > 0) {
      out[out.length - 1].text += chunk;
    } else {
      out.push({ text: chunk, width: cellW > 0 ? cellW : 1 });
    }
    i += step;
  }
  return out;
}

function joinCellRange(cells: OrbitScopedTerminalCell[], start: number, end: number): string {
  let text = '';
  for (let i = start; i < end; i++) {
    text += cells[i].text;
  }
  return text;
}

/**
 * Scoped PTY Terminal tab (`kind === 'terminal'`).
 * - P1-1: Isolates lineInput, sticky modifiers, raw mode, and activeSlot per `(scopeKey, tab.hostId, tab.tabId)`.
 * - P1-3: Syncs chosen slot back into `tab.terminalSlot` so `terminal_changed` events for slots 2/3
 *   refresh live; includes render revision + row signature in `ForEach` keys so in-place DTO replacement
 *   always repaints.
 * - P1-8: Restores full ANSI colors, cursor block inversion, dynamic `onAreaChange` PTY resize,
 *   Raw/Line mode, sticky CTRL/ALT + VT100 keybar, hardware keys, Copy, Quote, and terminal attach/kill.
 */
@ComponentV2
export struct OrbitScopedTerminalTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private activeSlot: number = 1;
  @Local private screen: TerminalScreenDto | null = null;
  @Local private screenRev: number = 0;
  @Local private lineInput: string = '';
  @Local private rawMode: boolean = false;
  @Local private ctrlSticky: boolean = false;
  @Local private altSticky: boolean = false;
  @Local private copiedFeedback: boolean = false;
  @Local private gridCols: number = 80;
  @Local private gridRows: number = 24;

  private lastTargetKey: string = '';
  private copiedTimerId: number = -1;
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetStore: OrbitFleetStore = orbitRuntime().fleetStore;

  private currentTargetKey(): string {
    return `${this.scopeKey}|${this.tab.hostId}::${this.tab.tabId}`;
  }

  private resolveTabInitialSlot(): number {
    const raw = (this.tab.terminalSlot !== undefined && this.tab.terminalSlot !== null && this.tab.terminalSlot > 0)
      ? this.tab.terminalSlot
      : 1;
    return Math.max(1, Math.min(3, raw));
  }

  aboutToAppear(): void {
    this.lastTargetKey = this.currentTargetKey();
    this.activeSlot = this.resolveTabInitialSlot();
    if (this.tab.hostId.trim().length > 0) {
      this.store.ensureScopedHostDetailAsync(this.tab.hostId);
    }
    this.reloadScreen();
  }

  aboutToDisappear(): void {
    this.store.shellStore.setTerminalFocused(false);
    if (this.copiedTimerId !== -1) {
      clearTimeout(this.copiedTimerId);
      this.copiedTimerId = -1;
    }
  }

  @Monitor('tab', 'scopeKey', 'store.terminalPanelRevision', 'store.panelDataRevision', 'fleetStore.hostDetailRevision')
  onDataChanged(_mon: IMonitor): void {
    const nextKey = this.currentTargetKey();
    if (nextKey !== this.lastTargetKey) {
      this.lastTargetKey = nextKey;
      this.lineInput = '';
      this.ctrlSticky = false;
      this.altSticky = false;
      this.activeSlot = this.resolveTabInitialSlot();
      if (this.tab.hostId.trim().length > 0) {
        this.store.ensureScopedHostDetailAsync(this.tab.hostId);
      }
    }
    this.reloadScreen();
  }

  private reloadScreen(): void {
    if (this.tab.hostId.trim().length === 0) {
      this.screen = null;
      this.screenRev += 1;
      return;
    }
    this.screen = this.store.loadScopedTerminalScreen(this.tab.hostId, this.activeSlot);
    this.screenRev += 1;
  }

  private selectSlot(slotNum: number): void {
    const clamped = Math.max(1, Math.min(3, slotNum));
    this.activeSlot = clamped;
    this.store.sidePanelStore.updateActiveTabTerminalSlot(this.tab.hostId, this.tab.tabId, clamped);
    this.reloadScreen();
    if (this.gridCols >= 24 && this.gridRows >= 8) {
      this.store.resizeScopedTerminalIfChanged(this.tab.hostId, clamped, this.gridCols, this.gridRows);
    }
  }

  @Computed
  get terminals(): TerminalSummaryDto[] {
    const detail = this.store.getScopedHostDetail(this.tab.hostId);
    return detail !== null ? detail.terminals : [];
  }

  @Computed
  get activeTerminalId(): string {
    const detail = this.store.getScopedHostDetail(this.tab.hostId);
    return detail !== null && detail.active_terminal_id !== null ? detail.active_terminal_id : '';
  }

  private appendStyledSpan(
    target: OrbitScopedTerminalRenderSpan[],
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
    themeTag: string,
    rev: number
  ): OrbitScopedTerminalRenderRow {
    const spans: OrbitScopedTerminalRenderSpan[] = [];
    const defaultFgTag = `dfg_${themeTag}`;
    const defaultBgTag = `dbg_${themeTag}`;
    let colOffset = 0;
    let plainSig = '';

    for (let s = 0; s < row.spans.length; s++) {
      const sp: TerminalSpanDto = row.spans[s];
      plainSig += sp.text;
      const hasFg = sp.fg_hex.length > 0;
      const rawFg: ResourceColor = hasFg ? sp.fg_hex : defaultFg;
      const rawFgTag = hasFg ? sp.fg_hex : defaultFgTag;
      const hasBg = sp.bg_hex !== null && sp.bg_hex.length > 0;
      const rawBg: ResourceColor | null = hasBg ? (sp.bg_hex as string) : null;
      const rawBgTag = hasBg ? (sp.bg_hex as string) : '';

      const cells = splitTerminalCells(sp.text);
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
            spans, row.row_index, joinCellRange(cells, 0, cursorCellIdx),
            rawFg, rawFgTag, rawBg, rawBgTag, defaultBg, defaultBgTag, false, sp.bold, sp.underline
          );
        }
        this.appendStyledSpan(
          spans, row.row_index, cells[cursorCellIdx].text,
          rawFg, rawFgTag, rawBg, rawBgTag, defaultBg, defaultBgTag, true, sp.bold, sp.underline
        );
        if (cursorCellIdx + 1 < cells.length) {
          this.appendStyledSpan(
            spans, row.row_index, joinCellRange(cells, cursorCellIdx + 1, cells.length),
            rawFg, rawFgTag, rawBg, rawBgTag, defaultBg, defaultBgTag, false, sp.bold, sp.underline
          );
        }
      } else {
        this.appendStyledSpan(
          spans, row.row_index, sp.text,
          rawFg, rawFgTag, rawBg, rawBgTag, defaultBg, defaultBgTag, false, sp.bold, sp.underline
        );
      }

      colOffset += spanCols;
    }

    if (cursorCol >= colOffset && cursorCol >= 0) {
      const padCount = cursorCol - colOffset;
      if (padCount > 0) {
        this.appendStyledSpan(
          spans, row.row_index, ' '.repeat(padCount),
          defaultFg, defaultFgTag, null, '', defaultBg, defaultBgTag, false, false, false
        );
      }
      this.appendStyledSpan(
        spans, row.row_index, ' ',
        defaultFg, defaultFgTag, null, '', defaultBg, defaultBgTag, true, false, false
      );
    } else if (spans.length === 0) {
      this.appendStyledSpan(
        spans, row.row_index, ' ',
        defaultFg, defaultFgTag, null, '', defaultBg, defaultBgTag, false, false, false
      );
    }

    return {
      rowKey: `sr_${rev}_${row.row_index}_${cursorCol}_${plainSig.length}_${spans.length}`,
      rowIndex: row.row_index,
      spans: spans
    };
  }

  @Computed
  get renderedRows(): OrbitScopedTerminalRenderRow[] {
    const rev = this.screenRev;
    const screen = this.screen;
    if (screen === null || screen.rows_data.length === 0) {
      return [];
    }
    const rows = screen.rows_data;
    const cursorVisible = screen.cols > 0;
    const cursorRow = cursorVisible ? screen.cursor_row : -1;
    const cursorCol = cursorVisible && screen.cursor_col >= 0
      ? Math.min(screen.cursor_col, screen.cols - 1)
      : -1;
    const defaultFg = this.store.palette.terminalFg;
    const defaultBg = this.store.palette.terminalBg;
    const themeTag = this.store.palette.isDark ? 'd' : 'l';
    const out: OrbitScopedTerminalRenderRow[] = [];
    for (let i = 0; i < rows.length; i++) {
      const row = rows[i];
      out.push(
        this.buildRenderedRow(
          row,
          row.row_index === cursorRow ? cursorCol : -1,
          defaultFg,
          defaultBg,
          themeTag,
          rev
        )
      );
    }
    return out;
  }

  private sendWithStickyModifiers(raw: string): void {
    const host = this.tab.hostId.trim();
    if (host.length === 0 || raw.length === 0) {
      return;
    }
    if (this.ctrlSticky && raw.length === 1) {
      const upper = raw.toUpperCase().charCodeAt(0);
      if (upper >= 64 && upper <= 95) {
        const ctrlByte = upper - 64;
        const payload = this.altSticky ? new Uint8Array([0x1B, ctrlByte]) : new Uint8Array([ctrlByte]);
        this.ctrlSticky = false;
        this.altSticky = false;
        this.store.sendScopedTerminalBytes(host, this.activeSlot, payload);
        return;
      }
      this.ctrlSticky = false;
    }
    if (this.altSticky) {
      this.altSticky = false;
      this.store.sendScopedTerminalUtf8(host, this.activeSlot, `\x1b${raw}`);
      return;
    }
    this.store.sendScopedTerminalUtf8(host, this.activeSlot, raw);
  }

  private handleWillInsert(info: InsertValue): boolean {
    if (this.rawMode) {
      if (info.insertValue.length > 0) {
        this.sendWithStickyModifiers(info.insertValue);
        this.store.triggerHaptic(4);
      }
      return false;
    }
    if (this.ctrlSticky || this.altSticky) {
      if (info.insertValue.length === 1) {
        this.sendWithStickyModifiers(info.insertValue);
        this.store.triggerHaptic(6);
        return false;
      }
      this.ctrlSticky = false;
      this.altSticky = false;
    }
    return true;
  }

  private handleRawHardwareKey(event: KeyEvent): void {
    if (!this.rawMode || event.type !== KeyType.Down) {
      return;
    }
    const host = this.tab.hostId.trim();
    if (host.length === 0) {
      return;
    }
    if (event.keyCode === 2055) {
      this.store.sendScopedTerminalBytes(host, this.activeSlot, new Uint8Array([0x7F]));
    } else if (event.keyCode === 2054) {
      this.store.sendScopedTerminalBytes(host, this.activeSlot, new Uint8Array([0x0D]));
    } else if (event.keyCode === 2049) {
      this.store.sendScopedTerminalBytes(host, this.activeSlot, new Uint8Array([0x09]));
    } else if (event.keyCode === 2070) {
      this.store.sendScopedTerminalBytes(host, this.activeSlot, new Uint8Array([0x1B]));
    } else if (event.keyCode === 2012) {
      this.store.sendScopedTerminalUtf8(host, this.activeSlot, '\x1b[A');
    } else if (event.keyCode === 2013) {
      this.store.sendScopedTerminalUtf8(host, this.activeSlot, '\x1b[B');
    } else if (event.keyCode === 2014) {
      this.store.sendScopedTerminalUtf8(host, this.activeSlot, '\x1b[D');
    } else if (event.keyCode === 2015) {
      this.store.sendScopedTerminalUtf8(host, this.activeSlot, '\x1b[C');
    }
  }

  private handleDevKeyTap(item: DevKeyItem): void {
    if (item.id === 'ctrl') {
      this.ctrlSticky = !this.ctrlSticky;
      this.store.triggerHaptic(8);
      return;
    }
    if (item.id === 'alt') {
      this.altSticky = !this.altSticky;
      this.store.triggerHaptic(8);
      return;
    }
    const host = this.tab.hostId.trim();
    if (host.length === 0) {
      return;
    }
    if (item.id === 'bksp') {
      this.store.sendScopedTerminalBytes(host, this.activeSlot, new Uint8Array([0x7F]));
    } else {
      this.sendWithStickyModifiers(item.sequence);
    }
    this.store.triggerHaptic(6);
  }

  private submitCommand(): void {
    const host = this.tab.hostId.trim();
    if (host.length === 0) {
      return;
    }
    if (this.rawMode) {
      this.store.sendScopedTerminalBytes(host, this.activeSlot, new Uint8Array([0x0D]));
      this.store.triggerHaptic(6);
      return;
    }
    if (this.ctrlSticky || this.altSticky) {
      this.sendWithStickyModifiers('\r');
      return;
    }
    const cmd = this.lineInput;
    this.lineInput = '';
    this.store.sendScopedTerminalUtf8(host, this.activeSlot, `${cmd}\r`);
    this.reloadScreen();
  }

  private inputPlaceholder(): ResourceStr {
    if (this.ctrlSticky && this.altSticky) {
      return 'CTRL+ALT + key…';
    }
    if (this.ctrlSticky) {
      return 'CTRL + key…';
    }
    if (this.altSticky) {
      return 'ALT + key…';
    }
    return this.rawMode
      ? $r('app.string.terminal_raw_placeholder')
      : $r('app.string.terminal_line_placeholder');
  }

  build() {
    Column({ space: OrbitTokens.SPACE_XS }) {
      // Top toolbar: Slots 1..3 + Raw/Line + Copy + Quote + New Shell
      Scroll() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          ForEach(
            [1, 2, 3],
            (slotNum: number) => {
              Button(`S${slotNum}`, { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(
                    this.activeSlot === slotNum
                      ? this.store.palette.onAccent
                      : this.store.palette.fgSecondary,
                    this.activeSlot === slotNum
                      ? this.store.palette.accentPrimaryFill
                      : this.store.palette.subtleSurface
                  )
                )
                .height(28)
                .padding({ left: 9, right: 9 })
                .onClick(() => {
                  this.selectSlot(slotNum);
                })
            },
            (slotNum: number): string => `tslot_${slotNum}`
          )

          Button(
            this.rawMode ? $r('app.string.terminal_mode_raw') : $r('app.string.terminal_mode_line'),
            { type: ButtonType.Capsule, stateEffect: true }
          )
            .attributeModifier(
              orbitButtonMeta(
                this.rawMode ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                this.rawMode ? this.store.palette.accentPrimaryFill : this.store.palette.subtleSurface
              )
            )
            .height(28)
            .padding({ left: 9, right: 9 })
            .onClick(() => {
              this.rawMode = !this.rawMode;
              this.lineInput = '';
              this.store.triggerHaptic(8);
            })

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph(
              this.copiedFeedback ? $r('sys.symbol.checkmark') : $r('sys.symbol.square_and_square')
            )
              .fontSize(14)
              .fontColor([this.copiedFeedback ? this.store.palette.statusDirect : this.store.palette.iconPrimary])
          }
          .width(28)
          .height(28)
          .padding(0)
          .backgroundColor(
            this.copiedFeedback ? this.store.palette.statusDirectSoft : this.store.palette.subtleSurface
          )
          .accessibilityText('Copy terminal output')
          .onClick(() => {
            const text = this.store.extractTerminalScreenPlainText(this.screen);
            if (text.length === 0) {
              this.store.showToast('Terminal buffer is empty', true);
              return;
            }
            this.store.copyToClipboard(text, 'Copied terminal output');
            this.copiedFeedback = true;
            if (this.copiedTimerId !== -1) {
              clearTimeout(this.copiedTimerId);
            }
            this.copiedTimerId = setTimeout(() => {
              this.copiedFeedback = false;
              this.copiedTimerId = -1;
            }, 1500);
          })

          Button($r('app.string.action_quote'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft))
            .height(28)
            .padding({ left: 9, right: 9 })
            .onClick(() => {
              dismissHarmonyKeyboard(this.getUIContext());
              this.store.quoteScopedTerminalIntoComposer(this.screen, this.tab.cwd ?? '');
            })

          Button('+ New', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
            .height(28)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.store.startScopedTerminal(this.tab.hostId, this.tab.cwd ?? '~', this.gridCols, this.gridRows);
            })
        }
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')

      // Attached / available PTY pills with attach & kill actions
      if (this.terminals.length > 0) {
        Scroll() {
          Row({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              this.terminals,
              (term: TerminalSummaryDto, tIdx: number) => {
                Row({ space: 2 }) {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Text(term.title.length > 0 ? term.title : basenameOfPath(term.cwd) || `Shell ${tIdx + 1}`)
                      .attributeModifier(
                        orbitTextMono(
                          this.activeTerminalId === term.terminal_id
                            ? this.store.palette.accentPrimary
                            : this.store.palette.fgSecondary,
                          FontWeight.Medium
                        )
                      )
                  }
                  .height(28)
                  .padding({ left: 9, right: 4 })
                  .backgroundColor(Color.Transparent)
                  .onClick(() => {
                    this.store.attachScopedTerminal(this.tab.hostId, term.terminal_id, this.activeSlot);
                    this.reloadScreen();
                  })

                  Button({ type: ButtonType.Circle, stateEffect: true }) {
                    SymbolGlyph($r('sys.symbol.xmark'))
                      .fontSize(11)
                      .fontColor([this.store.palette.iconSecondary])
                  }
                  .width(24)
                  .height(24)
                  .padding(0)
                  .backgroundColor(Color.Transparent)
                  .accessibilityText(`Close terminal ${term.title}`)
                  .onClick(() => {
                    this.store.killScopedTerminal(this.tab.hostId, term.terminal_id);
                  })
                }
                .backgroundColor(
                  this.activeTerminalId === term.terminal_id
                    ? this.store.palette.accentPrimarySoft
                    : this.store.palette.listCard
                )
                .borderRadius(OrbitTokens.RADIUS_MD)
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .alignItems(VerticalAlign.Center)
              },
              (term: TerminalSummaryDto): string => `sterm_${this.tab.hostId}_${term.terminal_id}`
            )
          }
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        }
        .scrollable(ScrollDirection.Horizontal)
        .scrollBar(BarState.Off)
        .width('100%')
      }

      // Styled ANSI Terminal Canvas with dynamic PTY grid resize
      Column() {
        if (this.renderedRows.length === 0) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Blank()
            Text('No active terminal buffer in this slot.')
              .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            Button($r('app.string.terminal_spawn_shell'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(36)
              .padding({ left: 14, right: 14 })
              .onClick(() => {
                this.store.startScopedTerminal(this.tab.hostId, this.tab.cwd ?? '~', this.gridCols, this.gridRows);
              })
            Blank()
          }
          .width('100%')
          .height('100%')
          .alignItems(HorizontalAlign.Center)
        } else {
          List({ space: 0 }) {
            Repeat<OrbitScopedTerminalRenderRow>(this.renderedRows)
              .each((ri: RepeatItem<OrbitScopedTerminalRenderRow>) => {
                ListItem() {
                  Text() {
                    ForEach(
                      ri.item.spans,
                      (sp: OrbitScopedTerminalRenderSpan) => {
                        if (sp.bgColor !== null) {
                          Span(sp.text)
                            .font(orbitSpanMonoFont(sp.bold ? FontWeight.Bold : FontWeight.Normal))
                            .fontColor(sp.fgColor)
                            .textBackgroundStyle({ color: sp.bgColor, radius: 0 })
                            .decoration({
                              type: sp.underline ? TextDecorationType.Underline : TextDecorationType.None,
                              color: sp.fgColor
                            })
                        } else {
                          Span(sp.text)
                            .font(orbitSpanMonoFont(sp.bold ? FontWeight.Bold : FontWeight.Normal))
                            .fontColor(sp.fgColor)
                            .decoration({
                              type: sp.underline ? TextDecorationType.Underline : TextDecorationType.None,
                              color: sp.fgColor
                            })
                        }
                      },
                      (sp: OrbitScopedTerminalRenderSpan): string => sp.renderKey
                    )
                  }
                  .copyOption(CopyOptions.LocalDevice)
                  .width('100%')
                }
              })
              .key((r: OrbitScopedTerminalRenderRow): string => r.rowKey)
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
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.terminalBg)
      .onAreaChange((_oldArea: Area, newArea: Area) => {
        const w = Number(newArea.width);
        const h = Number(newArea.height);
        if (w > 120 && h > 64) {
          const cols = Math.max(24, Math.floor((w - 16) / 7.4));
          const rows = Math.max(8, Math.floor((h - 16) / 16.0));
          this.gridCols = cols;
          this.gridRows = rows;
          this.store.resizeScopedTerminalIfChanged(this.tab.hostId, this.activeSlot, cols, rows);
        }
      })

      // Compact VT100 Accessory Keybar
      Scroll() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          ForEach(
            SCOPED_TERMINAL_KEYBAR,
            (keyItem: DevKeyItem) => {
              Button(keyItem.label)
                .attributeModifier(
                  orbitButtonMeta(
                    (keyItem.id === 'ctrl' && this.ctrlSticky) || (keyItem.id === 'alt' && this.altSticky)
                      ? this.store.palette.onAccent
                      : this.store.palette.fgPrimary,
                    (keyItem.id === 'ctrl' && this.ctrlSticky) || (keyItem.id === 'alt' && this.altSticky)
                      ? this.store.palette.accentPrimaryFill
                      : this.store.palette.listCard
                  )
                )
                .fontFamily(OrbitTokens.FAMILY_MONO)
                .height(32)
                .constraintSize({ minWidth: 34 })
                .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                .borderRadius(OrbitTokens.RADIUS_SM)
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .accessibilityText(keyItem.a11yLabel)
                .onClick(() => {
                  this.handleDevKeyTap(keyItem);
                })
            },
            (keyItem: DevKeyItem): string => keyItem.id
          )
        }
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')

      // Input Bar (Raw keystroke forwarding vs Line submission)
      Row({ space: OrbitTokens.SPACE_XS }) {
        TextInput({ placeholder: this.inputPlaceholder(), text: this.lineInput })
          .attributeModifier(
            orbitInputMono(
              this.store.palette.fgPrimary,
              this.store.palette.fgMuted,
              this.store.palette.recessed
            )
          )
          .height(38)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .layoutWeight(1)
          .onFocus(() => {
            this.store.shellStore.setTerminalFocused(true);
            this.store.shellStore.setTextInputFocused(true);
          })
          .onBlur(() => {
            this.store.shellStore.setTerminalFocused(false);
            this.store.shellStore.setTextInputFocused(false);
          })
          .onWillInsert((info: InsertValue): boolean => {
            return this.handleWillInsert(info);
          })
          .onKeyEvent((event: KeyEvent) => {
            this.handleRawHardwareKey(event);
          })
          .onChange((val: string) => {
            if (!this.rawMode) {
              this.lineInput = val;
            } else if (val.length > 0) {
              this.lineInput = '';
            }
          })
          .onSubmit(() => {
            this.submitCommand();
          })

        Button('Send', { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
          .height(38)
          .padding({ left: 12, right: 12 })
          .onClick(() => {
            this.submitCommand();
          })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: 2,
        bottom: Math.max(OrbitTokens.SPACE_SM, this.store.safeBottomVp)
      })
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}

/**
 * Scoped Workspace Board & Sibling Peers tab (`kind === 'board'`).
 * - P0-3: Stores drafts strictly per `(hostId, workspaceId)`, verifies edit-origin target matches
 *   current target before writing, and disables all write/spawn actions when no workspace matches
 *   the tab's cwd/agent (NEVER falling back to the host's first workspace).
 * - P1-6: Clicking a child-peer routes through `store.openPeerPanelTab` so child-peers obey
 *   "root scope + subagent tab".
 * - P1-8: Restores Copy BOARD.md, Spawn Peer form, and Recent Activity (`boardData.events`).
 */
@ComponentV2
export struct OrbitScopedBoardTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private subTab: string = 'board'; // 'board' | 'peers'
  @Local private editing: boolean = false;
  @Local private editingTargetKey: string = '';
  @Local private boardDraft: string = '';
  @Local private boardData: OrbitScopedBoardData | null = null;
  @Local private peerHandleDraft: string = '';
  @Local private peerPromptDraft: string = '';

  private draftByWorkspaceKey: Map<string, string> = new Map<string, string>();
  private lastTabIdentity: string = '';
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetStore: OrbitFleetStore = orbitRuntime().fleetStore;

  private tabIdentityKey(): string {
    const ws = this.tab.workspaceId !== undefined && this.tab.workspaceId !== null ? this.tab.workspaceId : '';
    const cwd = this.tab.cwd !== null ? this.tab.cwd : '';
    const ag = this.tab.agentId !== null ? this.tab.agentId : '';
    return encodeSidePanelKeyTuple(['board_tab', this.scopeKey, this.tab.tabId, this.tab.hostId, ws, cwd, ag]);
  }

  @Computed
  get isHostDetailLoading(): boolean {
    const host = this.tab.hostId.trim();
    return host.length > 0 && this.store.getScopedHostDetail(host) === null;
  }

  @Computed
  get hasVerifiedWorkspace(): boolean {
    return (
      this.boardData !== null &&
      this.boardData.workspaceVerified &&
      this.boardData.workspaceId.trim().length > 0
    );
  }

  @Computed
  get isSnapshotLoading(): boolean {
    return this.hasVerifiedWorkspace && this.boardData !== null && !this.boardData.snapshotLoaded;
  }

  @Computed
  get canEditBoard(): boolean {
    return this.hasVerifiedWorkspace && this.boardData !== null && this.boardData.snapshotLoaded;
  }

  private currentWorkspaceTargetKey(): string {
    if (!this.canEditBoard || this.boardData === null) {
      return '';
    }
    const wsId = this.boardData.workspaceId.trim();
    const hostId = this.tab.hostId.trim();
    if (hostId.length === 0 || wsId.length === 0) {
      return '';
    }
    return encodeSidePanelKeyTuple(['board_ws', hostId, wsId]);
  }

  aboutToAppear(): void {
    this.lastTabIdentity = this.tabIdentityKey();
    if (this.tab.hostId.trim().length > 0) {
      this.store.ensureScopedHostDetailAsync(this.tab.hostId);
    }
    this.reloadBoard(true);
  }

  @Monitor('tab', 'scopeKey', 'store.panelDataRevision', 'fleetStore.hostDetailRevision')
  onBoardChanged(_mon: IMonitor): void {
    const nextIdentity = this.tabIdentityKey();
    if (nextIdentity !== this.lastTabIdentity) {
      // Persist unfinished draft strictly under its originating verified workspace key, then exit edit mode (P0-3 + R2-P0-2).
      if (this.editing && this.editingTargetKey.length > 0) {
        this.draftByWorkspaceKey.set(this.editingTargetKey, this.boardDraft);
      }
      this.lastTabIdentity = nextIdentity;
      this.editing = false;
      this.editingTargetKey = '';
      this.boardDraft = '';
      this.peerHandleDraft = '';
      this.peerPromptDraft = '';
      this.boardData = null;
      if (this.tab.hostId.trim().length > 0) {
        this.store.ensureScopedHostDetailAsync(this.tab.hostId);
      }
      this.reloadBoard(true);
      return;
    }
    const prevVerified = this.hasVerifiedWorkspace;
    const prevSnapshotLoaded = this.boardData !== null && this.boardData.snapshotLoaded;
    this.reloadBoard(false);
    // Once host detail hydrates and verifies a workspace whose snapshot is not yet cached, fetch it automatically.
    if (!prevVerified && this.hasVerifiedWorkspace && !prevSnapshotLoaded && !this.canEditBoard) {
      this.reloadBoard(true);
    }
    // If verified workspace or loaded snapshot disappeared/changed underneath an active editor, drop out safely.
    if (this.editing && (!this.canEditBoard || this.editingTargetKey !== this.currentWorkspaceTargetKey())) {
      this.editing = false;
      this.editingTargetKey = '';
    }
  }

  private reloadBoard(triggerRemote: boolean): void {
    if (this.tab.hostId.trim().length === 0) {
      this.boardData = null;
      return;
    }
    const wsId = (this.tab.workspaceId !== undefined && this.tab.workspaceId !== null)
      ? this.tab.workspaceId
      : '';
    this.boardData = this.store.loadScopedWorkspaceBoard(
      this.tab.hostId,
      this.tab.cwd ?? '',
      wsId,
      this.tab.agentId,
      triggerRemote
    );
  }

  private beginEditing(): void {
    const targetKey = this.currentWorkspaceTargetKey();
    if (!this.canEditBoard || targetKey.length === 0 || this.boardData === null) {
      return;
    }
    const savedDraft = this.draftByWorkspaceKey.get(targetKey);
    this.boardDraft = savedDraft !== undefined ? savedDraft : this.boardData.content;
    this.editingTargetKey = targetKey;
    this.editing = true;
    this.store.triggerHaptic(6);
  }

  private saveBoard(): void {
    const targetKey = this.currentWorkspaceTargetKey();
    if (
      !this.editing ||
      !this.canEditBoard ||
      this.boardData === null ||
      targetKey.length === 0 ||
      this.editingTargetKey !== targetKey
    ) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const ok = this.store.saveScopedWorkspaceBoard(
      this.tab.hostId,
      this.boardData.workspaceId,
      this.boardDraft,
      this.tab.cwd ?? '',
      this.tab.agentId
    );
    if (ok) {
      this.draftByWorkspaceKey.delete(targetKey);
      this.editing = false;
      this.editingTargetKey = '';
      this.reloadBoard(false);
    }
  }

  private submitSpawnPeer(): void {
    if (!this.hasVerifiedWorkspace || this.boardData === null) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const ok = this.store.spawnScopedWorkspacePeer(
      this.tab.hostId,
      this.boardData.workspaceId,
      this.tab.cwd ?? '',
      this.peerHandleDraft,
      this.peerPromptDraft
    );
    if (ok) {
      this.peerHandleDraft = '';
      this.peerPromptDraft = '';
      this.reloadBoard(true);
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Button('BOARD.md', { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonMeta(
              this.subTab === 'board' ? this.store.palette.onAccent : this.store.palette.fgSecondary,
              this.subTab === 'board' ? this.store.palette.accentPrimaryFill : this.store.palette.subtleSurface
            )
          )
          .height(30)
          .padding({ left: 12, right: 12 })
          .onClick(() => {
            this.subTab = 'board';
          })

        Button(`Peers (${this.boardData?.peers.length ?? 0})`, { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonMeta(
              this.subTab === 'peers' ? this.store.palette.onAccent : this.store.palette.fgSecondary,
              this.subTab === 'peers' ? this.store.palette.accentPrimaryFill : this.store.palette.subtleSurface
            )
          )
          .height(30)
          .padding({ left: 12, right: 12 })
          .onClick(() => {
            this.subTab = 'peers';
          })

        Blank()

        if (this.subTab === 'board' && this.canEditBoard && this.boardData !== null) {
          if (this.editing) {
            Button('Cancel', { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
              .height(30)
              .padding({ left: 10, right: 10 })
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                if (this.editingTargetKey.length > 0) {
                  this.draftByWorkspaceKey.delete(this.editingTargetKey);
                }
                this.editing = false;
                this.editingTargetKey = '';
              })
            Button('Save', { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(30)
              .padding({ left: 12, right: 12 })
              .onClick(() => {
                this.saveBoard();
              })
          } else {
            if (this.boardData.content.length > 0) {
              Button('Copy', { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
                .height(30)
                .padding({ left: 10, right: 10 })
                .onClick(() => {
                  if (this.boardData !== null) {
                    this.store.copyToClipboard(this.boardData.content, 'Copied BOARD.md');
                  }
                })
            }
            Button('Edit', { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
              .height(30)
              .padding({ left: 10, right: 10 })
              .onClick(() => {
                this.beginEditing();
              })
          }
        }

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(14)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(30)
        .height(30)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Refresh board')
        .onClick(() => {
          this.reloadBoard(true);
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      if (!this.hasVerifiedWorkspace) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          if (this.isHostDetailLoading) {
            LoadingProgress()
              .width(24)
              .height(24)
              .color(this.store.palette.accentPrimary)
            Text('Loading workspace metadata…')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text('Editing stays locked until this host confirms workspace ownership.')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .textAlign(TextAlign.Center)
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          } else {
            SymbolGlyph($r('sys.symbol.doc_text'))
              .fontSize(26)
              .fontColor([this.store.palette.fgMuted])
            Text('No workspace bound to this directory')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text('BOARD.md editing and peer spawning stay disabled until this tab matches a Mission Workspace on its host.')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .textAlign(TextAlign.Center)
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          }
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (this.subTab === 'board') {
        if (this.isSnapshotLoading) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Blank()
            LoadingProgress()
              .width(24)
              .height(24)
              .color(this.store.palette.accentPrimary)
            Text('Loading BOARD.md snapshot…')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text('Editing unlocks once the initial snapshot loads from the host.')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .textAlign(TextAlign.Center)
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            Blank()
          }
          .width('100%')
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Center)
        } else if (this.editing) {
          TextArea({ placeholder: '## Goal\n\n## Contracts', text: this.boardDraft })
            .fontSize(OrbitTokens.FONT_META)
            .fontFamily(OrbitTokens.FAMILY_MONO)
            .fontColor(this.store.palette.fgPrimary)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .padding(OrbitTokens.SPACE_MD)
            .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: OrbitTokens.SPACE_MD })
            .layoutWeight(1)
            .onFocus(() => {
              this.store.shellStore.setTextInputFocused(true);
            })
            .onBlur(() => {
              this.store.shellStore.setTextInputFocused(false);
            })
            .onChange((val: string) => {
              this.boardDraft = val;
              if (this.editingTargetKey.length > 0) {
                this.draftByWorkspaceKey.set(this.editingTargetKey, val);
              }
            })
        } else {
          Scroll() {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Text(
                this.boardData !== null && this.boardData.revision > 0
                  ? `Revision ${this.boardData.revision}${this.boardData.updatedBy.length > 0 ? ` · ${this.boardData.updatedBy}` : ''}`
                  : (this.boardData?.workspace?.title ?? 'Shared Blackboard')
              )
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              Text(
                this.boardData !== null && this.boardData.content.trim().length > 0
                  ? this.boardData.content
                  : 'BOARD.md is empty for this workspace.'
              )
                .attributeModifier(orbitTextBody(this.store.palette.fgPrimary))
                .copyOption(CopyOptions.LocalDevice)
                .width('100%')
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .alignItems(HorizontalAlign.Start)
          }
          .width('100%')
          .layoutWeight(1)
          .scrollBar(BarState.Auto)
        }
      } else {
        List({ space: OrbitTokens.SPACE_SM }) {
          if ((this.boardData?.peers ?? []).length === 0) {
            ListItem() {
              Text('No active sibling peers in this workspace yet.')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .padding(OrbitTokens.SPACE_MD)
                .width('100%')
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
          } else {
            ForEach(
              this.boardData?.peers ?? [],
              (peer: AgentCardDto) => {
                ListItem() {
                  Button({ type: ButtonType.Normal, stateEffect: true }) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Row()
                        .width(8)
                        .height(8)
                        .borderRadius(4)
                        .backgroundColor(
                          peer.is_awaiting_permission
                            ? this.store.palette.statusWarning
                            : (peer.is_running ? this.store.palette.accentPrimary : this.store.palette.statusDirect)
                        )
                      Column({ space: 2 }) {
                        Text(
                          peer.handle !== undefined && peer.handle !== null && peer.handle.length > 0
                            ? `@${peer.handle} · ${peer.title}`
                            : (peer.title.trim().length > 0 ? peer.title : peer.provider_label)
                        )
                          .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                        Text(`${formatProviderLabel(peer.provider)} · ${formatAgentStatusLabel(peer)}`)
                          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                          .maxLines(1)
                      }
                      .alignItems(HorizontalAlign.Start)
                      .layoutWeight(1)
                      SymbolGlyph($r('sys.symbol.chevron_right'))
                        .fontSize(12)
                        .fontColor([this.store.palette.fgMuted])
                    }
                    .width('100%')
                    .alignItems(VerticalAlign.Center)
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_MD)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                  .onClick(() => {
                    this.store.openPeerPanelTab(
                      this.tab.hostId,
                      peer,
                      this.tab.parentHostId ?? this.tab.hostId
                    );
                  })
                }
              },
              (peer: AgentCardDto): string => `sbp_${this.tab.hostId}_${peer.agent_id}_${peer.latest_seq}`
            )
          }

          // Spawn Workspace Peer form (P1-8)
          ListItem() {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text('Spawn Peer')
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Bold))

              TextInput({ placeholder: 'Optional @handle (e.g. reviewer)', text: this.peerHandleDraft })
                .attributeModifier(
                  orbitInputSubhead(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.recessed
                  )
                )
                .height(36)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .width('100%')
                .onFocus(() => {
                  this.store.shellStore.setTextInputFocused(true);
                })
                .onBlur(() => {
                  this.store.shellStore.setTextInputFocused(false);
                })
                .onChange((val: string) => {
                  this.peerHandleDraft = val;
                })

              TextArea({ placeholder: 'Task brief for the new peer…', text: this.peerPromptDraft })
                .fontSize(OrbitTokens.FONT_META)
                .fontColor(this.store.palette.fgPrimary)
                .placeholderColor(this.store.palette.fgMuted)
                .backgroundColor(this.store.palette.recessed)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .constraintSize({ minHeight: 56, maxHeight: 110 })
                .width('100%')
                .onFocus(() => {
                  this.store.shellStore.setTextInputFocused(true);
                })
                .onBlur(() => {
                  this.store.shellStore.setTextInputFocused(false);
                })
                .onChange((val: string) => {
                  this.peerPromptDraft = val;
                })

              Button('Spawn Workspace Peer', { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
                .width('100%')
                .height(34)
                .enabled(this.peerPromptDraft.trim().length > 0)
                .onClick(() => {
                  this.submitSpawnPeer();
                })
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
            .alignItems(HorizontalAlign.Start)
          }

          // Recent Workspace Events (P1-8)
          if ((this.boardData?.events ?? []).length > 0) {
            ListItem() {
              Text('Recent Activity')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
                .padding({ top: OrbitTokens.SPACE_XS })
            }

            ForEach(
              this.boardData?.events ?? [],
              (ev: WorkspaceEventRecordDto) => {
                ListItem() {
                  Column({ space: 2 }) {
                    Text(ev.summary)
                      .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary))
                      .maxLines(2)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                    Text(
                      `${ev.agent_handle !== undefined && ev.agent_handle !== null ? `@${ev.agent_handle} · ` : ''}${formatCompactRelativeTime(ev.created_at)}`
                    )
                      .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  }
                  .width('100%')
                  .padding(OrbitTokens.SPACE_SM)
                  .backgroundColor(this.store.palette.listCard)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .alignItems(HorizontalAlign.Start)
                }
              },
              (ev: WorkspaceEventRecordDto): string => `sb_ev_${this.boardData?.workspaceId ?? ''}_${ev.seq}`
            )
          }
        }
        .width('100%')
        .layoutWeight(1)
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
        })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
