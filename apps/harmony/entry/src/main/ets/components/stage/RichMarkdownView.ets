import {
  HighlightedLineDto,
  HighlightedTokenDto,
  InlineSpanDto,
  RichBlockDto,
  TableCellDto,
  TableRowDto
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { openExternalWebUrl } from '../../model/OrbitDeviceBridge';
import {
  OrbitThemePalette,
  OrbitTokens,
  orbitSpanBodyFont,
  orbitSpanHeadingFont,
  orbitSpanMonoFont,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitCopyIconButton } from '../common/OrbitCommonControls';
import { DiagramBlockView } from './DiagramBlockView';

const QUOTE_MENU_ID: string = 'orbit_quote';

function adaptSyntaxColor(hex: string, palette: OrbitThemePalette): ResourceColor {
  if (hex.length !== 7 || !hex.startsWith('#')) {
    return palette.fgPrimary;
  }
  const upper = hex.toUpperCase();
  if (upper === '#E2E8F0' || upper === '#C0C5CE' || upper === '#EFF1F5' || upper === '#DFE1E8') {
    return palette.fgPrimary;
  }
  if (upper === '#A3BE8C') {
    return $r('app.color.orbit_syntax_string');
  }
  if (upper === '#EBCB8B') {
    return $r('app.color.orbit_syntax_number');
  }
  if (upper === '#8FA1B3' || upper === '#96B5B4') {
    return $r('app.color.orbit_syntax_keyword');
  }
  if (upper === '#B48EAD') {
    return $r('app.color.orbit_syntax_type');
  }
  if (upper === '#BF616A' || upper === '#D08770') {
    return $r('app.color.orbit_syntax_error');
  }
  return hex;
}

interface ExtractedGfmTable {
  headers: string[];
  rows: string[][];
}

interface FlattenedProseSpan {
  key: string;
  text: string;
  headingLevel: number;
  bold: boolean;
  italic: boolean;
  strikethrough: boolean;
  inlineCode: boolean;
  linkUrl: string;
  isListMarker: boolean;
}

interface NormalizedRenderBlock {
  key: string;
  kind: string; // 'prose_run' | 'task_item' | 'code_block' | 'block_quote' | 'horizontal_rule' | 'table'
  tableHeaders: string[];
  tableRows: string[][];
  proseSpans: FlattenedProseSpan[];
  plainText: string;
  block: RichBlockDto | null;
}

function splitGfmPipeRow(line: string): string[] {
  let clean = line.trim();
  if (clean.startsWith('|')) {
    clean = clean.substring(1);
  }
  if (clean.endsWith('|')) {
    clean = clean.substring(0, clean.length - 1);
  }
  const rawParts = clean.split('|');
  const cells: string[] = [];
  for (let i = 0; i < rawParts.length; i++) {
    cells.push(rawParts[i].trim());
  }
  return cells;
}

function isGfmDelimiterRow(line: string): boolean {
  const trimmed = line.trim();
  if (trimmed.indexOf('|') < 0 || trimmed.indexOf('-') < 0) {
    return false;
  }
  const cells = splitGfmPipeRow(trimmed);
  if (cells.length === 0) {
    return false;
  }
  for (let i = 0; i < cells.length; i++) {
    if (!/^:?-{2,}:?$/.test(cells[i])) {
      return false;
    }
  }
  return true;
}

function extractGfmTablesFromMarkdown(markdown: string): ExtractedGfmTable[] {
  if (markdown.indexOf('|') < 0) {
    return [];
  }
  const lines = markdown.split('\n');
  const tables: ExtractedGfmTable[] = [];
  let inFence = false;
  let i = 0;
  while (i < lines.length) {
    const trimmed = lines[i].trim();
    if (trimmed.startsWith('```') || trimmed.startsWith('~~~')) {
      inFence = !inFence;
      i++;
      continue;
    }
    if (
      !inFence &&
      i + 1 < lines.length &&
      trimmed.indexOf('|') >= 0 &&
      isGfmDelimiterRow(lines[i + 1])
    ) {
      const headers = splitGfmPipeRow(trimmed);
      const rows: string[][] = [];
      let r = i + 2;
      while (r < lines.length) {
        const rowLine = lines[r].trim();
        if (
          rowLine.length === 0 ||
          rowLine.indexOf('|') < 0 ||
          rowLine.startsWith('```') ||
          rowLine.startsWith('~~~')
        ) {
          break;
        }
        rows.push(splitGfmPipeRow(rowLine));
        r++;
      }
      if (headers.length > 0) {
        tables.push({ headers: headers, rows: rows });
      }
      i = r;
      continue;
    }
    i++;
  }
  return tables;
}

function spansPlainText(spans: InlineSpanDto[] | undefined): string {
  if (spans === undefined || spans.length === 0) {
    return '';
  }
  let out = '';
  for (let i = 0; i < spans.length; i++) {
    out += spans[i].text;
  }
  return out;
}

function isPipeTableParagraphBlock(blk: RichBlockDto): boolean {
  if (blk.kind !== 'paragraph' || blk.spans === undefined) {
    return false;
  }
  return spansPlainText(blk.spans).indexOf('  │  ') >= 0;
}

function parsePipeCellsFromParagraphs(run: RichBlockDto[]): ExtractedGfmTable {
  const rowCells: string[][] = [];
  for (let i = 0; i < run.length; i++) {
    const raw = spansPlainText(run[i].spans);
    const parts = raw.split('  │  ');
    const cleaned: string[] = [];
    for (let j = 0; j < parts.length; j++) {
      const trimmed = parts[j].trim();
      if (j === parts.length - 1 && trimmed.length === 0) {
        continue;
      }
      cleaned.push(trimmed);
    }
    if (cleaned.length > 0) {
      rowCells.push(cleaned);
    }
  }
  if (rowCells.length === 0) {
    return { headers: [], rows: [] };
  }
  if (rowCells.length >= 2) {
    const colCount = rowCells[1].length;
    if (colCount > 0 && rowCells[0].length === colCount * 2) {
      const headers = rowCells[0].slice(0, colCount);
      const firstRow = rowCells[0].slice(colCount);
      const rows: string[][] = [firstRow];
      for (let r = 1; r < rowCells.length; r++) {
        rows.push(rowCells[r]);
      }
      return { headers: headers, rows: rows };
    }
  } else if (rowCells[0].length >= 4 && rowCells[0].length % 2 === 0) {
    const half = rowCells[0].length / 2;
    return {
      headers: rowCells[0].slice(0, half),
      rows: [rowCells[0].slice(half)]
    };
  }
  return {
    headers: rowCells[0],
    rows: rowCells.slice(1)
  };
}

function isMergeableProseBlock(blk: RichBlockDto): boolean {
  if (blk.kind === 'heading' || blk.kind === 'paragraph') {
    return !isPipeTableParagraphBlock(blk);
  }
  if (blk.kind === 'list_item') {
    return blk.checked === undefined || blk.checked === null;
  }
  return false;
}

@ComponentV2
export struct MarkdownTableBox {
  @Param headers: string[] = [];
  @Param rows: string[][] = [];

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get colCount(): number {
    let maxCols = this.headers.length;
    for (let i = 0; i < this.rows.length; i++) {
      if (this.rows[i].length > maxCols) {
        maxCols = this.rows[i].length;
      }
    }
    return maxCols;
  }

  @Computed
  get colIndices(): number[] {
    const out: number[] = [];
    for (let i = 0; i < this.colCount; i++) {
      out.push(i);
    }
    return out;
  }

  @Computed
  get colWidths(): number[] {
    const widths: number[] = [];
    for (let c = 0; c < this.colCount; c++) {
      let maxChars = c < this.headers.length ? this.headers[c].length : 4;
      for (let r = 0; r < this.rows.length; r++) {
        if (c < this.rows[r].length && this.rows[r][c].length > maxChars) {
          maxChars = this.rows[r][c].length;
        }
      }
      const rawVp = Math.ceil((maxChars * 7.8 + 28) / 4) * 4;
      widths.push(Math.min(264, Math.max(96, rawVp)));
    }
    return widths;
  }

  build() {
    Column() {
      Scroll() {
        Column() {
          if (this.headers.length > 0) {
            Row() {
              ForEach(
                this.colIndices,
                (cIdx: number) => {
                  Row() {
                    Text(cIdx < this.headers.length ? this.headers[cIdx] : '')
                      .attributeModifier(
                        orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold)
                      )
                      .copyOption(CopyOptions.LocalDevice)
                  }
                  .width(cIdx < this.colWidths.length ? this.colWidths[cIdx] : 112)
                  .padding({
                    left: OrbitTokens.SPACE_MD,
                    right: OrbitTokens.SPACE_MD,
                    top: OrbitTokens.SPACE_SM,
                    bottom: OrbitTokens.SPACE_SM
                  })
                  .border({
                    width: {
                      right: cIdx < this.colCount - 1 ? OrbitTokens.HAIRLINE : 0
                    },
                    color: this.store.palette.hairline
                  })
                  .alignItems(VerticalAlign.Center)
                },
                (cIdx: number) => `th_${cIdx}`
              )
            }
            .backgroundColor(this.store.palette.subtleSurface)
            .border({
              width: { bottom: this.rows.length > 0 ? OrbitTokens.HAIRLINE : 0 },
              color: this.store.palette.hairline
            })
          }

          ForEach(
            this.rows,
            (rowCells: string[], rIdx: number) => {
              Row() {
                ForEach(
                  this.colIndices,
                  (cIdx: number) => {
                    Row() {
                      Text(cIdx < rowCells.length ? rowCells[cIdx] : '')
                        .attributeModifier(
                          orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Normal)
                        )
                        .copyOption(CopyOptions.LocalDevice)
                    }
                    .width(cIdx < this.colWidths.length ? this.colWidths[cIdx] : 112)
                    .padding({
                      left: OrbitTokens.SPACE_MD,
                      right: OrbitTokens.SPACE_MD,
                      top: OrbitTokens.SPACE_SM,
                      bottom: OrbitTokens.SPACE_SM
                    })
                    .border({
                      width: {
                        right: cIdx < this.colCount - 1 ? OrbitTokens.HAIRLINE : 0
                      },
                      color: this.store.palette.hairline
                    })
                    .alignItems(VerticalAlign.Top)
                  },
                  (cIdx: number) => `td_${rIdx}_${cIdx}`
                )
              }
              .backgroundColor(
                rIdx % 2 === 1 ? this.store.palette.listCard : this.store.palette.canvas
              )
              .border({
                width: { bottom: rIdx < this.rows.length - 1 ? OrbitTokens.HAIRLINE : 0 },
                color: this.store.palette.hairline
              })
            },
            (_rowCells: string[], rIdx: number) => `tr_${rIdx}`
          )
        }
        .alignItems(HorizontalAlign.Start)
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Auto)
      .width('100%')
    }
    .width('100%')
    .borderRadius(OrbitTokens.RADIUS_MD)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.palette.hairline
    })
    .clip(true)
  }
}

