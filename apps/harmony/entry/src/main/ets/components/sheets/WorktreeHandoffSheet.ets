import {
  ExecuteWorktreeHandoffRequestDto,
  HandoffFileChangeDto,
  OrbitHandoffGuardState,
  PreviewWorktreeHandoffRequestDto,
  WorktreeHandoffOutcomeDto,
  WorktreeHandoffPreviewDto,
  basenameOfPath,
  evaluateWorktreeHandoffGuards
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

/**
 * Bidirectional Git Worktree <-> Local Handoff Sheet (PR-5).
 * Strictly bound to `(hostId, workspaceId, sourceAgentId)` with monotonic request sequencing
 * so cross-session switches never leak preview data or execute against the wrong workspace.
 */
@ComponentV2
export struct WorktreeHandoffSheet {
  @Param hostId: string = '';
  @Param workspaceId: string = '';
  @Param sourceAgentId: string = '';
  @Param cwd: string = '';
  @Param title: string = '';
  @Event onClose: () => void = () => {};

  @Local private handoffDirection: string = 'worktree_to_local';
  @Local private handoffMode: string = 'apply';
  @Local private includeCommitted: boolean = true;
  @Local private allowThreeWay: boolean = false;
  @Local private branchInput: string = '';
  @Local private preview: WorktreeHandoffPreviewDto | null = null;
  @Local private outcome: WorktreeHandoffOutcomeDto | null = null;
  @Local private loadingPreview: boolean = false;
  @Local private executing: boolean = false;

  private lastTargetSig: string = '';
  private previewReqSeq: number = 0;
  private executeReqSeq: number = 0;
  private disposed: boolean = false;
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  private targetSig(): string {
    return `${this.hostId}|${this.workspaceId}|${this.sourceAgentId}`;
  }

  aboutToAppear(): void {
    this.disposed = false;
    this.lastTargetSig = this.targetSig();
    void this.refreshPreview();
  }

  aboutToDisappear(): void {
    this.disposed = true;
    this.previewReqSeq += 1;
    this.executeReqSeq += 1;
  }

  @Monitor('hostId', 'workspaceId', 'sourceAgentId')
  onTargetChanged(_mon: IMonitor): void {
    const nextSig = this.targetSig();
    if (nextSig !== this.lastTargetSig) {
      this.lastTargetSig = nextSig;
      this.previewReqSeq += 1;
      this.executeReqSeq += 1;
      this.preview = null;
      this.outcome = null;
      this.loadingPreview = false;
      this.executing = false;
      this.allowThreeWay = false;
      this.branchInput = '';
      void this.refreshPreview();
    }
  }

  private async refreshPreview(): Promise<void> {
    if (this.executing) {
      return;
    }
    const cleanHost = this.hostId.trim();
    const cleanWs = this.workspaceId.trim();
    if (cleanHost.length === 0 || cleanWs.length === 0) {
      this.preview = null;
      return;
    }
    const sig = this.targetSig();
    this.previewReqSeq += 1;
    const seq = this.previewReqSeq;
    this.loadingPreview = true;
    this.outcome = null;
    const req: PreviewWorktreeHandoffRequestDto = {
      host_id: cleanHost,
      workspace_id: cleanWs,
      source_agent_id: this.sourceAgentId.trim().length > 0 ? this.sourceAgentId.trim() : null,
      direction: this.handoffDirection,
      target_workspace_id: null,
      include_committed: this.includeCommitted
    };
    const res = await this.store.previewScopedWorktreeHandoff(req);
    if (!this.disposed && !this.executing && seq === this.previewReqSeq && sig === this.targetSig()) {
      this.preview = res.ok ? res.data : null;
      this.loadingPreview = false;
      if (!res.ok) {
        this.store.showToastResource(
          res.errorDetail.length > 0
            ? $r('app.string.handoff_preview_failed_detail', res.errorDetail)
            : $r('app.string.handoff_preview_failed'),
          true,
          res.errorDetail
        );
      }
    }
  }

  private async runExecute(): Promise<void> {
    if (!this.canExecute) {
      return;
    }
    const cleanHost = this.hostId.trim();
    const cleanWs = this.workspaceId.trim();
    if (cleanHost.length === 0 || cleanWs.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const sig = this.targetSig();
    // Freeze target & parameters and invalidate any earlier in-flight preview without colliding with executeReqSeq.
    this.previewReqSeq += 1;
    this.loadingPreview = false;
    this.executeReqSeq += 1;
    const execSeq = this.executeReqSeq;
    this.executing = true;
    const frozenDirection = this.handoffDirection;
    const frozenMode = this.handoffMode;
    const frozenIncludeCommitted = this.includeCommitted;
    const frozenBranch = this.branchInput.trim().length > 0 ? this.branchInput.trim() : null;
    const frozenAllowThreeWay = this.guardState.canOfferThreeWay && this.allowThreeWay;
    const req: ExecuteWorktreeHandoffRequestDto = {
      host_id: cleanHost,
      workspace_id: cleanWs,
      source_agent_id: this.sourceAgentId.trim().length > 0 ? this.sourceAgentId.trim() : null,
      direction: frozenDirection,
      mode: frozenMode,
      target_workspace_id: null,
      include_committed: frozenIncludeCommitted,
      branch: frozenBranch,
      allow_three_way_conflicts: frozenAllowThreeWay
    };
    const res = await this.store.executeScopedWorktreeHandoff(req);
    if (!this.disposed && execSeq === this.executeReqSeq && sig === this.targetSig()) {
      this.executing = false;
      if (res.ok && res.data !== null) {
        this.outcome = res.data;
        this.store.showToastResource(this.localizedOutcomeResource(res.data), false);
        this.store.triggerHaptic(12);
      } else {
        this.store.showToastResource(
          res.errorDetail.length > 0
            ? $r('app.string.handoff_execute_failed_detail', res.errorDetail)
            : $r('app.string.handoff_execute_failed'),
          true,
          res.errorDetail
        );
      }
    }
  }

  @Computed
  get guardState(): OrbitHandoffGuardState {
    return evaluateWorktreeHandoffGuards(
      this.preview,
      this.handoffDirection,
      null,
      this.allowThreeWay,
      this.loadingPreview,
      this.executing
    );
  }

  @Computed
  get canExecute(): boolean {
    return this.guardState.canExecute;
  }

  private localizedOutcomeResource(outcome: WorktreeHandoffOutcomeDto): Resource {
    const fileCount = Math.max(0, Math.floor(outcome.applied_files));
    const isMove = outcome.mode === 'move';
    const wsBranch = outcome.target_workspace !== undefined && outcome.target_workspace !== null
      ? (outcome.target_workspace.git_branch ?? '').trim()
      : '';
    const wsTitle = outcome.target_workspace !== undefined && outcome.target_workspace !== null
      ? outcome.target_workspace.title.trim()
      : '';
    const targetLabel = wsBranch.length > 0
      ? wsBranch
      : (wsTitle.length > 0 ? wsTitle : basenameOfPath(outcome.target_cwd).trim());
    if (targetLabel.length > 0) {
      return isMove
        ? $r('app.string.handoff_success_move_branch', fileCount, targetLabel)
        : $r('app.string.handoff_success_apply_branch', fileCount, targetLabel);
    }
    return isMove
      ? $r('app.string.handoff_success_move', fileCount)
      : $r('app.string.handoff_success_apply', fileCount);
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      // Sheet Header
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text($r('app.string.handoff_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
          Text(this.title.length > 0 ? this.title : basenameOfPath(this.cwd))
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_clockwise'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.handoff_preview_refresh'))
        .enabled(!this.executing && !this.loadingPreview)
        .onClick(() => {
          void this.refreshPreview();
        })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.xmark'))
            .fontSize(15)
            .fontColor([this.store.palette.iconPrimary])
        }
        .width(OrbitTokens.HIT_COMPACT)
        .height(OrbitTokens.HIT_COMPACT)
        .padding(0)
        .backgroundColor(this.store.palette.subtleSurface)
        .accessibilityText($r('app.string.action_close'))
        .onClick(() => {
          this.onClose();
        })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG, top: OrbitTokens.SPACE_MD })
      .alignItems(VerticalAlign.Center)

      Scroll() {
        Column({ space: OrbitTokens.SPACE_MD }) {
          // Direction Selector
          Row({ space: OrbitTokens.SPACE_XS }) {
            Button($r('app.string.handoff_dir_to_local'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(
                  this.handoffDirection === 'worktree_to_local'
                    ? this.store.palette.onAccent
                    : this.store.palette.fgPrimary,
                  this.handoffDirection === 'worktree_to_local'
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.subtleSurface
                )
              )
              .height(34)
              .layoutWeight(1)
              .enabled(!this.executing)
              .onClick(() => {
                if (!this.executing && this.handoffDirection !== 'worktree_to_local') {
                  this.handoffDirection = 'worktree_to_local';
                  void this.refreshPreview();
                }
              })

            Button($r('app.string.handoff_dir_to_worktree'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(
                  this.handoffDirection === 'local_to_worktree'
                    ? this.store.palette.onAccent
                    : this.store.palette.fgPrimary,
                  this.handoffDirection === 'local_to_worktree'
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.subtleSurface
                )
              )
              .height(34)
              .layoutWeight(1)
              .enabled(!this.executing)
              .onClick(() => {
                if (!this.executing && this.handoffDirection !== 'local_to_worktree') {
                  this.handoffDirection = 'local_to_worktree';
                  void this.refreshPreview();
                }
              })
          }
          .width('100%')

          // Mode Selector (Apply vs Move)
          Row({ space: OrbitTokens.SPACE_XS }) {
            Button($r('app.string.handoff_mode_apply'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(
                  this.handoffMode === 'apply'
                    ? this.store.palette.onAccent
                    : this.store.palette.fgPrimary,
                  this.handoffMode === 'apply'
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.subtleSurface
                )
              )
              .height(34)
              .layoutWeight(1)
              .enabled(!this.executing)
              .onClick(() => {
                if (!this.executing) {
                  this.handoffMode = 'apply';
                }
              })

            Button($r('app.string.handoff_mode_move'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(
                  this.handoffMode === 'move'
                    ? this.store.palette.onAccent
                    : this.store.palette.fgPrimary,
                  this.handoffMode === 'move'
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.subtleSurface
                )
              )
              .height(34)
              .layoutWeight(1)
              .enabled(!this.executing)
              .onClick(() => {
                if (!this.executing) {
                  this.handoffMode = 'move';
                }
              })
          }
          .width('100%')

          // Options Card
          Column({ space: OrbitTokens.SPACE_SM }) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              Text($r('app.string.handoff_include_committed'))
                .attributeModifier(orbitTextBody(this.store.palette.fgPrimary))
                .layoutWeight(1)
              Toggle({ type: ToggleType.Switch, isOn: this.includeCommitted })
                .selectedColor(this.store.palette.accentPrimaryFill)
                .enabled(!this.executing)
                .onChange((isOn: boolean) => {
                  if (!this.executing) {
                    this.includeCommitted = isOn;
                    void this.refreshPreview();
                  }
                })
            }
            .width('100%')

            if (this.handoffDirection === 'local_to_worktree') {
              TextInput({
                placeholder: $r('app.string.handoff_branch_placeholder'),
                text: this.branchInput
              })
                .attributeModifier(
                  orbitInputMono(
                    this.store.palette.fgPrimary,
                    this.store.palette.fgMuted,
                    this.store.palette.recessed
                  )
                )
                .height(38)
                .borderRadius(OrbitTokens.RADIUS_MD)
                .width('100%')
                .enabled(!this.executing)
                .onChange((val: string) => {
                  if (!this.executing) {
                    this.branchInput = val;
                  }
                })
            }
          }
          .width('100%')
          .padding(OrbitTokens.SPACE_MD)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .attributeModifier(OrbitTheme.raisedCard(this.store.palette))

          // Dry-Run Preview Area
          if (this.loadingPreview) {
            Row({ space: OrbitTokens.SPACE_SM }) {
              LoadingProgress()
                .width(20)
                .height(20)
                .color(this.store.palette.accentPrimary)
              Text($r('app.string.handoff_preview_refresh'))
                .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
            }
            .width('100%')
            .justifyContent(FlexAlign.Center)
            .padding(OrbitTokens.SPACE_LG)
          } else if (this.preview !== null) {
            Column({ space: OrbitTokens.SPACE_SM }) {
              if (this.guardState.hasHardBlocker) {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  Text($r('app.string.handoff_hard_blocker'))
                    .attributeModifier(orbitTextSubhead(this.store.palette.statusDanger, FontWeight.Bold))
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_SM)
                .backgroundColor(this.store.palette.statusDangerSoft)
                .borderRadius(OrbitTokens.RADIUS_MD)
              } else if (this.guardState.hasUntrackedConflict) {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  Text($r('app.string.handoff_conflicts_warning', this.preview.conflicts.length))
                    .attributeModifier(orbitTextSubhead(this.store.palette.statusDanger, FontWeight.Bold))
                  Text($r('app.string.handoff_untracked_conflict'))
                    .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_SM)
                .backgroundColor(this.store.palette.statusDangerSoft)
                .borderRadius(OrbitTokens.RADIUS_MD)
              } else if (this.guardState.hasConflicts) {
                Column({ space: OrbitTokens.SPACE_XS }) {
                  Text($r('app.string.handoff_conflicts_warning', this.preview.conflicts.length))
                    .attributeModifier(orbitTextSubhead(this.store.palette.statusDanger, FontWeight.Bold))
                  if (this.guardState.canOfferThreeWay) {
                    Row({ space: OrbitTokens.SPACE_SM }) {
                      Text($r('app.string.handoff_allow_three_way'))
                        .attributeModifier(orbitTextMeta(this.store.palette.fgPrimary))
                        .layoutWeight(1)
                      Toggle({ type: ToggleType.Switch, isOn: this.allowThreeWay })
                        .selectedColor(this.store.palette.statusDanger)
                        .enabled(!this.executing)
                        .onChange((isOn: boolean) => {
                          if (!this.executing) {
                            this.allowThreeWay = isOn;
                          }
                        })
                    }
                    .width('100%')
                  }
                }
                .width('100%')
                .padding(OrbitTokens.SPACE_SM)
                .backgroundColor(this.store.palette.statusDangerSoft)
                .borderRadius(OrbitTokens.RADIUS_MD)
              } else if (
                this.preview.can_apply_cleanly &&
                (this.preview.files.length > 0 || this.handoffDirection === 'local_to_worktree')
              ) {
                Text($r('app.string.handoff_clean_preview', this.preview.files.length))
                  .attributeModifier(orbitTextMeta(this.store.palette.statusDirect, FontWeight.Medium))
                  .width('100%')
              } else {
                Text($r('app.string.handoff_empty_preview'))
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                  .width('100%')
              }

              if (this.preview.warning !== undefined && this.preview.warning !== null && this.preview.warning.length > 0) {
                Text(this.preview.warning)
                  .attributeModifier(orbitTextMeta(this.store.palette.statusWarning))
                  .width('100%')
              }

              ForEach(
                this.preview.files,
                (file: HandoffFileChangeDto) => {
                  Row({ space: OrbitTokens.SPACE_XS }) {
                    Text(file.status)
                      .attributeModifier(
                        orbitTextMono(
                          file.has_conflict === true
                            ? this.store.palette.statusDanger
                            : this.store.palette.accentPrimary,
                          FontWeight.Bold
                        )
                      )
                      .width(24)
                    Text(file.path)
                      .attributeModifier(orbitTextMono(this.store.palette.fgPrimary))
                      .maxLines(1)
                      .textOverflow({ overflow: TextOverflow.Ellipsis })
                      .layoutWeight(1)
                    if (file.insertions > 0) {
                      Text(`+${file.insertions}`)
                        .attributeModifier(orbitTextMeta(this.store.palette.statusDirect))
                    }
                    if (file.deletions > 0) {
                      Text(`-${file.deletions}`)
                        .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                    }
                  }
                  .width('100%')
                  .padding({ top: 4, bottom: 4 })
                  .alignItems(VerticalAlign.Center)
                },
                (file: HandoffFileChangeDto): string => `hf_${file.path}_${file.status}`
              )
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
          }

          if (this.outcome !== null) {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text(this.localizedOutcomeResource(this.outcome))
                .attributeModifier(orbitTextBody(this.store.palette.statusDirect, FontWeight.Medium))
                .width('100%')
              if (this.outcome.backup_ref !== undefined && this.outcome.backup_ref !== null && this.outcome.backup_ref.length > 0) {
                Text($r('app.string.handoff_backup_ref', this.outcome.backup_ref))
                  .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
                  .copyOption(CopyOptions.LocalDevice)
                  .width('100%')
              }
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .backgroundColor(this.store.palette.statusDirectSoft)
            .borderRadius(OrbitTokens.RADIUS_MD)
          }
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Auto)

      Row({ space: OrbitTokens.SPACE_SM }) {
        Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonSubhead(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
          .height(42)
          .layoutWeight(1)
          .onClick(() => {
            this.onClose();
          })

        Button($r('app.string.handoff_execute'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
          .height(42)
          .layoutWeight(2)
          .enabled(this.canExecute)
          .onClick(() => {
            void this.runExecute();
          })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        bottom: Math.max(OrbitTokens.SPACE_LG, this.store.safeBottomVp)
      })
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.drawer)
  }
}
