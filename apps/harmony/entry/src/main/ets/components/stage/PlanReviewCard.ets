import { PendingPlanApprovalDto, stripWorkspacePrefix } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitStageStore } from '../../model/stores/OrbitStageStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';
import { RichMarkdownView } from './RichMarkdownView';

@ComponentV2
export struct PlanReviewCard {
  @Local private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();
  @Local private stageStore: OrbitStageStore = OrbitHarmonyStore.getInstance().stageStore;
  @Local private isEditingPlan: boolean = false;
  @Local private editedMarkdown: string = '';
  @Local private feedbackText: string = '';
  @Local private isResolving: boolean = false;
  @Local private activeRequestId: string = '';

  @Monitor('stageStore.activePendingPlanApproval')
  onPlanChanged(_mon: IMonitor): void {
    const pending = this.store.stagePendingPlanApproval;
    if (pending === null) {
      this.activeRequestId = '';
      this.isEditingPlan = false;
      this.editedMarkdown = '';
      this.feedbackText = '';
      this.isResolving = false;
      return;
    }
    if (pending.request_id !== this.activeRequestId) {
      this.activeRequestId = pending.request_id;
      this.editedMarkdown = pending.plan_markdown;
      this.feedbackText = '';
      this.isEditingPlan = false;
      this.isResolving = false;
    }
  }

  aboutToAppear(): void {
    const pending = this.store.stagePendingPlanApproval;
    if (pending !== null) {
      this.activeRequestId = pending.request_id;
      this.editedMarkdown = pending.plan_markdown;
    }
  }

  private async submitResolution(
    pending: PendingPlanApprovalDto,
    action: string,
    targetMode: string
  ): Promise<void> {
    if (this.isResolving) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    this.isResolving = true;
    const editedArg: string | null =
      this.editedMarkdown.trim() !== pending.plan_markdown.trim()
        ? this.editedMarkdown
        : null;
    const feedbackArg: string | null =
      this.feedbackText.trim().length > 0 ? this.feedbackText.trim() : null;
    try {
      await this.store.resolvePlanApproval(
        pending.request_id,
        action,
        editedArg,
        feedbackArg,
        targetMode
      );
    } finally {
      this.isResolving = false;
    }
  }

  build() {
    if (this.store.stagePendingPlanApproval !== null) {
      this.renderCard(this.store.stagePendingPlanApproval)
    }
  }

