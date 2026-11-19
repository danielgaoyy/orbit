import {
  DiffFileDto,
  DiffLineDto,
  InlineSpanDto,
  PromptBadgeDto,
  RichBlockDto,
  StageTurnDto,
  TimelineItemDto,
  ToolStepDescriptorDto,
  TurnFileDiffSummaryDto,
  TurnLiveSegmentDto,
  basenameOfPath,
  formatCompactDuration
} from '../../model/OrbitTypes';
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
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { RichMarkdownView } from './RichMarkdownView';

interface MiniDiffPreviewRow {
  key: string;
  isFileHeader: boolean;
  filePath: string;
  line: DiffLineDto | null;
}

@ComponentV2
struct StepItemRow {
  @Param item: TimelineItemDto = { item_type: 'tool_call', seq: 0 };
  @Local stepCopied: boolean = false;
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private stepKey(): string {
    if (this.item.call_id !== undefined && this.item.call_id.length > 0) {
      return this.item.call_id;
    }
    return `seq_${this.item.seq}`;
  }

  private hasStepDiffFiles(): boolean {
    return this.item.diff_files !== undefined && this.item.diff_files.length > 0;
  }

  private firstDiffFilePath(): string {
    if (this.item.diff_files !== undefined && this.item.diff_files.length > 0) {
      return this.item.diff_files[0].path;
    }
    if (this.item.detail !== undefined && (this.item.detail.path ?? '').length > 0) {
      return this.item.detail.path ?? '';
    }
    return '';
  }

  private miniDiffPreviewRows(): MiniDiffPreviewRow[] {
    const files = this.item.diff_files ?? [];
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
    if (det !== undefined) {
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
      return (this.item.summary_title ?? this.item.text ?? 'Tool call completed').trim();
    }
    return parts.join('\n');
  }

  build() {
    if (this.item.item_type === 'assistant_thinking') {
      if ((this.item.text ?? '').trim().length > 0) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text('Thought')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
            Text(this.store.isToolExpanded(this.stepKey()) ? '▾' : '▸')
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          }
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
          .alignItems(VerticalAlign.Center)
          .onClick(() => {
            this.store.toggleToolExpanded(this.stepKey());
          })

          if (this.store.isToolExpanded(this.stepKey())) {
            Text((this.item.text ?? '').trim())
              .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
              .padding({ bottom: OrbitTokens.SPACE_XS })
          }
        }
        .alignItems(HorizontalAlign.Start)
        .width('100%')
      }
    } else if (this.item.item_type === 'assistant_message') {
      if ((this.item.text ?? '').trim().length > 0) {
        RichMarkdownView({
          blocks: this.item.blocks ?? [],
          fallbackText: this.item.text ?? ''
        })
      }
    } else if (this.item.item_type === 'tool_call') {
      Column({ space: OrbitTokens.SPACE_XS }) {
        this.renderDescriptorRow(this.store.describeToolStepFallback(this.item))

        if (this.store.isToolExpanded(this.stepKey())) {
          this.renderExpandedToolDetail(this.store.describeToolStepFallback(this.item))
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
        Row()
          .width(OrbitTokens.DOT_SIZE)
          .height(OrbitTokens.DOT_SIZE)
          .borderRadius(3)
          .backgroundColor(
            desc.is_failed || desc.is_denied
              ? this.store.palette.statusDanger
              : desc.is_running
                ? this.store.palette.accentPrimary
                : this.store.palette.fgMuted
          )

        Text() {
          Span(desc.verb)
            .font(orbitSpanSubheadFont(FontWeight.Normal))
            .fontColor(
              desc.is_failed || desc.is_denied
                ? this.store.palette.statusDanger
                : this.store.palette.fgSecondary
            )
          if (desc.target.length > 0) {
            Span(`  ${desc.target}`)
              .font(orbitSpanMonoFont(FontWeight.Medium))
              .fontColor(this.store.palette.fgPrimary)
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

        Text(this.store.isToolExpanded(this.stepKey()) ? '▾' : '▸')
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.store.toggleToolExpanded(this.stepKey());
      })
    } else {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text(this.item.summary_title ?? 'Tool step')
          .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)
        Text(this.store.isToolExpanded(this.stepKey()) ? '▾' : '▸')
          .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
      }
      .width('100%')
      .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
      .padding({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
      .alignItems(VerticalAlign.Center)
      .onClick(() => {
        this.store.toggleToolExpanded(this.stepKey());
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
                Text(row.filePath)
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
                            ? this.store.palette.accentPrimary
                            : this.store.palette.fgSecondary
                    )
                  )
                  .width('100%')
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
            (row: MiniDiffPreviewRow) => row.key
          )
        }
        .width('100%')

        Row() {
          Button('Inspect Full Diff')
            .attributeModifier(
              orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.accentPrimarySoft)
            )
            .height(36)
            .borderRadius(OrbitTokens.PILL_RADIUS)
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
      .borderRadius(OrbitTokens.PILL_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
    } else {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row() {
          Blank()
          Button(this.stepCopied ? 'Copied' : 'Copy')
            .attributeModifier(
              orbitButtonMeta(
                this.stepCopied ? this.store.palette.statusDirect : this.store.palette.fgSecondary,
                this.stepCopied ? this.store.palette.statusDirectSoft : this.store.palette.elevated
              )
            )
            .height(28)
            .borderRadius(OrbitTokens.PILL_RADIUS)
            .onClick(() => {
              const snippet = this.buildFullStepSnippet(desc);
              this.store.copyToClipboard(snippet, 'Copied step detail');
              this.stepCopied = true;
              setTimeout(() => {
                this.stepCopied = false;
              }, 1600);
            })
        }
        .width('100%')

        Text(this.buildFullStepSnippet(desc))
          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
          .width('100%')
          .maxLines(14)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      }
      .width('100%')
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.subtleSurface)
      .borderRadius(OrbitTokens.PILL_RADIUS)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
    }
  }
}

