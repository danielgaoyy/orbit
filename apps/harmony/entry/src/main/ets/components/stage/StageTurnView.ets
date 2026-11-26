import {
  DiffLineDto,
  InlineSpanDto,
  PromptBadgeDto,
  RichBlockDto,
  ToolStepDescriptorDto,
  TurnFileDiffSummaryDto,
  basenameOfPath,
  formatRelativeTimeLabel,
  stripWorkspacePrefix
} from '../../model/OrbitTypes';
import {
  OrbitLiveSegmentModel,
  OrbitStageTurnModel,
  OrbitTimelineItemModel
} from '../../model/viewmodels/OrbitStageModels';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitSpanMetaFont,
  orbitSpanMonoFont,
  orbitSpanSubheadFont,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  resolveSemanticFileSymbol
} from '../../theme/OrbitTokens';
import { OrbitCopyIconButton } from '../common/OrbitCommonControls';
import { RichMarkdownView } from './RichMarkdownView';

interface MiniDiffPreviewRow {
  key: string;
  isFileHeader: boolean;
  filePath: string;
  line: DiffLineDto | null;
}

interface CleanedPromptText {
  body: string;
  inferredSlash: string;
}

const LEGACY_WRAPPER_MARKERS: string[] = [
  'Attached image file:',
  'Referenced workspace files:',
  'Working Tree Diff:',
  'Staged Diff:',
  'Active terminal output:',
  'Reference `',
  'Quoted diff lines:',
  'Quoted assistant context:'
];

function cleanLegacyPromptText(rawText: string): CleanedPromptText {
  let working = rawText.replace(/<WORKFLOW_INSTRUCTION[^>]*>[\s\S]*?<\/WORKFLOW_INSTRUCTION>\s*/gi, '').trim();

  let startsWithLegacy = false;
  for (let i = 0; i < LEGACY_WRAPPER_MARKERS.length; i++) {
    if (working.startsWith(LEGACY_WRAPPER_MARKERS[i])) {
      startsWithLegacy = true;
      break;
    }
  }
  if (startsWithLegacy && working.indexOf('\n\n') >= 0) {
    const reqIdx = working.indexOf('<USER_REQUEST>');
    if (reqIdx >= 0) {
      working = working.substring(reqIdx + '<USER_REQUEST>'.length);
      const endIdx = working.indexOf('</USER_REQUEST>');
      if (endIdx >= 0) {
        working = working.substring(0, endIdx);
      }
      working = working.trim();
    } else {
      const blocks = working.split('\n\n');
      for (let b = blocks.length - 1; b >= 0; b--) {
        const cand = blocks[b].trim();
        let isMarker = false;
        for (let m = 0; m < LEGACY_WRAPPER_MARKERS.length; m++) {
          if (cand.startsWith(LEGACY_WRAPPER_MARKERS[m])) {
            isMarker = true;
            break;
          }
        }
        if (cand.length > 0 && !isMarker && !cand.startsWith('```')) {
          working = cand;
          break;
        }
      }
    }
  }

  const slashCmds: string[] = ['/plan', '/review', '/compact'];
  const lower = working.toLowerCase();
  for (let i = 0; i < slashCmds.length; i++) {
    const cmd = slashCmds[i];
    if (lower === cmd) {
      return { body: '', inferredSlash: cmd };
    }
    if (lower.startsWith(`${cmd} `) || lower.startsWith(`${cmd}\n`) || lower.startsWith(`${cmd}\t`)) {
      return {
        body: working.substring(cmd.length).trim(),
        inferredSlash: cmd
      };
    }
  }
  return { body: working, inferredSlash: '' };
}

function isBareCompactionPrompt(prompt: OrbitTimelineItemModel): boolean {
  const cleaned = cleanLegacyPromptText(prompt.rawText);
  const badges = prompt.badges;
  let hasCompactBadge = false;
  for (let i = 0; i < badges.length; i++) {
    if (badges[i].kind === 'slash' && badges[i].label.toLowerCase() === '/compact') {
      hasCompactBadge = true;
      break;
    }
  }
  if (hasCompactBadge && cleaned.body.length === 0) {
    return true;
  }
  return prompt.rawText.trim().toLowerCase() === '/compact';
}

function stepDescriptorSymbol(desc: ToolStepDescriptorDto | null): Resource {
  if (desc === null) {
    return $r('sys.symbol.wrench_and_screwdriver');
  }
  const k = desc.kind.toLowerCase();
  if (k === 'file_read' || k === 'read') {
    return resolveSemanticFileSymbol(basenameOfPath(desc.target));
  }
  if (k === 'file_edit' || k === 'file_create' || k === 'edit' || k === 'write') {
    return $r('sys.symbol.doc_plaintext_and_pencil');
  }
  if (k === 'shell' || k === 'bash' || k === 'terminal') {
    return $r('sys.symbol.code');
  }
  if (k === 'search' || k === 'web') {
    const lowerTarget = desc.target.toLowerCase();
    if (
      lowerTarget.startsWith('http://') ||
      lowerTarget.startsWith('https://') ||
      lowerTarget.indexOf('web') >= 0
    ) {
      return $r('sys.symbol.website');
    }
    return $r('sys.symbol.magnifyingglass');
  }
  if (k === 'subagent') {
    return $r('sys.symbol.person_2');
  }
  return $r('sys.symbol.wrench_and_screwdriver');
}

