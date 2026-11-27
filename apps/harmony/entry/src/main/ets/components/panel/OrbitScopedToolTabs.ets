import { basenameOfPath } from '../../model/OrbitTypes';
import {
  OrbitHarmonyStore,
  OrbitScopedSideQuestionState
} from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitSidePanelTab } from '../../model/stores/OrbitSidePanelStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitInputSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

export {
  OrbitScopedFileViewerTabView,
  OrbitScopedFilesTabView,
  OrbitScopedGitTabView
} from './OrbitScopedGitAndFilesTabs';
export {
  OrbitScopedBoardTabView,
  OrbitScopedTerminalTabView
} from './OrbitScopedTerminalAndBoardTabs';
export { OrbitScopedAutomationsTabView } from './OrbitScopedAutomationsTab';
export { WorktreeHandoffSheet } from '../sheets/WorktreeHandoffSheet';
export { WorkspaceScriptsSheet } from '../sheets/WorkspaceScriptsSheet';
export { CreateWorkspaceV2Sheet } from '../sheets/CreateWorkspaceV2Sheet';
export { RenameWorkspaceSheet } from '../sheets/RenameWorkspaceSheet';

/**
 * Unified Scoped Side Question (`/btw`) tab (`kind === 'side_question'`).
 * - P1-1 + P1-2: Reads and writes the per-scope side-question state hosted in `OrbitHarmonyStore`
 *   (shared cleanly with Composer `/btw` submissions), isolating drafts/prompts/answers/streaming
 *   across sessions and preserving live answers across tab/scope switches without leaking or getting stuck.
 */
@ComponentV2
export struct OrbitScopedSideQuestionTabView {
  @Param @Require tab: OrbitSidePanelTab;
  @Param scopeKey: string = '';

  @Local private store: OrbitHarmonyStore = orbitRuntime();

  @Computed
  get effectiveScopeKey(): string {
    if (this.scopeKey.trim().length > 0) {
      return this.scopeKey.trim();
    }
    return this.store.sidePanelStore.scopeKeyFor(this.tab.hostId, this.tab.agentId);
  }

  @Computed
  get sideState(): OrbitScopedSideQuestionState {
    const _rev = this.store.sideQuestionRevision;
    if (_rev < 0) {
      // Keep V2 reactive dependency on sideQuestionRevision
    }
    return this.store.getScopedSideQuestionState(
      this.effectiveScopeKey,
      this.tab.hostId,
      this.tab.agentId,
      this.tab.cwd ?? ''
    );
  }

  private submitQuestion(): void {
    const clean = this.sideState.draft.trim();
    if (clean.length === 0 || this.tab.hostId.trim().length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    this.store.submitScopedSideQuestion(
      this.effectiveScopeKey,
      this.tab.hostId,
      this.tab.agentId,
      this.tab.cwd ?? '',
      clean,
      'user'
    );
  }

  build() {
    Column({ space: OrbitTokens.SPACE_SM }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text($r('app.string.side_question_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
          Text(
            `${this.tab.cwd !== null && this.tab.cwd.length > 0 ? basenameOfPath(this.tab.cwd) : this.tab.hostId}` +
            `${this.sideState.model !== null && this.sideState.model.length > 0 ? ` · ${this.sideState.model}` : ` · ${this.sideState.provider}`}`
          )
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.sideState.streaming) {
          LoadingProgress()
            .width(16)
            .height(16)
            .color(this.store.palette.accentPrimary)
        }

        if (
          this.sideState.prompt.length > 0 ||
          this.sideState.answer.length > 0 ||
          this.sideState.error.length > 0
        ) {
          Button($r('app.string.side_question_clear'), { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
            .height(28)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.store.clearScopedSideQuestion(this.effectiveScopeKey);
            })
        }
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD, top: OrbitTokens.SPACE_SM })
      .alignItems(VerticalAlign.Center)

      Scroll() {
        Column({ space: OrbitTokens.SPACE_SM }) {
          if (
            this.sideState.prompt.length === 0 &&
            this.sideState.answer.length === 0 &&
            this.sideState.error.length === 0 &&
            !this.sideState.streaming
          ) {
            Text($r('app.string.side_question_empty_hint'))
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .padding(OrbitTokens.SPACE_MD)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .width('100%')
          } else {
            if (this.sideState.prompt.length > 0) {
              Text(this.sideState.prompt)
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.subtleSurface)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
            if (this.sideState.error.length > 0) {
              Text(this.sideState.error)
                .attributeModifier(orbitTextMeta(this.store.palette.statusDanger))
                .width('100%')
                .padding(OrbitTokens.SPACE_SM)
                .backgroundColor(this.store.palette.statusDangerSoft)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
            if (this.sideState.answer.length > 0 || this.sideState.streaming) {
              Text(this.sideState.answer.length > 0 ? this.sideState.answer : 'Thinking…')
                .attributeModifier(orbitTextBody(this.store.palette.fgPrimary))
                .copyOption(CopyOptions.LocalDevice)
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.listCard)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }
          }
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Auto)

      Row({ space: OrbitTokens.SPACE_XS }) {
        TextInput({ placeholder: $r('app.string.side_question_input_placeholder'), text: this.sideState.draft })
          .attributeModifier(
            orbitInputSubhead(
              this.store.palette.fgPrimary,
              this.store.palette.fgMuted,
              this.store.palette.recessed
            )
          )
          .height(40)
          .borderRadius(OrbitTokens.RADIUS_XL)
          .padding({ left: OrbitTokens.SPACE_MD, right: OrbitTokens.SPACE_MD })
          .layoutWeight(1)
          .onFocus(() => {
            this.store.shellStore.setTextInputFocused(true);
          })
          .onBlur(() => {
            this.store.shellStore.setTextInputFocused(false);
          })
          .onChange((val: string) => {
            this.store.setScopedSideQuestionDraft(
              this.effectiveScopeKey,
              this.tab.hostId,
              this.tab.agentId,
              this.tab.cwd ?? '',
              val
            );
          })
          .onSubmit(() => {
            this.submitQuestion();
          })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_up'))
            .fontSize(15)
            .fontColor([
              this.sideState.draft.trim().length > 0
                ? this.store.palette.onAccent
                : this.store.palette.fgMuted
            ])
        }
        .width(40)
        .height(40)
        .padding(0)
        .backgroundColor(
          this.sideState.draft.trim().length > 0
            ? this.store.palette.accentPrimaryFill
            : this.store.palette.subtleSurface
        )
        .onClick(() => {
          this.submitQuestion();
        })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_MD,
        right: OrbitTokens.SPACE_MD,
        bottom: Math.max(OrbitTokens.SPACE_MD, this.store.safeBottomVp)
      })
      .alignItems(VerticalAlign.Center)
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