@ComponentV2
export struct StageTurnView {
  @Param turn: StageTurnDto = {
    key: '',
    turn_index: 0,
    anchor_seq: 0,
    user_prompt: null,
    is_compaction_command: false,
    folded_steps: [],
    work_counts: {
      edited_file_count: 0,
      created_file_count: 0,
      command_count: 0,
      read_file_count: 0,
      search_count: 0,
      subagent_count: 0,
      mcp_server_count: 0,
      other_tool_count: 0,
      thought_count: 0,
      narration_count: 0,
      action_steps: 0,
      total_steps: 0,
      failed_count: 0,
      denied_count: 0,
      unrecovered_failure_count: 0,
      additions: 0,
      deletions: 0
    },
    work_summary_label: '',
    work_summary_live_label: '',
    work_summary_includes_failure: false,
    thought_duration_ms: null,
    turn_duration_ms: null,
    terminal_reply: null,
    notices: [],
    live_segments: [],
    turn_diffs: [],
    has_error: false,
    has_unrecovered_error: false,
    has_active_item: false,
    started_at_ms: null,
    completed_at_ms: null
  };

  @Local pulseBright: boolean = true;
  @Local replyCopied: boolean = false;
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private isSettledTurn(): boolean {
    return this.turn.terminal_reply !== null && !this.turn.has_active_item;
  }

