import {
  AgentCardDto,
  DevKeyItem,
  OrbitTerminalSlotViewState,
  TerminalRowDto,
  TerminalScreenDto,
  TerminalSpanDto,
  TerminalSummaryDto,
  WorkspaceEventRecordDto,
  applyTerminalScreenDelta,
  basenameOfPath,
  formatCompactRelativeTime,
  mergeBoardCasDraft,
  parsePeerHandleAliases
} from '../../model/OrbitTypes';
import {
  OrbitHarmonyStore,
  OrbitScopedBoardData,
  OrbitScopedBoardSaveOutcome
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
import {
  OrbitAgentListItem,
  mapAgentCardToListStatus,
  resolveAgentEffortDisplay,
  resolveAgentFallbackLabel,
  resolveCompactAgentModelLabel
} from '../common/OrbitAgentListItem';

export interface OrbitScopedPeerRowProjection {
  peer: AgentCardDto;
  title: string;
  status: string;
  modelLabel: string;
  effortLabel: string;
  agentLabel: string;
  aliasesMeta: string;
  stableKey: string;
}

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
  @Local private activeDeltaRevision: number = 0;
  @Local private lineInput: string = '';
  @Local private rawMode: boolean = false;
  @Local private ctrlSticky: boolean = false;
  @Local private altSticky: boolean = false;
  @Local private copiedFeedback: boolean = false;
  @Local private gridCols: number = 80;
  @Local private gridRows: number = 24;

  private lastTargetKey: string = '';
  private lastConsumedRevision: number = 0;
  private copiedTimerId: number = -1;
  private slotStateByKey: Map<string, OrbitTerminalSlotViewState> = new Map<string, OrbitTerminalSlotViewState>();
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private fleetStore: OrbitFleetStore = orbitRuntime().fleetStore;

  @Computed
  get isCompactLandscape(): boolean {
    return this.store.windowWidthVp > this.store.windowHeightVp && this.store.windowHeightVp < 540;
  }

  private effectiveScopeKey(): string {
    const clean = this.scopeKey.trim();
    return clean.length > 0 ? clean : this.store.sidePanelStore.activeScopeKey;
  }

  private currentTargetKey(): string {
    return encodeSidePanelKeyTuple(['pty_tab', this.effectiveScopeKey(), this.tab.hostId.trim(), this.tab.tabId]);
  }

  private slotStateKeyFor(targetKey: string, slotNum: number): string {
    const clamped = Math.max(1, Math.min(3, Math.floor(slotNum)));
    return encodeSidePanelKeyTuple(['pty_slot', targetKey, String(clamped)]);
  }

  private saveSlotState(targetKey: string, slotNum: number): void {
    if (targetKey.length === 0) {
      return;
    }
    const key = this.slotStateKeyFor(targetKey, slotNum);
    this.slotStateByKey.set(key, {
      screen: this.screen,
      revision: this.activeDeltaRevision,
      lastRequestedRevision: this.lastConsumedRevision,
      lineInput: this.lineInput,
      rawMode: this.rawMode,
      ctrlSticky: this.ctrlSticky,
      altSticky: this.altSticky
    });
  }

  private restoreSlotState(targetKey: string, slotNum: number): void {
    const key = this.slotStateKeyFor(targetKey, slotNum);
    const saved = this.slotStateByKey.get(key);
    if (saved !== undefined) {
      this.screen = saved.screen;
      this.activeDeltaRevision = saved.revision;
      this.lastConsumedRevision = saved.lastRequestedRevision;
      this.lineInput = saved.lineInput;
      this.rawMode = saved.rawMode;
      this.ctrlSticky = saved.ctrlSticky;
      this.altSticky = saved.altSticky;
    } else {
      this.screen = null;
      this.activeDeltaRevision = 0;
      this.lastConsumedRevision = 0;
      this.lineInput = '';
      this.rawMode = false;
      this.ctrlSticky = false;
      this.altSticky = false;
    }
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
    this.restoreSlotState(this.lastTargetKey, this.activeSlot);
    if (this.tab.hostId.trim().length > 0) {
      this.store.ensureScopedHostDetailAsync(this.tab.hostId);
    }
    this.reloadScreen();
  }

  aboutToDisappear(): void {
    this.saveSlotState(this.lastTargetKey, this.activeSlot);
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
      this.saveSlotState(this.lastTargetKey, this.activeSlot);
      this.lastTargetKey = nextKey;
      this.activeSlot = this.resolveTabInitialSlot();
      this.restoreSlotState(nextKey, this.activeSlot);
      if (this.tab.hostId.trim().length > 0) {
        this.store.ensureScopedHostDetailAsync(this.tab.hostId);
      }
    }
    this.reloadScreen();
  }

  private reloadScreen(forceReset: boolean = false, sinceOverride?: number): void {
    const cleanHost = this.tab.hostId.trim();
    const targetKey = this.currentTargetKey();
    const slot = Math.max(1, Math.min(3, this.activeSlot));
    if (cleanHost.length === 0) {
      this.screen = null;
      this.activeDeltaRevision = 0;
      this.lastConsumedRevision = 0;
      this.saveSlotState(targetKey, slot);
      this.screenRev += 1;
      return;
    }
    const prevScreen = forceReset ? null : this.screen;
    let sinceRev = forceReset
      ? 0
      : (sinceOverride !== undefined ? sinceOverride : this.activeDeltaRevision);
    // P1-1: If this consumer's own cursor ever regresses below its high watermark, request a clean reset (0).
    if (!forceReset && sinceRev > 0 && sinceRev < this.lastConsumedRevision) {
      sinceRev = 0;
    }
    const delta = this.store.loadScopedTerminalDelta(cleanHost, slot, sinceRev);
    if (delta !== null) {
      const applied = applyTerminalScreenDelta(sinceRev === 0 ? null : prevScreen, sinceRev, delta);
      const cursorMoved = prevScreen === null ||
        prevScreen.cursor_row !== applied.screen.cursor_row ||
        prevScreen.cursor_col !== applied.screen.cursor_col;
      this.screen = applied.screen;
      this.activeDeltaRevision = applied.revision;
      this.lastConsumedRevision = delta.reset || sinceRev === 0
        ? applied.revision
        : Math.max(this.lastConsumedRevision, applied.revision);
      this.saveSlotState(targetKey, slot);
      if (delta.reset || applied.updatedRowCount > 0 || cursorMoved || forceReset || sinceRev === 0) {
        this.screenRev += 1;
      }
      return;
    }
    const fallback = this.store.loadScopedTerminalScreen(cleanHost, slot);
    this.screen = fallback;
    this.activeDeltaRevision = 0;
    this.lastConsumedRevision = 0;
    this.saveSlotState(targetKey, slot);
    this.screenRev += 1;
  }

  private selectSlot(slotNum: number): void {
    const clamped = Math.max(1, Math.min(3, slotNum));
    const targetKey = this.currentTargetKey();
    if (clamped !== this.activeSlot) {
      this.saveSlotState(targetKey, this.activeSlot);
      this.activeSlot = clamped;
      this.restoreSlotState(targetKey, clamped);
    }
    this.store.sidePanelStore.updateTabTerminalSlot(
      this.effectiveScopeKey(),
      this.tab.hostId,
      this.tab.tabId,
      clamped
    );
    this.screenRev += 1;
    this.reloadScreen(false);
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
        this.store.triggerHapticPreset('selection');
      }
      return false;
    }
    if (this.ctrlSticky || this.altSticky) {
      if (info.insertValue.length === 1) {
        this.sendWithStickyModifiers(info.insertValue);
        this.store.triggerHapticPreset('selection');
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
      this.store.triggerHapticPreset('selection');
      return;
    }
    if (item.id === 'alt') {
      this.altSticky = !this.altSticky;
      this.store.triggerHapticPreset('selection');
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
    this.store.triggerHapticPreset('selection');
  }

  private submitCommand(): void {
    const host = this.tab.hostId.trim();
    if (host.length === 0) {
      return;
    }
    if (this.rawMode) {
      this.store.sendScopedTerminalBytes(host, this.activeSlot, new Uint8Array([0x0D]));
      this.store.triggerHapticPreset('selection');
      return;
    }
    if (this.ctrlSticky || this.altSticky) {
      this.sendWithStickyModifiers('\r');
      return;
    }
    const cmd = this.lineInput;
    this.lineInput = '';
    this.saveSlotState(this.currentTargetKey(), this.activeSlot);
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
              this.saveSlotState(this.currentTargetKey(), this.activeSlot);
              this.store.triggerHapticPreset('selection');
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
          .accessibilityText($r('app.string.action_copy'))
          .onClick(() => {
            const text = this.store.extractTerminalScreenPlainText(this.screen);
            if (text.length === 0) {
              this.store.showToastResource($r('app.string.terminal_buffer_empty'), true);
              return;
            }
            this.store.copyToClipboard(text, '');
            this.store.showToastResource($r('app.string.terminal_copied_toast'), false);
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

          Button($r('app.string.terminal_new_shell'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
            .height(28)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              const targetSlot = this.activeSlot;
              this.store.startScopedTerminal(
                this.tab.hostId,
                this.tab.cwd ?? '~',
                this.gridCols,
                this.gridRows,
                targetSlot
              );
            })

          if (this.isCompactLandscape && this.terminals.length > 0) {
            ForEach(
              this.terminals,
              (term: TerminalSummaryDto, tIdx: number) => {
                Button(
                  term.title.length > 0 ? term.title : (basenameOfPath(term.cwd) || `Shell ${tIdx + 1}`),
                  { type: ButtonType.Capsule, stateEffect: true }
                )
                  .attributeModifier(
                    orbitButtonMeta(
                      this.activeTerminalId === term.terminal_id
                        ? this.store.palette.accentPrimary
                        : this.store.palette.fgSecondary,
                      this.activeTerminalId === term.terminal_id
                        ? this.store.palette.accentPrimarySoft
                        : this.store.palette.listCard
                    )
                  )
                  .height(28)
                  .padding({ left: 8, right: 8 })
                  .onClick(() => {
                    this.store.attachScopedTerminal(this.tab.hostId, term.terminal_id, this.activeSlot);
                    this.reloadScreen(true);
                  })
              },
              (term: TerminalSummaryDto): string => `sterm_compact_${this.tab.hostId}_${term.terminal_id}`
            )
          }
        }
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          top: this.isCompactLandscape ? 2 : OrbitTokens.SPACE_SM
        })
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')

      // Attached / available PTY pills with attach & kill actions (merged into top bar in compact landscape)
      if (!this.isCompactLandscape && this.terminals.length > 0) {
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
                    this.reloadScreen(true);
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
            Text($r('app.string.terminal_no_buffer'))
              .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            Button($r('app.string.terminal_spawn_shell'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(36)
              .padding({ left: 14, right: 14 })
              .onClick(() => {
                const targetSlot = this.activeSlot;
                this.store.startScopedTerminal(
                  this.tab.hostId,
                  this.tab.cwd ?? '~',
                  this.gridCols,
                  this.gridRows,
                  targetSlot
                );
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
          .cachedCount(8)
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

        Button($r('app.string.action_send'), { type: ButtonType.Capsule, stateEffect: true })
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
  @Local private savingBoard: boolean = false;
  @Local private editingTargetKey: string = '';
  @Local private editBaseRevision: number = 0;
  @Local private boardDraft: string = '';
  @Local private boardData: OrbitScopedBoardData | null = null;
  @Local private peerRows: OrbitScopedPeerRowProjection[] = [];
  @Local private casConflictRevision: number = 0;
  @Local private casConflictRemoteContent: string = '';
  @Local private showCasConflictDialog: boolean = false;
  @Local private peerHandleDraft: string = '';
  @Local private peerPromptDraft: string = '';

  private lastTabIdentity: string = '';
  private lastPeerProjectionSig: string = '';
  private boardSaveSeq: number = 0;
  private disposed: boolean = false;
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

  @Computed
  get boardCasSupported(): boolean {
    const _rev = this.fleetStore.hostDetailRevision;
    if (_rev < 0) {
      // Keep reactive dependency on host capability updates
    }
    return this.store.isHostBoardCasSupported(this.tab.hostId);
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
    this.disposed = false;
    this.lastTabIdentity = this.tabIdentityKey();
    if (this.tab.hostId.trim().length > 0) {
      this.store.ensureScopedHostDetailAsync(this.tab.hostId);
    }
    this.reloadBoard(true);
  }

  aboutToDisappear(): void {
    // R2-P1-1: Persist unfinished draft (content + original baseRevision) in Store so it
    // survives full component unmount/remount across side-panel tab switches.
    if (this.editing && this.editingTargetKey.length > 0) {
      this.store.setWorkspaceBoardDraft(this.editingTargetKey, this.boardDraft, this.editBaseRevision);
    }
    this.disposed = true;
    this.boardSaveSeq += 1;
  }

  @Monitor('tab', 'scopeKey', 'store.panelDataRevision', 'fleetStore.hostDetailRevision')
  onBoardChanged(_mon: IMonitor): void {
    const nextIdentity = this.tabIdentityKey();
    if (nextIdentity !== this.lastTabIdentity) {
      // Persist unfinished draft WITH its original baseRevision strictly under its verified workspace key (P0-1 / R2-P1-1).
      if (this.editing && this.editingTargetKey.length > 0) {
        this.store.setWorkspaceBoardDraft(this.editingTargetKey, this.boardDraft, this.editBaseRevision);
      }
      this.lastTabIdentity = nextIdentity;
      this.boardSaveSeq += 1;
      this.savingBoard = false;
      this.editing = false;
      this.editingTargetKey = '';
      this.editBaseRevision = 0;
      this.boardDraft = '';
      this.casConflictRevision = 0;
      this.casConflictRemoteContent = '';
      this.showCasConflictDialog = false;
      this.peerHandleDraft = '';
      this.peerPromptDraft = '';
      this.boardData = null;
      this.peerRows = [];
      this.lastPeerProjectionSig = '';
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
    // If verified workspace or loaded snapshot disappeared/changed underneath an active editor,
    // preserve the draft in Store before dropping out of edit mode cleanly.
    if (this.editing && (!this.canEditBoard || this.editingTargetKey !== this.currentWorkspaceTargetKey())) {
      if (this.editingTargetKey.length > 0) {
        this.store.setWorkspaceBoardDraft(this.editingTargetKey, this.boardDraft, this.editBaseRevision);
      }
      this.editing = false;
      this.editingTargetKey = '';
    }
  }

  private rebuildPeerRowsIfChanged(): void {
    const peers = this.boardData !== null ? this.boardData.peers : [];
    const hostId = this.tab.hostId.trim();
    const detailRev = this.fleetStore.hostDetailRevision;
    let sig = `${hostId}|${detailRev}|${peers.length}`;
    for (let i = 0; i < peers.length; i++) {
      const p = peers[i];
      const rawAliases = p.labels !== undefined && p.labels !== null ? (p.labels['orbit.handle_aliases'] ?? '') : '';
      sig += `|${p.agent_id}:${p.latest_seq}:${p.status}:${p.handle ?? ''}:${p.title}:${rawAliases}`;
    }
    if (sig === this.lastPeerProjectionSig) {
      return;
    }
    this.lastPeerProjectionSig = sig;
    const detail = this.store.getScopedHostDetail(hostId);
    const catalogs = detail !== null ? detail.provider_catalogs : [];
    const projected: OrbitScopedPeerRowProjection[] = [];
    for (let i = 0; i < peers.length; i++) {
      const peer = peers[i];
      const aliasesMeta = this.resolvePeerAliasesMeta(peer);
      projected.push({
        peer: peer,
        title: this.resolvePeerTitle(peer),
        status: mapAgentCardToListStatus(peer),
        modelLabel: resolveCompactAgentModelLabel(peer, catalogs),
        effortLabel: resolveAgentEffortDisplay(peer),
        agentLabel: resolveAgentFallbackLabel(peer),
        aliasesMeta: aliasesMeta,
        stableKey: `sbp_${hostId}_${peer.agent_id}_${peer.latest_seq}_${peer.handle ?? ''}_${aliasesMeta}`
      });
    }
    this.peerRows = projected;
  }

  private reloadBoard(triggerRemote: boolean): void {
    if (this.tab.hostId.trim().length === 0) {
      this.boardData = null;
      this.peerRows = [];
      this.lastPeerProjectionSig = '';
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
    this.rebuildPeerRowsIfChanged();
  }

  private beginEditing(): void {
    const targetKey = this.currentWorkspaceTargetKey();
    if (!this.canEditBoard || targetKey.length === 0 || this.boardData === null) {
      return;
    }
    const savedDraft = this.store.getWorkspaceBoardDraft(targetKey);
    if (savedDraft !== null) {
      // Preserve the draft's original baseRevision so remote updates made while away trigger CAS conflict (P0-1 / R2-P1-1).
      this.boardDraft = savedDraft.content;
      this.editBaseRevision = savedDraft.baseRevision;
    } else {
      this.boardDraft = this.boardData.content;
      this.editBaseRevision = this.boardData.revision;
    }
    this.editingTargetKey = targetKey;
    this.casConflictRevision = 0;
    this.casConflictRemoteContent = '';
    this.showCasConflictDialog = false;
    this.editing = true;
    this.store.triggerHapticPreset('selection');
  }

  private async saveBoard(forceOverwrite: boolean = false): Promise<void> {
    const targetKey = this.currentWorkspaceTargetKey();
    if (
      !this.editing ||
      this.savingBoard ||
      !this.canEditBoard ||
      this.boardData === null ||
      targetKey.length === 0 ||
      this.editingTargetKey !== targetKey
    ) {
      return;
    }
    if (!forceOverwrite && !this.boardCasSupported) {
      this.store.showToastResource($r('app.string.board_cas_unsupported_toast'), true);
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    this.boardSaveSeq += 1;
    const saveSeq = this.boardSaveSeq;
    const originIdentity = this.tabIdentityKey();
    const originHost = this.tab.hostId;
    const originWs = this.boardData.workspaceId;
    const originCwd = this.tab.cwd ?? '';
    const originAgentId = this.tab.agentId;
    const draftToSave = this.boardDraft;
    const baseRev = this.editBaseRevision;
    this.savingBoard = true;
    const res: OrbitScopedBoardSaveOutcome = await this.store.saveScopedWorkspaceBoardWithCas(
      originHost,
      originWs,
      draftToSave,
      baseRev,
      forceOverwrite,
      originCwd,
      originAgentId
    );
    if (this.disposed || saveSeq !== this.boardSaveSeq) {
      return;
    }
    // R2-P1-1: ALWAYS clear savingBoard for the active saveSeq BEFORE checking transient
    // snapshotLoaded/targetKey state so a brief snapshot flicker during save can NEVER deadlock UI.
    this.savingBoard = false;
    if (res.ok) {
      this.store.clearWorkspaceBoardDraft(targetKey);
    }
    if (
      originIdentity !== this.tabIdentityKey() ||
      targetKey !== this.currentWorkspaceTargetKey()
    ) {
      return;
    }
    if (res.conflict) {
      this.casConflictRevision = res.currentRevision;
      this.casConflictRemoteContent = res.currentContent;
      this.showCasConflictDialog = true;
      this.store.showToastResource(
        $r('app.string.board_cas_conflict_body', res.currentRevision, baseRev),
        true
      );
      return;
    }
    if (res.ok) {
      this.editing = false;
      this.editingTargetKey = '';
      this.editBaseRevision = 0;
      this.casConflictRevision = 0;
      this.casConflictRemoteContent = '';
      this.showCasConflictDialog = false;
      this.store.showToastResource($r('app.string.board_saved_toast'), false);
      this.store.triggerHapticPreset('success');
      this.reloadBoard(false);
      return;
    }
    if (res.errorCode === 'cas_unsupported') {
      this.store.showToastResource($r('app.string.board_cas_unsupported_toast'), true);
    } else if (res.errorCode === 'snapshot_not_loaded') {
      this.store.showToastResource($r('app.string.board_snapshot_not_loaded'), true);
    } else if (res.errorCode === 'unbound') {
      this.store.showToastResource($r('app.string.board_workspace_unbound'), true);
    } else if (res.errorCode === 'loading') {
      this.store.showToastResource($r('app.string.board_workspace_loading'), true);
    } else if (res.errorCode === 'changed') {
      this.store.showToastResource($r('app.string.board_workspace_changed'), true);
    } else {
      this.store.showToastResource(
        res.errorMessage.length > 0
          ? $r('app.string.board_save_failed_detail', res.errorMessage)
          : $r('app.string.board_save_failed'),
        true,
        res.errorMessage
      );
    }
  }

  private resolveCasReloadAndMerge(): void {
    const merged = mergeBoardCasDraft(this.casConflictRemoteContent, this.boardDraft);
    this.boardDraft = merged;
    this.editBaseRevision = this.casConflictRevision;
    if (this.editingTargetKey.length > 0) {
      this.store.setWorkspaceBoardDraft(this.editingTargetKey, merged, this.casConflictRevision);
    }
    this.showCasConflictDialog = false;
    this.store.triggerHapticPreset('selection');
  }

  private resolvePeerTitle(peer: AgentCardDto): string {
    const cleanHandle = (peer.handle ?? '').trim().replace(/^@+/, '');
    const cleanTitle = peer.title.trim();
    if (cleanHandle.length > 0) {
      return cleanTitle.length > 0 ? `@${cleanHandle} · ${cleanTitle}` : `@${cleanHandle}`;
    }
    return cleanTitle.length > 0 ? cleanTitle : peer.provider_label;
  }

  private resolvePeerAliasesMeta(peer: AgentCardDto): string {
    const aliases = parsePeerHandleAliases(peer);
    if (aliases.length === 0) {
      return '';
    }
    const tagged: string[] = [];
    for (let i = 0; i < aliases.length; i++) {
      tagged.push(`@${aliases[i]}`);
    }
    return `aka ${tagged.join(', ')}`;
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

        Button($r('app.string.board_peers_tab_count', this.boardData?.peers.length ?? 0), { type: ButtonType.Capsule, stateEffect: true })
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

        if (this.hasVerifiedWorkspace && this.boardData !== null && this.boardData.revision > 0) {
          Text($r('app.string.board_revision_badge', this.boardData.revision))
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            .padding({ left: 6, right: 6, top: 2, bottom: 2 })
            .backgroundColor(this.store.palette.subtleSurface)
            .borderRadius(OrbitTokens.RADIUS_SM)
        }

        Blank()

        if (this.hasVerifiedWorkspace && this.boardData !== null && !this.editing) {
          Button($r('app.string.board_dev_scripts'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(30)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              if (this.boardData !== null) {
                this.store.openWorkspaceScriptsSheet(
                  this.tab.hostId,
                  this.boardData.workspaceId,
                  this.boardData.workspace?.title ?? ''
                );
              }
            })
        }

        if (this.subTab === 'board' && this.canEditBoard && this.boardData !== null) {
          if (this.editing) {
            Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
              .height(30)
              .padding({ left: 10, right: 10 })
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                if (this.editingTargetKey.length > 0) {
                  this.store.clearWorkspaceBoardDraft(this.editingTargetKey);
                }
                this.editing = false;
                this.editingTargetKey = '';
                this.showCasConflictDialog = false;
              })
            Button($r('app.string.action_save'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(30)
              .padding({ left: 12, right: 12 })
              .enabled(!this.savingBoard && this.boardCasSupported)
              .onClick(() => {
                void this.saveBoard(false);
              })
          } else {
            if (this.boardData.content.length > 0) {
              Button($r('app.string.action_copy'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
                .height(30)
                .padding({ left: 10, right: 10 })
                .onClick(() => {
                  if (this.boardData !== null) {
                    this.store.copyToClipboard(this.boardData.content, 'Copied BOARD.md');
                  }
                })
            }
            Button($r('app.string.action_edit'), { type: ButtonType.Capsule, stateEffect: true })
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
        .accessibilityText($r('app.string.board_refresh_aria'))
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
            Text($r('app.string.board_loading_metadata'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text($r('app.string.board_loading_metadata_hint'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .textAlign(TextAlign.Center)
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          } else {
            SymbolGlyph($r('sys.symbol.doc_text'))
              .fontSize(26)
              .fontColor([this.store.palette.fgMuted])
            Text($r('app.string.board_unbound_title'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text($r('app.string.board_unbound_hint'))
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
            Text($r('app.string.board_loading_snapshot'))
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text($r('app.string.board_loading_snapshot_hint'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .textAlign(TextAlign.Center)
              .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
            Blank()
          }
          .width('100%')
          .layoutWeight(1)
          .alignItems(HorizontalAlign.Center)
        } else if (this.editing) {
          Column({ space: OrbitTokens.SPACE_XS }) {
            if (!this.boardCasSupported) {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Text($r('app.string.board_cas_unsupported_banner'))
                  .attributeModifier(orbitTextMeta(this.store.palette.statusWarning, FontWeight.Medium))
                  .width('100%')
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Button($r('app.string.board_cas_force_overwrite'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
                    .height(28)
                    .padding({ left: 10, right: 10 })
                    .enabled(!this.savingBoard)
                    .onClick(() => {
                      void this.saveBoard(true);
                    })
                }
                .width('100%')
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_SM)
              .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .backgroundColor(this.store.palette.statusWarningSoft)
              .borderRadius(OrbitTokens.RADIUS_MD)
            }

            if (this.showCasConflictDialog) {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Text($r('app.string.board_cas_conflict_title'))
                  .attributeModifier(orbitTextSubhead(this.store.palette.statusDanger, FontWeight.Bold))
                  .width('100%')
                Text(
                  $r(
                    'app.string.board_cas_conflict_body',
                    this.casConflictRevision,
                    this.editBaseRevision
                  )
                )
                  .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary))
                  .width('100%')
                Row({ space: OrbitTokens.SPACE_XS }) {
                  Button($r('app.string.board_cas_reload_merge'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.listCard))
                    .height(30)
                    .padding({ left: 10, right: 10 })
                    .onClick(() => {
                      this.resolveCasReloadAndMerge();
                    })
                  Button($r('app.string.board_cas_force_overwrite'), { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
                    .height(30)
                    .padding({ left: 10, right: 10 })
                    .enabled(!this.savingBoard)
                    .onClick(() => {
                      void this.saveBoard(true);
                    })
                }
                .width('100%')
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_SM)
              .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .backgroundColor(this.store.palette.statusDangerSoft)
              .borderRadius(OrbitTokens.RADIUS_MD)
            }

            TextArea({ placeholder: $r('app.string.board_editor_placeholder'), text: this.boardDraft })
              .fontSize(OrbitTokens.FONT_META)
              .fontFamily(OrbitTokens.FAMILY_MONO)
              .fontColor(this.store.palette.fgPrimary)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .padding(OrbitTokens.SPACE_MD)
              .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: OrbitTokens.SPACE_MD })
              .layoutWeight(1)
              .enabled(!this.savingBoard)
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((val: string) => {
                this.boardDraft = val;
                if (this.editingTargetKey.length > 0) {
                  this.store.setWorkspaceBoardDraft(this.editingTargetKey, val, this.editBaseRevision);
                }
              })
          }
          .width('100%')
          .layoutWeight(1)
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
                  : $r('app.string.board_empty_notice')
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
          if (this.peerRows.length === 0) {
            ListItem() {
              Text($r('app.string.peers_empty_notice'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .padding(OrbitTokens.SPACE_MD)
                .width('100%')
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
          } else {
            ForEach(
              this.peerRows,
              (row: OrbitScopedPeerRowProjection) => {
                ListItem() {
                  OrbitAgentListItem({
                    variant: 'card',
                    leadingMode: 'avatar',
                    title: row.title,
                    status: row.status,
                    avatarSeedKey: row.peer.agent_id,
                    provider: row.peer.provider,
                    modelLabel: row.modelLabel,
                    effortLabel: row.effortLabel,
                    agentLabel: row.agentLabel,
                    secondaryMeta: row.aliasesMeta,
                    onItemClick: () => {
                      this.store.openPeerPanelTab(
                        this.tab.hostId,
                        row.peer,
                        this.tab.parentHostId ?? this.tab.hostId
                      );
                    }
                  })
                }
              },
              (row: OrbitScopedPeerRowProjection): string => row.stableKey
            )
          }

          // Spawn Workspace Peer form (P1-8)
          ListItem() {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text($r('app.string.peers_spawn_title'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Bold))

              TextInput({ placeholder: $r('app.string.peers_handle_placeholder'), text: this.peerHandleDraft })
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

              TextArea({ placeholder: $r('app.string.peers_prompt_placeholder'), text: this.peerPromptDraft })
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

              Button($r('app.string.board_spawn_peer_action'), { type: ButtonType.Capsule, stateEffect: true })
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
              Text($r('app.string.board_recent_activity'))
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
