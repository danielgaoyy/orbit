import {
  QuestionItemDto,
  formatPermissionDecisionLabel
} from '../../model/OrbitTypes';
import { OrbitTimelineItemModel } from '../../model/viewmodels/OrbitStageModels';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitStageStore } from '../../model/stores/OrbitStageStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';

import {
  QueuedDecisionCard,
  findFocusedDecisionQueueIndex
} from '../../model/OrbitDecisionFocusModel';

export {
  DecisionFocusTarget,
  QueuedDecisionCard,
  buildDecisionFocusTarget,
  findFocusedDecisionQueueIndex
} from '../../model/OrbitDecisionFocusModel';

@ComponentV2
export struct StickyDecisionDock {
  @Local activeQueueIndex: number = 0;
  @Local selectedMultiOptions: string[] = [];
  @Local customAnswersRevision: number = 0;
  @Local isPreviewExpanded: boolean = false;
  private customAnswersMap: Map<string, string> = new Map<string, string>();

  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private stageStore: OrbitStageStore = orbitRuntime().stageStore;

  aboutToAppear(): void {
    this.syncFocusedDecisionCard();
  }

  @Monitor(
    'store.decisionFocusRevision',
    'store.pendingFocusDecisionHostId',
    'store.pendingFocusDecisionAgentId',
    'store.pendingFocusDecisionEventKind',
    'store.pendingFocusDecisionRequestId',
    'store.pendingFocusDecisionSeq',
    'stageStore.activeRevision'
  )
  onDecisionFocusChanged(_mon: IMonitor): void {
    this.syncFocusedDecisionCard();
  }

  private syncFocusedDecisionCard(): void {
    if (this.store.pendingFocusDecisionEventKind !== 'permission_required') {
      return;
    }
    const targetHost = this.store.pendingFocusDecisionHostId.trim();
    const targetAgent = this.store.pendingFocusDecisionAgentId.trim();
    const reqId = this.store.pendingFocusDecisionRequestId;
    const seq = this.store.pendingFocusDecisionSeq;
    // Require complete (host, agent) scope before consuming any decision-card focus.
    if (targetHost.length === 0 || targetAgent.length === 0 || (reqId.length === 0 && seq <= 0)) {
      return;
    }
    const matched = findFocusedDecisionQueueIndex(
      this.decisionQueue,
      reqId,
      seq,
      targetHost,
      targetAgent,
      this.store.selectedHostId,
      this.store.selectedAgentId
    );
    if (matched >= 0) {
      this.activeQueueIndex = matched;
      this.store.clearPendingDecisionFocus();
    }
  }

  @Computed
  get decisionQueue(): QueuedDecisionCard[] {
    return this.orderedDecisionQueue();
  }

  @Computed
  get activeIndex(): number {
    return this.safeIndex(this.decisionQueue.length);
  }

  private orderedDecisionQueue(): QueuedDecisionCard[] {
    const combined: QueuedDecisionCard[] = [];
    for (let i = 0; i < this.store.pendingPermissions.length; i++) {
      const perm = this.store.pendingPermissions[i];
      combined.push({
        key: `perm_${perm.requestId.length > 0 ? perm.requestId : perm.seq}`,
        kind: 'permission',
        seq: perm.seq,
        item: perm
      });
    }
    for (let i = 0; i < this.store.pendingQuestions.length; i++) {
      const q = this.store.pendingQuestions[i];
      combined.push({
        key: `q_${q.requestId.length > 0 ? q.requestId : q.seq}`,
        kind: 'question',
        seq: q.seq,
        item: q
      });
    }
    combined.sort((a: QueuedDecisionCard, b: QueuedDecisionCard) => a.seq - b.seq);
    return combined;
  }

  private safeIndex(total: number): number {
    if (total <= 0) {
      return 0;
    }
    return Math.max(0, Math.min(this.activeQueueIndex, total - 1));
  }

  private customKey(reqId: string, qId: string): string {
    return `${reqId}::${qId}`;
  }

  private getCustomAnswer(reqId: string, qId: string): string {
    const _rev = this.customAnswersRevision;
    const val = this.customAnswersMap.get(this.customKey(reqId, qId));
    return val !== undefined ? val : (_rev >= 0 ? '' : '');
  }

  private setCustomAnswer(reqId: string, qId: string, val: string): void {
    const key = this.customKey(reqId, qId);
    if (val.length === 0) {
      this.customAnswersMap.delete(key);
    } else {
      this.customAnswersMap.set(key, val);
    }
    this.customAnswersRevision = this.customAnswersRevision + 1;
  }

  private isOptionSelected(key: string): boolean {
    return this.selectedMultiOptions.indexOf(key) >= 0;
  }

