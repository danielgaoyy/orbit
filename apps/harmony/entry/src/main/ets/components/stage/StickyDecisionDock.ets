import {
  QuestionItemDto,
  TimelineItemDto,
  formatPermissionDecisionLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputSubhead,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';

interface QueuedDecisionCard {
  key: string;
  kind: string; // 'permission' | 'question'
  seq: number;
  item: TimelineItemDto;
}

@ComponentV2
export struct StickyDecisionDock {
  @Local activeQueueIndex: number = 0;
  @Local selectedMultiOptions: string[] = [];
  @Local customAnswersRevision: number = 0;
  private customAnswersMap: Map<string, string> = new Map<string, string>();

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private orderedDecisionQueue(): QueuedDecisionCard[] {
    const combined: QueuedDecisionCard[] = [];
    for (let i = 0; i < this.store.pendingPermissions.length; i++) {
      const perm = this.store.pendingPermissions[i];
      combined.push({
        key: `perm_${perm.request_id ?? perm.seq}`,
        kind: 'permission',
        seq: perm.seq,
        item: perm
      });
    }
    for (let i = 0; i < this.store.pendingQuestions.length; i++) {
      const q = this.store.pendingQuestions[i];
      combined.push({
        key: `q_${q.request_id ?? q.seq}`,
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

  private permissionTargetPreview(item: TimelineItemDto): string {
    if (item.detail !== undefined) {
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
    return item.title ?? 'Tool execution requires approval';
  }

  private submitSingleCustomAnswer(reqId: string, qId: string): void {
    const custom = this.getCustomAnswer(reqId, qId).trim();
    if (reqId.length === 0 || custom.length === 0) {
      return;
    }
    this.setCustomAnswer(reqId, qId, '');
    this.store.answerQuestion(reqId, [[custom]]);
  }

  private submitMultiQuestion(req: TimelineItemDto): void {
    const reqId = req.request_id ?? '';
    const questions = req.questions ?? [];
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
        .height(24)
        .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
        .backgroundColor(this.store.palette.subtleSurface)
        .borderRadius(OrbitTokens.PILL_RADIUS)
        .alignItems(VerticalAlign.Center)

        Stack({ alignContent: Alignment.Center }) {
          Row() {
            Text('<')
              .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Bold))
          }
          .width(28)
          .height(28)
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.store.palette.subtleSurface)
          .borderRadius(OrbitTokens.PILL_RADIUS)
        }
        .width(OrbitTokens.HIT_MIN)
        .height(OrbitTokens.HIT_MIN)
        .accessibilityText('Previous pending decision')
        .onClick(() => {
          this.activeQueueIndex = (currentIdx - 1 + total) % total;
          this.store.triggerHaptic(6);
        })

        Stack({ alignContent: Alignment.Center }) {
          Row() {
            Text('>')
              .attributeModifier(orbitTextMono(this.store.palette.fgPrimary, FontWeight.Bold))
          }
          .width(28)
          .height(28)
          .justifyContent(FlexAlign.Center)
          .backgroundColor(this.store.palette.subtleSurface)
          .borderRadius(OrbitTokens.PILL_RADIUS)
        }
        .width(OrbitTokens.HIT_MIN)
        .height(OrbitTokens.HIT_MIN)
        .accessibilityText('Next pending decision')
        .onClick(() => {
          this.activeQueueIndex = (currentIdx + 1) % total;
          this.store.triggerHaptic(6);
        })
      }
      .alignItems(VerticalAlign.Center)
    }
  }

  @Builder
  private renderQuestionOptionsList(reqId: string, qItem: TimelineItemDto, q: QuestionItemDto) {
    Column({ space: OrbitTokens.SPACE_XS }) {
      ForEach(
        q.options,
        (opt: string) => {
          Row({ space: OrbitTokens.SPACE_SM }) {
            Text(
              q.is_multi_select || (qItem.questions?.length ?? 1) > 1
                ? this.isOptionSelected(`${reqId}_${q.id}_${opt}`)
                  ? '☑'
                  : '☐'
                : '○'
            )
              .attributeModifier(
                orbitTextSubhead(
                  this.isOptionSelected(`${reqId}_${q.id}_${opt}`)
                    ? this.store.palette.accentPrimary
                    : this.store.palette.fgMuted,
                  FontWeight.Medium
                )
              )

            Text(opt)
              .attributeModifier(
                orbitTextSubhead(
                  this.isOptionSelected(`${reqId}_${q.id}_${opt}`)
                    ? this.store.palette.accentPrimary
                    : this.store.palette.fgPrimary,
                  FontWeight.Normal
                )
              )
              .layoutWeight(1)
          }
          .width('100%')
          .constraintSize({ minHeight: 40 })
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
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .border({
            width: OrbitTokens.HAIRLINE,
            color: this.isOptionSelected(`${reqId}_${q.id}_${opt}`)
              ? this.store.palette.accentPrimary
              : this.store.palette.hairline
          })
          .alignItems(VerticalAlign.Center)
          .onClick(() => {
            if (reqId.length === 0) {
              return;
            }
            if (!q.is_multi_select && (qItem.questions?.length ?? 1) === 1) {
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
  private renderPermissionCard(perm: TimelineItemDto, currentIdx: number, total: number) {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(OrbitTokens.DOT_SIZE)
          .height(OrbitTokens.DOT_SIZE)
          .borderRadius(3)
          .backgroundColor(this.store.palette.statusWarning)

        Text(perm.title ?? 'Approval Required')
          .attributeModifier(orbitTextSubhead(this.store.palette.statusWarning, FontWeight.Bold))
          .maxLines(1)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
          .layoutWeight(1)

        this.renderQueueStepper(currentIdx, total)
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Text(this.permissionTargetPreview(perm))
        .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
        .width('100%')
        .maxLines(3)
        .textOverflow({ overflow: TextOverflow.Ellipsis })
        .padding(OrbitTokens.SPACE_SM)
        .backgroundColor(this.store.palette.code)
        .borderRadius(OrbitTokens.PILL_RADIUS)

      Row({ space: OrbitTokens.SPACE_SM }) {
        ForEach(
          (perm.allowed_decisions !== undefined && perm.allowed_decisions.length > 0)
            ? perm.allowed_decisions
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
              .borderRadius(OrbitTokens.PILL_RADIUS)
              .layoutWeight(1)
              .onClick(() => {
                if (perm.request_id !== undefined) {
                  this.store.resolvePermission(perm.request_id, dec);
                }
              })
          },
          (dec: string) => `${perm.request_id ?? perm.seq}_${dec}`
        )
      }
      .width('100%')
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_MD)
    .backgroundColor(this.store.palette.elevated)
    .borderRadius(OrbitTokens.CARD_RADIUS)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.palette.hairline
    })
  }

  @Builder
  private renderQuestionCard(qItem: TimelineItemDto, currentIdx: number, total: number) {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Row()
          .width(OrbitTokens.DOT_SIZE)
          .height(OrbitTokens.DOT_SIZE)
          .borderRadius(3)
          .backgroundColor(this.store.palette.accentPrimary)

        Text(
          (qItem.questions !== undefined &&
            qItem.questions.length > 0 &&
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
        qItem.questions ?? [],
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
              this.renderQuestionOptionsList(qItem.request_id ?? '', qItem, q)
            } else {
              Scroll() {
                this.renderQuestionOptionsList(qItem.request_id ?? '', qItem, q)
              }
              .constraintSize({ maxHeight: 224 })
              .scrollBar(BarState.Auto)
              .width('100%')
            }

            // Optional "Custom answer…" text input row
            Row({ space: OrbitTokens.SPACE_SM }) {
              TextInput({
                placeholder: 'Custom answer…',
                text: this.getCustomAnswer(qItem.request_id ?? '', q.id)
              })
                .attributeModifier(
                  orbitInputSubhead(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.code
                  )
                )
                .height(40)
                .borderRadius(OrbitTokens.PILL_RADIUS)
                .border({
                  width: OrbitTokens.HAIRLINE,
                  color: this.store.palette.hairline
                })
                .layoutWeight(1)
                .onChange((val: string) => {
                  this.setCustomAnswer(qItem.request_id ?? '', q.id, val);
                })
                .onSubmit(() => {
                  if (!q.is_multi_select && (qItem.questions?.length ?? 1) === 1) {
                    this.submitSingleCustomAnswer(qItem.request_id ?? '', q.id);
                  }
                })

              if (!q.is_multi_select && (qItem.questions?.length ?? 1) === 1) {
                Button('Send')
                  .attributeModifier(
                    orbitButtonMeta(
                      this.getCustomAnswer(qItem.request_id ?? '', q.id).trim().length > 0
                        ? this.store.palette.onAccent
                        : this.store.palette.fgMuted,
                      this.getCustomAnswer(qItem.request_id ?? '', q.id).trim().length > 0
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.subtleSurface
                    )
                  )
                  .height(40)
                  .borderRadius(OrbitTokens.PILL_RADIUS)
                  .onClick(() => {
                    this.submitSingleCustomAnswer(qItem.request_id ?? '', q.id);
                  })
              }
            }
            .width('100%')

            if (q.is_multi_select && (qItem.questions?.length ?? 1) === 1) {
              Button('Submit Selection')
                .attributeModifier(
                  orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
                )
                .height(OrbitTokens.HIT_MIN)
                .width('100%')
                .borderRadius(OrbitTokens.PILL_RADIUS)
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

      if ((qItem.questions?.length ?? 0) > 1) {
        Button('Submit Answers')
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .height(OrbitTokens.HIT_MIN)
          .width('100%')
          .borderRadius(OrbitTokens.PILL_RADIUS)
          .onClick(() => {
            this.submitMultiQuestion(qItem);
          })
      }
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_MD)
    .backgroundColor(this.store.palette.elevated)
    .borderRadius(OrbitTokens.CARD_RADIUS)
    .border({
      width: OrbitTokens.HAIRLINE,
      color: this.store.palette.hairline
    })
  }

  build() {
    if (this.orderedDecisionQueue().length > 0) {
      Column() {
        if (this.orderedDecisionQueue()[this.safeIndex(this.orderedDecisionQueue().length)].kind === 'permission') {
          this.renderPermissionCard(
            this.orderedDecisionQueue()[this.safeIndex(this.orderedDecisionQueue().length)].item,
            this.safeIndex(this.orderedDecisionQueue().length),
            this.orderedDecisionQueue().length
          )
        } else {
          this.renderQuestionCard(
            this.orderedDecisionQueue()[this.safeIndex(this.orderedDecisionQueue().length)].item,
            this.safeIndex(this.orderedDecisionQueue().length),
            this.orderedDecisionQueue().length
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