interface HighlightedRunSpan {
  key: string;
  text: string;
  color: ResourceColor;
  bold: boolean;
  italic: boolean;
}

@ComponentV2
export struct CodeHighlightBox {
  @Param language: string = '';
  @Param rawCode: string = '';
  @Param lines: HighlightedLineDto[] = [];
  @Local showLineNumbers: boolean = true;

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Computed
  get displayedLineCount(): number {
    if (this.lines.length > 0) {
      return this.lines.length;
    }
    if (this.rawCode.length > 0) {
      return this.rawCode.split('\n').length;
    }
    return 0;
  }

  @Computed
  get codeSpans(): HighlightedRunSpan[] {
    const out: HighlightedRunSpan[] = [];
    if (this.lines.length > 0) {
      for (let lIdx = 0; lIdx < this.lines.length; lIdx++) {
        const ln = this.lines[lIdx];
        if (this.showLineNumbers) {
          const lineNo = ln.line_number > 0 ? ln.line_number : lIdx + 1;
          out.push({
            key: `ln_${lIdx}`,
            text: `${lineNo.toString().padStart(3, ' ')}  `,
            color: this.store.palette.fgMuted,
            bold: false,
            italic: false
          });
        }
        for (let tIdx = 0; tIdx < ln.tokens.length; tIdx++) {
          const tok: HighlightedTokenDto = ln.tokens[tIdx];
          out.push({
            key: `tok_${lIdx}_${tIdx}`,
            text: tok.text,
            color: adaptSyntaxColor(tok.fg_hex, this.store.palette),
            bold: tok.bold,
            italic: tok.italic
          });
        }
        if (lIdx < this.lines.length - 1) {
          out.push({
            key: `nl_${lIdx}`,
            text: '\n',
            color: this.store.palette.fgPrimary,
            bold: false,
            italic: false
          });
        }
      }
      return out;
    }

    const rawLines = this.rawCode.split('\n');
    for (let lIdx = 0; lIdx < rawLines.length; lIdx++) {
      if (this.showLineNumbers) {
        out.push({
          key: `rln_${lIdx}`,
          text: `${(lIdx + 1).toString().padStart(3, ' ')}  `,
          color: this.store.palette.fgMuted,
          bold: false,
          italic: false
        });
      }
      out.push({
        key: `raw_${lIdx}`,
        text: rawLines[lIdx],
        color: this.store.palette.fgPrimary,
        bold: false,
        italic: false
      });
      if (lIdx < rawLines.length - 1) {
        out.push({
          key: `rnl_${lIdx}`,
          text: '\n',
          color: this.store.palette.fgPrimary,
          bold: false,
          italic: false
        });
      }
    }
    return out;
  }