@ComponentV2
struct StepItemRow {
  @Param item: OrbitTimelineItemModel = new OrbitTimelineItemModel({ item_type: 'tool_call', seq: 0 });
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private toggleExpanded(): void {
    this.getUIContext().animateTo({ duration: 180, curve: Curve.EaseInOut }, () => {
      this.item.detailExpanded = !this.item.detailExpanded;
    });
    this.store.triggerHaptic(6);
  }

  private hasStepDiffFiles(): boolean {
    return this.item.diffFiles.length > 0;
  }

  private firstDiffFilePath(): string {
    if (this.item.diffFiles.length > 0) {
      return this.item.diffFiles[0].path;
    }
    if (this.item.detail !== null && (this.item.detail.path ?? '').length > 0) {
      return this.item.detail.path ?? '';
    }
    return '';
  }

  private miniDiffPreviewRows(): MiniDiffPreviewRow[] {
    const files = this.item.diffFiles;
    const rows: MiniDiffPreviewRow[] = [];
    const multiFile = files.length > 1;
    let diffLineCount = 0;
    for (let fIdx = 0; fIdx < files.length && diffLineCount < 16; fIdx++) {
      const df = files[fIdx];
      if (multiFile) {
        rows.push({
          key: `fh_${df.path}_${fIdx}`,
          isFileHeader: true,
          filePath: df.path,
          line: null
        });
      }
      for (let lIdx = 0; lIdx < df.lines.length && diffLineCount < 16; lIdx++) {
        rows.push({
          key: `dl_${df.path}_${lIdx}`,
          isFileHeader: false,
          filePath: df.path,
          line: df.lines[lIdx]
        });
        diffLineCount++;
      }
    }
    return rows;
  }

  private buildFullStepSnippet(desc: ToolStepDescriptorDto | null): string {
    const det = this.item.detail;
    const parts: string[] = [];
    if (det !== null) {
      if ((det.command ?? '').trim().length > 0) {
        parts.push(`$ ${(det.command ?? '').trim()}`);
      }
      if (det.cwd !== undefined && det.cwd !== null && det.cwd.trim().length > 0) {
        parts.push(`cwd: ${det.cwd.trim()}`);
      }
      if ((det.path ?? '').trim().length > 0) {
        let pathLine = (det.path ?? '').trim();
        if (det.start_line !== undefined && det.start_line !== null) {
          pathLine = det.end_line !== undefined && det.end_line !== null
            ? `${pathLine}:${det.start_line}-${det.end_line}`
            : `${pathLine}:${det.start_line}`;
        }
        parts.push(pathLine);
      }
      if ((det.query ?? '').trim().length > 0) {
        parts.push(`query: ${(det.query ?? '').trim()}`);
      }
      if (det.scope !== undefined && det.scope !== null && det.scope.trim().length > 0) {
        parts.push(`scope: ${det.scope.trim()}`);
      }
      if ((det.role ?? '').trim().length > 0) {
        parts.push(`subagent: ${(det.role ?? '').trim()}`);
      }
      if ((det.tool_name ?? '').trim().length > 0 && parts.length === 0) {
        parts.push(`tool: ${(det.tool_name ?? '').trim()}`);
      }
      if ((det.prompt_preview ?? '').trim().length > 0) {
        parts.push((det.prompt_preview ?? '').trim());
      }
      if (
        det.output_preview !== undefined &&
        det.output_preview !== null &&
        det.output_preview.trim().length > 0
      ) {
        parts.push(det.output_preview.trim());
      } else if (
        det.unified_diff !== undefined &&
        det.unified_diff !== null &&
        det.unified_diff.trim().length > 0
      ) {
        parts.push(det.unified_diff.trim());
      } else if ((det.arguments_json ?? '').trim().length > 0) {
        parts.push((det.arguments_json ?? '').trim());
      }
      if (det.exit_code !== undefined && det.exit_code !== null && det.exit_code !== 0) {
        parts.push(`exit code: ${det.exit_code}`);
      }
    }
    if (parts.length === 0) {
      if (desc !== null) {
        const header = desc.target.length > 0 ? `${desc.verb} ${desc.target}` : desc.verb;
        if (desc.secondary_meta !== null && desc.secondary_meta.length > 0) {
          return `${header}\n${desc.secondary_meta}`;
        }
        return header;
      }
      return (this.item.title.length > 0 ? this.item.title : (this.item.rawText || 'Tool call completed')).trim();
    }
    return parts.join('\n');
  }

