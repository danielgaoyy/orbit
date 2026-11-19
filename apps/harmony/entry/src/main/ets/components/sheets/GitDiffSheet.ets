import {
  DiffFileDto,
  DiffLineDto,
  FlatDiffRowDto,
  basenameOfPath,
  isAutoCollapsedDiffPath
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputSubhead,
  orbitSpanMonoFont,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';

const INITIAL_DIFF_LINE_CAP: number = 120;

@ComponentV2
export struct GitDiffSheet {
  @Param isEmbeddedPane: boolean = false;
  @Local fileFilterQuery: string = '';
  @Local manuallyExpandedPaths: string[] = [];
  @Local manuallyCollapsedPaths: string[] = [];
  @Local uncappedPaths: string[] = [];
  @Local isRefreshing: boolean = false;

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private handlePullRefresh(): void {
    this.isRefreshing = true;
    this.store.pullRefreshGitReview();
    setTimeout(() => {
      this.isRefreshing = false;
    }, 450);
  }

  private displayedDiffFiles(): DiffFileDto[] {
    if (this.store.gitDiffFiles.length > 0) {
      return this.store.gitDiffFiles;
    }
    return this.store.sessionDiffs;
  }

  private filteredDiffFiles(): DiffFileDto[] {
    const all = this.displayedDiffFiles();
    const q = this.fileFilterQuery.trim().toLowerCase();
    if (q.length === 0) {
      return all;
    }
    const matched: DiffFileDto[] = [];
    for (let i = 0; i < all.length; i++) {
      const df = all[i];
      if (
        df.path.toLowerCase().indexOf(q) >= 0 ||
        basenameOfPath(df.path).toLowerCase().indexOf(q) >= 0
      ) {
        matched.push(df);
      }
    }
    return matched;
  }

  private areAllCodeFilesCollapsed(): boolean {
    const files = this.filteredDiffFiles();
    if (files.length === 0) {
      return false;
    }
    let hasCodeFile = false;
    for (let i = 0; i < files.length; i++) {
      const p = files[i].path;
      if (!isAutoCollapsedDiffPath(p)) {
        hasCodeFile = true;
        if (!this.isFileCollapsed(p)) {
          return false;
        }
      }
    }
    if (!hasCodeFile) {
      for (let i = 0; i < files.length; i++) {
        if (!this.isFileCollapsed(files[i].path)) {
          return false;
        }
      }
    }
    return true;
  }

  private toggleCollapseExpandCode(): void {
    if (this.areAllCodeFilesCollapsed()) {
      // Expand Code: reopen all source code files while keeping generated/lock files collapsed
      this.manuallyCollapsedPaths = [];
    } else {
      // Collapse All: collapse every modified file cleanly
      const all = this.displayedDiffFiles();
      const collapsed: string[] = [];
      for (let i = 0; i < all.length; i++) {
        if (!isAutoCollapsedDiffPath(all[i].path)) {
          collapsed.push(all[i].path);
        }
      }
      this.manuallyExpandedPaths = [];
      this.manuallyCollapsedPaths = collapsed;
    }
    this.store.triggerHaptic(8);
  }

  private isFileCollapsed(filePath: string): boolean {
    if (isAutoCollapsedDiffPath(filePath)) {
      return this.manuallyExpandedPaths.indexOf(filePath) < 0;
    }
    return this.manuallyCollapsedPaths.indexOf(filePath) >= 0;
  }

  private toggleFileCollapsed(filePath: string): void {
    if (isAutoCollapsedDiffPath(filePath)) {
      const idx = this.manuallyExpandedPaths.indexOf(filePath);
      const next: string[] = [];
      for (let i = 0; i < this.manuallyExpandedPaths.length; i++) {
        if (i !== idx) {
          next.push(this.manuallyExpandedPaths[i]);
        }
      }
      if (idx < 0) {
        next.push(filePath);
      }
      this.manuallyExpandedPaths = next;
    } else {
      const idx = this.manuallyCollapsedPaths.indexOf(filePath);
      const next: string[] = [];
      for (let i = 0; i < this.manuallyCollapsedPaths.length; i++) {
        if (i !== idx) {
          next.push(this.manuallyCollapsedPaths[i]);
        }
      }
      if (idx < 0) {
        next.push(filePath);
      }
      this.manuallyCollapsedPaths = next;
    }
    this.store.triggerHaptic(6);
  }

  private expandAllLinesForFile(filePath: string): void {
    if (this.uncappedPaths.indexOf(filePath) >= 0) {
      return;
    }
    const next: string[] = [];
    for (let i = 0; i < this.uncappedPaths.length; i++) {
      next.push(this.uncappedPaths[i]);
    }
    next.push(filePath);
    this.uncappedPaths = next;
    this.store.triggerHaptic(8);
  }

  private flatDiffRows(): FlatDiffRowDto[] {
    const files = this.filteredDiffFiles();
    const rows: FlatDiffRowDto[] = [];
    for (let fIdx = 0; fIdx < files.length; fIdx++) {
      const df: DiffFileDto = files[fIdx];
      const autoGen = isAutoCollapsedDiffPath(df.path);
      const collapsed = this.isFileCollapsed(df.path);
      const headerRow: FlatDiffRowDto = {
        key: `hdr_${df.path}_${collapsed ? 'c' : 'e'}_${df.additions}_${df.deletions}`,
        rowKind: 'file_header',
        filePath: df.path,
        additions: df.additions,
        deletions: df.deletions,
        isCollapsed: collapsed,
        isAutoGenerated: autoGen,
        totalLineCount: df.lines.length,
        hiddenLineCount: 0,
        line: null
      };
      rows.push(headerRow);

      if (!collapsed) {
        const isUncapped = this.uncappedPaths.indexOf(df.path) >= 0;
        const limit = isUncapped
          ? df.lines.length
          : Math.min(INITIAL_DIFF_LINE_CAP, df.lines.length);
        for (let lIdx = 0; lIdx < limit; lIdx++) {
          const ln: DiffLineDto = df.lines[lIdx];
          const lineRow: FlatDiffRowDto = {
            key: `ln_${df.path}_${lIdx}`,
            rowKind: 'diff_line',
            filePath: df.path,
            additions: 0,
            deletions: 0,
            isCollapsed: false,
            isAutoGenerated: autoGen,
            totalLineCount: df.lines.length,
            hiddenLineCount: 0,
            line: ln
          };
          rows.push(lineRow);
        }

        if (!isUncapped && df.lines.length > INITIAL_DIFF_LINE_CAP) {
          const moreRow: FlatDiffRowDto = {
            key: `more_${df.path}_${df.lines.length}`,
            rowKind: 'show_more',
            filePath: df.path,
            additions: 0,
            deletions: 0,
            isCollapsed: false,
            isAutoGenerated: autoGen,
            totalLineCount: df.lines.length,
            hiddenLineCount: df.lines.length - INITIAL_DIFF_LINE_CAP,
            line: null
          };
          rows.push(moreRow);
        }
      }
    }
    return rows;
  }

  private branchLabel(): string {
    const st = this.store.gitStatus;
    if (st !== null && st.branch !== null && st.branch.length > 0) {
      let suffix = '';
      if (st.ahead > 0) {
        suffix += ` ↑${st.ahead}`;
      }
      if (st.behind > 0) {
        suffix += ` ↓${st.behind}`;
      }
      return `${st.branch}${suffix}`;
    }
    return basenameOfPath(this.store.activeCwd);
  }

  private formatLineGutter(ln: DiffLineDto): string {
    if (ln.kind === 'hunk_header' || ln.kind === 'meta') {
      return '';
    }
    const oldStr = ln.old_line !== null ? ln.old_line.toString().padStart(3, ' ') : '   ';
    const newStr = ln.new_line !== null ? ln.new_line.toString().padStart(3, ' ') : '   ';
    return `${oldStr} ${newStr} `;
  }

  @Builder
  private renderFlatRow(row: FlatDiffRowDto) {
    if (row.rowKind === 'file_header') {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text(row.isCollapsed ? '▸' : '▾')
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

        Text(row.filePath)
          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Bold))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        if (row.isAutoGenerated && row.isCollapsed) {
          Text('Generated')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }

        if (row.additions > 0) {
          Text(`+${row.additions}`)
            .attributeModifier(orbitTextMeta(this.store.palette.statusDirect, FontWeight.Bold))
        }
        if (row.deletions > 0) {
          Text(`-${row.deletions}`)
            .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Bold))
        }
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      .margin({ top: OrbitTokens.SPACE_SM })
      .backgroundColor(this.store.palette.elevated)
      .borderRadius(OrbitTokens.PILL_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.toggleFileCollapsed(row.filePath);
      })
    } else if (row.rowKind === 'show_more') {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Text(`Show all ${row.totalLineCount} lines`)
          .attributeModifier(orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Medium))
        Text(`(+${row.hiddenLineCount} more)`)
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
      }
      .width('100%')
      .height(OrbitTokens.HIT_MIN)
      .justifyContent(FlexAlign.Center)
      .backgroundColor(this.store.palette.subtleSurface)
      .borderRadius(OrbitTokens.PILL_RADIUS)
      .margin({ top: 2, bottom: OrbitTokens.SPACE_XS })
      .onClick(() => {
        this.expandAllLinesForFile(row.filePath);
      })
    } else if (row.line !== null) {
      Row() {
        Text() {
          Span(this.formatLineGutter(row.line))
            .font(orbitSpanMonoFont())
            .fontColor(this.store.palette.fgMuted)
          Span(
            row.line.kind === 'added'
              ? '+ '
              : row.line.kind === 'removed'
                ? '- '
                : row.line.kind === 'hunk_header' || row.line.kind === 'meta'
                  ? ''
                  : '  '
          )
            .font(orbitSpanMonoFont(FontWeight.Bold))
            .fontColor(
              row.line.kind === 'added'
                ? this.store.palette.statusDirect
                : row.line.kind === 'removed'
                  ? this.store.palette.statusDanger
                  : this.store.palette.fgMuted
            )
          Span(row.line.content)
            .font(orbitSpanMonoFont())
            .fontColor(
              row.line.kind === 'added'
                ? this.store.palette.statusDirect
                : row.line.kind === 'removed'
                  ? this.store.palette.statusDanger
                  : row.line.kind === 'hunk_header' || row.line.kind === 'meta'
                    ? this.store.palette.accentPrimary
                    : this.store.palette.fgPrimary
            )
        }
        .lineHeight(OrbitTokens.LH_META)
        .width('100%')
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_SM,
        right: OrbitTokens.SPACE_SM,
        top: OrbitTokens.SPACE_XS,
        bottom: OrbitTokens.SPACE_XS
      })
      .backgroundColor(
        row.line.kind === 'added'
          ? this.store.palette.diffAddedBg
          : row.line.kind === 'removed'
            ? this.store.palette.diffRemovedBg
            : this.store.palette.code
      )
      .onClick(() => {
        const ln = row.line;
        if (ln !== null && ln.kind !== 'meta' && ln.kind !== 'hunk_header') {
          this.store.quoteDiffLineIntoComposer(row.filePath, ln);
        }
      })
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          if (this.isEmbeddedPane) {
            Text('Git Inspector')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
          } else {
            Text('Git Review')
              .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
          }
          Text(`⎇ ${this.branchLabel()} · Tap line to quote`)
            .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button('Refresh')
          .attributeModifier(
            orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
          )
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .accessibilityText('Refresh Git diff')
          .onClick(() => {
            this.handlePullRefresh();
          })

        if (this.isEmbeddedPane) {
          Row() {
            Text('×')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary, FontWeight.Bold))
          }
          .width(OrbitTokens.HIT_MIN)
          .height(OrbitTokens.HIT_MIN)
          .justifyContent(FlexAlign.Center)
          .accessibilityText('Close Git Diff Inspector')
          .onClick(() => {
            this.store.showInlineDiffInspector = false;
            this.store.triggerHaptic(6);
          })
        } else {
          Button('Done')
            .attributeModifier(
              orbitButtonSubhead(this.store.palette.accentPrimary, Color.Transparent)
            )
            .height(OrbitTokens.HIT_MIN)
            .onClick(() => {
              this.store.showGitDiffSheet = false;
            })
        }
      }
      .width('100%')

      Row({ space: OrbitTokens.SPACE_SM }) {
        Button('Unstaged')
          .attributeModifier(
            orbitButtonSubhead(
              !this.store.gitStagedMode
                ? this.store.palette.onAccent
                : this.store.palette.fgPrimary,
              !this.store.gitStagedMode
                ? this.store.palette.accentPrimaryFill
                : this.store.palette.subtleSurface
            )
          )
          .height(36)
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .layoutWeight(1)
          .onClick(() => {
            this.store.setGitStagedMode(false);
          })

        Button('Staged')
          .attributeModifier(
            orbitButtonSubhead(
              this.store.gitStagedMode
                ? this.store.palette.onAccent
                : this.store.palette.fgPrimary,
              this.store.gitStagedMode
                ? this.store.palette.accentPrimaryFill
                : this.store.palette.subtleSurface
            )
          )
          .height(36)
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .layoutWeight(1)
          .onClick(() => {
            this.store.setGitStagedMode(true);
          })
      }
      .width('100%')

      if (this.displayedDiffFiles().length > 0) {
        // Instant File Filter + 1-Tap "Collapse All" / "Expand Code" Toolbar
        Row({ space: OrbitTokens.SPACE_SM }) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            Text('⌕')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Normal))

            TextInput({
              placeholder: 'Filter modified files…',
              text: this.fileFilterQuery
            })
              .attributeModifier(
                orbitInputSubhead(
                  this.store.palette.fgPrimary,
                  this.store.palette.fgMuted,
                  Color.Transparent
                )
              )
              .layoutWeight(1)
              .height(36)
              .padding(0)
              .onChange((val: string) => {
                this.fileFilterQuery = val;
              })

            if (this.fileFilterQuery.length > 0) {
              Stack({ alignContent: Alignment.Center }) {
                Text('×')
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgMuted, FontWeight.Bold))
              }
              .width(OrbitTokens.HIT_MIN)
              .height(OrbitTokens.HIT_MIN)
              .accessibilityText('Clear modified file filter')
              .onClick(() => {
                this.fileFilterQuery = '';
                this.store.triggerHaptic(6);
              })
            }
          }
          .layoutWeight(1)
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
          .padding({ left: OrbitTokens.SPACE_SM, right: 0 })
          .backgroundColor(this.store.palette.elevated)
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.store.palette.hairline
          })
          .alignItems(VerticalAlign.Center)

          Button(this.areAllCodeFilesCollapsed() ? 'Expand Code' : 'Collapse All')
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
            )
            .height(OrbitTokens.HIT_MIN)
            .borderRadius(OrbitTokens.PILL_RADIUS)
            .onClick(() => {
              this.toggleCollapseExpandCode();
            })
        }
        .width('100%')

        if (this.filteredDiffFiles().length > 0) {
          Scroll() {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Row() {
                Text('All Files')
                  .attributeModifier(
                    orbitTextMeta(
                      this.store.gitSelectedPath.length === 0
                        ? this.store.palette.onAccent
                        : this.store.palette.fgPrimary,
                      FontWeight.Medium
                    )
                  )
              }
              .height(32)
              .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .backgroundColor(
                this.store.gitSelectedPath.length === 0
                  ? this.store.palette.accentPrimaryFill
                  : this.store.palette.subtleSurface
              )
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .onClick(() => {
                this.store.selectGitFileFilter('');
              })

              ForEach(
                this.filteredDiffFiles(),
                (df: DiffFileDto) => {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    Text(basenameOfPath(df.path))
                      .attributeModifier(
                        orbitTextMono(
                          this.store.gitSelectedPath === df.path
                            ? this.store.palette.accentPrimary
                            : this.store.palette.fgPrimary,
                          FontWeight.Medium
                        )
                      )
                    Text(`+${df.additions}`)
                      .attributeModifier(
                        orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium)
                      )
                    Text(`-${df.deletions}`)
                      .attributeModifier(
                        orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium)
                      )
                  }
                  .height(32)
                  .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
                  .backgroundColor(
                    this.store.gitSelectedPath === df.path
                      ? this.store.palette.accentPrimarySoft
                      : this.store.palette.subtleSurface
                  )
                  .borderRadius(OrbitTokens.PILL_RADIUS)
                  .onClick(() => {
                    this.store.selectGitFileFilter(df.path);
                  })
                },
                (df: DiffFileDto) => df.path
              )
            }
          }
          .scrollable(ScrollDirection.Horizontal)
          .scrollBar(BarState.Off)
          .width('100%')
        }
      }

      if (this.displayedDiffFiles().length === 0) {
        Refresh({ refreshing: $$this.isRefreshing }) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Blank()
            Text('Working tree clean')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text('No unstaged or session changes in the active workspace.')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .textAlign(TextAlign.Center)
            Blank()
          }
          .width('100%')
          .height('100%')
          .alignItems(HorizontalAlign.Center)
        }
        .onRefreshing(() => {
          this.handlePullRefresh();
        })
        .width('100%')
        .layoutWeight(1)
      } else if (this.filteredDiffFiles().length === 0) {
        Refresh({ refreshing: $$this.isRefreshing }) {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Blank()
            Text('No matching modified files')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
            Text('Clear the filter above to view all modified files.')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .textAlign(TextAlign.Center)
            Blank()
          }
          .width('100%')
          .height('100%')
          .alignItems(HorizontalAlign.Center)
        }
        .onRefreshing(() => {
          this.handlePullRefresh();
        })
        .width('100%')
        .layoutWeight(1)
      } else {
        // 120fps Flat Virtualized Diff List wrapped in native ArkUI Pull-to-Refresh
        Refresh({ refreshing: $$this.isRefreshing }) {
          List({ space: 0 }) {
            Repeat<FlatDiffRowDto>(this.flatDiffRows())
              .each((ri: RepeatItem<FlatDiffRowDto>) => {
                ListItem() {
                  this.renderFlatRow(ri.item)
                }
              })
              .key((row: FlatDiffRowDto) => row.key)
              .virtualScroll()
          }
          .width('100%')
          .height('100%')
          .scrollBar(BarState.Auto)
          .edgeEffect(EdgeEffect.Spring)
        }
        .onRefreshing(() => {
          this.handlePullRefresh();
        })
        .width('100%')
        .layoutWeight(1)
      }
    }
    .width('100%')
    .height('100%')
    .padding({
      left: this.isEmbeddedPane ? OrbitTokens.SPACE_MD : OrbitTokens.SPACE_LG,
      right: this.isEmbeddedPane ? OrbitTokens.SPACE_MD : OrbitTokens.SPACE_LG,
      top: this.isEmbeddedPane ? OrbitTokens.SPACE_MD : OrbitTokens.SPACE_LG,
      bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
    })
    .backgroundColor(this.isEmbeddedPane ? this.store.palette.drawer : this.store.palette.canvas)
  }
}