  private toggleMultiOption(key: string): void {
    const idx = this.selectedMultiOptions.indexOf(key);
    if (idx >= 0) {
      const next: string[] = [];
      for (let i = 0; i < this.selectedMultiOptions.length; i++) {
        if (i !== idx) {
          next.push(this.selectedMultiOptions[i]);
        }
      }
      this.selectedMultiOptions = next;
    } else {
      const next: string[] = [];
      for (let i = 0; i < this.selectedMultiOptions.length; i++) {
        next.push(this.selectedMultiOptions[i]);
      }
      next.push(key);
      this.selectedMultiOptions = next;
    }
    this.store.triggerHaptic(8);
  }

  private permissionTargetPreview(item: OrbitTimelineItemModel): string {
    if (item.detail !== null) {
      if (item.detail.kind === 'shell' && (item.detail.command ?? '').length > 0) {
        return `$ ${item.detail.command ?? ''}`;
      }
      if ((item.detail.path ?? '').length > 0) {
        return item.detail.path ?? '';
      }
      if ((item.detail.tool_name ?? '').length > 0) {
        return item.detail.tool_name ?? '';
      }
    }
    return item.title.length > 0 ? item.title : 'Tool execution requires approval';
  }

  private submitSingleCustomAnswer(reqId: string, qId: string): void {
    const custom = this.getCustomAnswer(reqId, qId).trim();
    if (reqId.length === 0 || custom.length === 0) {
      return;
    }
    this.setCustomAnswer(reqId, qId, '');
    this.store.answerQuestion(reqId, [[custom]]);
  }

  private submitMultiQuestion(req: OrbitTimelineItemModel): void {
    const reqId = req.requestId;
    const questions = req.questions;
    if (reqId.length === 0 || questions.length === 0) {
      return;
    }
    const matrix: string[][] = [];
    for (let qIdx = 0; qIdx < questions.length; qIdx++) {
      const q = questions[qIdx];
      const chosen: string[] = [];
      for (let oIdx = 0; oIdx < q.options.length; oIdx++) {
        const opt = q.options[oIdx];
        const key = `${reqId}_${q.id}_${opt}`;
        if (this.isOptionSelected(key)) {
          chosen.push(opt);
        }
      }
      const custom = this.getCustomAnswer(reqId, q.id).trim();
      if (custom.length > 0) {
        chosen.push(custom);
        this.setCustomAnswer(reqId, q.id, '');
      }
      if (chosen.length === 0 && q.options.length > 0) {
        chosen.push(q.options[0]);
      }
      matrix.push(chosen);
    }
    this.selectedMultiOptions = [];
    this.store.answerQuestion(reqId, matrix);
  }

  @Builder
  private renderQueueStepper(currentIdx: number, total: number) {
    if (total > 1) {
      Row({ space: OrbitTokens.SPACE_XS }) {
        Row() {
          Text(`${currentIdx + 1} of ${total}`)
            .attributeModifier(orbitTextMono(this.store.palette.fgSecondary, FontWeight.Medium))
        }
        .constraintSize({ minHeight: 28 })
        .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
        .backgroundColor(this.store.palette.subtleSurface)
        .borderRadius(OrbitTokens.RADIUS_SM)
        .alignItems(VerticalAlign.Center)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.chevron_left'))
            .fontSize(14)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Previous pending decision')
        .onClick(() => {
          this.activeQueueIndex = (currentIdx - 1 + total) % total;
          this.isPreviewExpanded = false;
          this.store.triggerHaptic(6);
        })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.chevron_right'))
            .fontSize(14)
            .renderingStrategy(SymbolRenderingStrategy.SINGLE)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Next pending decision')
        .onClick(() => {
          this.activeQueueIndex = (currentIdx + 1) % total;
          this.isPreviewExpanded = false;
          this.store.triggerHaptic(6);
        })
      }
      .alignItems(VerticalAlign.Center)
    }
  }