  build() {
    if (this.item.itemType === 'assistant_thinking') {
      if (this.item.rawText.trim().length > 0) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            SymbolGlyph($r('sys.symbol.wand_and_stars'))
              .fontSize(13)
              .fontColor([this.store.palette.fgMuted])
            Text('Thought')
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
            SymbolGlyph(
              this.item.detailExpanded
                ? $r('sys.symbol.chevron_down')
                : $r('sys.symbol.chevron_right')
            )
              .fontSize(12)
              .fontColor([this.store.palette.fgMuted])
          }
          .constraintSize({ minHeight: 36 })
          .alignItems(VerticalAlign.Center)
          .onClick(() => {
            this.toggleExpanded();
          })

          if (this.item.detailExpanded) {
            Column() {
              Text(this.item.rawText.trim())
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                .copyOption(CopyOptions.LocalDevice)
                .padding({ bottom: OrbitTokens.SPACE_XS })
            }
            .width('100%')
            .alignItems(HorizontalAlign.Start)
            .clip(true)
          }
        }
        .alignItems(HorizontalAlign.Start)
        .width('100%')
      }
    } else if (this.item.itemType === 'assistant_message') {
      if (this.item.rawText.trim().length > 0) {
        RichMarkdownView({
          blocks: this.item.rawBlocks,
          fallbackText: this.item.rawText
        })
      }
    } else if (this.item.itemType === 'tool_call') {
      Column({ space: OrbitTokens.SPACE_XS }) {
        this.renderDescriptorRow(this.item.stepDescriptor)

        if (this.item.detailExpanded) {
          Column() {
            this.renderExpandedToolDetail(this.item.stepDescriptor)
          }
          .width('100%')
          .clip(true)
        }
      }
      .alignItems(HorizontalAlign.Start)
      .width('100%')
    }
  }

  @Builder
  private renderDescriptorRow(desc: ToolStepDescriptorDto | null) {
    if (desc !== null) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        if (desc.is_running) {
          LoadingProgress()
            .width(14)
            .height(14)
            .color(this.store.palette.accentPrimary)
        } else {
          SymbolGlyph(stepDescriptorSymbol(desc))
            .fontSize(13)
            .fontColor([
              desc.is_failed || desc.is_denied
                ? this.store.palette.statusDanger
                : this.store.palette.fgMuted
            ])
        }

        Text() {
          Span(desc.verb)
            .font(orbitSpanSubheadFont(FontWeight.Normal))
            .fontColor(
              desc.is_failed || desc.is_denied
                ? this.store.palette.statusDanger
                : this.store.palette.fgSecondary
            )
          if (desc.target.length > 0) {
            Span(`  ${stripWorkspacePrefix(desc.target, this.store.activeCwd)}`)
              .font(orbitSpanMonoFont(FontWeight.Medium))
              .fontColor(
                desc.is_failed || desc.is_denied
                  ? this.store.palette.statusDanger
                  : this.store.palette.fgPrimary
              )
          }
          if (desc.secondary_meta !== null && desc.secondary_meta.length > 0) {
            Span(`  ${desc.secondary_meta}`)
              .font(orbitSpanMetaFont())
              .fontColor(this.store.palette.fgMuted)
          }
        }
        .maxLines(1)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .layoutWeight(1)

        if (desc.additions > 0 || desc.deletions > 0) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            if (desc.additions > 0) {
              Text(`+${desc.additions}`)
                .attributeModifier(
                  orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium)
                )
            }
            if (desc.deletions > 0) {
              Text(`-${desc.deletions}`)
                .attributeModifier(
                  orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium)
                )
            }
          }
        } else if (desc.exit_code !== null && desc.exit_code !== 0) {
          Text(`exit ${desc.exit_code}`)
            .attributeModifier(orbitTextMono(this.store.palette.statusDanger, FontWeight.Medium))
        }

        SymbolGlyph(
          this.item.detailExpanded
            ? $r('sys.symbol.chevron_down')
            : $r('sys.symbol.chevron_right')
        )
          .fontSize(12)
          .fontColor([this.store.palette.fgMuted])
      }
      .width('100%')
      .constraintSize({ minHeight: 36 })
      .padding({ top: 3, bottom: 3 })
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.toggleExpanded();
      })
    } else {
      Row({ space: OrbitTokens.SPACE_SM }) {
        SymbolGlyph($r('sys.symbol.wrench_and_screwdriver'))
          .fontSize(13)
          .fontColor([this.store.palette.fgMuted])
        Text(this.item.title.length > 0 ? this.item.title : 'Tool step')
          .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
        SymbolGlyph(
          this.item.detailExpanded
            ? $r('sys.symbol.chevron_down')
            : $r('sys.symbol.chevron_right')
        )
          .fontSize(12)
          .fontColor([this.store.palette.fgMuted])
      }
      .width('100%')
      .constraintSize({ minHeight: 36 })
      .padding({ top: 3, bottom: 3 })
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.toggleExpanded();
      })
    }
  }

  @Builder
  private renderExpandedToolDetail(desc: ToolStepDescriptorDto | null) {
    if (this.hasStepDiffFiles()) {
      Column({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          ForEach(
            this.miniDiffPreviewRows(),
            (row: MiniDiffPreviewRow) => {
              if (row.isFileHeader) {
                Text(stripWorkspacePrefix(row.filePath, this.store.activeCwd))
                  .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Bold))
                  .padding({ top: OrbitTokens.SPACE_XS, bottom: 2 })
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
              } else if (row.line !== null) {
                Text(
                  row.line.kind === 'hunk_header' || row.line.kind === 'meta'
                    ? row.line.content
                    : `${row.line.kind === 'added' ? '+' : row.line.kind === 'removed' ? '-' : ' '} ${row.line.content}`
                )
                  .attributeModifier(
                    orbitTextMono(
                      row.line.kind === 'added'
                        ? this.store.palette.statusDirect
                        : row.line.kind === 'removed'
                          ? this.store.palette.statusDanger
                          : row.line.kind === 'hunk_header' || row.line.kind === 'meta'
                            ? this.store.palette.fgMuted
                            : this.store.palette.fgSecondary
                    )
                  )
                  .width('100%')
                  .copyOption(CopyOptions.LocalDevice)
                  .backgroundColor(
                    row.line.kind === 'added'
                      ? this.store.palette.diffAddedBg
                      : row.line.kind === 'removed'
                        ? this.store.palette.diffRemovedBg
                        : Color.Transparent
                  )
                  .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS })
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
              }
            },
            (row: MiniDiffPreviewRow): string => row.key
          )
        }
        .width('100%')

        Row({ space: OrbitTokens.SPACE_SM }) {
          if (desc !== null && desc.open_path !== null && desc.open_path.length > 0) {
            Button($r('app.string.action_open'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
              )
              .height(OrbitTokens.HIT_COMPACT)
              .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
              .onClick(() => {
                if (desc.open_path !== null) {
                  this.store.openFileExplorer(desc.open_path);
                }
              })
          }
          Button('Inspect Full Diff', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
            )
            .height(OrbitTokens.HIT_COMPACT)
            .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
            .onClick(() => {
              this.store.openGitInspector(this.firstDiffFilePath());
            })
        }
        .width('100%')
        .justifyContent(FlexAlign.End)
      }
      .width('100%')
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.code)
      .borderRadius(OrbitTokens.RADIUS_SM)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
    } else {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          if (desc !== null && desc.open_path !== null && desc.open_path.length > 0) {
            Button($r('app.string.action_open'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
              )
              .height(28)
              .padding({ left: 10, right: 10 })
              .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
              .onClick(() => {
                if (desc.open_path !== null) {
                  this.store.openFileExplorer(desc.open_path);
                }
              })
          }
          Blank()
          OrbitCopyIconButton({
            content: this.buildFullStepSnippet(desc),
            toastLabel: 'Copied step detail',
            accessibilityLabel: 'Copy step detail',
            buttonSize: OrbitTokens.HIT_COMPACT
          })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)

        Text(this.buildFullStepSnippet(desc))
          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
          .width('100%')
          .copyOption(CopyOptions.LocalDevice)
          .maxLines(14)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      }
      .width('100%')
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.code)
      .borderRadius(OrbitTokens.RADIUS_SM)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
    }
  }
}

