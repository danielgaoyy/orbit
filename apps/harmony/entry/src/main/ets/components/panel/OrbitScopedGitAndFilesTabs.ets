import {
  DiffFileDto,
  DiffLineDto,
  FlatDiffRowDto,
  GitCreatePrResultDto,
  GitReviewDto,
  GitStashEntryDto,
  HighlightedCodeDto,
  HighlightedLineDto,
  HighlightedTokenDto,
  WorkspaceFileViewDto,
  basenameOfPath,
  isAutoCollapsedDiffPath,
  isWorkspaceFileEditable,
  matchesGitDiscardConfirmation,
  mergeWorkspaceFileChunk,
  nonBlankPath,
  resolveGitDiffRowActionPath
} from '../../model/OrbitTypes';
import {
  OrbitGitDiffModeState,
  OrbitHarmonyStore,
  OrbitScopedGitCollabData,
  OrbitWorkspaceFileWriteOutcome
} from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitSidePanelTab,
  canonicalFileTabId,
  encodeSidePanelKeyTuple,
  isSameFileTarget
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
  orbitTextSubhead,
  resolveSemanticFileSymbol
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';
import { RichMarkdownView } from '../stage/RichMarkdownView';

const INITIAL_DIFF_LINE_CAP: number = 120;

interface OrbitFileTarget {
  hostId: string;
  cwd: string;
  path: string;
}

function fileTargetRenderSig(target: OrbitFileTarget | null): string {
  if (target === null) {
    return '';
  }
  return encodeSidePanelKeyTuple(['file_target', target.hostId, target.cwd, target.path]);
}

function emptyGitCollab(): OrbitScopedGitCollabData {
  return {
    prStatus: null,
    stashes: [],
    conflicts: [],
    rebaseStatus: null
  };
}

/**
 * Scoped Git Review & Collaboration tab (`kind === 'review'`).
 * Strictly reads `tab.hostId`, `tab.cwd`, and `tab.path` — NEVER falls back to global host/cwd.
 * Restores full baseline Git actions: stage/unstage (file & all), commit + amend + AI message,
 * stash save/pop, PR status/create/copy, conflict & rebase continue/abort, and tap-to-quote lines.
 */
