import {
  CapsuleSegmentButtonV2,
  LengthMetrics,
  SegmentButtonV2Items
} from '@kit.ArkUI';
import {
  ConditionalTimerRecordDto,
  GoalCriterionDto,
  GoalRunRecordDto,
  MonitorWatchRecordDto,
  ScheduleSpecDto,
  WorkflowRunRecordDto,
  WorkflowStepRecordDto,
  formatProviderLabel,
  formatTimerConditionLabel,
  formatWorkflowKindLabel
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitInputSubhead,
  orbitSegmentSelectedBg,
  orbitSegmentSelectedFg,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import {
  OrbitSectionHeader,
  dismissHarmonyKeyboard
} from '../common/OrbitCommonControls';

const CRON_PRESETS: string[] = ['*/15 * * * *', '0 * * * *', '0 9 * * *'];

@ComponentV2
export struct AutomationsSheet {
  @Local activeTabIndex: number = 0;
  @Local showCreateForm: boolean = false;
  @Local draftName: string = '';
  @Local draftCron: string = '0 9 * * *';
  @Local draftPrompt: string = '';
  @Local draftWorktree: boolean = false;
  @Local tabItems: SegmentButtonV2Items = new SegmentButtonV2Items([
    { text: 'Schedules' },
    { text: 'Timers' },
    { text: 'Goals' },
    { text: 'Workflows' }
  ]);

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  aboutToAppear(): void {
    this.store.refreshAutomations();
  }

  private scheduleStatusColor(status: string): ResourceColor {
    if (status === 'active') {
      return this.store.palette.statusDirect;
    }
    if (status === 'paused') {
      return this.store.palette.statusWarning;
    }
    return this.store.palette.fgMuted;
  }

  private runStatusColor(status: string): ResourceColor {
    if (status === 'running' || status === 'verifying' || status === 'armed' || status === 'active') {
      return this.store.palette.accentPrimary;
    }
    if (status === 'succeeded' || status === 'fired' || status === 'triggered') {
      return this.store.palette.statusDirect;
    }
    if (status === 'paused' || status === 'partial_failure') {
      return this.store.palette.statusWarning;
    }
    if (status === 'failed' || status === 'error') {
      return this.store.palette.statusDanger;
    }
    return this.store.palette.fgMuted;
  }

  private formatStatusLabel(raw: string): string {
    if (raw.length === 0) {
      return 'Unknown';
    }
    return raw.replace(/_/g, ' ').replace(/\b\w/g, (c: string): string => c.toUpperCase());
  }

  private formatRemainingTimer(timer: ConditionalTimerRecordDto): string {
    if (timer.state !== 'armed') {
      return this.formatStatusLabel(timer.state);
    }
    const remSec = Math.max(0, Math.round((timer.fire_at_ms - Date.now()) / 1000));
    if (remSec < 60) {
      return `${remSec}s left`;
    }
    const mins = Math.floor(remSec / 60);
    const secs = remSec % 60;
    return secs > 0 ? `${mins}m ${secs}s` : `${mins}m`;
  }

  private confirmDeleteSchedule(sched: ScheduleSpecDto): void {
    this.getUIContext().showAlertDialog({
      title: `Delete "${sched.name}"?`,
      message: 'This recurring schedule will be removed permanently.',
      autoCancel: true,
      primaryButton: {
        value: 'Cancel',
        fontColor: this.store.palette.fgSecondary,
        action: () => {
          // Dismissed
        }
      },
      secondaryButton: {
        value: 'Delete',
        fontColor: this.store.palette.statusDanger,
        action: () => {
          this.store.deleteSchedule(sched.schedule_id);
        }
      }
    });
  }

  private submitNewSchedule(): void {
    dismissHarmonyKeyboard(this.getUIContext());
    const ok = this.store.createSchedule(
      this.draftName,
      this.draftCron,
      this.draftPrompt,
      this.store.composerProvider,
      this.draftWorktree
    );
    if (ok) {
      this.draftName = '';
      this.draftPrompt = '';
      this.showCreateForm = false;
    }
  }

  @Computed
  get armedTimers(): ConditionalTimerRecordDto[] {
    return this.store.conditionalTimers.filter(
      (t: ConditionalTimerRecordDto): boolean => t.state === 'armed'
    );
  }

  @Builder
  private renderEmptyState(title: string, subtitle: string) {
    Column({ space: OrbitTokens.SPACE_XS }) {
      Text(title)
        .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
      Text(subtitle)
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
        .textAlign(TextAlign.Center)
    }
    .width('100%')
    .padding(OrbitTokens.SPACE_XL)
    .backgroundColor(this.store.palette.listCard)
    .borderRadius(OrbitTokens.RADIUS_LG)
    .alignItems(HorizontalAlign.Center)
  }

  @Builder
  private renderSchedulesTab() {
    List({ space: OrbitTokens.SPACE_MD }) {
      ListItem() {
        Row({ space: OrbitTokens.SPACE_SM }) {
          Text(`${this.store.schedules.length} configured`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
          Blank()
          Button(this.showCreateForm ? 'Close Form' : 'New Schedule', {
            type: ButtonType.Capsule,
            stateEffect: true
          })
            .attributeModifier(
              orbitButtonMeta(
                this.showCreateForm ? this.store.palette.fgPrimary : this.store.palette.onAccent,
                this.showCreateForm ? this.store.palette.subtleSurface : this.store.palette.accentPrimaryFill
              )
            )
            .height(34)
            .padding({ left: 12, right: 12 })
            .onClick(() => {
              this.showCreateForm = !this.showCreateForm;
              this.store.triggerHaptic(6);
            })
        }
        .width('100%')
        .alignItems(VerticalAlign.Center)
      }

      if (this.showCreateForm) {
        ListItem() {
          Column({ space: OrbitTokens.SPACE_SM }) {
            Text('Create Schedule')
              .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))

            TextInput({ placeholder: 'Schedule name', text: this.draftName })
              .attributeModifier(
                orbitInputSubhead(
                  this.store.palette.fgPrimary,
                  this.store.palette.fgMuted,
                  this.store.palette.subtleSurface
                )
              )
              .height(40)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .width('100%')
              .onChange((val: string) => {
                this.draftName = val;
              })

            TextInput({ placeholder: 'Cron (e.g. 0 9 * * *)', text: this.draftCron })
              .attributeModifier(
                orbitInputMono(
                  this.store.palette.fgPrimary,
                  this.store.palette.fgMuted,
                  this.store.palette.subtleSurface
                )
              )
              .height(40)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
              .width('100%')
              .onChange((val: string) => {
                this.draftCron = val;
              })

            Row({ space: OrbitTokens.SPACE_XS }) {
              ForEach(
                CRON_PRESETS,
                (preset: string) => {
                  Button(preset, { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(
                        this.draftCron === preset
                          ? this.store.palette.accentPrimary
                          : this.store.palette.fgSecondary,
                        this.store.palette.subtleSurface
                      )
                    )
                    .height(30)
                    .padding({ left: 10, right: 10 })
                    .onClick(() => {
                      this.draftCron = preset;
                    })
                },
                (preset: string): string => `preset_${preset}`
              )
            }
            .width('100%')

            TextArea({ placeholder: 'Prompt to run on each trigger…', text: this.draftPrompt })
              .fontSize(OrbitTokens.FONT_SUBHEAD_RES)
              .fontColor(this.store.palette.fgPrimary)
              .placeholderColor(this.store.palette.fgMuted)
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .constraintSize({ minHeight: 72, maxHeight: 140 })
              .width('100%')
              .onChange((val: string) => {
                this.draftPrompt = val;
              })

            Row() {
              Text('Run in isolated git worktree')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgSecondary))
                .layoutWeight(1)
              Toggle({ type: ToggleType.Switch, isOn: this.draftWorktree })
                .onChange((isOn: boolean) => {
                  this.draftWorktree = isOn;
                })
            }
            .width('100%')
            .alignItems(VerticalAlign.Center)

            Button('Create Schedule', { type: ButtonType.Normal, stateEffect: true })
              .attributeModifier(
                orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
              )
              .width('100%')
              .height(OrbitTokens.HIT_MIN)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .onClick(() => {
                this.submitNewSchedule();
              })
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_LG)
          .backgroundColor(this.store.palette.listCard)
          .borderRadius(OrbitTokens.RADIUS_LG)
          .alignItems(HorizontalAlign.Start)
        }
      }

      if (this.store.schedules.length === 0) {
        ListItem() {
          this.renderEmptyState(
            'No schedules yet',
            'Create a cron schedule to run automated agent tasks on this host.'
          )
        }
      } else {
        ForEach(
          this.store.schedules,
          (sched: ScheduleSpecDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Row()
                    .width(8)
                    .height(8)
                    .borderRadius(4)
                    .backgroundColor(this.scheduleStatusColor(sched.status))

                  Text(sched.name)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .layoutWeight(1)

                  Text(sched.cron_expr)
                    .attributeModifier(orbitTextMono(this.store.palette.fgSecondary))
                    .padding({ left: 8, right: 8, top: 2, bottom: 2 })
                    .borderRadius(OrbitTokens.RADIUS_SM)
                    .backgroundColor(this.store.palette.subtleSurface)
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)

                Text(sched.prompt)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                  .maxLines(2)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .width('100%')

                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text(
                    `${this.formatStatusLabel(sched.status)} · ${sched.run_count} runs`
                  )
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                    .layoutWeight(1)

                  if (sched.status === 'paused') {
                    Button('Resume', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                      )
                      .height(32)
                      .padding({ left: 10, right: 10 })
                      .onClick(() => {
                        this.store.resumeSchedule(sched.schedule_id);
                      })
                  } else if (sched.status === 'active') {
                    Button('Pause', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
                      )
                      .height(32)
                      .padding({ left: 10, right: 10 })
                      .onClick(() => {
                        this.store.pauseSchedule(sched.schedule_id);
                      })
                  }

                  Button('Run', { type: ButtonType.Capsule, stateEffect: true })
                    .attributeModifier(
                      orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                    )
                    .height(32)
                    .padding({ left: 10, right: 10 })
                    .onClick(() => {
                      this.store.runScheduleOnce(sched.schedule_id);
                    })

                  Button({ type: ButtonType.Circle, stateEffect: true }) {
                    SymbolGlyph($r('sys.symbol.trash'))
                      .fontSize(14)
                      .fontColor([this.store.palette.statusDanger])
                  }
                  .width(32)
                  .height(32)
                  .padding(0)
                  .backgroundColor(Color.Transparent)
                  .accessibilityText(`Delete ${sched.name}`)
                  .onClick(() => {
                    this.confirmDeleteSchedule(sched);
                  })
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          },
          (sched: ScheduleSpecDto): string => `sched_${sched.schedule_id}_${sched.status}_${sched.run_count}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
    .onScrollStart(() => {
      dismissHarmonyKeyboard(this.getUIContext());
    })
  }

  @Builder
  private renderTimersTab() {
    List({ space: OrbitTokens.SPACE_MD }) {
      if (this.armedTimers.length === 0) {
        ListItem() {
          this.renderEmptyState(
            'No active timers',
            'One-shot conditional timers armed by agents will appear here.'
          )
        }
      } else {
        ForEach(
          this.armedTimers,
          (timer: ConditionalTimerRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text(formatTimerConditionLabel(timer.condition))
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .layoutWeight(1)

                  Text(this.formatRemainingTimer(timer))
                    .attributeModifier(orbitTextMono(this.runStatusColor(timer.state), FontWeight.Medium))
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)

                Text(timer.prompt)
                  .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                  .maxLines(3)
                  .textOverflow({ overflow: TextOverflow.Ellipsis })
                  .width('100%')

                if (timer.state === 'armed') {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Blank()
                    Button('Fire Now', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
                      )
                      .height(32)
                      .padding({ left: 12, right: 12 })
                      .onClick(() => {
                        this.store.fireConditionalTimerNow(timer.timer_id);
                      })

                    Button('Cancel', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
                      )
                      .height(32)
                      .padding({ left: 12, right: 12 })
                      .onClick(() => {
                        this.store.cancelConditionalTimer(timer.timer_id);
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          },
          (timer: ConditionalTimerRecordDto): string => `tmr_${timer.timer_id}_${timer.state}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
  }

  @Builder
  private renderGoalsTab() {
    List({ space: OrbitTokens.SPACE_MD }) {
      ListItem() {
        OrbitSectionHeader({ title: `Goals (${this.store.goalRuns.length})`, isFirst: true })
      }

      if (this.store.goalRuns.length === 0) {
        ListItem() {
          this.renderEmptyState(
            'No active goals',
            'Autonomous goal loops with verification criteria will appear here.'
          )
        }
      } else {
        ForEach(
          this.store.goalRuns,
          (goal: GoalRunRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text(goal.objective)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(2)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .layoutWeight(1)

                  Text(this.formatStatusLabel(goal.status))
                    .attributeModifier(orbitTextMeta(this.runStatusColor(goal.status), FontWeight.Medium))
                    .padding({ left: 8, right: 8, top: 2, bottom: 2 })
                    .borderRadius(OrbitTokens.RADIUS_SM)
                    .backgroundColor(this.store.palette.subtleSurface)
                }
                .width('100%')
                .alignItems(VerticalAlign.Top)

                Text(`Iteration ${goal.current_iteration} / ${goal.max_iterations}`)
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))

                if (goal.criteria.length > 0) {
                  Column({ space: 4 }) {
                    ForEach(
                      goal.criteria,
                      (crit: GoalCriterionDto) => {
                        Row({ space: 6 }) {
                          SymbolGlyph(
                            crit.passed ? $r('sys.symbol.checkmark_circle_fill') : $r('sys.symbol.circle')
                          )
                            .fontSize(13)
                            .fontColor([
                              crit.passed ? this.store.palette.statusDirect : this.store.palette.fgMuted
                            ])
                          Text(crit.label)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                            .layoutWeight(1)
                        }
                        .width('100%')
                        .alignItems(VerticalAlign.Center)
                      },
                      (crit: GoalCriterionDto): string => `crit_${goal.goal_id}_${crit.id}_${crit.passed ? 1 : 0}`
                    )
                  }
                  .width('100%')
                }

                if (goal.status === 'running' || goal.status === 'verifying' || goal.status === 'paused') {
                  Row({ space: OrbitTokens.SPACE_SM }) {
                    Blank()
                    if (goal.status === 'paused') {
                      Button('Resume', { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(this.store.palette.accentPrimary, this.store.palette.subtleSurface)
                        )
                        .height(32)
                        .padding({ left: 12, right: 12 })
                        .onClick(() => {
                          this.store.controlGoalRun(goal.goal_id, 'resume');
                        })
                    } else {
                      Button('Pause', { type: ButtonType.Capsule, stateEffect: true })
                        .attributeModifier(
                          orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
                        )
                        .height(32)
                        .padding({ left: 12, right: 12 })
                        .onClick(() => {
                          this.store.controlGoalRun(goal.goal_id, 'pause');
                        })
                    }

                    Button('Stop', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
                      )
                      .height(32)
                      .padding({ left: 12, right: 12 })
                      .onClick(() => {
                        this.store.controlGoalRun(goal.goal_id, 'cancel');
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          },
          (goal: GoalRunRecordDto): string => `goal_${goal.goal_id}_${goal.status}_${goal.current_iteration}_${goal.updated_at_ms}`
        )
      }

      if (this.store.monitorWatches.length > 0) {
        ListItem() {
          OrbitSectionHeader({ title: `Monitors (${this.store.monitorWatches.length})` })
        }

        ForEach(
          this.store.monitorWatches,
          (mon: MonitorWatchRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_XS }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text(mon.name)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .layoutWeight(1)
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                  Text(`${this.formatStatusLabel(mon.status)} · ${mon.match_count} matches`)
                    .attributeModifier(orbitTextMeta(this.runStatusColor(mon.status)))
                }
                .width('100%')

                if (mon.last_event_preview !== undefined && mon.last_event_preview !== null && mon.last_event_preview.length > 0) {
                  Text(mon.last_event_preview)
                    .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                    .maxLines(2)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .width('100%')
                }

                if (mon.status === 'active' || mon.status === 'triggered' || mon.status === 'paused') {
                  Row() {
                    Blank()
                    Button('Stop', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
                      )
                      .height(30)
                      .padding({ left: 10, right: 10 })
                      .onClick(() => {
                        this.store.stopMonitorWatch(mon.monitor_id);
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          },
          (mon: MonitorWatchRecordDto): string => `mon_${mon.monitor_id}_${mon.status}_${mon.match_count}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
  }

  @Builder
  private renderWorkflowsTab() {
    List({ space: OrbitTokens.SPACE_MD }) {
      if (this.store.workflowRuns.length === 0) {
        ListItem() {
          this.renderEmptyState(
            'No workflow runs',
            'Batch CSV fan-out, DAG pipelines, and Best-of-N arenas will appear here.'
          )
        }
      } else {
        ForEach(
          this.store.workflowRuns,
          (run: WorkflowRunRecordDto) => {
            ListItem() {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text(run.name)
                    .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                    .maxLines(1)
                    .textOverflow({ overflow: TextOverflow.Ellipsis })
                    .layoutWeight(1)

                  Text(formatWorkflowKindLabel(run.kind))
                    .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
                    .padding({ left: 8, right: 8, top: 2, bottom: 2 })
                    .borderRadius(OrbitTokens.RADIUS_SM)
                    .backgroundColor(this.store.palette.subtleSurface)

                  Text(this.formatStatusLabel(run.status))
                    .attributeModifier(orbitTextMeta(this.runStatusColor(run.status), FontWeight.Medium))
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)

                if (run.steps.length > 0) {
                  Column({ space: 6 }) {
                    ForEach(
                      run.steps,
                      (step: WorkflowStepRecordDto) => {
                        Row({ space: OrbitTokens.SPACE_SM }) {
                          Row()
                            .width(6)
                            .height(6)
                            .borderRadius(3)
                            .backgroundColor(this.runStatusColor(step.status))

                          Text(step.name)
                            .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary, FontWeight.Medium))
                            .maxLines(1)
                            .textOverflow({ overflow: TextOverflow.Ellipsis })
                            .layoutWeight(1)

                          Text(formatProviderLabel(step.provider))
                            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))

                          if (step.is_winner) {
                            Text('Winner')
                              .attributeModifier(orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium))
                          }
                        }
                        .width('100%')
                        .alignItems(VerticalAlign.Center)
                      },
                      (step: WorkflowStepRecordDto): string => `wf_step_${run.run_id}_${step.step_id}_${step.status}`
                    )
                  }
                  .width('100%')
                }

                if (run.status === 'running' || run.status === 'pending') {
                  Row() {
                    Blank()
                    Button('Cancel Run', { type: ButtonType.Capsule, stateEffect: true })
                      .attributeModifier(
                        orbitButtonMeta(this.store.palette.statusDanger, this.store.palette.statusDangerSoft)
                      )
                      .height(32)
                      .padding({ left: 12, right: 12 })
                      .onClick(() => {
                        this.store.cancelWorkflowRun(run.run_id);
                      })
                  }
                  .width('100%')
                }
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          },
          (run: WorkflowRunRecordDto): string => `wf_${run.run_id}_${run.status}_${run.updated_at_ms}`
        )
      }
    }
    .width('100%')
    .layoutWeight(1)
    .scrollBar(BarState.Auto)
    .edgeEffect(EdgeEffect.Spring)
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Text('Automations')
          .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
          .layoutWeight(1)

        if (this.store.automationsLoading) {
          LoadingProgress()
            .width(18)
            .height(18)
            .color(this.store.palette.iconSecondary)
        }

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(17)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText('Refresh automations')
        .onClick(() => {
          this.store.refreshAutomations();
          this.store.triggerHaptic(6);
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      CapsuleSegmentButtonV2({
        items: this.tabItems,
        selectedIndex: this.activeTabIndex,
        onItemClicked: (idx: number) => {
          this.activeTabIndex = idx;
          this.store.triggerHaptic(6);
        },
        itemFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
        itemSelectedFontSize: LengthMetrics.fp(OrbitTokens.FONT_META),
        itemSelectedBackgroundColor: orbitSegmentSelectedBg(this.store.palette),
        itemSelectedFontColor: orbitSegmentSelectedFg(this.store.palette),
        itemMaxFontScale: 1.3,
        itemMinHeight: LengthMetrics.vp(36)
      })
        .width('100%')

      if (this.activeTabIndex === 0) {
        this.renderSchedulesTab()
      } else if (this.activeTabIndex === 1) {
        this.renderTimersTab()
      } else if (this.activeTabIndex === 2) {
        this.renderGoalsTab()
      } else {
        this.renderWorkflowsTab()
      }
    }
    .width('100%')
    .height('100%')
    .padding({
      left: OrbitTokens.SPACE_LG,
      right: OrbitTokens.SPACE_LG,
      top: OrbitTokens.SPACE_LG,
      bottom: this.store.keyboardHeightVp > 0
        ? OrbitTokens.SPACE_MD
        : Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
    })
    .backgroundColor(this.store.palette.drawer)
  }
}