@ComponentV2
export struct StageTurnView {
  @Param turn: OrbitStageTurnModel = new OrbitStageTurnModel();
  @Param isTurnActive: boolean = false;

  @Local userPromptExpanded: boolean = false;
  @Local elapsedSeconds: number = 0;

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  private timerHandle: number = -1;

  @Computed
  get cleanedUserPrompt(): CleanedPromptText {
    return cleanLegacyPromptText(this.turn.userPrompt !== null ? this.turn.userPrompt.rawText : '');
  }

  aboutToAppear(): void {
    this.syncElapsedTimer();
  }

  aboutToDisappear(): void {
    this.clearElapsedTimer();
  }

  @Monitor('isTurnActive', 'turn')
  onActiveStateChanged(_mon: IMonitor): void {
    this.syncElapsedTimer();
  }

  private clearElapsedTimer(): void {
    if (this.timerHandle >= 0) {
      clearInterval(this.timerHandle);
      this.timerHandle = -1;
    }
  }

  private syncElapsedTimer(): void {
    const active = this.isTurnActive || this.turn.hasActiveItem;
    if (!active) {
      this.clearElapsedTimer();
      return;
    }
    if (this.timerHandle >= 0) {
      return;
    }
    const updateTick = (): void => {
      const startMs = this.turn.startedAtMs > 0 ? this.turn.startedAtMs : Date.now();
      this.elapsedSeconds = Math.max(0, Math.floor((Date.now() - startMs) / 1000));
    };
    updateTick();
    this.timerHandle = setInterval(() => {
      updateTick();
    }, 1000);
  }

  private isEffectiveActive(): boolean {
    return this.isTurnActive || this.turn.hasActiveItem;
  }

  private isCompactionOnly(): boolean {
    if (this.turn.isCompactionCommand) {
      return true;
    }
    if (this.turn.userPrompt !== null) {
      return isBareCompactionPrompt(this.turn.userPrompt);
    }
    return false;
  }

  private extractReplyText(reply: OrbitTimelineItemModel): string {
    const direct = reply.rawText.trim();
    if (direct.length > 0) {
      return direct;
    }
    const blocks: RichBlockDto[] = reply.rawBlocks;
    if (blocks.length === 0) {
      return '';
    }
    const parts: string[] = [];
    for (let i = 0; i < blocks.length; i++) {
      const blk = blocks[i];
      if (blk.kind === 'code_block' && blk.raw_code !== undefined) {
        parts.push(blk.raw_code);
      } else if (blk.spans !== undefined) {
        const spans: InlineSpanDto[] = blk.spans;
        let line = '';
        for (let j = 0; j < spans.length; j++) {
          line += spans[j].text;
        }
        if (line.trim().length > 0) {
          parts.push(line.trim());
        }
      }
    }
    return parts.join('\n\n').trim();
  }