  @Builder
  private renderCard(pending: PendingPlanApprovalDto) {
    Column({ space: OrbitTokens.SPACE_SM }) {
      // Header bar: title + Preview/Edit toggle
      Row({ space: OrbitTokens.SPACE_SM }) {
        SymbolGlyph($r('sys.symbol.list_checkmark'))
          .fontSize(15)
          .fontColor([this.store.palette.accentPrimary])

        Column({ space: 2 }) {
          Text(
            pending.plan_title !== undefined && pending.plan_title !== null && pending.plan_title.trim().length > 0
              ? pending.plan_title.trim()
              : (pending.title.trim().length > 0 ? pending.title.trim() : $r('app.string.plan_review_title'))
          )
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })

          if (
            pending.plan_file_path !== undefined &&
            pending.plan_file_path !== null &&
            pending.plan_file_path.trim().length > 0
          ) {
            Text(stripWorkspacePrefix(pending.plan_file_path, this.store.activeCwd))
              .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        Button(
          this.isEditingPlan ? $r('app.string.plan_preview_markdown') : $r('app.string.plan_edit_markdown'),
          { type: ButtonType.Capsule, stateEffect: true }
        )
          .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: 10, right: 10 })
          .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
          .onClick(() => {
            this.getUIContext().animateTo(OrbitTheme.MOTION_UI, () => {
              if (!this.isEditingPlan && this.editedMarkdown.length === 0) {
                this.editedMarkdown = pending.plan_markdown;
              }
              this.isEditingPlan = !this.isEditingPlan;
            });
          })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      if (pending.summary !== undefined && pending.summary !== null && pending.summary.trim().length > 0) {
        Text(pending.summary.trim())
          .attributeModifier(orbitTextMeta(this.store.palette.fgSecondary))
          .width('100%')
          .maxLines(3)
          .textOverflow({ overflow: TextOverflow.Ellipsis })
      }

      // Plan content viewport (Markdown Preview or Editable TextArea)
      if (this.isEditingPlan) {
        TextArea({ text: this.editedMarkdown })
          .fontSize(OrbitTokens.FONT_SUBHEAD)
          .fontFamily(OrbitTokens.FAMILY_MONO)
          .fontColor(this.store.palette.fgPrimary)
          .backgroundColor(this.store.palette.recessed)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
          .padding(OrbitTokens.SPACE_SM)
          .width('100%')
          .constraintSize({ minHeight: 140, maxHeight: 260 })
          .onChange((val: string) => {
            this.editedMarkdown = val;
          })
      } else {
        Scroll() {
          Column() {
            RichMarkdownView({
              blocks: pending.blocks ?? [],
              fallbackText: this.editedMarkdown.length > 0 ? this.editedMarkdown : pending.plan_markdown
            })
          }
          .width('100%')
          .alignItems(HorizontalAlign.Start)
        }
        .width('100%')
        .constraintSize({ maxHeight: 240 })
        .padding(OrbitTokens.SPACE_SM)
        .backgroundColor(this.store.palette.recessed)
        .borderRadius(OrbitTokens.RADIUS_MD)
        .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
        .scrollBar(BarState.Auto)
      }

      // Optional revision feedback
      TextInput({
        placeholder: $r('app.string.plan_feedback_placeholder'),
        text: this.feedbackText
      })
        .attributeModifier(
          orbitInputSubhead(
            this.store.palette.fgPrimary,
            this.store.palette.fgMuted,
            this.store.palette.recessed
          )
        )
        .height(OrbitTokens.HIT_COMPACT)
        .borderRadius(OrbitTokens.RADIUS_MD)
        .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
        .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
        .width('100%')
        .onChange((val: string) => {
          this.feedbackText = val;
        })

      // Action buttons: Approve & Build | Request Changes | Dismiss
      Row({ space: OrbitTokens.SPACE_SM }) {
        Button($r('app.string.plan_approve_build'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill)
          )
          .height(OrbitTokens.HIT_COMPACT)
          .layoutWeight(1)
          .enabled(!this.isResolving)
          .onClick(() => {
            this.submitResolution(pending, 'approve_and_build', 'accept_edits');
          })

        Button($r('app.string.plan_request_changes'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonMeta(this.store.palette.fgPrimary, this.store.palette.subtleSurface)
          )
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
          .border({ width: OrbitTokens.HAIRLINE, color: this.store.palette.hairline })
          .enabled(!this.isResolving)
          .onClick(() => {
            this.submitResolution(pending, 'request_changes', 'plan');
          })

        Button($r('app.string.plan_dismiss'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(
            orbitButtonMeta(this.store.palette.fgMuted, Color.Transparent)
          )
          .height(OrbitTokens.HIT_COMPACT)
          .padding({ left: OrbitTokens.SPACE_SM, right: OrbitTokens.SPACE_SM })
          .enabled(!this.isResolving)
          .onClick(() => {
            this.submitResolution(pending, 'dismiss', 'plan');
          })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .constraintSize({ maxWidth: OrbitTokens.STAGE_MAX_READING_WIDTH })
    .padding(OrbitTokens.SPACE_MD)
    .borderRadius(OrbitTokens.RADIUS_XL)
    .attributeModifier(OrbitTheme.dockedCard(this.store.palette))
    .margin({
      left: OrbitTokens.SPACE_MD,
      right: OrbitTokens.SPACE_MD,
      bottom: OrbitTokens.SPACE_XS
    })
  }
}