  @Builder
  private renderQuestionOptionsList(reqId: string, qItem: OrbitTimelineItemModel, q: QuestionItemDto) {
    Column({ space: OrbitTokens.SPACE_XS }) {
      ForEach(
        q.options,
        (opt: string) => {
          Button({ type: ButtonType.Normal, stateEffect: true }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              if (q.is_multi_select || qItem.questions.length > 1) {
                Checkbox({ name: `${reqId}_${q.id}_${opt}` })
                  .select(this.isOptionSelected(`${reqId}_${q.id}_${opt}`))
                  .selectedColor(this.store.palette.accentPrimaryFill)
                  .width(18)
                  .height(18)
                  .hitTestBehavior(HitTestMode.None)
              } else {
                Radio({ value: opt, group: `${reqId}_${q.id}` })
                  .checked(this.isOptionSelected(`${reqId}_${q.id}_${opt}`))
                  .width(18)
                  .height(18)
                  .hitTestBehavior(HitTestMode.None)
              }

              Text(opt)
                .attributeModifier(
                  orbitTextSubhead(
                    this.isOptionSelected(`${reqId}_${q.id}_${opt}`)
                      ? this.store.palette.accentPrimary
                      : this.store.palette.fgPrimary,
                    FontWeight.Normal
                  )
                )
                .textAlign(TextAlign.Start)
                .layoutWeight(1)
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)
          }
          .width('100%')
          .constraintSize({ minHeight: OrbitTokens.HIT_MIN })
          .padding({
            left: OrbitTokens.SPACE_MD,
            right: OrbitTokens.SPACE_MD,
            top: OrbitTokens.SPACE_SM,
            bottom: OrbitTokens.SPACE_SM
          })
          .backgroundColor(
            this.isOptionSelected(`${reqId}_${q.id}_${opt}`)
              ? this.store.palette.accentPrimarySoft
              : this.store.palette.subtleSurface
          )
          .borderRadius(OrbitTokens.RADIUS_MD)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.isOptionSelected(`${reqId}_${q.id}_${opt}`)
              ? this.store.palette.accentPrimary
              : this.store.palette.hairline
          })
          .onClick(() => {
            if (reqId.length === 0) {
              return;
            }
            if (!q.is_multi_select && qItem.questions.length === 1) {
              this.setCustomAnswer(reqId, q.id, '');
              this.store.answerQuestion(reqId, [[opt]]);
            } else {
              this.toggleMultiOption(`${reqId}_${q.id}_${opt}`);
            }
          })
        },
        (opt: string) => `${q.id}_${opt}`
      )
    }
    .width('100%')
  }

  @Builder
  private renderPermissionCard(perm: OrbitTimelineItemModel, currentIdx: number, total: number) {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(OrbitTokens.DOT_SIZE)
          .height(OrbitTokens.DOT_SIZE)
          .borderRadius(3)
          .backgroundColor(this.store.palette.statusWarning)

        Text(perm.title.length > 0 ? perm.title : 'Approval Required')
          .attributeModifier(orbitTextSubhead(this.store.palette.statusWarning, FontWeight.Bold))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        this.renderQueueStepper(currentIdx, total)
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Column({ space: 4 }) {
        Text(this.permissionTargetPreview(perm))
          .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
          .copyOption(CopyOptions.LocalDevice)
          .width('100%')
          .maxLines(this.isPreviewExpanded ? 24 : 3)
          .textOverflow({ overflow: TextOverflow.Ellipsis })

        if (this.permissionTargetPreview(perm).length > 100) {
          Text(this.isPreviewExpanded ? 'Show less' : 'Tap to inspect full command')
            .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Medium))
        }
      }
      .width('100%')
      .alignItems(HorizontalAlign.Start)
      .padding(OrbitTokens.SPACE_SM)
      .backgroundColor(this.store.palette.code)
      .borderRadius(OrbitTokens.RADIUS_SM)
      .onClick(() => {
        if (this.permissionTargetPreview(perm).length > 100) {
          this.isPreviewExpanded = !this.isPreviewExpanded;
        }
      })

      Row({ space: OrbitTokens.SPACE_SM }) {
        ForEach(
          perm.allowedDecisions.length > 0
            ? perm.allowedDecisions
            : ['allow', 'allow_always', 'deny'],
          (dec: string) => {
            Button(formatPermissionDecisionLabel(dec))
              .attributeModifier(
                orbitButtonSubhead(
                  dec.toLowerCase().indexOf('deny') >= 0
                    ? this.store.palette.statusDanger
                    : dec.toLowerCase().indexOf('always') >= 0 ||
                        dec.toLowerCase().indexOf('session') >= 0
                      ? this.store.palette.fgPrimary
                      : this.store.palette.onAccent,
                  dec.toLowerCase().indexOf('deny') >= 0
                    ? this.store.palette.statusDangerSoft
                    : dec.toLowerCase().indexOf('always') >= 0 ||
                        dec.toLowerCase().indexOf('session') >= 0
                      ? this.store.palette.subtleSurface
                      : this.store.palette.accentPrimaryFill
                )
              )
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .layoutWeight(1)
              .onClick(() => {
                if (perm.requestId.length > 0) {
                  this.store.resolvePermission(perm.requestId, dec);
                }
              })
          },
          (dec: string) => `${perm.requestId.length > 0 ? perm.requestId : perm.seq}_${dec}`
        )
      }
      .width('100%')
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_MD)
    .backgroundColor(this.store.palette.listCard)
    .borderRadius(OrbitTokens.RADIUS_LG)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.palette.hairline
    })
  }

  @Builder
  private renderQuestionCard(qItem: OrbitTimelineItemModel, currentIdx: number, total: number) {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(OrbitTokens.DOT_SIZE)
          .height(OrbitTokens.DOT_SIZE)
          .borderRadius(3)
          .backgroundColor(this.store.palette.accentPrimary)

        Text(
          (qItem.questions.length > 0 &&
            qItem.questions[0].header !== null &&
            qItem.questions[0].header.length > 0)
            ? qItem.questions[0].header
            : 'Question Required'
        )
          .attributeModifier(orbitTextSubhead(this.store.palette.accentPrimary, FontWeight.Bold))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        this.renderQueueStepper(currentIdx, total)
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      ForEach(
        qItem.questions,
        (q: QuestionItemDto, qIdx: number) => {
          Column({ space: OrbitTokens.SPACE_SM }) {
            if (qIdx > 0 && q.header !== null && q.header.length > 0) {
              Text(q.header.toUpperCase())
                .attributeModifier(orbitTextMeta(this.store.palette.accentPrimary, FontWeight.Bold))
            }

            Text(q.question)
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              .width('100%')

            // Natural-height unclipped Column for <= 6 options; bounded Scroll only when > 6
            if (q.options.length <= 6) {
              this.renderQuestionOptionsList(qItem.requestId, qItem, q)
            } else {
              Scroll() {
                this.renderQuestionOptionsList(qItem.requestId, qItem, q)
              }
              .constraintSize({ maxHeight: 224 })
              .scrollBar(BarState.Auto)
              .width('100%')
            }

            // Optional "Custom answer…" text input row
            Row({ space: OrbitTokens.SPACE_SM }) {
              TextInput({
                placeholder: 'Custom answer…',
                text: this.getCustomAnswer(qItem.requestId, q.id)
              })
                .attributeModifier(
                  orbitInputSubhead(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.code
                  )
                )
                .height(OrbitTokens.HIT_COMPACT)
                .borderRadius(OrbitTokens.RADIUS_SM)
                .border({
                  width: OrbitTokens.HAIRLINE,
                  color: this.store.palette.hairline
                })
                .layoutWeight(1)
                .onChange((val: string) => {
                  this.setCustomAnswer(qItem.requestId, q.id, val);
                })
                .onSubmit(() => {
                  if (!q.is_multi_select && qItem.questions.length === 1) {
                    this.submitSingleCustomAnswer(qItem.requestId, q.id);
                  }
                })

              if (!q.is_multi_select && qItem.questions.length === 1) {
                Button('Send')
                  .attributeModifier(
                    orbitButtonMeta(
                      this.getCustomAnswer(qItem.requestId, q.id).trim().length > 0
                        ? this.store.palette.onAccent
                        : this.store.palette.fgMuted,
                      this.getCustomAnswer(qItem.requestId, q.id).trim().length > 0
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.subtleSurface
                    )
                  )
                  .height(OrbitTokens.HIT_COMPACT)
                  .borderRadius(OrbitTokens.RADIUS_SM)
                  .onClick(() => {
                    this.submitSingleCustomAnswer(qItem.requestId, q.id);
                  })
              }
            }
            .width('100%')

            if (q.is_multi_select && qItem.questions.length === 1) {
              Button('Submit Selection')
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .height(OrbitTokens.HIT_MIN)
                .width('100%')
                .borderRadius(OrbitTokens.RADIUS_MD)
                .onClick(() => {
                  this.submitMultiQuestion(qItem);
                })
            }
          }
          .alignItems(HorizontalAlign.Start)
          .width('100%')
        },
        (q: QuestionItemDto) => q.id
      )

      if (qItem.questions.length > 1) {
        Button('Submit Answers')
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .height(OrbitTokens.HIT_MIN)
          .width('100%')
          .borderRadius(OrbitTokens.RADIUS_MD)
          .onClick(() => {
            this.submitMultiQuestion(qItem);
          })
      }
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_MD)
    .backgroundColor(this.store.palette.listCard)
    .borderRadius(OrbitTokens.RADIUS_LG)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.palette.hairline
    })
  }

  build() {
    if (this.decisionQueue.length > 0) {
      Column() {
        if (this.decisionQueue[this.activeIndex].kind === 'permission') {
          this.renderPermissionCard(
            this.decisionQueue[this.activeIndex].item,
            this.activeIndex,
            this.decisionQueue.length
          )
        } else {
          this.renderQuestionCard(
            this.decisionQueue[this.activeIndex].item,
            this.activeIndex,
            this.decisionQueue.length
          )
        }
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_SM,
        bottom: OrbitTokens.SPACE_XS
      })
    }
  }
}