  private badgeSymbol(badge: PromptBadgeDto): Resource {
    if (badge.kind === 'image') {
      return $r('sys.symbol.picture');
    }
    if (badge.kind === 'git_diff' || badge.kind === 'git' || badge.kind === 'diff' || badge.kind === 'staged') {
      return $r('sys.symbol.arrow_left_arrow_right');
    }
    if (badge.kind === 'terminal') {
      return $r('sys.symbol.code');
    }
    if (badge.kind === 'quote') {
      return $r('sys.symbol.quote');
    }
    const clean = badge.label.startsWith('@') ? badge.label.substring(1) : badge.label;
    return resolveSemanticFileSymbol(clean);
  }

  @Builder
  private renderCompactionDivider(label: string) {
    Row({ space: OrbitTokens.SPACE_SM }) {
      Divider()
        .strokeWidth(OrbitTokens.HAIRLINE)
        .color(this.store.palette.hairline)
        .layoutWeight(1)
      Row({ space: 5 }) {
        SymbolGlyph($r('sys.symbol.cut'))
          .fontSize(12)
          .fontColor([this.store.palette.fgMuted])
        Text(label)
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
      }
      .alignItems(VerticalAlign.Center)
      Divider()
        .strokeWidth(OrbitTokens.HAIRLINE)
        .color(this.store.palette.hairline)
        .layoutWeight(1)
    }
    .width('100%')
    .padding({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
  }

  @Builder
  private renderUserBubble(prompt: OrbitTimelineItemModel) {
    Row() {
      Blank()
      Column({ space: OrbitTokens.SPACE_XS }) {
        // Non-slash Context Badges
        if (prompt.badges.some((b: PromptBadgeDto): boolean => b.kind !== 'slash')) {
          Flex({ wrap: FlexWrap.Wrap }) {
            ForEach(
              prompt.badges.filter((b: PromptBadgeDto): boolean => b.kind !== 'slash'),
              (badge: PromptBadgeDto) => {
                Row({ space: 4 }) {
                  SymbolGlyph(this.badgeSymbol(badge))
                    .fontSize(11)
                    .fontColor([this.store.palette.fgSecondary])
                  Text(badge.label)
                    .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
                }
                .padding({ left: 7, right: 7, top: 2, bottom: 2 })
                .margin({ right: 5, bottom: 4 })
                .backgroundColor(this.store.palette.subtleSurface)
                .borderRadius(7)
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .alignItems(VerticalAlign.Center)
              },
              (badge: PromptBadgeDto, bIdx: number): string => `ub_${badge.kind}_${badge.label}_${bIdx}`
            )
          }
        }

        // Cleaned Body + Inline Leading Slash Prefix (natively selectable!)
        Row({ space: 6 }) {
          if (
            prompt.badges.some((b: PromptBadgeDto): boolean => b.kind === 'slash') ||
            this.cleanedUserPrompt.inferredSlash.length > 0
          ) {
            Text(
              prompt.badges.find((b: PromptBadgeDto): boolean => b.kind === 'slash')?.label ??
                this.cleanedUserPrompt.inferredSlash
            )
              .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Medium))
              .padding({ left: 6, right: 6, top: 2, bottom: 2 })
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.RADIUS_XS)
              .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
          }

          if (this.cleanedUserPrompt.body.length > 0) {
            Text(this.cleanedUserPrompt.body)
              .attributeModifier(orbitTextBody(this.store.palette.fgPrimary, FontWeight.Normal))
              .maxLines(this.userPromptExpanded ? 200 : 8)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .copyOption(CopyOptions.LocalDevice)
              .flexShrink(1)
          }
        }
        .alignItems(VerticalAlign.Top)

        if (this.cleanedUserPrompt.body.split('\n').length > 8) {
          Button(this.userPromptExpanded ? 'Show less' : 'Show more', { type: ButtonType.Normal, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.accentPrimary, Color.Transparent))
            .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
            .padding({ left: 0, right: OrbitTokens.SPACE_SM })
            .onClick(() => {
              this.getUIContext().animateTo({ duration: 180, curve: Curve.EaseInOut }, () => {
                this.userPromptExpanded = !this.userPromptExpanded;
              });
            })
        }
      }
      .alignItems(HorizontalAlign.Start)
      .constraintSize({ maxWidth: '84%' })
      .padding({ left: 14, right: 14, top: 10, bottom: 10 })
      .backgroundColor(this.store.palette.userBubble)
      .borderRadius(OrbitTokens.RADIUS_XL)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
      .shadow(ShadowStyle.OUTER_DEFAULT_XS)
    }
    .width('100%')
  }

  private deliveryPillLabel(status: string): Resource {
    if (status === 'queued') {
      return $r('app.string.stage_delivery_queued');
    }
    if (status === 'dispatched') {
      return $r('app.string.stage_delivery_dispatched');
    }
    if (status === 'dropped') {
      return $r('app.string.stage_delivery_dropped');
    }
    return $r('app.string.stage_delivery_settled');
  }

  private deliveryPillTextColor(status: string): ResourceColor {
    if (status === 'queued') {
      return this.store.palette.statusWarning;
    }
    if (status === 'dispatched') {
      return this.store.palette.accentPrimary;
    }
    if (status === 'dropped') {
      return this.store.palette.statusDanger;
    }
    return this.store.palette.statusDirect;
  }

  private deliveryPillBgColor(status: string): ResourceColor {
    if (status === 'queued') {
      return this.store.palette.statusWarningSoft;
    }
    if (status === 'dispatched') {
      return this.store.palette.accentPrimarySoft;
    }
    if (status === 'dropped') {
      return this.store.palette.statusDangerSoft;
    }
    return this.store.palette.statusDirectSoft;
  }

  private agentMessageSenderLabel(item: OrbitTimelineItemModel): string {
    if (item.sourceTitle.trim().length > 0) {
      return item.sourceTitle.trim();
    }
    if (item.kind.length > 0) {
      return item.kind.replace(/_/g, ' ');
    }
    return 'Agent message';
  }

  @Builder
  private renderAgentMessageCard(item: OrbitTimelineItemModel) {
    Column({ space: OrbitTokens.SPACE_XS }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        SymbolGlyph($r('sys.symbol.person_2'))
          .fontSize(12)
          .fontColor([this.store.palette.accentPrimary])

        Text(this.agentMessageSenderLabel(item))
          .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary, FontWeight.Medium))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        if (item.deliveryStatus.length > 0) {
          Text(this.deliveryPillLabel(item.deliveryStatus))
            .attributeModifier(
              orbitTextMeta(this.deliveryPillTextColor(item.deliveryStatus), FontWeight.Medium)
            )
            .padding({ left: 8, right: 8, top: 2, bottom: 2 })
            .backgroundColor(this.deliveryPillBgColor(item.deliveryStatus))
            .borderRadius(999)
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      RichMarkdownView({
        blocks: item.rawBlocks,
        fallbackText: item.rawText,
        turnIndex: this.turn.turnIndex
      })

      if (item.deliveryStatus === 'queued' && this.store.isActiveAgentRunning()) {
        Row() {
          Blank()
          Button($r('app.string.stage_interrupt_and_deliver'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
            )
            .height(28)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.store.interruptAndContinueCurrentAgent();
            })
        }
        .width('100%')
      }
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_MD)
    .backgroundColor(this.store.palette.subtleSurface)
    .borderRadius(OrbitTokens.RADIUS_LG)
    .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
  }

  @Builder
  private renderTurnFooter() {
    Row({ space: OrbitTokens.SPACE_SM }) {
      if (this.isEffectiveActive()) {
        LoadingProgress()
          .width(14)
          .height(14)
          .color(this.store.palette.fgMuted)
        Text(`${Math.floor(this.elapsedSeconds / 60)}:${(this.elapsedSeconds % 60).toString().padStart(2, '0')}`)
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        Blank()
        if (this.elapsedSeconds >= 8) {
          Button($r('app.string.stage_interrupt_and_continue'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
            )
            .height(28)
            .padding({ left: 10, right: 10 })
            .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
            .onClick(() => {
              this.store.interruptAndContinueCurrentAgent();
            })
        }
      } else {
        if (this.turn.terminalReply !== null && this.extractReplyText(this.turn.terminalReply).length > 0) {
          OrbitCopyIconButton({
            content: this.extractReplyText(this.turn.terminalReply),
            toastLabel: 'Copied reply to clipboard',
            accessibilityLabel: 'Copy response',
            buttonSize: OrbitTokens.HIT_COMPACT
          })
        }

        if ((this.turn.completedAtMs > 0 ? this.turn.completedAtMs : this.turn.startedAtMs) > 0) {
          Text(
            formatRelativeTimeLabel(
              Math.floor((this.turn.completedAtMs > 0 ? this.turn.completedAtMs : this.turn.startedAtMs) / 1000)
            )
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        }
      }
    }
    .width('100%')
    .padding({ top: 2 })
    .alignItems(VerticalAlign.Center)
  }

  private settledSummaryLabel(): string {
    const base = this.turn.workSummaryLabel.length > 0
      ? this.turn.workSummaryLabel
      : `${this.turn.foldedSteps.length} steps`;
    if (this.turn.thoughtDurationMs >= 1000) {
      return `${base} · Thought ${Math.floor(this.turn.thoughtDurationMs / 1000)}s`;
    }
    return base;
  }

  @Builder
  private renderSettledSummaryAndRail() {
    if (this.turn.foldedSteps.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: OrbitTokens.SPACE_XS }) {
          SymbolGlyph(
            this.turn.stepsExpanded
              ? $r('sys.symbol.chevron_down')
              : $r('sys.symbol.chevron_right')
          )
            .fontSize(13)
            .fontColor([
              this.turn.workSummaryIncludesFailure
                ? this.store.palette.statusDanger
                : this.store.palette.fgMuted
            ])

          Text(this.settledSummaryLabel())
            .attributeModifier(
              orbitTextSubhead(
                this.turn.workSummaryIncludesFailure
                  ? this.store.palette.statusDanger
                  : this.store.palette.fgSecondary,
                FontWeight.Medium
              )
            )
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)
        }
        .width('100%')
        .constraintSize({ minHeight: 38 })
        .alignItems(VerticalAlign.Center)
        .onClick(() => {
          this.getUIContext().animateTo({ duration: 180, curve: Curve.EaseInOut }, () => {
            this.turn.stepsExpanded = !this.turn.stepsExpanded;
          });
          this.store.triggerHaptic(8);
        })

        if (this.turn.stepsExpanded) {
          Column({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              this.turn.foldedSteps,
              (step: OrbitTimelineItemModel) => {
                StepItemRow({ item: step })
              },
              (step: OrbitTimelineItemModel): string => step.itemKey
            )
          }
          .width('100%')
          .padding({ left: OrbitTokens.SPACE_MD, top: 2, bottom: OrbitTokens.SPACE_XS })
          .margin({ left: OrbitTokens.SPACE_XS })
          .border({
            width: { left: 1 },
            color: this.store.palette.hairline
          })
          .clip(true)
        }
      }
      .width('100%')
    }
  }

  @Builder
  private renderLiveSegments() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      ForEach(
        this.turn.liveSegments,
        (seg: OrbitLiveSegmentModel) => {
          if (seg.kind === 'step_group' && seg.items.length > 0) {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Row({ space: OrbitTokens.SPACE_XS }) {
                if (seg.isRunning) {
                  LoadingProgress()
                    .width(14)
                    .height(14)
                    .color(this.store.palette.accentPrimary)
                } else {
                  SymbolGlyph(
                    seg.stepsExpanded || (seg.items.length < 5 && !seg.hasFollowingContent)
                      ? $r('sys.symbol.chevron_down')
                      : $r('sys.symbol.chevron_right')
                  )
                    .fontSize(13)
                    .fontColor([this.store.palette.fgMuted])
                }

                Text(
                  seg.isRunning
                    ? (seg.liveSummaryLabel.length > 0 ? seg.liveSummaryLabel : 'Working…')
                    : (seg.summary.length > 0 ? seg.summary : 'Completed steps')
                )
                  .attributeModifier(
                    orbitTextSubhead(
                      seg.summaryIncludesFailure
                        ? this.store.palette.statusDanger
                        : this.store.palette.fgSecondary,
                      FontWeight.Medium
                    )
                  )
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .layoutWeight(1)
              }
              .width('100%')
              .constraintSize({ minHeight: 38 })
              .alignItems(VerticalAlign.Center)
              .onClick(() => {
                this.getUIContext().animateTo({ duration: 180, curve: Curve.EaseInOut }, () => {
                  seg.stepsExpanded = !seg.stepsExpanded;
                });
                this.store.triggerHaptic(8);
              })

              if (seg.stepsExpanded || (seg.items.length < 5 && (seg.isRunning || !seg.hasFollowingContent))) {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  ForEach(
                    seg.items,
                    (step: OrbitTimelineItemModel) => {
                      StepItemRow({ item: step })
                    },
                    (step: OrbitTimelineItemModel): string => step.itemKey
                  )
                }
                .width('100%')
                .padding({ left: OrbitTokens.SPACE_MD, top: 2, bottom: OrbitTokens.SPACE_XS })
                .margin({ left: OrbitTokens.SPACE_XS })
                .border({
                  width: { left: 1 },
                  color: this.store.palette.hairline
                })
                .clip(true)
              } else if (seg.items.length >= 5) {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  StepItemRow({ item: seg.items[seg.items.length - 1] })
                }
                .width('100%')
                .padding({ left: OrbitTokens.SPACE_MD, top: 2, bottom: OrbitTokens.SPACE_XS })
                .margin({ left: OrbitTokens.SPACE_XS })
                .border({
                  width: { left: 1 },
                  color: this.store.palette.hairline
                })
              }
            }
            .width('100%')
          } else if (seg.kind === 'assistant_message' && seg.item !== null) {
            RichMarkdownView({
              blocks: seg.item.rawBlocks,
              fallbackText: seg.item.rawText,
              turnIndex: this.turn.turnIndex
            })
          } else if (seg.kind === 'agent_message' && seg.item !== null) {
            this.renderAgentMessageCard(seg.item)
          } else if (seg.kind === 'compaction') {
            this.renderCompactionDivider(seg.label.length > 0 ? seg.label : 'Context compacted')
          } else if (seg.kind === 'wakeup') {
            this.renderCompactionDivider(seg.label.length > 0 ? seg.label : 'Resumed')
          }
        },
        // Strictly stable key from core — never append item count or running flag.
        (seg: OrbitLiveSegmentModel): string => seg.segKey
      )
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Computed
  get totalTurnAdditions(): number {
    let total = 0;
    for (let i = 0; i < this.turn.turnDiffs.length; i++) {
      total += this.turn.turnDiffs[i].additions;
    }
    return total;
  }

  @Computed
  get totalTurnDeletions(): number {
    let total = 0;
    for (let i = 0; i < this.turn.turnDiffs.length; i++) {
      total += this.turn.turnDiffs[i].deletions;
    }
    return total;
  }

  @Builder
  private renderTurnChangesBar() {
    if (this.turn.turnDiffs.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Button({ type: ButtonType.Normal, stateEffect: true }) {
          Row({ space: OrbitTokens.SPACE_XS }) {
            SymbolGlyph($r('sys.symbol.arrow_left_arrow_right'))
              .fontSize(14)
              .fontColor([this.store.palette.iconSecondary])
            Text(
              this.turn.turnDiffs.length === 1
                ? stripWorkspacePrefix(this.turn.turnDiffs[0].path, this.store.activeCwd)
                : `${this.turn.turnDiffs.length} files changed`
            )
              .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Medium))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
              .layoutWeight(1)
              .textAlign(TextAlign.Start)

            if (this.totalTurnAdditions > 0) {
              Text(`+${this.totalTurnAdditions}`)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDirect))
            }
            if (this.totalTurnDeletions > 0) {
              Text(`-${this.totalTurnDeletions}`)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
            }
            if (this.turn.turnDiffs.length > 1) {
              SymbolGlyph(
                this.turn.changesExpanded ? $r('sys.symbol.chevron_down') : $r('sys.symbol.chevron_right')
              )
                .fontSize(12)
                .fontColor([this.store.palette.fgMuted])
            }
          }
          .width('100%')
          .alignItems(VerticalAlign.Center)
        }
        .width('100%')
        .constraintSize({ minHeight: OrbitTokens.HIT_COMPACT })
        .padding({ left: OrbitTokens.SPACE_XS, right: OrbitTokens.SPACE_XS })
        .backgroundColor(Color.Transparent)
        .onClick(() => {
          if (this.turn.turnDiffs.length === 1) {
            this.store.openGitInspector(this.turn.turnDiffs[0].path);
          } else {
            this.getUIContext().animateTo({ duration: 160, curve: Curve.EaseInOut }, () => {
              this.turn.changesExpanded = !this.turn.changesExpanded;
            });
          }
        })

        if (this.turn.turnDiffs.length > 1 && this.turn.changesExpanded) {
          Flex({ wrap: FlexWrap.Wrap }) {
            ForEach(
              this.turn.turnDiffs,
              (df: TurnFileDiffSummaryDto) => {
                Button({ type: ButtonType.Capsule, stateEffect: true }) {
                  Row({ space: 4 }) {
                    Text(basenameOfPath(df.path))
                      .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
                    if (df.additions > 0) {
                      Text(`+${df.additions}`)
                        .attributeModifier(orbitTextMeta(this.store.palette.statusDirect))
                    }
                    if (df.deletions > 0) {
                      Text(`-${df.deletions}`)
                        .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                    }
                  }
                }
                .height(32)
                .padding({ left: 10, right: 10 })
                .margin({ right: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
                .backgroundColor(this.store.palette.canvas)
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
                .onClick(() => {
                  this.store.openGitInspector(df.path);
                })
              },
              (df: TurnFileDiffSummaryDto): string => df.path
            )
          }
          .clip(true)
        }
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM, top: 4, bottom: 4 })
      .backgroundColor(this.store.palette.subtleSurface)
      .borderRadius(OrbitTokens.RADIUS_MD)
      .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      if (this.isCompactionOnly()) {
        this.renderCompactionDivider('Context compacted')
      } else if (this.turn.userPrompt !== null) {
        this.renderUserBubble(this.turn.userPrompt)
      } else if (this.turn.agentMessage !== null) {
        this.renderAgentMessageCard(this.turn.agentMessage)
      }

      if (this.isEffectiveActive()) {
        this.renderLiveSegments()
      } else {
        this.renderSettledSummaryAndRail()

        if (this.turn.notices.length > 0) {
          Column({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              this.turn.notices,
              (notice: OrbitTimelineItemModel) => {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  SymbolGlyph($r('sys.symbol.warning'))
                    .fontSize(14)
                    .fontColor([
                      notice.level === 'error'
                        ? this.store.palette.statusDanger
                        : this.store.palette.fgSecondary
                    ])
                  Text(notice.message.length > 0 ? notice.message : notice.rawText)
                    .attributeModifier(
                      orbitTextMeta(
                        notice.level === 'error'
                          ? this.store.palette.statusDanger
                          : this.store.palette.fgSecondary
                      )
                    )
                    .copyOption(CopyOptions.LocalDevice)
                    .layoutWeight(1)
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_SM)
                .backgroundColor(
                  notice.level === 'error'
                    ? this.store.palette.diffRemovedBg
                    : this.store.palette.subtleSurface
                )
                .borderRadius(OrbitTokens.RADIUS_SM)
                .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
              },
              (notice: OrbitTimelineItemModel): string => notice.itemKey
            )
          }
          .width('100%')
        }

        if (this.turn.terminalReply !== null) {
          RichMarkdownView({
            blocks: this.turn.terminalReply.rawBlocks,
            fallbackText: this.turn.terminalReply.rawText,
            turnIndex: this.turn.turnIndex
          })
        }
      }

      this.renderTurnChangesBar()

      if (
        this.isEffectiveActive() ||
        (!this.isCompactionOnly() &&
          (this.turn.terminalReply !== null ||
            this.turn.foldedSteps.length > 0 ||
            this.turn.completedAtMs > 0))
      ) {
        this.renderTurnFooter()
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }
}