@ComponentV2
export struct OrbitScopedGitTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private stagedMode: boolean = false;
  @Local private reviewData: GitReviewDto | null = null;
  @Local private collabData: OrbitScopedGitCollabData = emptyGitCollab();
  @Local private manuallyExpandedPaths: string[] = [];
  @Local private manuallyCollapsedPaths: string[] = [];
  @Local private uncappedPaths: string[] = [];
  @Local private filterQuery: string = '';
  @Local private writeBusy: boolean = false;
  @Local private showCommitPanel: boolean = false;
  @Local private commitMessageInput: string = '';
  @Local private commitAmend: boolean = false;
  @Local private showPrPanel: boolean = false;
  @Local private prTitleInput: string = '';
  @Local private prBodyInput: string = '';
  @Local private prDraft: boolean = false;
  @Local private showPushPanel: boolean = false;
  @Local private pushRemoteInput: string = '';
  @Local private pushBranchInput: string = '';
  @Local private pushSetUpstream: boolean = false;
  @Local private showStashListPanel: boolean = false;
  @Local private discardTargetPath: string = '';
  @Local private discardConfirmInput: string = '';
  @Local private discardIncludeUntracked: boolean = false;
  @Local private diffModeState: OrbitGitDiffModeState = 'idle';

  private lastTargetKey: string = '';
  private lastBoundHostId: string = '';
  private lastBoundCwd: string | null = null;
  private lastBoundPath: string | null = null;
  private targetVersion: number = 0;
  private collabSeq: number = 0;
  private writeSeq: number = 0;
  private commitDraftVersion: number = 0;
  private prDraftVersion: number = 0;
  private disposed: boolean = false;
  @Local private store: OrbitHarmonyStore = orbitRuntime();

  private effectiveScopeKey(): string {
    const clean = this.scopeKey.trim();
    return clean.length > 0 ? clean : this.store.sidePanelStore.activeScopeKey;
  }

  private currentTargetKey(): string {
    const cwd = this.tab.cwd !== null ? this.tab.cwd : '';
    const path = this.tab.path !== null ? this.tab.path : '';
    return encodeSidePanelKeyTuple(['git_review', this.scopeKey, this.tab.tabId, this.tab.hostId, cwd, path]);
  }

  aboutToAppear(): void {
    this.disposed = false;
    this.stagedMode = this.tab.gitStaged === true;
    this.lastTargetKey = this.currentTargetKey();
    this.lastBoundHostId = this.tab.hostId;
    this.lastBoundCwd = this.tab.cwd;
    this.lastBoundPath = this.tab.path;
    this.reloadScopedGit(true);
    void this.refreshCollabStatus();
  }

  aboutToDisappear(): void {
    this.disposed = true;
    this.targetVersion += 1;
    this.collabSeq += 1;
    this.writeSeq += 1;
    this.commitDraftVersion += 1;
    this.prDraftVersion += 1;
    const hostToRelease = this.lastBoundHostId.length > 0 ? this.lastBoundHostId : this.tab.hostId;
    const cwdToRelease = this.lastBoundHostId.length > 0 ? this.lastBoundCwd : this.tab.cwd;
    const pathToRelease = this.lastBoundHostId.length > 0 ? this.lastBoundPath : this.tab.path;
    this.store.releaseGitDiffTrackerIfUnused(hostToRelease, cwdToRelease, pathToRelease);
  }

  @Monitor('tab', 'scopeKey', 'store.gitPanelRevision', 'store.panelDataRevision')
  onTabOrDataChanged(_mon: IMonitor): void {
    const nextKey = this.currentTargetKey();
    if (nextKey !== this.lastTargetKey) {
      const prevHost = this.lastBoundHostId;
      const prevCwd = this.lastBoundCwd;
      const prevPath = this.lastBoundPath;
      this.lastTargetKey = nextKey;
      this.lastBoundHostId = this.tab.hostId;
      this.lastBoundCwd = this.tab.cwd;
      this.lastBoundPath = this.tab.path;
      // Same-kind Review tab takeover (A closed -> B rebinds this component): release A's heavy
      // diff body cache immediately (R5-P1-1).
      if (prevHost.length > 0) {
        this.store.releaseGitDiffTrackerIfUnused(prevHost, prevCwd, prevPath);
      }
      this.targetVersion += 1;
      this.collabSeq += 1;
      this.writeSeq += 1;
      this.commitDraftVersion += 1;
      this.prDraftVersion += 1;
      this.stagedMode = this.tab.gitStaged === true;
      this.reviewData = null;
      this.diffModeState = 'idle';
      this.collabData = emptyGitCollab();
      this.manuallyExpandedPaths = [];
      this.manuallyCollapsedPaths = [];
      this.uncappedPaths = [];
      this.filterQuery = '';
      this.writeBusy = false;
      this.showCommitPanel = false;
      this.commitMessageInput = '';
      this.commitAmend = false;
      this.showPrPanel = false;
      this.prTitleInput = '';
      this.prBodyInput = '';
      this.prDraft = false;
      this.showPushPanel = false;
      this.pushRemoteInput = '';
      this.pushBranchInput = '';
      this.pushSetUpstream = false;
      this.showStashListPanel = false;
      this.discardTargetPath = '';
      this.discardConfirmInput = '';
      this.discardIncludeUntracked = false;
      this.reloadScopedGit(true, true);
      void this.refreshCollabStatus();
      return;
    }
    const desiredStaged = this.tab.gitStaged === true;
    if (this.tab.gitStaged !== undefined && desiredStaged !== this.stagedMode) {
      this.stagedMode = desiredStaged;
      this.reloadScopedGit(true, false);
      return;
    }
    this.reloadScopedGit(false, false);
  }

  private reloadScopedGit(
    triggerRemote: boolean,
    includeStatus: boolean = true,
    reuseInFlight: boolean = false
  ): void {
    const host = this.tab.hostId.trim();
    const cwd = nonBlankPath(this.tab.cwd);
    if (host.length === 0 || cwd.length === 0) {
      this.reviewData = null;
      this.diffModeState = 'idle';
      return;
    }
    this.reviewData = this.store.loadScopedGitReview(
      host,
      cwd,
      this.stagedMode,
      this.tab.path,
      triggerRemote,
      includeStatus,
      reuseInFlight
    );
    this.diffModeState = this.store.getScopedGitDiffModeState(
      host,
      cwd,
      this.stagedMode,
      this.tab.path
    );
  }

  private async refreshCollabStatus(): Promise<void> {
    const host = this.tab.hostId.trim();
    const cwd = nonBlankPath(this.tab.cwd);
    const targetKey = this.currentTargetKey();
    const tv = this.targetVersion;
    this.collabSeq += 1;
    const cSeq = this.collabSeq;
    if (host.length === 0 || cwd.length === 0) {
      this.collabData = emptyGitCollab();
      return;
    }
    const loaded = await this.store.loadScopedGitCollabStatus(host, cwd);
    if (!this.disposed && tv === this.targetVersion && cSeq === this.collabSeq && targetKey === this.currentTargetKey()) {
      this.collabData = loaded;
    }
  }

  private async runScopedWrite(fn: (host: string, cwd: string) => Promise<boolean>): Promise<boolean> {
    const host = this.tab.hostId.trim();
    const cwd = nonBlankPath(this.tab.cwd);
    const targetKey = this.currentTargetKey();
    if (host.length === 0 || cwd.length === 0 || this.writeBusy) {
      return false;
    }
    const tv = this.targetVersion;
    this.writeSeq += 1;
    const wSeq = this.writeSeq;
    this.writeBusy = true;
    const ok = await fn(host, cwd);
    // Use targetVersion + writeSeq (isolated from collabSeq) so concurrent manual refresh never strands writeBusy (P1-8).
    if (this.disposed || tv !== this.targetVersion || wSeq !== this.writeSeq || targetKey !== this.currentTargetKey()) {
      return false;
    }
    this.writeBusy = false;
    if (ok) {
      // R19-N2: Reuse the single post-mutation in-flight request already dispatched by native core
      // (while manual refresh buttons continue to call `reloadScopedGit(true)` with reuse=false).
      this.reloadScopedGit(true, false, true);
      void this.refreshCollabStatus();
    }
    return ok;
  }

  @Computed
  get diffFiles(): DiffFileDto[] {
    if (this.diffModeState !== 'confirmed') {
      return [];
    }
    const all = this.reviewData !== null ? this.reviewData.diff_files : [];
    const q = this.filterQuery.trim().toLowerCase();
    if (q.length === 0) {
      return all;
    }
    const out: DiffFileDto[] = [];
    for (let i = 0; i < all.length; i++) {
      if (
        all[i].path.toLowerCase().indexOf(q) >= 0 ||
        basenameOfPath(all[i].path).toLowerCase().indexOf(q) >= 0
      ) {
        out.push(all[i]);
      }
    }
    return out;
  }

  @Computed
  get allCodeCollapsed(): boolean {
    const files = this.diffFiles;
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

  @Computed
  get flatDiffRows(): FlatDiffRowDto[] {
    const files = this.diffFiles;
    const rows: FlatDiffRowDto[] = [];
    for (let fIdx = 0; fIdx < files.length; fIdx++) {
      const df: DiffFileDto = files[fIdx];
      const autoGen = isAutoCollapsedDiffPath(df.path);
      const collapsed = this.isFileCollapsed(df.path);
      rows.push({
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
      });
      if (!collapsed) {
        const isUncapped = this.uncappedPaths.indexOf(df.path) >= 0;
        const limit = isUncapped ? df.lines.length : Math.min(INITIAL_DIFF_LINE_CAP, df.lines.length);
        for (let lIdx = 0; lIdx < limit; lIdx++) {
          rows.push({
            key: `ln_${df.path}_${lIdx}_${df.lines[lIdx].kind}`,
            rowKind: 'diff_line',
            filePath: df.path,
            additions: 0,
            deletions: 0,
            isCollapsed: false,
            isAutoGenerated: autoGen,
            totalLineCount: df.lines.length,
            hiddenLineCount: 0,
            line: df.lines[lIdx]
          });
        }
        if (!isUncapped && df.lines.length > INITIAL_DIFF_LINE_CAP) {
          rows.push({
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
          });
        }
      }
    }
    return rows;
  }

  @Computed
  get branchText(): string {
    const st = this.reviewData?.status ?? null;
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
    const cwd = this.tab.cwd !== null ? this.tab.cwd : '';
    return cwd.length > 0 ? basenameOfPath(cwd) : 'Working Tree';
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
      this.manuallyExpandedPaths = idx < 0
        ? this.manuallyExpandedPaths.concat([filePath])
        : this.manuallyExpandedPaths.filter((p: string) => p !== filePath);
    } else {
      const idx = this.manuallyCollapsedPaths.indexOf(filePath);
      this.manuallyCollapsedPaths = idx < 0
        ? this.manuallyCollapsedPaths.concat([filePath])
        : this.manuallyCollapsedPaths.filter((p: string) => p !== filePath);
    }
    this.store.triggerHaptic(6);
  }

  private toggleCollapseExpandAll(): void {
    if (this.allCodeCollapsed) {
      this.manuallyCollapsedPaths = [];
    } else {
      const collapsed: string[] = [];
      for (let i = 0; i < this.diffFiles.length; i++) {
        if (!isAutoCollapsedDiffPath(this.diffFiles[i].path)) {
          collapsed.push(this.diffFiles[i].path);
        }
      }
      this.manuallyExpandedPaths = [];
      this.manuallyCollapsedPaths = collapsed;
    }
    this.store.triggerHaptic(8);
  }

  private expandAllLinesForFile(filePath: string): void {
    if (this.uncappedPaths.indexOf(filePath) >= 0) {
      return;
    }
    this.uncappedPaths = this.uncappedPaths.concat([filePath]);
    this.store.triggerHaptic(8);
  }

  private resolveRowActionPath(rowPath: string): string {
    return resolveGitDiffRowActionPath(
      rowPath,
      this.reviewData !== null ? this.reviewData.selected_path : null
    );
  }

  private isUntrackedPath(targetPath: string): boolean {
    const files = this.reviewData?.status?.files ?? [];
    for (let i = 0; i < files.length; i++) {
      if (files[i].path === targetPath) {
        return files[i].status.indexOf('?') >= 0;
      }
    }
    return false;
  }

  private openDiscardConfirmForFile(targetPath: string): void {
    this.discardTargetPath = targetPath;
    this.discardConfirmInput = '';
    this.discardIncludeUntracked = this.isUntrackedPath(targetPath);
    this.showCommitPanel = false;
    this.showPrPanel = false;
    this.showPushPanel = false;
    this.showStashListPanel = false;
  }

  private canConfirmDiscard(): boolean {
    return !this.writeBusy && matchesGitDiscardConfirmation(this.discardTargetPath, this.discardConfirmInput);
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
      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          SymbolGlyph(row.isCollapsed ? $r('sys.symbol.chevron_right') : $r('sys.symbol.chevron_down'))
            .fontSize(12)
            .fontColor([this.store.palette.fgMuted])

          Text(row.filePath)
            .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)

          if (row.additions > 0) {
            Text(`+${row.additions}`)
              .attributeModifier(orbitTextMeta(this.store.palette.statusDirect, FontWeight.Bold))
          }
          if (row.deletions > 0) {
            Text(`-${row.deletions}`)
              .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Bold))
          }

          if (!this.stagedMode) {
            Button($r('app.string.git_discard'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
              )
              .height(26)
              .padding({ left: 8, right: 8 })
              .enabled(!this.writeBusy)
              .onClick(() => {
                const targetPath = this.resolveRowActionPath(row.filePath);
                this.openDiscardConfirmForFile(targetPath);
              })
          }

          Button(
            this.stagedMode ? $r('app.string.git_unstage') : $r('app.string.git_stage'),
            { type: ButtonType.Capsule, stateEffect: true }
          )
            .attributeModifier(
              orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
            )
            .height(26)
            .padding({ left: 8, right: 8 })
            .enabled(!this.writeBusy)
            .onClick(() => {
              const targetPath = this.resolveRowActionPath(row.filePath);
              if (this.stagedMode) {
                void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                  this.store.gitUnstageScopedPaths(h, c, [targetPath], false)
                );
              } else {
                void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                  this.store.gitStageScopedPaths(h, c, [targetPath], false)
                );
              }
            })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
      .margin({ top: OrbitTokens.SPACE_XS })
      .backgroundColor(this.store.palette.listCard)
      .borderRadius(OrbitTokens.RADIUS_MD)
      .onClick(() => {
        dismissHarmonyKeyboard(this.getUIContext());
        this.toggleFileCollapsed(row.filePath);
      })
    } else if (row.rowKind === 'show_more') {
      Button({ type: ButtonType.Normal, stateEffect: true }) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(`Show all ${row.totalLineCount} lines`)
            .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
          Text(`(+${row.hiddenLineCount} more)`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }
      }
      .width('100%')
      .constraintSize({ minHeight: 36 })
      .backgroundColor(this.store.palette.subtleSurface)
      .borderRadius(OrbitTokens.RADIUS_MD)
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
              : (row.line.kind === 'removed'
                ? '- '
                : (row.line.kind === 'hunk_header' || row.line.kind === 'meta' ? '' : '  '))
          )
            .font(orbitSpanMonoFont(FontWeight.Bold))
            .fontColor(
              row.line.kind === 'added'
                ? this.store.palette.statusDirect
                : (row.line.kind === 'removed' ? this.store.palette.statusDanger : this.store.palette.fgMuted)
            )
          Span(row.line.content)
            .font(orbitSpanMonoFont())
            .fontColor(
              row.line.kind === 'added'
                ? this.store.palette.statusDirect
                : (row.line.kind === 'removed'
                  ? this.store.palette.statusDanger
                  : (row.line.kind === 'hunk_header' || row.line.kind === 'meta'
                    ? this.store.palette.accentPrimary
                    : this.store.palette.fgPrimary))
            )
        }
        .copyOption(CopyOptions.LocalDevice)
        .width('100%')
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 2, bottom: 2 })
      .backgroundColor(
        row.line.kind === 'added'
          ? this.store.palette.diffAddedBg
          : (row.line.kind === 'removed' ? this.store.palette.diffRemovedBg : this.store.palette.code)
      )
      .onClick(() => {
        const ln = row.line;
        if (ln !== null && ln.kind !== 'meta' && ln.kind !== 'hunk_header') {
          this.store.quoteDiffLineIntoComposer(this.resolveRowActionPath(row.filePath), ln);
        }
      })
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_XS }) {
      // Header bar: branch + Unstaged/Staged + Collapse All + Refresh
      Row({ space: OrbitTokens.SPACE_XS }) {
        Column({ space: 2 }) {
          Text(this.branchText)
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text(`${this.diffFiles.length} file(s) · ${this.stagedMode ? 'Staged' : 'Unstaged'}`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button(this.stagedMode ? 'Staged' : 'Unstaged', { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonMeta(
              this.stagedMode ? this.store.palette.onAccent : this.store.palette.fgPrimary,
              this.stagedMode ? this.store.palette.accentPrimaryFill : this.store.palette.subtleSurface
            )
          )
          .height(30)
          .padding({ left: 10, right: 10 })
          .onClick(() => {
            this.stagedMode = !this.stagedMode;
            this.store.sidePanelStore.updateTabGitStaged(
              this.effectiveScopeKey(),
              this.tab.tabId,
              this.stagedMode
            );
            this.reloadScopedGit(true, false);
          })

        if (this.diffFiles.length > 0) {
          Button(this.allCodeCollapsed ? 'Expand' : 'Collapse', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(30)
            .padding({ left: 9, right: 9 })
            .onClick(() => {
              this.toggleCollapseExpandAll();
            })
        }

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.git_refresh_review_aria'))
        .onClick(() => {
          this.reloadScopedGit(true);
          void this.refreshCollabStatus();
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      // Action strip: Stage/Unstage All, Commit, PR, Stash Save, Stash Pop
      Scroll() {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Button(
            this.stagedMode ? $r('app.string.git_unstage_all') : $r('app.string.git_stage_all'),
            { type: ButtonType.Capsule, stateEffect: true }
          )
            .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.listCard))
            .height(30)
            .padding({ left: 10, right: 10 })
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .enabled(!this.writeBusy && this.tab.hostId.length > 0 && (this.tab.cwd ?? '').length > 0)
            .onClick(() => {
              if (this.stagedMode) {
                void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                  this.store.gitUnstageScopedPaths(h, c, [], true)
                );
              } else {
                void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                  this.store.gitStageScopedPaths(h, c, [], true)
                );
              }
            })

          Button($r('app.string.git_commit'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(
                this.showCommitPanel ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                this.showCommitPanel ? this.store.palette.accentPrimaryFill : this.store.palette.listCard
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .onClick(() => {
              this.showCommitPanel = !this.showCommitPanel;
              if (this.showCommitPanel) {
                this.showPrPanel = false;
                this.showPushPanel = false;
                this.showStashListPanel = false;
              }
            })

          Button($r('app.string.git_push'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(
                this.showPushPanel ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                this.showPushPanel ? this.store.palette.accentPrimaryFill : this.store.palette.listCard
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .enabled(!this.writeBusy && this.tab.hostId.length > 0 && (this.tab.cwd ?? '').length > 0)
            .onClick(() => {
              this.showPushPanel = !this.showPushPanel;
              if (this.showPushPanel) {
                this.showCommitPanel = false;
                this.showPrPanel = false;
                this.showStashListPanel = false;
              }
            })

          Button($r('app.string.git_pr_section'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(
                this.showPrPanel ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                this.showPrPanel ? this.store.palette.accentPrimaryFill : this.store.palette.listCard
              )
            )
            .height(30)
            .padding({ left: 10, right: 10 })
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .onClick(() => {
              this.showPrPanel = !this.showPrPanel;
              if (this.showPrPanel) {
                this.showCommitPanel = false;
                this.showPushPanel = false;
                this.showStashListPanel = false;
                void this.refreshCollabStatus();
              }
            })

          Button($r('app.string.git_stash_save'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.listCard))
            .height(30)
            .padding({ left: 10, right: 10 })
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .enabled(!this.writeBusy && this.tab.hostId.length > 0 && (this.tab.cwd ?? '').length > 0)
            .onClick(() => {
              void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                this.store.gitSaveScopedStash(h, c, null, true)
              );
            })

          if (this.collabData.stashes.length > 0) {
            Button(
              $r('app.string.git_stashes_count', this.collabData.stashes.length),
              { type: ButtonType.Capsule, stateEffect: true }
            )
              .attributeModifier(
                orbitButtonMeta(
                  this.showStashListPanel ? this.store.palette.onAccent : this.store.palette.accentPrimary,
                  this.showStashListPanel
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.accentPrimarySoft
                )
              )
              .height(30)
              .padding({ left: 10, right: 10 })
              .enabled(!this.writeBusy)
              .onClick(() => {
                this.showStashListPanel = !this.showStashListPanel;
                if (this.showStashListPanel) {
                  this.showCommitPanel = false;
                  this.showPrPanel = false;
                  this.showPushPanel = false;
                }
              })
          }
        }
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')

      // Conflicts / Rebase Alert Strip
      if (
        this.collabData.conflicts.length > 0 ||
        (this.collabData.rebaseStatus !== null && this.collabData.rebaseStatus.in_progress)
      ) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            SymbolGlyph($r('sys.symbol.exclamationmark_triangle_fill'))
              .fontSize(14)
              .fontColor([this.store.palette.statusDanger])
            Text(
              this.collabData.conflicts.length > 0
                ? `${this.collabData.conflicts.length} merge conflict(s)`
                : 'Rebase in progress'
            )
              .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Bold))
              .layoutWeight(1)
          }
          .width('100%')

          Row({ space: OrbitTokens.SPACE_XS }) {
            Button($r('app.string.git_rebase_continue'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(30)
              .padding({ left: 10, right: 10 })
              .enabled(!this.writeBusy)
              .onClick(() => {
                void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                  this.store.gitRebaseContinueScoped(h, c)
                );
              })
            Button($r('app.string.git_rebase_abort'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.listCard))
              .height(30)
              .padding({ left: 10, right: 10 })
              .enabled(!this.writeBusy)
              .onClick(() => {
                void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                  this.store.gitRebaseAbortScoped(h, c)
                );
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

      // Two-Step Destructive Discard Confirmation Dialog (requires exact filename input)
      if (this.discardTargetPath.length > 0) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Text($r('app.string.git_discard_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.statusDanger, FontWeight.Bold))
            .width('100%')
          Text($r('app.string.git_discard_warning', this.discardTargetPath))
            .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary))
            .width('100%')

          TextInput({
            placeholder: $r('app.string.git_discard_input_placeholder'),
            text: this.discardConfirmInput
          })
            .attributeModifier(
              orbitInputMono(
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
              this.discardConfirmInput = val;
            })

          Row({ space: OrbitTokens.SPACE_XS }) {
            Toggle({ type: ToggleType.Switch, isOn: this.discardIncludeUntracked })
              .selectedColor(this.store.palette.statusDanger)
              .onChange((on: boolean) => {
                this.discardIncludeUntracked = on;
              })
            Text($r('app.string.git_discard_include_untracked'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
              .layoutWeight(1)

            Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
              .height(30)
              .padding({ left: 10, right: 10 })
              .onClick(() => {
                this.discardTargetPath = '';
                this.discardConfirmInput = '';
              })

            Button($r('app.string.git_discard_confirm_action'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
              .height(30)
              .padding({ left: 10, right: 10 })
              .enabled(this.canConfirmDiscard())
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                const target = this.discardTargetPath;
                const includeUntracked = this.discardIncludeUntracked;
                const tv = this.targetVersion;
                const tKey = this.currentTargetKey();
                void this.runScopedWrite(async (h: string, c: string): Promise<boolean> => {
                  const res = await this.store.gitDiscardScopedFiles(h, c, [target], includeUntracked);
                  if (!this.disposed && tv === this.targetVersion && tKey === this.currentTargetKey()) {
                    if (res.ok && res.data !== null) {
                      this.store.showToastResource($r('app.string.git_discard_success', res.data), false);
                      this.store.triggerHaptic(12);
                    } else {
                      this.store.showToastResource(
                        res.errorDetail.length > 0
                          ? $r('app.string.git_discard_failed_detail', res.errorDetail)
                          : $r('app.string.git_discard_failed'),
                        true,
                        res.errorDetail
                      );
                    }
                  }
                  return res.ok;
                }).then((ok: boolean) => {
                  if (ok && !this.disposed && tv === this.targetVersion && tKey === this.currentTargetKey()) {
                    this.discardTargetPath = '';
                    this.discardConfirmInput = '';
                    this.discardIncludeUntracked = false;
                  }
                });
              })
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_SM)
        .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .backgroundColor(this.store.palette.statusDangerSoft)
        .borderRadius(OrbitTokens.RADIUS_MD)
      }

      // Collapsible Push Panel
      if (this.showPushPanel) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            TextInput({
              placeholder: $r('app.string.git_push_remote_placeholder'),
              text: this.pushRemoteInput
            })
              .attributeModifier(
                orbitInputMono(
                  this.store.palette.fgPrimary,
                  this.store.palette.fgMuted,
                  this.store.palette.recessed
                )
              )
              .height(36)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .layoutWeight(1)
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((val: string) => {
                this.pushRemoteInput = val;
              })

            TextInput({
              placeholder: $r('app.string.git_push_branch_placeholder'),
              text: this.pushBranchInput
            })
              .attributeModifier(
                orbitInputMono(
                  this.store.palette.fgPrimary,
                  this.store.palette.fgMuted,
                  this.store.palette.recessed
                )
              )
              .height(36)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .layoutWeight(1)
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((val: string) => {
                this.pushBranchInput = val;
              })
          }
          .width('100%')

          Row({ space: OrbitTokens.SPACE_XS }) {
            Toggle({ type: ToggleType.Switch, isOn: this.pushSetUpstream })
              .selectedColor(this.store.palette.accentPrimaryFill)
              .onChange((on: boolean) => {
                this.pushSetUpstream = on;
              })
            Text($r('app.string.git_push_set_upstream'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
              .layoutWeight(1)

            Button($r('app.string.git_push'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(30)
              .padding({ left: 12, right: 12 })
              .enabled(!this.writeBusy)
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                const remote = this.pushRemoteInput.trim().length > 0 ? this.pushRemoteInput.trim() : null;
                const branch = this.pushBranchInput.trim().length > 0 ? this.pushBranchInput.trim() : null;
                const setUpstream = this.pushSetUpstream;
                const tv = this.targetVersion;
                const tKey = this.currentTargetKey();
                void this.runScopedWrite(async (h: string, c: string): Promise<boolean> => {
                  const res = await this.store.gitPushScoped(
                    h,
                    c,
                    remote,
                    branch,
                    setUpstream
                  );
                  if (!this.disposed && tv === this.targetVersion && tKey === this.currentTargetKey()) {
                    if (res.ok && res.data !== null) {
                      this.store.showToastResource(
                        $r('app.string.git_push_success', res.data.branch, res.data.remote),
                        false
                      );
                      this.store.triggerHaptic(10);
                    } else {
                      this.store.showToastResource(
                        res.errorDetail.length > 0
                          ? $r('app.string.git_push_failed_detail', res.errorDetail)
                          : $r('app.string.git_push_failed'),
                        true,
                        res.errorDetail
                      );
                    }
                  }
                  return res.ok;
                }).then((ok: boolean) => {
                  if (ok && !this.disposed && tv === this.targetVersion && tKey === this.currentTargetKey()) {
                    this.showPushPanel = false;
                  }
                });
              })
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
        .padding(OrbitTokens.SPACE_SM)
        .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .borderRadius(OrbitTokens.RADIUS_MD)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
      }

      // Collapsible Stash List Panel
      if (this.showStashListPanel && this.collabData.stashes.length > 0) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Text($r('app.string.git_stash_list_title'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Bold))
            .width('100%')
          ForEach(
            this.collabData.stashes,
            (entry: GitStashEntryDto) => {
              Row({ space: OrbitTokens.SPACE_XS }) {
                Column({ space: 2 }) {
                  Text(entry.stash_ref)
                    .attributeModifier(orbitTextMono(this.store.palette.accentPrimary, FontWeight.Bold))
                    .maxLines(1)
                  Text(entry.message)
                    .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                }
                .alignItems(HorizontalAlign.Start)
                .layoutWeight(1)

                Button($r('app.string.git_stash_pop'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
                  )
                  .height(28)
                  .padding({ left: 10, right: 10 })
                  .enabled(!this.writeBusy)
                  .onClick(() => {
                    void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                      this.store.gitPopScopedStash(h, c, entry.stash_ref, entry.oid ?? null)
                    );
                  })
              }
              .width('100%')
              .padding({ top: 4, bottom: 4 })
              .alignItems(VerticalAlign.Center)
            },
            (entry: GitStashEntryDto): string => `gst_${entry.stash_ref}_${entry.oid ?? ''}`
          )
        }
        .padding(OrbitTokens.SPACE_SM)
        .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .borderRadius(OrbitTokens.RADIUS_MD)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
      }

      // Collapsible Commit Panel
      if (this.showCommitPanel) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          TextArea({
            placeholder: $r('app.string.git_commit_placeholder'),
            text: this.commitMessageInput
          })
            .fontSize(OrbitTokens.FONT_META)
            .fontColor(this.store.palette.fgPrimary)
            .placeholderColor(this.store.palette.fgMuted)
            .backgroundColor(this.store.palette.recessed)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .padding(OrbitTokens.SPACE_SM)
            .width('100%')
            .constraintSize({ minHeight: 60, maxHeight: 110 })
            .onFocus(() => {
              this.store.shellStore.setTextInputFocused(true);
            })
            .onBlur(() => {
              this.store.shellStore.setTextInputFocused(false);
            })
            .onChange((val: string) => {
              this.commitMessageInput = val;
              this.commitDraftVersion += 1;
            })

          Row({ space: OrbitTokens.SPACE_XS }) {
            Toggle({ type: ToggleType.Switch, isOn: this.commitAmend })
              .selectedColor(this.store.palette.accentPrimaryFill)
              .onChange((on: boolean) => {
                this.commitAmend = on;
                this.commitDraftVersion += 1;
              })
            Text($r('app.string.git_amend'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
              .layoutWeight(1)

            Button($r('app.string.git_generate_message'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft))
              .height(30)
              .padding({ left: 10, right: 10 })
              .enabled(!this.writeBusy)
              .onClick(() => {
                const host = this.tab.hostId.trim();
                const cwd = nonBlankPath(this.tab.cwd);
                const targetKey = this.currentTargetKey();
                const tv = this.targetVersion;
                const draftVer = this.commitDraftVersion;
                this.writeSeq += 1;
                const wSeq = this.writeSeq;
                this.writeBusy = true;
                this.store.gitGenerateScopedCommitMessage(host, cwd).then((msg: string) => {
                  if (!this.disposed && tv === this.targetVersion && wSeq === this.writeSeq && targetKey === this.currentTargetKey()) {
                    this.writeBusy = false;
                    // Fill generated message ONLY if the user did not edit commit text while in flight (R3-P1-7).
                    if (msg.length > 0 && draftVer === this.commitDraftVersion) {
                      this.commitMessageInput = msg;
                      this.commitDraftVersion += 1;
                    }
                  }
                });
              })

            Button($r('app.string.git_commit'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(30)
              .padding({ left: 12, right: 12 })
              .enabled(!this.writeBusy && this.commitMessageInput.trim().length > 0)
              .onClick(() => {
                dismissHarmonyKeyboard(this.getUIContext());
                const msg = this.commitMessageInput;
                const amend = this.commitAmend;
                const targetKey = this.currentTargetKey();
                const tv = this.targetVersion;
                const draftVer = this.commitDraftVersion;
                void this.runScopedWrite((h: string, c: string): Promise<boolean> =>
                  this.store.gitCommitScoped(h, c, msg, amend)
                ).then((ok: boolean) => {
                  if (
                    ok &&
                    !this.disposed &&
                    tv === this.targetVersion &&
                    targetKey === this.currentTargetKey() &&
                    draftVer === this.commitDraftVersion
                  ) {
                    this.commitMessageInput = '';
                    this.commitAmend = false;
                    this.showCommitPanel = false;
                    this.commitDraftVersion += 1;
                  }
                });
              })
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
        .padding(OrbitTokens.SPACE_SM)
        .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .borderRadius(OrbitTokens.RADIUS_MD)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
      }

      // Collapsible PR Panel
      if (this.showPrPanel) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          if (
            this.collabData.prStatus !== null &&
            this.collabData.prStatus.number !== undefined &&
            this.collabData.prStatus.number !== null
          ) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(`#${this.collabData.prStatus.number} ${this.collabData.prStatus.title ?? ''}`)
                .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Bold))
                .maxLines(2)
                .layoutWeight(1)
              Text((this.collabData.prStatus.state ?? 'OPEN').toUpperCase())
                .attributeModifier(orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium))
                .padding({ left: 8, right: 8, top: 2, bottom: 2 })
                .backgroundColor(this.store.palette.statusDirectSoft)
                .borderRadius(999)
            }
            .width('100%')

            if ((this.collabData.prStatus.url ?? '').length > 0) {
              Row({ space: OrbitTokens.SPACE_XS }) {
                Text(
                  `${this.collabData.prStatus.passing_checks ?? 0} passing · ${this.collabData.prStatus.failing_checks ?? 0} failing`
                )
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .layoutWeight(1)
                Button($r('app.string.action_copy'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                  .height(28)
                  .padding({ left: 10, right: 10 })
                  .onClick(() => {
                    this.store.copyToClipboard(this.collabData.prStatus?.url ?? '', 'Copied PR URL');
                  })
              }
              .width('100%')
            }
          } else {
            TextInput({
              placeholder: $r('app.string.git_pr_title_placeholder'),
              text: this.prTitleInput
            })
              .attributeModifier(
                orbitInputSubhead(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
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
                this.prTitleInput = val;
                this.prDraftVersion += 1;
              })

            TextArea({
              placeholder: $r('app.string.git_pr_body_placeholder'),
              text: this.prBodyInput
            })
              .fontSize(OrbitTokens.FONT_META)
              .fontColor(this.store.palette.fgPrimary)
              .placeholderColor(this.store.palette.fgMuted)
              .backgroundColor(this.store.palette.recessed)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .padding(OrbitTokens.SPACE_SM)
              .width('100%')
              .constraintSize({ minHeight: 52, maxHeight: 96 })
              .onFocus(() => {
                this.store.shellStore.setTextInputFocused(true);
              })
              .onBlur(() => {
                this.store.shellStore.setTextInputFocused(false);
              })
              .onChange((val: string) => {
                this.prBodyInput = val;
                this.prDraftVersion += 1;
              })

            Row({ space: OrbitTokens.SPACE_XS }) {
              Toggle({ type: ToggleType.Switch, isOn: this.prDraft })
                .selectedColor(this.store.palette.accentPrimaryFill)
                .onChange((on: boolean) => {
                  this.prDraft = on;
                  this.prDraftVersion += 1;
                })
              Text($r('app.string.git_pr_draft'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .layoutWeight(1)
              Button($r('app.string.git_create_pr'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
                .height(30)
                .padding({ left: 12, right: 12 })
                .enabled(!this.writeBusy && this.prTitleInput.trim().length > 0)
                .onClick(() => {
                  dismissHarmonyKeyboard(this.getUIContext());
                  const host = this.tab.hostId.trim();
                  const cwd = nonBlankPath(this.tab.cwd);
                  const title = this.prTitleInput;
                  const body = this.prBodyInput;
                  const draft = this.prDraft;
                  const targetKey = this.currentTargetKey();
                  const tv = this.targetVersion;
                  const prVer = this.prDraftVersion;
                  this.writeSeq += 1;
                  const wSeq = this.writeSeq;
                  this.writeBusy = true;
                  this.store.gitCreateScopedPr(host, cwd, title, body, null, draft).then(
                    (res: GitCreatePrResultDto | null) => {
                      if (!this.disposed && tv === this.targetVersion && wSeq === this.writeSeq && targetKey === this.currentTargetKey()) {
                        this.writeBusy = false;
                        if (res !== null) {
                          if (prVer === this.prDraftVersion) {
                            this.prTitleInput = '';
                            this.prBodyInput = '';
                            this.prDraft = false;
                            this.prDraftVersion += 1;
                          }
                          void this.refreshCollabStatus();
                        }
                      }
                    }
                  );
                })
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }
        }
        .padding(OrbitTokens.SPACE_SM)
        .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .borderRadius(OrbitTokens.RADIUS_MD)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
      }

      Search({ value: this.filterQuery, placeholder: $r('app.string.git_filter_changed_placeholder') })
        .width('100%')
        .height(36)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .backgroundColor(this.store.palette.recessed)
        .fontColor(this.store.palette.fgPrimary)
        .placeholderColor(this.store.palette.fgMuted)
        .onFocus(() => {
          this.store.shellStore.setTextInputFocused(true);
        })
        .onBlur(() => {
          this.store.shellStore.setTextInputFocused(false);
        })
        .onChange((val: string) => {
          this.filterQuery = val;
        })

      if (this.diffModeState === 'loading' && this.flatDiffRows.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          LoadingProgress()
            .width(24)
            .height(24)
            .color(this.store.palette.accentPrimary)
          Text(this.stagedMode ? 'Loading staged diff…' : 'Loading unstaged diff…')
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (this.diffModeState === 'unverified' && this.flatDiffRows.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.exclamationmark_triangle_fill'))
            .fontSize(24)
            .fontColor([this.store.palette.statusWarning])
          Text(
            this.stagedMode
              ? 'Staged diff could not be verified for this scope'
              : 'Unstaged diff could not be verified for this scope'
          )
            .attributeModifier(orbitTextBody(this.store.palette.fgPrimary))
            .textAlign(TextAlign.Center)
          Text($r('app.string.git_diff_unconfirmed_notice'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .textAlign(TextAlign.Center)
            .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
          Button($r('app.string.git_refresh_diff'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
            )
            .height(32)
            .padding({ left: 14, right: 14 })
            .onClick(() => {
              this.reloadScopedGit(true, true);
            })
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (this.flatDiffRows.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.checkmark_circle'))
            .fontSize(26)
            .fontColor([this.store.palette.fgMuted])
          Text($r('app.string.git_working_tree_clean_scope'))
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: 0 }) {
          Repeat<FlatDiffRowDto>(this.flatDiffRows)
            .each((ri: RepeatItem<FlatDiffRowDto>) => {
              ListItem() {
                this.renderFlatRow(ri.item)
              }
            })
            .key((r: FlatDiffRowDto): string => r.key)
            .virtualScroll()
        }
        .width('100%')
        .layoutWeight(1)
        .cachedCount(8)
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
        })
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}

/**
 * Scoped Workspace Files Explorer tab (`kind === 'files'`).
 * Strictly reads `tab.hostId` and `tab.cwd`.
 * Restores baseline create file/folder entry workflow bound to `(tab.hostId, tab.cwd)`.
 */
@ComponentV2
export struct OrbitScopedFilesTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private files: string[] = [];
  @Local private query: string = '';
  @Local private showCreateForm: boolean = false;
  @Local private createPathInput: string = '';
  @Local private createAsDir: boolean = false;
  @Local private busy: boolean = false;

  private lastTargetKey: string = '';
  private loadSeq: number = 0;
  private disposed: boolean = false;
  @Local private store: OrbitHarmonyStore = orbitRuntime();

  private effectiveScopeKey(): string {
    const clean = this.scopeKey.trim();
    return clean.length > 0 ? clean : this.store.sidePanelStore.activeScopeKey;
  }

  private currentTargetKey(): string {
    const cwd = this.tab.cwd !== null ? this.tab.cwd : '';
    return encodeSidePanelKeyTuple(['files', this.scopeKey, this.tab.tabId, this.tab.hostId, cwd]);
  }

  aboutToAppear(): void {
    this.disposed = false;
    this.lastTargetKey = this.currentTargetKey();
    this.reloadFiles(true);
  }

  aboutToDisappear(): void {
    this.disposed = true;
    this.loadSeq += 1;
  }

  @Monitor('tab', 'scopeKey', 'store.gitPanelRevision', 'store.panelDataRevision')
  onTabChanged(_mon: IMonitor): void {
    const nextKey = this.currentTargetKey();
    if (nextKey !== this.lastTargetKey) {
      this.lastTargetKey = nextKey;
      this.loadSeq += 1;
      this.files = [];
      this.query = '';
      this.showCreateForm = false;
      this.createPathInput = '';
      this.createAsDir = false;
      this.busy = false;
      this.reloadFiles(true);
      return;
    }
    this.reloadFiles(false);
  }

  private reloadFiles(triggerRemote: boolean): void {
    const host = this.tab.hostId.trim();
    const cwd = nonBlankPath(this.tab.cwd);
    if (host.length === 0 || cwd.length === 0) {
      this.files = [];
      return;
    }
    this.files = this.store.loadScopedWorkspaceFiles(host, cwd, triggerRemote);
  }

  private async handleCreateEntry(): Promise<void> {
    const host = this.tab.hostId.trim();
    const cwd = nonBlankPath(this.tab.cwd);
    const clean = this.createPathInput.trim();
    if (host.length === 0 || cwd.length === 0 || clean.length === 0 || this.busy) {
      return;
    }
    const originScopeKey = this.effectiveScopeKey();
    const originAgentId = this.tab.agentId;
    const originParentHostId = this.tab.parentHostId;
    const originParentAgentId = this.tab.parentAgentId;
    const originCwd = this.tab.cwd;
    const targetKey = this.currentTargetKey();
    const seq = this.loadSeq;
    const isDir = this.createAsDir;
    dismissHarmonyKeyboard(this.getUIContext());
    this.busy = true;
    const ok = await this.store.createScopedWorkspaceEntry(host, cwd, clean, isDir, isDir ? null : '');
    if (this.disposed || seq !== this.loadSeq || targetKey !== this.currentTargetKey()) {
      return;
    }
    this.busy = false;
    if (ok) {
      this.createPathInput = '';
      this.createAsDir = false;
      this.showCreateForm = false;
      this.reloadFiles(true);
      if (!isDir) {
        const nextTab: OrbitSidePanelTab = {
          tabId: canonicalFileTabId(host, cwd, clean),
          kind: 'file',
          hostId: host,
          agentId: originAgentId,
          cwd: originCwd,
          path: clean,
          parentHostId: originParentHostId,
          parentAgentId: originParentAgentId,
          title: basenameOfPath(clean),
          closable: true
        };
        this.store.sidePanelStore.openTabInScope(originScopeKey, nextTab, { source: 'user', activate: true });
      }
    }
  }

  @Computed
  get filteredFiles(): string[] {
    const q = this.query.trim().toLowerCase();
    if (q.length === 0) {
      return this.files.slice(0, 200);
    }
    const out: string[] = [];
    for (let i = 0; i < this.files.length && out.length < 200; i++) {
      if (this.files[i].toLowerCase().indexOf(q) >= 0) {
        out.push(this.files[i]);
      }
    }
    return out;
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Column({ space: 2 }) {
          Text($r('app.string.files_panel_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
          Text(this.tab.cwd !== null && this.tab.cwd.length > 0 ? basenameOfPath(this.tab.cwd) : 'No directory')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button($r('app.string.files_new_entry'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonMeta(
              this.showCreateForm ? this.store.palette.fgPrimary : this.store.palette.onAccent,
              this.showCreateForm ? this.store.palette.subtleSurface : this.store.palette.accentPrimaryFill
            )
          )
          .height(30)
          .padding({ left: 10, right: 10 })
          .enabled(this.tab.hostId.length > 0 && (this.tab.cwd ?? '').length > 0)
          .onClick(() => {
            this.showCreateForm = !this.showCreateForm;
          })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(32)
        .height(32)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.files_refresh_list_aria'))
        .onClick(() => {
          this.reloadFiles(true);
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      if (this.showCreateForm) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          TextInput({
            text: this.createPathInput,
            placeholder: $r('app.string.files_new_path_placeholder')
          })
            .attributeModifier(
              orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
            )
            .height(38)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .width('100%')
            .onFocus(() => {
              this.store.shellStore.setTextInputFocused(true);
            })
            .onBlur(() => {
              this.store.shellStore.setTextInputFocused(false);
            })
            .onChange((val: string) => {
              this.createPathInput = val;
            })

          Row({ space: OrbitTokens.SPACE_XS }) {
            Toggle({ type: ToggleType.Switch, isOn: this.createAsDir })
              .selectedColor(this.store.palette.accentPrimaryFill)
              .onChange((checked: boolean) => {
                this.createAsDir = checked;
              })
            Text($r('app.string.files_create_as_dir'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
              .layoutWeight(1)
            Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(30)
              .padding({ left: 12, right: 12 })
              .enabled(!this.busy && this.createPathInput.trim().length > 0)
              .onClick(() => {
                void this.handleCreateEntry();
              })
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_SM)
        .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .borderRadius(OrbitTokens.RADIUS_MD)
        .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
      }

      Search({ value: this.query, placeholder: $r('app.string.files_filter_placeholder') })
        .width('100%')
        .height(36)
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .backgroundColor(this.store.palette.recessed)
        .fontColor(this.store.palette.fgPrimary)
        .placeholderColor(this.store.palette.fgMuted)
        .onFocus(() => {
          this.store.shellStore.setTextInputFocused(true);
        })
        .onBlur(() => {
          this.store.shellStore.setTextInputFocused(false);
        })
        .onChange((val: string) => {
          this.query = val;
        })

      if (this.filteredFiles.length === 0) {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph($r('sys.symbol.folder'))
            .fontSize(26)
            .fontColor([this.store.palette.fgMuted])
          Text($r('app.string.files_empty_indexed'))
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else {
        List({ space: OrbitTokens.SPACE_XS }) {
          Repeat<string>(this.filteredFiles)
            .each((ri: RepeatItem<string>) => {
              ListItem() {
                Button({ type: ButtonType.Normal, stateEffect: true }) {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    SymbolGlyph(resolveSemanticFileSymbol(basenameOfPath(ri.item)))
                      .fontSize(14)
                      .fontColor([this.store.palette.iconSecondary])
                    Column({ space: 2 }) {
                      Text(basenameOfPath(ri.item))
                        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                        .maxLines(1)
                        .textOverflow({ overflow: TextOverflow.Ellipsis })
                      if (ri.item.indexOf('/') >= 0) {
                        Text(ri.item)
                          .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                          .maxLines(1)
                          .textOverflow({ overflow: TextOverflow.Ellipsis })
                      }
                    }
                    .alignItems(HorizontalAlign.Start)
                    .layoutWeight(1)
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Center)
                }
                .width('100%')
                .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
                .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 6, bottom: 6 })
                .borderRadius(OrbitTokens.RADIUS_MD)
                .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
                .onClick(() => {
                  const cwd = this.tab.cwd !== null ? this.tab.cwd : '';
                  const originScopeKey = this.effectiveScopeKey();
                  const nextTab: OrbitSidePanelTab = {
                    tabId: canonicalFileTabId(this.tab.hostId, cwd, ri.item),
                    kind: 'file',
                    hostId: this.tab.hostId,
                    agentId: this.tab.agentId,
                    cwd: this.tab.cwd,
                    path: ri.item,
                    parentHostId: this.tab.parentHostId,
                    parentAgentId: this.tab.parentAgentId,
                    title: basenameOfPath(ri.item),
                    closable: true
                  };
                  this.store.sidePanelStore.openTabInScope(originScopeKey, nextTab, { source: 'user', activate: true });
                })
              }
            })
            .key((p: string): string => `sf_${p}`)
            .virtualScroll()
        }
        .width('100%')
        .layoutWeight(1)
        .cachedCount(5)
        .padding({
          left: OrbitTokens.SPACE_MD,
          right: OrbitTokens.SPACE_MD,
          bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
        })
        .scrollBar(BarState.Auto)
        .edgeEffect(EdgeEffect.Spring)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}

/**
 * Scoped File Viewer & Concurrency-Safe Editor tab (`kind === 'file'`).
 * - P0-1: Binds read, edit buffer, save request, and response strictly to immutable
 *   `(hostId, cwd, path)` + monotonic `loadSeq`. Switching tabs immediately bumps `loadSeq`
 *   and clears stale `fileView` + edit state BEFORE any early return.
 * - P0-2: Only enables editing for full, writable, non-binary text (`isWorkspaceFileEditable`).
 *   Normal save sends `expected_etag` + `force: false`, surfaces conflict banner while preserving
 *   draft, and only uses `force: true` upon explicit user confirmation.
 * - P1-8: Restores Copy, Rename, Delete, syntax-highlighted lines, and truncated notice.
 */
@ComponentV2
export struct OrbitScopedFileViewerTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private fileView: WorkspaceFileViewDto | null = null;
  @Local private loadedTarget: OrbitFileTarget | null = null;
  @Local private loading: boolean = false;
  @Local private saving: boolean = false;
  @Local private editing: boolean = false;
  @Local private editingTarget: OrbitFileTarget | null = null;
  @Local private editingEtag: string | null = null;
  @Local private draftContent: string = '';
  @Local private conflictNotice: string = '';
  @Local private showRenameForm: boolean = false;
  @Local private renameInput: string = '';
  @Local private confirmDelete: boolean = false;
  @Local private cappedHighlight: HighlightedCodeDto | null = null;
  @Local private showFullFile: boolean = false;
  @Local private loadingChunk: boolean = false;
  @Local private chunkConflictError: boolean = false;
  @Local private chunkBinaryTailLocked: boolean = false;
  @Local private chunkSequenceLocked: boolean = false;
  @Local private loadedByteOffset: number = 0;

  private lastScopeKey: string = '';
  private lastTabId: string = '';
  private lastTarget: OrbitFileTarget | null = null;
  private loadSeq: number = 0;
  private disposed: boolean = false;
  @Local private store: OrbitHarmonyStore = orbitRuntime();

  private effectiveScopeKey(): string {
    const clean = this.scopeKey.trim();
    return clean.length > 0 ? clean : this.store.sidePanelStore.activeScopeKey;
  }

  private currentTarget(): OrbitFileTarget {
    const hostId = this.tab.hostId.trim();
    const cwd = nonBlankPath(this.tab.cwd);
    const path = nonBlankPath(this.tab.path);
    return {
      hostId: hostId,
      cwd: cwd,
      path: path
    };
  }

  private targetsEqual(a: OrbitFileTarget | null, b: OrbitFileTarget | null): boolean {
    if (a === null || b === null) {
      return false;
    }
    return isSameFileTarget(a.hostId, a.cwd, a.path, b.hostId, b.cwd, b.path);
  }

  private matchesCurrentTarget(target: OrbitFileTarget): boolean {
    return this.targetsEqual(target, this.currentTarget());
  }

  private resetForTargetSwitch(): void {
    // Always invalidate in-flight reads/saves BEFORE any early return (P0-1 + R2-P0-1).
    this.loadSeq += 1;
    this.fileView = null;
    this.loadedTarget = null;
    this.loading = false;
    this.saving = false;
    this.editing = false;
    this.editingTarget = null;
    this.editingEtag = null;
    this.draftContent = '';
    this.conflictNotice = '';
    this.showRenameForm = false;
    this.renameInput = '';
    this.confirmDelete = false;
    this.cappedHighlight = null;
    this.showFullFile = false;
    this.loadingChunk = false;
    this.chunkConflictError = false;
    this.chunkBinaryTailLocked = false;
    this.chunkSequenceLocked = false;
    this.loadedByteOffset = 0;
  }

  aboutToAppear(): void {
    this.disposed = false;
    this.lastScopeKey = this.scopeKey;
    this.lastTabId = this.tab.tabId;
    this.lastTarget = this.currentTarget();
    void this.loadFile();
  }

  aboutToDisappear(): void {
    // Invalidate all in-flight file reads/saves/renames/deletes when unmounted (P1-1).
    this.disposed = true;
    this.loadSeq += 1;
  }

  @Monitor('tab', 'scopeKey')
  onTabChanged(_mon: IMonitor): void {
    const nextTarget = this.currentTarget();
    if (
      this.scopeKey === this.lastScopeKey &&
      this.tab.tabId === this.lastTabId &&
      this.targetsEqual(this.lastTarget, nextTarget)
    ) {
      return;
    }
    this.lastScopeKey = this.scopeKey;
    this.lastTabId = this.tab.tabId;
    this.lastTarget = nextTarget;
    this.resetForTargetSwitch();
    void this.loadFile();
  }

  @Computed
  get isTargetReady(): boolean {
    return (
      !this.loading &&
      this.fileView !== null &&
      this.loadedTarget !== null &&
      this.matchesCurrentTarget(this.loadedTarget)
    );
  }

  @Computed
  get canEditCurrentFile(): boolean {
    return this.isTargetReady && this.fileView !== null && isWorkspaceFileEditable(this.fileView);
  }

  private formatBytes(bytes: number): string {
    if (bytes < 1024) {
      return `${bytes} B`;
    }
    if (bytes < 1024 * 1024) {
      return `${(bytes / 1024).toFixed(1)} KB`;
    }
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }

  private fastCountLines(text: string): number {
    if (text.length === 0) {
      return 0;
    }
    let newlines = 0;
    for (let i = 0; i < text.length; i++) {
      if (text.charCodeAt(i) === 10) {
        newlines += 1;
      }
    }
    return text.charCodeAt(text.length - 1) === 10 ? newlines : newlines + 1;
  }

  private get canLoadNextChunk(): boolean {
    if (
      this.fileView === null ||
      !this.fileView.truncated ||
      this.loadingChunk ||
      this.chunkConflictError ||
      this.chunkBinaryTailLocked ||
      this.chunkSequenceLocked
    ) {
      return false;
    }
    const etag = (this.fileView.etag ?? '').trim();
    const maxChunkedCap = 16 * 1024 * 1024;
    return etag.length > 0 && this.loadedByteOffset > 0 && this.loadedByteOffset < maxChunkedCap &&
      this.loadedByteOffset < this.fileView.size_bytes;
  }

  private async loadFile(): Promise<void> {
    const target = this.currentTarget();
    this.loadSeq += 1;
    const seq = this.loadSeq;
    this.fileView = null;
    this.loadedTarget = null;
    this.conflictNotice = '';
    this.chunkConflictError = false;
    this.chunkBinaryTailLocked = false;
    this.chunkSequenceLocked = false;
    this.loadingChunk = false;
    this.loadedByteOffset = 0;

    if (target.hostId.length === 0 || target.cwd.length === 0 || target.path.length === 0) {
      this.loading = false;
      return;
    }

    this.loading = true;
    this.cappedHighlight = null;
    this.showFullFile = false;
    const view = await this.store.readScopedWorkspaceFile(target.hostId, target.cwd, target.path);
    if (!this.disposed && seq === this.loadSeq && this.matchesCurrentTarget(target)) {
      this.fileView = view;
      this.loadedTarget = view !== null ? target : null;
      if (view !== null) {
        this.loadedByteOffset = view.next_offset_bytes !== undefined && view.next_offset_bytes > 0
          ? view.next_offset_bytes
          : view.content.length;
      }
      if (
        view !== null &&
        view.content_kind === 'text' &&
        view.language.toLowerCase() !== 'markdown' &&
        view.content.length > 0
      ) {
        // P1-2 / R2-P1-3: Reuse bounded pre-highlighted prefix already computed on the async worker
        // thread, preserving `highlight_truncated` when per-line/total byte budgets clipped a line.
        if (view.highlighted_lines.length > 0) {
          const previewLines = view.highlighted_lines.length > 250
            ? view.highlighted_lines.slice(0, 250)
            : view.highlighted_lines;
          const totalLines = this.fastCountLines(view.content);
          this.cappedHighlight = {
            language: view.language,
            lines: previewLines,
            total_lines: Math.max(totalLines, previewLines.length),
            total_lines_exact: !view.truncated,
            truncated: view.highlight_truncated === true || totalLines > previewLines.length
          };
        } else if (view.language.toLowerCase() !== 'text') {
          this.cappedHighlight = this.store.highlightScopedWorkspaceCode(view.language, view.content, 250);
        }
      }
      this.loading = false;
    }
  }

  private async loadNextFileChunk(): Promise<void> {
    if (!this.isTargetReady || !this.canLoadNextChunk || this.fileView === null) {
      return;
    }
    const target = this.currentTarget();
    const seq = this.loadSeq;
    const expectedEtag = (this.fileView.etag ?? '').trim();
    const offset = this.loadedByteOffset;
    this.loadingChunk = true;
    const res = await this.store.readScopedWorkspaceFileChunk({
      host_id: target.hostId,
      cwd: target.cwd,
      path: target.path,
      offset_bytes: offset,
      max_bytes: 2 * 1024 * 1024,
      expected_etag: expectedEtag
    });
    if (this.disposed || seq !== this.loadSeq || !this.matchesCurrentTarget(target) || this.fileView === null) {
      return;
    }
    this.loadingChunk = false;
    if (!res.ok || res.data === null) {
      this.store.showToastResource($r('app.string.files_chunk_load_failed'), true, res.errorDetail);
      return;
    }
    // R2-P0-C: Strictly validate continuation chunk content kind, ETag, offset continuity, and
    // raw-byte forward progress before merging; any binary tail or mismatch locks read-only.
    const mergeRes = mergeWorkspaceFileChunk(
      this.fileView,
      offset,
      res.data,
      target.hostId,
      target.cwd,
      target.path,
      expectedEtag
    );
    this.fileView = mergeRes.mergedView;
    this.loadedByteOffset = mergeRes.nextLoadedRawBytes;
    if (!mergeRes.ok) {
      if (mergeRes.etagMismatch) {
        this.chunkConflictError = true;
        this.store.showToastResource($r('app.string.files_chunk_etag_conflict'), true);
      } else if (mergeRes.binaryTail) {
        this.chunkBinaryTailLocked = true;
        this.store.showToastResource($r('app.string.files_chunk_binary_tail_locked'), true);
      } else {
        this.chunkSequenceLocked = true;
        this.store.showToastResource($r('app.string.files_chunk_incomplete_locked'), true);
      }
      return;
    }
    if (this.cappedHighlight !== null) {
      const totalLines = this.fastCountLines(mergeRes.mergedView.content);
      this.cappedHighlight = {
        language: this.cappedHighlight.language,
        lines: this.cappedHighlight.lines,
        total_lines: Math.max(totalLines, this.cappedHighlight.lines.length),
        total_lines_exact: !mergeRes.mergedView.truncated,
        truncated: this.cappedHighlight.truncated ||
          mergeRes.mergedView.highlight_truncated === true ||
          totalLines > this.cappedHighlight.lines.length
      };
    }
    this.showFullFile = true;
  }

  private effectiveHighlightedLines(): HighlightedLineDto[] {
    if (this.showFullFile) {
      return [];
    }
    if (this.cappedHighlight !== null && this.cappedHighlight.lines.length > 0) {
      return this.cappedHighlight.lines;
    }
    return this.fileView !== null ? this.fileView.highlighted_lines : [];
  }

  private tokenColorForTheme(tok: HighlightedTokenDto): ResourceColor {
    if (this.store.palette.isDark) {
      return tok.fg_hex.length > 0 ? tok.fg_hex : this.store.palette.fgPrimary;
    }
    const light = tok.light_fg_hex !== undefined && tok.light_fg_hex.length > 0 ? tok.light_fg_hex : tok.fg_hex;
    return light.length > 0 ? light : this.store.palette.fgPrimary;
  }

  private beginEdit(): void {
    if (!this.canEditCurrentFile || this.fileView === null || this.loadedTarget === null) {
      return;
    }
    this.editingTarget = {
      hostId: this.loadedTarget.hostId,
      cwd: this.loadedTarget.cwd,
      path: this.loadedTarget.path
    };
    this.editingEtag = this.fileView.etag !== undefined && this.fileView.etag !== null && this.fileView.etag.length > 0
      ? this.fileView.etag
      : null;
    this.draftContent = this.fileView.content;
    this.conflictNotice = '';
    this.showRenameForm = false;
    this.confirmDelete = false;
    this.editing = true;
  }

  private async saveFile(forceOverwrite: boolean): Promise<void> {
    if (!this.editing || this.saving || !this.isTargetReady) {
      return;
    }
    const target = this.currentTarget();
    if (
      target.hostId.length === 0 ||
      target.cwd.length === 0 ||
      target.path.length === 0 ||
      !this.targetsEqual(this.editingTarget, target) ||
      !this.targetsEqual(this.loadedTarget, target)
    ) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const saveSeq = this.loadSeq;
    const body = this.draftContent;
    const expectedEtag = forceOverwrite ? null : this.editingEtag;
    this.saving = true;
    const outcome: OrbitWorkspaceFileWriteOutcome = await this.store.saveScopedWorkspaceFile(
      target.hostId,
      target.cwd,
      target.path,
      body,
      expectedEtag,
      forceOverwrite
    );
    if (this.disposed || saveSeq !== this.loadSeq || !this.matchesCurrentTarget(target)) {
      return;
    }
    this.saving = false;
    if (outcome.conflict) {
      // Preserve draft & edit mode so user can inspect, reload disk, or explicitly overwrite (P0-2).
      this.conflictNotice = outcome.errorMessage.length > 0
        ? outcome.errorMessage
        : 'File changed on host since it was opened.';
      return;
    }
    if (outcome.ok) {
      this.editing = false;
      this.editingTarget = null;
      this.editingEtag = null;
      this.conflictNotice = '';
      await this.loadFile();
    }
  }

  private async handleRename(): Promise<void> {
    if (!this.isTargetReady || this.saving) {
      return;
    }
    const target = this.currentTarget();
    const nextPath = this.renameInput.trim();
    if (nextPath.length === 0 || nextPath === target.path) {
      this.showRenameForm = false;
      return;
    }
    const originScopeKey = this.effectiveScopeKey();
    const oldTabId = this.tab.tabId;
    const originAgentId = this.tab.agentId;
    const originCwd = this.tab.cwd;
    const originParentHostId = this.tab.parentHostId;
    const originParentAgentId = this.tab.parentAgentId;
    dismissHarmonyKeyboard(this.getUIContext());
    const seq = this.loadSeq;
    this.saving = true;
    const ok = await this.store.renameScopedWorkspaceEntry(target.hostId, target.cwd, target.path, nextPath);
    if (this.disposed || seq !== this.loadSeq || !this.matchesCurrentTarget(target)) {
      return;
    }
    this.saving = false;
    if (ok) {
      this.showRenameForm = false;
      const renamedTab: OrbitSidePanelTab = {
        tabId: canonicalFileTabId(target.hostId, target.cwd, nextPath),
        kind: 'file',
        hostId: target.hostId,
        agentId: originAgentId,
        cwd: originCwd,
        path: nextPath,
        parentHostId: originParentHostId,
        parentAgentId: originParentAgentId,
        title: basenameOfPath(nextPath),
        closable: true
      };
      this.store.sidePanelStore.openTabInScope(originScopeKey, renamedTab, { source: 'user', activate: true });
      this.store.sidePanelStore.closeTabInScope(originScopeKey, oldTabId);
    }
  }

  private async handleDelete(): Promise<void> {
    if (!this.isTargetReady || this.saving) {
      return;
    }
    const target = this.currentTarget();
    const originScopeKey = this.effectiveScopeKey();
    const oldTabId = this.tab.tabId;
    const seq = this.loadSeq;
    this.saving = true;
    const ok = await this.store.deleteScopedWorkspaceEntry(target.hostId, target.cwd, target.path);
    if (this.disposed || seq !== this.loadSeq || !this.matchesCurrentTarget(target)) {
      return;
    }
    this.saving = false;
    if (ok) {
      this.confirmDelete = false;
      this.store.sidePanelStore.closeTabInScope(originScopeKey, oldTabId);
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_XS }) {
      // Top file header & primary action bar
      Row({ space: OrbitTokens.SPACE_XS }) {
        Column({ space: 2 }) {
          Text(this.tab.path !== null && this.tab.path.length > 0 ? basenameOfPath(this.tab.path) : this.tab.title)
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          Text(
            this.isTargetReady && this.fileView !== null
              ? `${this.fileView.language} · ${this.formatBytes(this.fileView.size_bytes)}`
              : (this.tab.path !== null ? this.tab.path : '')
          )
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.isTargetReady && this.fileView !== null) {
          if (this.editing) {
            Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
              .height(30)
              .padding({ left: 10, right: 10 })
              .enabled(!this.saving)
              .onClick(() => {
                this.editing = false;
                this.editingTarget = null;
                this.conflictNotice = '';
              })
            Button(this.saving ? 'Saving…' : 'Save', { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
              .height(30)
              .padding({ left: 10, right: 10 })
              .enabled(!this.saving)
              .onClick(() => {
                void this.saveFile(false);
              })
          } else {
            if (this.canEditCurrentFile) {
              Button($r('app.string.action_edit'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface))
                .height(30)
                .padding({ left: 10, right: 10 })
                .onClick(() => {
                  this.beginEdit();
                })
            }
            Button($r('app.string.files_action_quote'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft))
              .height(30)
              .padding({ left: 10, right: 10 })
              .onClick(() => {
                if (this.isTargetReady && this.fileView !== null && this.loadedTarget !== null) {
                  this.store.quoteScopedFileIntoComposer(
                    this.loadedTarget.hostId,
                    this.loadedTarget.cwd,
                    this.fileView.path,
                    this.fileView.content
                  );
                }
              })
          }
        }
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      // Secondary file actions: Copy, Rename, Delete, Reload
      if (this.isTargetReady && this.fileView !== null && !this.editing) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          if (this.fileView.content_kind === 'text') {
            Button($r('app.string.action_copy'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
              .height(28)
              .padding({ left: 10, right: 10 })
              .onClick(() => {
                if (this.isTargetReady && this.fileView !== null) {
                  this.store.copyToClipboard(this.fileView.content, `Copied ${basenameOfPath(this.fileView.path)}`);
                }
              })
          }

          Button($r('app.string.action_rename'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(28)
            .padding({ left: 10, right: 10 })
            .enabled(!this.saving)
            .onClick(() => {
              this.renameInput = this.tab.path !== null ? this.tab.path : '';
              this.showRenameForm = !this.showRenameForm;
              this.confirmDelete = false;
            })

          Button($r('app.string.action_delete'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft))
            .height(28)
            .padding({ left: 10, right: 10 })
            .enabled(!this.saving)
            .onClick(() => {
              this.confirmDelete = !this.confirmDelete;
              this.showRenameForm = false;
            })

          Blank()

          Button({ type: ButtonType.Circle, stateEffect: true }) {
            SymbolGlyph($r('sys.symbol.arrow_clockwise'))
              .fontSize(14)
              .fontColor([this.store.palette.iconPrimary])
          }
          .width(28)
          .height(28)
          .padding(0)
          .backgroundColor(this.store.palette.subtleSurface)
          .accessibilityText($r('app.string.files_reload_file_aria'))
          .onClick(() => {
            void this.loadFile();
          })
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .alignItems(VerticalAlign.Center)
      }

      if (this.showRenameForm && this.isTargetReady) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          TextInput({ text: this.renameInput, placeholder: $r('app.string.files_new_path_placeholder') })
            .attributeModifier(
              orbitInputMono(this.store.palette.fgPrimary, this.store.palette.fgMuted, this.store.palette.recessed)
            )
            .height(36)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .layoutWeight(1)
            .onFocus(() => {
              this.store.shellStore.setTextInputFocused(true);
            })
            .onBlur(() => {
              this.store.shellStore.setTextInputFocused(false);
            })
            .onChange((val: string) => {
              this.renameInput = val;
            })
          Button($r('app.string.action_confirm'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
            .height(36)
            .padding({ left: 12, right: 12 })
            .enabled(!this.saving)
            .onClick(() => {
              void this.handleRename();
            })
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }

      if (this.confirmDelete && this.isTargetReady) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          Text(`Delete ${basenameOfPath(this.tab.path ?? '')}?`)
            .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium))
            .layoutWeight(1)
          Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(28)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.confirmDelete = false;
            })
          Button($r('app.string.action_delete'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
            .height(28)
            .padding({ left: 10, right: 10 })
            .enabled(!this.saving)
            .onClick(() => {
              void this.handleDelete();
            })
        }
        .width('100%')
        .padding(OrbitTokens.SPACE_SM)
        .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        .backgroundColor(this.store.palette.statusDangerSoft)
        .borderRadius(OrbitTokens.RADIUS_MD)
      }

      // Optimistic-concurrency Save Conflict Banner (P0-2)
      if (this.conflictNotice.length > 0 && this.editing) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Text(this.conflictNotice)
            .attributeModifier(orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium))
            .width('100%')
          Row({ space: OrbitTokens.SPACE_XS }) {
            Button($r('app.string.files_reload_disk'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.listCard))
              .height(28)
              .padding({ left: 10, right: 10 })
              .enabled(!this.saving)
              .onClick(() => {
                this.editing = false;
                this.editingTarget = null;
                this.conflictNotice = '';
                void this.loadFile();
              })
            Button($r('app.string.files_overwrite_anyway'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
              .height(28)
              .padding({ left: 10, right: 10 })
              .enabled(!this.saving)
              .onClick(() => {
                void this.saveFile(true);
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

      // Truncated / Non-editable notices (P0-2)
      if (this.isTargetReady && this.fileView !== null) {
        if (this.fileView.truncated) {
          Text($r('app.string.files_truncated_notice'))
            .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        } else if (this.fileView.content_kind !== 'text') {
          Text(
            this.fileView.content_kind === 'image'
              ? 'Image file (read-only preview)'
              : 'Binary file cannot be edited in text mode'
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        } else if (this.fileView.writable !== true) {
          Text($r('app.string.files_readonly_notice'))
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .width('100%')
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
        }
      }

      if (this.loading) {
        Column() {
          Blank()
          LoadingProgress()
            .width(24)
            .height(24)
            .color(this.store.palette.accentPrimary)
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (!this.isTargetReady || this.fileView === null) {
        Column() {
          Blank()
          Text($r('app.string.files_unavailable_scope'))
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (this.editing) {
        TextArea({ text: this.draftContent })
          .fontSize(OrbitTokens.FONT_META)
          .fontFamily(OrbitTokens.FAMILY_MONO)
          .fontColor(this.store.palette.fgPrimary)
          .backgroundColor(this.store.palette.code)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .padding(OrbitTokens.SPACE_SM)
          .margin({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, bottom: OrbitTokens.SPACE_MD })
          .layoutWeight(1)
          .onFocus(() => {
            this.store.shellStore.setTextInputFocused(true);
          })
          .onBlur(() => {
            this.store.shellStore.setTextInputFocused(false);
          })
          .onChange((val: string) => {
            this.draftContent = val;
          })
      } else if (this.fileView.content_kind !== 'text') {
        Column({ space: OrbitTokens.SPACE_SM }) {
          Blank()
          SymbolGlyph(resolveSemanticFileSymbol(basenameOfPath(this.fileView.path)))
            .fontSize(28)
            .fontColor([this.store.palette.fgMuted])
          Text(`${basenameOfPath(this.fileView.path)} · ${this.formatBytes(this.fileView.size_bytes)}`)
            .attributeModifier(orbitTextBody(this.store.palette.fgSecondary))
          Blank()
        }
        .width('100%')
        .layoutWeight(1)
        .alignItems(HorizontalAlign.Center)
      } else if (this.fileView.markdown_blocks.length > 0 && this.fileView.language.toLowerCase() === 'markdown') {
        Scroll() {
          Column() {
            RichMarkdownView({
              blocks: this.fileView.markdown_blocks,
              fallbackText: this.fileView.content
            })
          }
          .width('100%')
          .padding({
            left: OrbitTokens.SPACE_MD,
            right: OrbitTokens.SPACE_MD,
            bottom: Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
          })
        }
        .width('100%')
        .layoutWeight(1)
        .scrollBar(BarState.Auto)
      } else if (this.effectiveHighlightedLines().length > 0) {
        Column({ space: 0 }) {
          if (this.chunkConflictError) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text($r('app.string.files_chunk_etag_conflict'))
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .layoutWeight(1)
              Button($r('app.string.files_reload_disk'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
                .height(26)
                .padding({ left: 10, right: 10 })
                .onClick(() => {
                  void this.loadFile();
                })
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.statusDangerSoft)
            .alignItems(VerticalAlign.Center)
          } else if (this.chunkBinaryTailLocked || this.chunkSequenceLocked) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(
                this.chunkBinaryTailLocked
                  ? $r('app.string.files_chunk_binary_tail_locked')
                  : $r('app.string.files_chunk_incomplete_locked')
              )
                .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                .layoutWeight(1)
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.statusWarningSoft)
            .alignItems(VerticalAlign.Center)
          } else if (this.fileView.truncated) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(
                this.canLoadNextChunk || this.loadingChunk
                  ? $r(
                      'app.string.files_source_truncated_banner',
                      this.formatBytes(this.loadedByteOffset),
                      this.formatBytes(this.fileView.size_bytes)
                    )
                  : $r(
                      'app.string.files_chunk_limit_reached',
                      this.formatBytes(this.loadedByteOffset),
                      this.formatBytes(this.fileView.size_bytes)
                    )
              )
                .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                .layoutWeight(1)
              if (this.canLoadNextChunk || this.loadingChunk) {
                Button(
                  this.loadingChunk
                    ? $r('app.string.files_loading_chunk')
                    : $r('app.string.files_load_next_chunk'),
                  { type: ButtonType.Capsule, stateEffect: true }
                )
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
                  )
                  .height(26)
                  .padding({ left: 10, right: 10 })
                  .enabled(!this.loadingChunk)
                  .onClick(() => {
                    void this.loadNextFileChunk();
                  })
              }
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.subtleSurface)
            .alignItems(VerticalAlign.Center)
          }

          if (this.cappedHighlight !== null && this.cappedHighlight.truncated) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(
                this.cappedHighlight.total_lines_exact === false
                  ? $r(
                    'app.string.files_preview_capped_lower_bound',
                    this.cappedHighlight.lines.length,
                    this.cappedHighlight.total_lines
                  )
                  : $r(
                    'app.string.files_preview_capped',
                    this.cappedHighlight.lines.length,
                    this.cappedHighlight.total_lines
                  )
              )
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .layoutWeight(1)
              Button($r('app.string.files_load_full'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(
                  orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
                )
                .height(26)
                .padding({ left: 10, right: 10 })
                .onClick(() => {
                  this.showFullFile = true;
                })
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.subtleSurface)
            .alignItems(VerticalAlign.Center)
          }

          List({ space: 2 }) {
            Repeat<HighlightedLineDto>(this.effectiveHighlightedLines())
              .each((ri: RepeatItem<HighlightedLineDto>) => {
                ListItem() {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Text(ri.item.line_number.toString().padStart(4, ' '))
                      .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                      .width(34)
                      .textAlign(TextAlign.End)
                    Text() {
                      ForEach(
                        ri.item.tokens,
                        (tok: HighlightedTokenDto) => {
                          Span(tok.text)
                            .font(orbitSpanMonoFont(tok.bold ? FontWeight.Bold : FontWeight.Normal))
                            .fontColor(this.tokenColorForTheme(tok))
                        },
                        (tok: HighlightedTokenDto, tIdx: number): string =>
                          `${ri.item.line_number}_${tIdx}_${tok.text.length}`
                      )
                    }
                    .copyOption(CopyOptions.LocalDevice)
                    .layoutWeight(1)
                  }
                  .width('100%')
                  .alignItems(VerticalAlign.Top)
                }
              })
              .key((ln: HighlightedLineDto): string => `shl_${fileTargetRenderSig(this.loadedTarget)}_${ln.line_number}`)
              .virtualScroll()
          }
          .width('100%')
          .layoutWeight(1)
          .cachedCount(8)
          .padding({
            left: OrbitTokens.SPACE_SM,
            right: OrbitTokens.SPACE_SM,
            top: OrbitTokens.SPACE_SM,
            bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
          })
          .backgroundColor(this.store.palette.code)
          .scrollBar(BarState.Auto)
        }
        .width('100%')
        .layoutWeight(1)
      } else {
        Column({ space: 0 }) {
          if (this.chunkConflictError) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text($r('app.string.files_chunk_etag_conflict'))
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .layoutWeight(1)
              Button($r('app.string.files_reload_disk'), { type: ButtonType.Capsule, stateEffect: true })
                .attributeModifier(orbitButtonMeta(this.store.palette.onAccent, this.store.palette.statusDanger))
                .height(26)
                .padding({ left: 10, right: 10 })
                .onClick(() => {
                  void this.loadFile();
                })
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.statusDangerSoft)
            .alignItems(VerticalAlign.Center)
          } else if (this.chunkBinaryTailLocked || this.chunkSequenceLocked) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(
                this.chunkBinaryTailLocked
                  ? $r('app.string.files_chunk_binary_tail_locked')
                  : $r('app.string.files_chunk_incomplete_locked')
              )
                .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                .layoutWeight(1)
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.statusWarningSoft)
            .alignItems(VerticalAlign.Center)
          } else if (this.fileView.truncated) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(
                $r(
                  'app.string.files_showing_partial',
                  this.formatBytes(this.loadedByteOffset),
                  this.formatBytes(this.fileView.size_bytes)
                )
              )
                .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                .layoutWeight(1)
              if (this.canLoadNextChunk || this.loadingChunk) {
                Button(
                  this.loadingChunk
                    ? $r('app.string.files_loading_chunk')
                    : $r('app.string.files_load_next_chunk'),
                  { type: ButtonType.Capsule, stateEffect: true }
                )
                  .attributeModifier(
                    orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
                  )
                  .height(26)
                  .padding({ left: 10, right: 10 })
                  .enabled(!this.loadingChunk)
                  .onClick(() => {
                    void this.loadNextFileChunk();
                  })
              }
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.subtleSurface)
            .alignItems(VerticalAlign.Center)
          } else if (this.showFullFile && this.cappedHighlight !== null) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              Text(
                this.cappedHighlight.total_lines_exact === false
                  ? $r('app.string.files_showing_full_lower_bound', this.cappedHighlight.total_lines)
                  : $r('app.string.files_showing_full', this.cappedHighlight.total_lines)
              )
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .layoutWeight(1)
            }
            .width('100%')
            .padding({
              left: OrbitTokens.SPACE_MD,
              right: OrbitTokens.SPACE_MD,
              top: OrbitTokens.SPACE_XS,
              bottom: OrbitTokens.SPACE_XS
            })
            .backgroundColor(this.store.palette.subtleSurface)
          }
          Scroll() {
            Text(this.fileView.content)
              .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
              .copyOption(CopyOptions.LocalDevice)
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
          }
          .width('100%')
          .layoutWeight(1)
          .backgroundColor(this.store.palette.code)
          .scrollBar(BarState.Auto)
        }
        .width('100%')
        .layoutWeight(1)
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