  private extractReplyText(reply: TimelineItemDto): string {
    const direct = (reply.text ?? '').trim();
    if (direct.length > 0) {
      return direct;
    }
    const blocks: RichBlockDto[] = reply.blocks ?? [];
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

  @Builder
  private renderTerminalReplyActions(reply: TimelineItemDto) {
    if (this.extractReplyText(reply).length > 0) {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Row() {
          Text(this.replyCopied ? 'Copied' : 'Copy')
            .attributeModifier(
              orbitTextMeta(
                this.replyCopied ? this.store.palette.statusDirect : this.store.palette.fgMuted,
                FontWeight.Medium
              )
            )
        }
        .height(28)
        .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
        .backgroundColor(
          this.replyCopied ? this.store.palette.statusDirectSoft : Color.Transparent
        )
        .borderRadius(OrbitTokens.PILL_RADIUS)
        .alignItems(VerticalAlign.Center)
        .onClick(() => {
          const text = this.extractReplyText(reply);
          this.store.copyToClipboard(text, 'Copied reply to clipboard');
          this.replyCopied = true;
          setTimeout(() => {
            this.replyCopied = false;
          }, 1800);
        })

        Row() {
          Text('Quote')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
        }
        .height(28)
        .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
        .borderRadius(OrbitTokens.PILL_RADIUS)
        .alignItems(VerticalAlign.Center)
        .onClick(() => {
          const text = this.extractReplyText(reply);
          this.store.quoteAssistantReplyIntoComposer(text, this.turn.turn_index);
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
    }
  }

  @Builder
  private renderUserBubble(prompt: TimelineItemDto) {
    Row() {
      Blank()
      Column({ space: OrbitTokens.SPACE_SM }) {
        if (prompt.badges !== undefined && prompt.badges.length > 0) {
          Flex({ wrap: FlexWrap.Wrap }) {
            ForEach(
              prompt.badges,
              (badge: PromptBadgeDto) => {
                Row() {
                  Text(badge.label)
                    .attributeModifier(
                      orbitTextMono(this.store.palette.accentPrimary, FontWeight.Medium)
                    )
                }
                .padding({
                  left: OrbitTokens.SPACE_SM,
                  right: OrbitTokens.SPACE_SM,
                  top: 2,
                  bottom: 2
                })
                .margin({ right: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
                .backgroundColor(this.store.palette.accentPrimarySoft)
                .borderRadius(OrbitTokens.PILL_RADIUS)
              },
              (badge: PromptBadgeDto, bIdx: number) => `${badge.label}_${bIdx}`
            )
          }
        }

        Text(prompt.text ?? '')
          .attributeModifier(orbitTextBody(this.store.palette.fgPrimary, FontWeight.Normal))
      }
      .alignItems(HorizontalAlign.Start)
      .constraintSize({ maxWidth: '88%' })
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        top: OrbitTokens.SPACE_SM + 2,
        bottom: OrbitTokens.SPACE_SM + 2
      })
      .backgroundColor(this.store.palette.userBubble)
      .borderRadius(OrbitTokens.SPACE_LG)
      .border({
        width: OrbitTokens.HAIRLINE,
        color: this.store.palette.hairline
      })
    }
    .width('100%')
  }

  @Builder
  private renderSettledSummaryAndRail() {
    if (this.turn.folded_steps.length > 0) {
      Column({ space: OrbitTokens.SPACE_XS }) {
        Row({ space: OrbitTokens.SPACE_SM }) {
          Text(this.store.isTurnExpanded(this.turn.key) ? '▾' : '▸')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))

          Text(
            this.turn.work_summary_label.length > 0
              ? this.turn.work_summary_label
              : `${this.turn.folded_steps.length} steps completed`
          )
            .attributeModifier(
              orbitTextSubhead(
                this.turn.has_unrecovered_error
                  ? this.store.palette.statusDanger
                  : this.store.palette.fgSecondary,
                FontWeight.Medium
              )
            )
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
            .layoutWeight(1)

          if (this.turn.work_counts.additions > 0 || this.turn.work_counts.deletions > 0) {
            Row({ space: OrbitTokens.SPACE_XS }) {
              if (this.turn.work_counts.additions > 0) {
                Text(`+${this.turn.work_counts.additions}`)
                  .attributeModifier(
                    orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium)
                  )
              }
              if (this.turn.work_counts.deletions > 0) {
                Text(`-${this.turn.work_counts.deletions}`)
                  .attributeModifier(
                    orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium)
                  )
              }
            }
          }

          if (this.turn.turn_duration_ms !== null && this.turn.turn_duration_ms >= 1000) {
            Text(formatCompactDuration(this.turn.turn_duration_ms))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          }
        }
        .width('100%')
        .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
        .alignItems(VerticalAlign.Center)
        .onClick(() => {
          this.store.toggleTurnExpanded(this.turn.key);
        })

        if (this.store.isTurnExpanded(this.turn.key)) {
          Column({ space: OrbitTokens.SPACE_XS }) {
            ForEach(
              this.turn.folded_steps,
              (step: TimelineItemDto) => {
                StepItemRow({ item: step })
              },
              (step: TimelineItemDto) => `${step.item_type}_${step.seq}`
            )
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
    }
  }

  @Builder
  private renderLiveSegments() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      ForEach(
        this.turn.live_segments,
        (seg: TurnLiveSegmentDto) => {
          if (seg.kind === 'step_group' && seg.items !== undefined) {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Row({ space: OrbitTokens.SPACE_SM }) {
                if (seg.has_running_step === true) {
                  Row()
                    .width(OrbitTokens.SPACE_SM)
                    .height(OrbitTokens.SPACE_SM)
                    .borderRadius(OrbitTokens.SPACE_XS)
                    .backgroundColor(this.store.palette.accentPrimary)
                    .opacity(this.pulseBright ? 1.0 : 0.35)
                    .onAppear(() => {
                      this.getUIContext().animateTo(
                        {
                          duration: 750,
                          iterations: -1,
                          playMode: PlayMode.Alternate,
                          curve: Curve.EaseInOut
                        },
                        () => {
                          this.pulseBright = false;
                        }
                      );
                    })
                } else {
                  Text(this.store.isTurnExpanded(seg.key) ? '▾' : '▸')
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Bold))
                }

                Text(
                  seg.has_running_step === true
                    ? (seg.summary?.live_label ?? 'Working…')
                    : (seg.summary?.label ?? 'Completed steps')
                )
                  .attributeModifier(
                    orbitTextSubhead(
                      seg.has_running_step === true
                        ? this.store.palette.fgPrimary
                        : this.store.palette.fgSecondary,
                      FontWeight.Medium
                    )
                  )
                  .maxLines(1)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .layoutWeight(1)
              }
              .width('100%')
              .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
              .alignItems(VerticalAlign.Center)
              .onClick(() => {
                this.store.toggleTurnExpanded(seg.key);
              })

              if (
                seg.has_running_step === true ||
                seg.has_following_content !== true ||
                this.store.isTurnExpanded(seg.key)
              ) {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  ForEach(
                    seg.items,
                    (step: TimelineItemDto) => {
                      StepItemRow({ item: step })
                    },
                    (step: TimelineItemDto) => `${step.item_type}_${step.seq}_${step.status ?? ''}`
                  )
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
          } else if (seg.kind === 'assistant_message' && seg.item !== undefined) {
            RichMarkdownView({
              blocks: seg.item.blocks ?? [],
              fallbackText: seg.item.text ?? ''
            })
          } else if (seg.kind === 'compaction') {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Divider()
                .strokeWidth(OrbitTokens.HAIRLINE)
                .color(this.store.palette.hairline)
                .layoutWeight(1)
              Text(seg.label ?? 'Context compacted')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              Divider()
                .strokeWidth(OrbitTokens.HAIRLINE)
                .color(this.store.palette.hairline)
                .layoutWeight(1)
            }
            .width('100%')
            .padding({ top: OrbitTokens.SPACE_XS, bottom: OrbitTokens.SPACE_XS })
          }
        },
        (seg: TurnLiveSegmentDto) => `${seg.key}_${seg.has_running_step ? '1' : '0'}_${seg.items?.length ?? 0}_${seg.item?.text?.length ?? 0}`
      )
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }

  @Builder
  private renderTurnDiffStrip() {
    if (this.turn.turn_diffs.length > 0) {
      Scroll() {
        Row({ space: OrbitTokens.SPACE_SM }) {
          ForEach(
            this.turn.turn_diffs,
            (df: TurnFileDiffSummaryDto) => {
              Row({ space: OrbitTokens.SPACE_XS }) {
                Text(basenameOfPath(df.path))
                  .attributeModifier(
                    orbitTextMono(this.store.palette.fgSecondary, FontWeight.Medium)
                  )
                if (df.additions > 0) {
                  Text(`+${df.additions}`)
                    .attributeModifier(
                      orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium)
                    )
                }
                if (df.deletions > 0) {
                  Text(`-${df.deletions}`)
                    .attributeModifier(
                      orbitTextMeta(this.store.palette.statusDanger, FontWeight.Medium)
                    )
                }
              }
              .height(32)
              .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .border({
                width: OrbitTokens.HAIRLINE,
                color: this.store.palette.hairline
              })
              .onClick(() => {
                this.store.openGitInspector(df.path);
              })
            },
            (df: TurnFileDiffSummaryDto) => df.path
          )
        }
      }
      .scrollable(ScrollDirection.Horizontal)
      .scrollBar(BarState.Off)
      .width('100%')
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      if (this.turn.user_prompt !== null) {
        this.renderUserBubble(this.turn.user_prompt)
      }

      if (this.turn.is_compaction_command) {
        Row({ space: OrbitTokens.SPACE_SM }) {
          Divider()
            .strokeWidth(OrbitTokens.HAIRLINE)
            .color(this.store.palette.hairline)
            .layoutWeight(1)
          Text('Context compacted')
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
          Divider()
            .strokeWidth(OrbitTokens.HAIRLINE)
            .color(this.store.palette.hairline)
            .layoutWeight(1)
        }
        .width('100%')
      }

      if (this.isSettledTurn()) {
        this.renderSettledSummaryAndRail()
        if (this.turn.terminal_reply !== null) {
          RichMarkdownView({
            blocks: this.turn.terminal_reply.blocks ?? [],
            fallbackText: this.turn.terminal_reply.text ?? ''
          })
          this.renderTerminalReplyActions(this.turn.terminal_reply)
        }
      } else {
        this.renderLiveSegments()
      }

      this.renderTurnDiffStrip()

      if (this.turn.notices.length > 0) {
        Column({ space: OrbitTokens.SPACE_XS }) {
          ForEach(
            this.turn.notices,
            (notice: TimelineItemDto) => {
              Row({ space: OrbitTokens.SPACE_SM }) {
                Row()
                  .width(OrbitTokens.DOT_SIZE)
                  .height(OrbitTokens.DOT_SIZE)
                  .borderRadius(3)
                  .backgroundColor(
                    notice.level === 'error'
                      ? this.store.palette.statusDanger
                      : this.store.palette.statusWarning
                  )
                Text(notice.message ?? '')
                  .attributeModifier(
                    orbitTextSubhead(
                      notice.level === 'error'
                        ? this.store.palette.statusDanger
                        : this.store.palette.statusWarning,
                      FontWeight.Normal
                    )
                  )
                  .layoutWeight(1)
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_SM)
              .backgroundColor(
                notice.level === 'error'
                  ? this.store.palette.statusDangerSoft
                  : this.store.palette.statusWarningSoft
              )
              .borderRadius(OrbitTokens.PILL_RADIUS)
            },
            (notice: TimelineItemDto) => `notice_${notice.seq}`
          )
        }
        .width('100%')
      }
    }
    .width('100%')
    .alignItems(HorizontalAlign.Start)
  }
}