  build() {
    Column() {
      // Restrained Code Block Header
      Row({ space: OrbitTokens.SPACE_XS }) {
        Text(this.language.trim().length > 0 ? this.language.trim() : 'text')
          .attributeModifier(orbitTextMono(this.store.palette.fgMuted, FontWeight.Medium))

        if (this.displayedLineCount >= 8) {
          Text(`${this.displayedLineCount} lines`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }

        Blank()

        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Text('#')
            .attributeModifier(
              orbitTextMono(
                this.showLineNumbers
                  ? this.store.palette.fgPrimary
                  : this.store.palette.fgMuted,
                FontWeight.Medium
              )
            )
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .borderRadius(OrbitTokens.RADIUS_XS)
        .backgroundColor(
          this.showLineNumbers ? this.store.palette.subtleSurface : Color.Transparent
        )
        .accessibilityText(this.showLineNumbers ? 'Hide line numbers' : 'Show line numbers')
        .onClick(() => {
          this.showLineNumbers = !this.showLineNumbers;
          this.store.triggerHaptic(6);
        })

        OrbitCopyIconButton({
          content: this.rawCode,
          toastLabel: 'Copied code block',
          accessibilityLabel: 'Copy code',
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

      // Selectable Code Body (multi-line native range selection)
      Scroll() {
        Column() {
          Text() {
            ForEach(
              this.codeSpans,
              (sp: HighlightedRunSpan) => {
                Span(sp.text)
                  .font(
                    orbitSpanMonoFont(
                      sp.bold ? FontWeight.Bold : FontWeight.Normal,
                      sp.italic
                    )
                  )
                  .fontColor(sp.color)
              },
              (sp: HighlightedRunSpan) => sp.key
            )
          }
          .copyOption(CopyOptions.LocalDevice)
        }
        .alignItems(HorizontalAlign.Start)
        .padding(OrbitTokens.SPACE_MD)
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Auto)
      .width('100%')
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

@ComponentV2
export struct RichMarkdownView {
  @Param blocks: RichBlockDto[] = [];
  @Param fallbackText: string = '';
  @Param turnIndex: number = 0;

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private buildEditMenuOptions(fullText: string): EditMenuOptions {
    const store = this.store;
    const turnIdx = this.turnIndex;
    const opts: EditMenuOptions = {
      onCreateMenu(menuItems: Array<TextMenuItem>): Array<TextMenuItem> {
        const quoteItem: TextMenuItem = {
          content: 'Quote',
          id: TextMenuItemId.of(QUOTE_MENU_ID)
        };
        const out: Array<TextMenuItem> = [];
        for (let i = 0; i < menuItems.length; i++) {
          out.push(menuItems[i]);
        }
        out.push(quoteItem);
        return out;
      },
      onMenuItemClick(menuItem: TextMenuItem, textRange: TextRange): boolean {
        if (menuItem.id.equals(TextMenuItemId.of(QUOTE_MENU_ID))) {
          const rawStart = textRange.start ?? 0;
          const rawEnd = textRange.end ?? 0;
          const s = Math.max(0, Math.min(rawStart, rawEnd));
          const e = Math.min(fullText.length, Math.max(rawStart, rawEnd));
          const selected = (e > s ? fullText.substring(s, e) : fullText).trim();
          if (selected.length > 0) {
            store.quoteAssistantReplyIntoComposer(selected, turnIdx);
          }
          return true;
        }
        return false;
      }
    };
    return opts;
  }

  @Builder
  private renderInlineSpans(spans: InlineSpanDto[], defaultColor: ResourceColor) {
    Text() {
      ForEach(
        spans,
        (sp: InlineSpanDto) => {
          if (sp.inline_code) {
            Span(` ${sp.text} `)
              .font(orbitSpanMonoFont(FontWeight.Medium))
              .fontColor(this.store.palette.fgPrimary)
              .textBackgroundStyle({
                color: this.store.palette.subtleSurface,
                radius: OrbitTokens.SPACE_XS
              })
          } else if (sp.link_url !== null && sp.link_url.length > 0) {
            Span(sp.text)
              .font(orbitSpanBodyFont(FontWeight.Medium, sp.italic))
              .fontColor(this.store.palette.accentPrimary)
              .decoration({
                type: TextDecorationType.Underline,
                color: this.store.palette.accentPrimary
              })
              .onClick(() => {
                const url = sp.link_url ?? '';
                if (!openExternalWebUrl(this.getUIContext().getHostContext(), url)) {
                  this.store.copyToClipboard(url, 'Copied link');
                }
              })
          } else if (sp.strikethrough) {
            Span(sp.text)
              .font(orbitSpanBodyFont(FontWeight.Normal, sp.italic))
              .fontColor(this.store.palette.fgMuted)
              .decoration({
                type: TextDecorationType.LineThrough,
                color: this.store.palette.fgMuted
              })
          } else {
            Span(sp.text)
              .font(
                orbitSpanBodyFont(
                  sp.bold ? FontWeight.Bold : FontWeight.Normal,
                  sp.italic
                )
              )
              .fontColor(defaultColor)
          }
        },
        (_sp: InlineSpanDto, sIdx: number) => sIdx.toString()
      )
    }
    .width('100%')
    .copyOption(CopyOptions.LocalDevice)
    .editMenuOptions(this.buildEditMenuOptions(spansPlainText(spans)))
  }

  private buildMergedProseEntry(run: RichBlockDto[], keyIndex: number): NormalizedRenderBlock {
    const proseSpans: FlattenedProseSpan[] = [];
    let plain = '';

    for (let bIdx = 0; bIdx < run.length; bIdx++) {
      const blk = run[bIdx];
      if (bIdx > 0) {
        const prev = run[bIdx - 1];
        const tightList = prev.kind === 'list_item' && blk.kind === 'list_item';
        const sep = tightList ? '\n' : '\n\n';
        proseSpans.push({
          key: `sep_${keyIndex}_${bIdx}`,
          text: sep,
          headingLevel: 0,
          bold: false,
          italic: false,
          strikethrough: false,
          inlineCode: false,
          linkUrl: '',
          isListMarker: false
        });
        plain += sep;
      }

      if (blk.kind === 'list_item') {
        const marker = blk.ordered === true ? `${blk.index ?? 1}.  ` : '•  ';
        proseSpans.push({
          key: `lm_${keyIndex}_${bIdx}`,
          text: marker,
          headingLevel: 0,
          bold: true,
          italic: false,
          strikethrough: false,
          inlineCode: false,
          linkUrl: '',
          isListMarker: true
        });
        plain += marker;
      }

      const hLevel = blk.kind === 'heading' ? (blk.level ?? 2) : 0;
      const spans = blk.spans ?? [];
      for (let sIdx = 0; sIdx < spans.length; sIdx++) {
        const sp = spans[sIdx];
        const segText = sp.inline_code ? ` ${sp.text} ` : sp.text;
        proseSpans.push({
          key: `sp_${keyIndex}_${bIdx}_${sIdx}`,
          text: segText,
          headingLevel: hLevel,
          bold: sp.bold || hLevel > 0,
          italic: sp.italic,
          strikethrough: sp.strikethrough,
          inlineCode: sp.inline_code,
          linkUrl: sp.link_url ?? '',
          isListMarker: false
        });
        plain += sp.text;
      }
    }

    return {
      key: `prose_${keyIndex}_${run.length}_${plain.length}`,
      kind: 'prose_run',
      tableHeaders: [],
      tableRows: [],
      proseSpans: proseSpans,
      plainText: plain,
      block: null
    };
  }

  @Computed
  get normalizedBlockList(): NormalizedRenderBlock[] {
    const out: NormalizedRenderBlock[] = [];
    const gfmTables = extractGfmTablesFromMarkdown(this.fallbackText);
    let gfmCursor = 0;

    if (this.blocks.length === 0 && this.fallbackText.length > 0) {
      if (gfmTables.length === 0) {
        out.push({
          key: 'fb_plain_0',
          kind: 'prose_run',
          tableHeaders: [],
          tableRows: [],
          proseSpans: [{
            key: 'fb_span_0',
            text: this.fallbackText,
            headingLevel: 0,
            bold: false,
            italic: false,
            strikethrough: false,
            inlineCode: false,
            linkUrl: '',
            isListMarker: false
          }],
          plainText: this.fallbackText,
          block: null
        });
        return out;
      }
      const lines = this.fallbackText.split('\n');
      let textBuf: string[] = [];
      let i = 0;
      while (i < lines.length) {
        const trimmed = lines[i].trim();
        if (
          i + 1 < lines.length &&
          trimmed.indexOf('|') >= 0 &&
          isGfmDelimiterRow(lines[i + 1]) &&
          gfmCursor < gfmTables.length
        ) {
          const bufText = textBuf.join('\n').trim();
          if (bufText.length > 0) {
            out.push({
              key: `fb_txt_${i}`,
              kind: 'prose_run',
              tableHeaders: [],
              tableRows: [],
              proseSpans: [{
                key: `fb_sp_${i}`,
                text: bufText,
                headingLevel: 0,
                bold: false,
                italic: false,
                strikethrough: false,
                inlineCode: false,
                linkUrl: '',
                isListMarker: false
              }],
              plainText: bufText,
              block: null
            });
            textBuf = [];
          }
          const tbl = gfmTables[gfmCursor];
          gfmCursor++;
          out.push({
            key: `fb_tbl_${i}`,
            kind: 'table',
            tableHeaders: tbl.headers,
            tableRows: tbl.rows,
            proseSpans: [],
            plainText: '',
            block: null
          });
          i += 2 + tbl.rows.length;
          continue;
        }
        textBuf.push(lines[i]);
        i++;
      }
      const rem = textBuf.join('\n').trim();
      if (rem.length > 0) {
        out.push({
          key: 'fb_txt_end',
          kind: 'prose_run',
          tableHeaders: [],
          tableRows: [],
          proseSpans: [{
            key: 'fb_sp_end',
            text: rem,
            headingLevel: 0,
            bold: false,
            italic: false,
            strikethrough: false,
            inlineCode: false,
            linkUrl: '',
            isListMarker: false
          }],
          plainText: rem,
          block: null
        });
      }
      return out;
    }

    let idx = 0;
    while (idx < this.blocks.length) {
      const blk = this.blocks[idx];
      if (blk.kind === 'table') {
        const headers: string[] = (blk.headers ?? []).map((h: string | TableCellDto) =>
          typeof h === 'string' ? h : spansPlainText(h.spans)
        );
        const rows: string[][] = (blk.rows ?? []).map((r: string[] | TableRowDto) =>
          Array.isArray(r)
            ? r
            : r.cells.map((c: TableCellDto) => spansPlainText(c.spans))
        );
        out.push({
          key: `tbl_${idx}`,
          kind: 'table',
          tableHeaders: headers,
          tableRows: rows,
          proseSpans: [],
          plainText: '',
          block: null
        });
        idx++;
        continue;
      }
      if (isPipeTableParagraphBlock(blk)) {
        const run: RichBlockDto[] = [blk];
        let next = idx + 1;
        while (next < this.blocks.length && isPipeTableParagraphBlock(this.blocks[next])) {
          run.push(this.blocks[next]);
          next++;
        }
        const parsedTable = gfmCursor < gfmTables.length
          ? gfmTables[gfmCursor]
          : parsePipeCellsFromParagraphs(run);
        if (gfmCursor < gfmTables.length) {
          gfmCursor++;
        }
        out.push({
          key: `ptbl_${idx}_${run.length}`,
          kind: 'table',
          tableHeaders: parsedTable.headers,
          tableRows: parsedTable.rows,
          proseSpans: [],
          plainText: '',
          block: null
        });
        idx = next;
        continue;
      }
      if (isMergeableProseBlock(blk)) {
        const proseRun: RichBlockDto[] = [blk];
        let next = idx + 1;
        while (next < this.blocks.length && isMergeableProseBlock(this.blocks[next])) {
          proseRun.push(this.blocks[next]);
          next++;
        }
        out.push(this.buildMergedProseEntry(proseRun, idx));
        idx = next;
        continue;
      }

      out.push({
        key: `${blk.kind}_${idx}_${blk.raw_code?.length ?? blk.spans?.length ?? 0}`,
        kind: blk.kind === 'list_item' ? 'task_item' : blk.kind,
        tableHeaders: [],
        tableRows: [],
        proseSpans: [],
        plainText: spansPlainText(blk.spans),
        block: blk
      });
      idx++;
    }
    return out;
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      ForEach(
        this.normalizedBlockList,
        (entry: NormalizedRenderBlock) => {
          if (entry.kind === 'table') {
            MarkdownTableBox({
              headers: entry.tableHeaders,
              rows: entry.tableRows
            })
          } else if (entry.kind === 'prose_run') {
            Text() {
              ForEach(
                entry.proseSpans,
                (sp: FlattenedProseSpan) => {
                  if (sp.inlineCode) {
                    Span(sp.text)
                      .font(orbitSpanMonoFont(FontWeight.Medium))
                      .fontColor(this.store.palette.fgPrimary)
                      .textBackgroundStyle({
                        color: this.store.palette.subtleSurface,
                        radius: OrbitTokens.SPACE_XS
                      })
                  } else if (sp.headingLevel > 0) {
                    Span(sp.text)
                      .font(orbitSpanHeadingFont(sp.headingLevel))
                      .fontColor(this.store.palette.fgPrimary)
                  } else if (sp.isListMarker) {
                    Span(sp.text)
                      .font(orbitSpanBodyFont(FontWeight.Medium))
                      .fontColor(this.store.palette.fgSecondary)
                  } else if (sp.linkUrl.length > 0) {
                    Span(sp.text)
                      .font(orbitSpanBodyFont(FontWeight.Medium, sp.italic))
                      .fontColor(this.store.palette.accentPrimary)
                      .decoration({
                        type: TextDecorationType.Underline,
                        color: this.store.palette.accentPrimary
                      })
                      .onClick(() => {
                        if (!openExternalWebUrl(this.getUIContext().getHostContext(), sp.linkUrl)) {
                          this.store.copyToClipboard(sp.linkUrl, 'Copied link');
                        }
                      })
                  } else if (sp.strikethrough) {
                    Span(sp.text)
                      .font(orbitSpanBodyFont(FontWeight.Normal, sp.italic))
                      .fontColor(this.store.palette.fgMuted)
                      .decoration({
                        type: TextDecorationType.LineThrough,
                        color: this.store.palette.fgMuted
                      })
                  } else {
                    Span(sp.text)
                      .font(
                        orbitSpanBodyFont(
                          sp.bold ? FontWeight.Bold : FontWeight.Normal,
                          sp.italic
                        )
                      )
                      .fontColor(this.store.palette.fgPrimary)
                  }
                },
                (sp: FlattenedProseSpan) => sp.key
              )
            }
            .width('100%')
            .copyOption(CopyOptions.LocalDevice)
            .editMenuOptions(this.buildEditMenuOptions(entry.plainText))
          } else if (entry.block !== null) {
            if (entry.block.kind === 'code_block') {
              if (
                this.store.detectDiagramKind(
                  entry.block.language ?? '',
                  entry.block.raw_code ?? ''
                ) !== null
              ) {
                DiagramBlockView({
                  diagramKind:
                    this.store.detectDiagramKind(
                      entry.block.language ?? '',
                      entry.block.raw_code ?? ''
                    ) ?? 'mermaid',
                  diagramLanguage: entry.block.language ?? '',
                  diagramSource: entry.block.raw_code ?? '',
                  diagramLines: entry.block.lines ?? []
                })
              } else {
                CodeHighlightBox({
                  language: entry.block.language ?? '',
                  rawCode: entry.block.raw_code ?? '',
                  lines: entry.block.lines ?? []
                })
              }
            } else if (entry.kind === 'task_item' && entry.block.spans !== undefined) {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Stack({ alignContent: Alignment.Center }) {
                  if (entry.block.checked === true) {
                    SymbolGlyph($r('sys.symbol.checkmark_square'))
                      .fontSize(15)
                      .fontColor([this.store.palette.statusDirect])
                  } else {
                    Row()
                      .width(14)
                      .height(14)
                      .borderRadius(3)
                      .border({ width: 1, color: this.store.palette.fgMuted })
                  }
                }
                .margin({ top: 3 })

                Column() {
                  this.renderInlineSpans(
                    entry.block.spans,
                    entry.block.checked === true
                      ? this.store.palette.fgMuted
                      : this.store.palette.fgPrimary
                  )
                }
                .layoutWeight(1)
              }
              .width('100%')
              .alignItems(VerticalAlign.Top)
            } else if (entry.block.kind === 'block_quote' && entry.block.spans !== undefined) {
              Row() {
                Column() {
                  this.renderInlineSpans(entry.block.spans, this.store.palette.fgSecondary)
                }
                .layoutWeight(1)
                .padding({ left: OrbitTokens.SPACE_MD, top: 2, bottom: 2 })
                .border({
                  width: { left: 2 },
                  color: this.store.palette.hairline
                })
              }
              .width('100%')
            } else if (entry.block.kind === 'horizontal_rule') {
              Divider()
                .strokeWidth(OrbitTokens.HAIRLINE)
                .color(this.store.palette.hairline)
                .margin({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
            } else if (entry.block.spans !== undefined) {
              this.renderInlineSpans(entry.block.spans, this.store.palette.fgPrimary)
            }
          }
        },
        (entry: NormalizedRenderBlock) => entry.key
      )
    }
    .alignItems(HorizontalAlign.Start)
    .width('100%')
  }
}
