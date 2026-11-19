import {
  HighlightedLineDto,
  HighlightedTokenDto,
  InlineSpanDto,
  RichBlockDto,
  TableCellDto,
  TableRowDto
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitThemePalette,
  OrbitTokens,
  orbitSpanBodyFont,
  orbitSpanMonoFont,
  orbitSpanTitleFont,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';

function adaptSyntaxColor(hex: string, palette: OrbitThemePalette): string {
  if (palette.isDark || hex.length !== 7 || !hex.startsWith('#')) {
    return hex;
  }
  const upper = hex.toUpperCase();
  if (upper === '#E2E8F0' || upper === '#C0C5CE' || upper === '#EFF1F5' || upper === '#DFE1E8') {
    return palette.fgPrimary;
  }
  if (upper === '#A3BE8C') {
    return '#15803D';
  }
  if (upper === '#EBCB8B') {
    return '#B45309';
  }
  if (upper === '#8FA1B3' || upper === '#96B5B4') {
    return '#1D4ED8';
  }
  if (upper === '#B48EAD') {
    return '#7E22CE';
  }
  if (upper === '#BF616A' || upper === '#D08770') {
    return '#B91C1C';
  }
  return hex;
}

interface ExtractedGfmTable {
  headers: string[];
  rows: string[][];
}

interface NormalizedRenderBlock {
  key: string;
  isTable: boolean;
  tableHeaders: string[];
  tableRows: string[][];
  block: RichBlockDto | null;
  plainFallback: string;
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

@ComponentV2
export struct MarkdownTableBox {
  @Param headers: string[] = [];
  @Param rows: string[][] = [];

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private columnCount(): number {
    let maxCols = this.headers.length;
    for (let i = 0; i < this.rows.length; i++) {
      if (this.rows[i].length > maxCols) {
        maxCols = this.rows[i].length;
      }
    }
    return maxCols;
  }

  private columnIndices(): number[] {
    const count = this.columnCount();
    const out: number[] = [];
    for (let i = 0; i < count; i++) {
      out.push(i);
    }
    return out;
  }

  private columnWidthVp(colIdx: number): number {
    let maxChars = colIdx < this.headers.length ? this.headers[colIdx].length : 4;
    for (let r = 0; r < this.rows.length; r++) {
      if (colIdx < this.rows[r].length && this.rows[r][colIdx].length > maxChars) {
        maxChars = this.rows[r][colIdx].length;
      }
    }
    const rawVp = Math.ceil((maxChars * 7.8 + 28) / 4) * 4;
    return Math.min(264, Math.max(96, rawVp));
  }

  build() {
    Column() {
      Scroll() {
        Column() {
          if (this.headers.length > 0) {
            Row() {
              ForEach(
                this.columnIndices(),
                (cIdx: number) => {
                  Row() {
                    Text(cIdx < this.headers.length ? this.headers[cIdx] : '')
                      .attributeModifier(
                        orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold)
                      )
                  }
                  .width(this.columnWidthVp(cIdx))
                  .padding({
                    left: OrbitTokens.SPACE_MD,
                    right: OrbitTokens.SPACE_MD,
                    top: OrbitTokens.SPACE_SM,
                    bottom: OrbitTokens.SPACE_SM
                  })
                  .border({
                    width: {
                      right: cIdx < this.columnCount() - 1 ? OrbitTokens.HAIRLINE : 0
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
                  this.columnIndices(),
                  (cIdx: number) => {
                    Row() {
                      Text(cIdx < rowCells.length ? rowCells[cIdx] : '')
                        .attributeModifier(
                          orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Normal)
                        )
                    }
                    .width(this.columnWidthVp(cIdx))
                    .padding({
                      left: OrbitTokens.SPACE_MD,
                      right: OrbitTokens.SPACE_MD,
                      top: OrbitTokens.SPACE_SM,
                      bottom: OrbitTokens.SPACE_SM
                    })
                    .border({
                      width: {
                        right: cIdx < this.columnCount() - 1 ? OrbitTokens.HAIRLINE : 0
                      },
                      color: this.store.palette.hairline
                    })
                    .alignItems(VerticalAlign.Top)
                  },
                  (cIdx: number) => `td_${rIdx}_${cIdx}`
                )
              }
              .backgroundColor(
                rIdx % 2 === 1 ? this.store.palette.elevated : this.store.palette.canvas
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
    .borderRadius(OrbitTokens.CARD_RADIUS)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.palette.hairline
    })
    .clip(true)
  }
}

@ComponentV2
export struct CodeHighlightBox {
  @Param language: string = '';
  @Param rawCode: string = '';
  @Param lines: HighlightedLineDto[] = [];
  @Local copied: boolean = false;
  @Local showLineNumbers: boolean = true;

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private displayedLineCount(): number {
    if (this.lines.length > 0) {
      return this.lines.length;
    }
    if (this.rawCode.length > 0) {
      return this.rawCode.split('\n').length;
    }
    return 0;
  }

  build() {
    Column() {
      // Code Block Header
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text(this.language.trim().length > 0 ? this.language.trim() : 'code')
          .attributeModifier(orbitTextMono(this.store.palette.fgSecondary, FontWeight.Medium))

        if (this.displayedLineCount() >= 8) {
          Text(`${this.displayedLineCount()} lines`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Normal))
        }

        Blank()

        Stack({ alignContent: Alignment.Center }) {
          Row() {
            Text('#')
              .attributeModifier(
                orbitTextMono(
                  this.showLineNumbers
                    ? this.store.palette.accentPrimary
                    : this.store.palette.fgMuted,
                  FontWeight.Bold
                )
              )
          }
          .height(28)
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
          .backgroundColor(
            this.showLineNumbers ? this.store.palette.accentPrimarySoft : Color.Transparent
          )
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .alignItems(VerticalAlign.Center)
        }
        .width(OrbitTokens.HIT_MIN)
        .height(OrbitTokens.HIT_MIN)
        .accessibilityText(this.showLineNumbers ? 'Hide line numbers' : 'Show line numbers')
        .onClick(() => {
          this.showLineNumbers = !this.showLineNumbers;
          this.store.triggerHaptic(6);
        })

        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(this.copied ? 'Copied' : 'Copy')
            .attributeModifier(
              orbitTextMeta(
                this.copied ? this.store.palette.statusDirect : this.store.palette.fgSecondary,
                FontWeight.Medium
              )
            )
        }
        .height(28)
        .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
        .borderRadius(OrbitTokens.PILL_RADIUS)
        .alignItems(VerticalAlign.Center)
        .onClick(() => {
          this.store.copyToClipboard(this.rawCode, 'Copied code block');
          this.copied = true;
          setTimeout(() => {
            this.copied = false;
          }, 1800);
        })
      }
      .width('100%')
      .height(36)
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_XS })
      .alignItems(VerticalAlign.Center)
      .border({
        width: { bottom: OrbitTokens.HAIRLINE },
        color: this.store.palette.hairline
      })

      // Syntax-Highlighted Scrollable Lines
      Scroll() {
        Column({ space: 2 }) {
          if (this.lines.length > 0) {
            ForEach(
              this.lines,
              (ln: HighlightedLineDto, lIdx: number) => {
                Text() {
                  Span(
                    this.showLineNumbers
                      ? `${(ln.line_number > 0 ? ln.line_number : lIdx + 1).toString().padStart(3, ' ')}  `
                      : ''
                  )
                    .font(orbitSpanMonoFont())
                    .fontColor(this.store.palette.fgMuted)
                  ForEach(
                    ln.tokens,
                    (tok: HighlightedTokenDto) => {
                      Span(tok.text)
                        .font(
                          orbitSpanMonoFont(
                            tok.bold ? FontWeight.Bold : FontWeight.Normal,
                            tok.italic
                          )
                        )
                        .fontColor(adaptSyntaxColor(tok.fg_hex, this.store.palette))
                    },
                    (_tok: HighlightedTokenDto, tIdx: number) => `${ln.line_number}_${tIdx}`
                  )
                }
                .lineHeight(OrbitTokens.LH_META)
              },
              (ln: HighlightedLineDto, lIdx: number) => `${ln.line_number}_${lIdx}`
            )
          } else if (this.rawCode.length > 0) {
            ForEach(
              this.rawCode.split('\n'),
              (rawLine: string, lIdx: number) => {
                Text() {
                  Span(
                    this.showLineNumbers
                      ? `${(lIdx + 1).toString().padStart(3, ' ')}  `
                      : ''
                  )
                    .font(orbitSpanMonoFont())
                    .fontColor(this.store.palette.fgMuted)
                  Span(rawLine)
                    .font(orbitSpanMonoFont())
                    .fontColor(this.store.palette.fgPrimary)
                }
                .lineHeight(OrbitTokens.LH_META)
              },
              (_rawLine: string, lIdx: number) => `raw_${lIdx}`
            )
          }
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
    .borderRadius(OrbitTokens.CARD_RADIUS)
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

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  @Builder
  private renderInlineSpans(spans: InlineSpanDto[], isHeading: boolean, defaultColor: string) {
    Text() {
      ForEach(
        spans,
        (sp: InlineSpanDto) => {
          if (sp.inline_code) {
            Span(` ${sp.text} `)
              .font(orbitSpanMonoFont(FontWeight.Medium))
              .fontColor(this.store.palette.accentPrimary)
              .textBackgroundStyle({
                color: this.store.palette.subtleSurface,
                radius: OrbitTokens.SPACE_XS
              })
          } else if (isHeading) {
            Span(sp.text)
              .font(orbitSpanTitleFont(FontWeight.Bold))
              .fontColor(defaultColor)
          } else if (sp.link_url !== null && sp.link_url.length > 0) {
            Span(sp.text)
              .font(orbitSpanBodyFont(FontWeight.Medium, sp.italic))
              .fontColor(this.store.palette.accentPrimary)
              .decoration({
                type: TextDecorationType.Underline,
                color: this.store.palette.accentPrimary
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
    .lineHeight(isHeading ? OrbitTokens.LH_TITLE : OrbitTokens.LH_BODY)
    .width('100%')
  }

  private normalizedBlocks(): NormalizedRenderBlock[] {
    const out: NormalizedRenderBlock[] = [];
    const gfmTables = extractGfmTablesFromMarkdown(this.fallbackText);
    let gfmCursor = 0;

    if (this.blocks.length === 0 && this.fallbackText.length > 0) {
      if (gfmTables.length === 0) {
        out.push({
          key: 'fb_plain_0',
          isTable: false,
          tableHeaders: [],
          tableRows: [],
          block: null,
          plainFallback: this.fallbackText
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
          if (textBuf.join('\n').trim().length > 0) {
            out.push({
              key: `fb_txt_${i}`,
              isTable: false,
              tableHeaders: [],
              tableRows: [],
              block: null,
              plainFallback: textBuf.join('\n').trim()
            });
            textBuf = [];
          }
          const tbl = gfmTables[gfmCursor];
          gfmCursor++;
          out.push({
            key: `fb_tbl_${i}`,
            isTable: true,
            tableHeaders: tbl.headers,
            tableRows: tbl.rows,
            block: null,
            plainFallback: ''
          });
          i += 2 + tbl.rows.length;
          continue;
        }
        textBuf.push(lines[i]);
        i++;
      }
      if (textBuf.join('\n').trim().length > 0) {
        out.push({
          key: `fb_txt_end`,
          isTable: false,
          tableHeaders: [],
          tableRows: [],
          block: null,
          plainFallback: textBuf.join('\n').trim()
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
          isTable: true,
          tableHeaders: headers,
          tableRows: rows,
          block: null,
          plainFallback: ''
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
          isTable: true,
          tableHeaders: parsedTable.headers,
          tableRows: parsedTable.rows,
          block: null,
          plainFallback: ''
        });
        idx = next;
        continue;
      }
      out.push({
        key: `${blk.kind}_${idx}_${blk.raw_code?.length ?? blk.spans?.length ?? 0}`,
        isTable: false,
        tableHeaders: [],
        tableRows: [],
        block: blk,
        plainFallback: ''
      });
      idx++;
    }
    return out;
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      ForEach(
        this.normalizedBlocks(),
        (entry: NormalizedRenderBlock, bIdx: number) => {
          if (entry.isTable) {
            MarkdownTableBox({
              headers: entry.tableHeaders,
              rows: entry.tableRows
            })
          } else if (entry.block === null && entry.plainFallback.length > 0) {
            Text(entry.plainFallback)
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Normal))
              .lineHeight(OrbitTokens.LH_BODY)
              .width('100%')
          } else if (entry.block !== null) {
            if (entry.block.kind === 'code_block') {
              CodeHighlightBox({
                language: entry.block.language ?? '',
                rawCode: entry.block.raw_code ?? '',
                lines: entry.block.lines ?? []
              })
            } else if (entry.block.kind === 'heading' && entry.block.spans !== undefined) {
              Column() {
                this.renderInlineSpans(
                  entry.block.spans,
                  (entry.block.level ?? 2) <= 2,
                  this.store.palette.fgPrimary
                )
              }
              .width('100%')
              .padding({ top: bIdx > 0 ? OrbitTokens.SPACE_XS : 0 })
            } else if (entry.block.kind === 'list_item' && entry.block.spans !== undefined) {
              Row({ space: OrbitTokens.SPACE_SM }) {
                if (entry.block.checked !== undefined && entry.block.checked !== null) {
                  Text(entry.block.checked ? '☑' : '☐')
                    .attributeModifier(
                      orbitTextSubhead(
                        entry.block.checked
                          ? this.store.palette.statusDirect
                          : this.store.palette.fgMuted,
                        FontWeight.Medium
                      )
                    )
                    .margin({ top: 2 })
                } else {
                  Text(entry.block.ordered === true ? `${entry.block.index ?? 1}.` : '•')
                    .attributeModifier(
                      orbitTextSubhead(this.store.palette.fgSecondary, FontWeight.Medium)
                    )
                    .margin({ top: 2 })
                }
                Column() {
                  this.renderInlineSpans(entry.block.spans, false, this.store.palette.fgPrimary)
                }
                .layoutWeight(1)
              }
              .width('100%')
              .alignItems(VerticalAlign.Top)
            } else if (entry.block.kind === 'block_quote' && entry.block.spans !== undefined) {
              Row() {
                Column() {
                  this.renderInlineSpans(entry.block.spans, false, this.store.palette.fgSecondary)
                }
                .layoutWeight(1)
                .padding({ left: OrbitTokens.SPACE_MD, top: 2, bottom: 2 })
                .border({
                  width: { left: 2 },
                  color: this.store.palette.accentPrimary
                })
              }
              .width('100%')
            } else if (entry.block.kind === 'horizontal_rule') {
              Divider()
                .strokeWidth(OrbitTokens.HAIRLINE)
                .color(this.store.palette.hairline)
                .margin({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
            } else if (entry.block.spans !== undefined) {
              this.renderInlineSpans(entry.block.spans, false, this.store.palette.fgPrimary)
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
