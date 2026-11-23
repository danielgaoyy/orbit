import { basenameOfPath } from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitInputSubhead,
  orbitTextBody,
  orbitTextMeta,
  orbitTextSubhead,
  orbitTextTitle
} from '../../theme/OrbitTokens';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

@ComponentV2
export struct SideQuestionSheet {
  @Local questionDraft: string = '';

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  private submitQuestion(): void {
    const clean = this.questionDraft.trim();
    if (clean.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    this.questionDraft = '';
    this.store.askSideQuestion(clean);
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text('Side Question')
            .attributeModifier(orbitTextTitle(this.store.palette.fgPrimary, FontWeight.Bold))
          Text(`/btw · Ephemeral Q&A in ${basenameOfPath(this.store.activeCwd)}`)
            .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

        if (this.store.sideQuestionAnswer.length > 0) {
          Button('Copy', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
            )
            .height(32)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.store.copyToClipboard(this.store.sideQuestionAnswer, 'Copied answer');
            })
        }

        if (this.store.sideQuestionPrompt.length > 0 || this.store.sideQuestionAnswer.length > 0) {
          Button('Clear', { type: ButtonType.Capsule, stateEffect: true })
            .attributeModifier(
              orbitButtonMeta(this.store.palette.fgSecondary, this.store.palette.subtleSurface)
            )
            .height(32)
            .padding({ left: 10, right: 10 })
            .onClick(() => {
              this.store.clearSideQuestion();
            })
        }
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)

      Scroll() {
        Column({ space: OrbitTokens.SPACE_MD }) {
          if (this.store.sideQuestionPrompt.length === 0 && !this.store.sideQuestionStreaming) {
            Column({ space: OrbitTokens.SPACE_XS }) {
              Text('Ask without disturbing the thread')
                .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
              Text('Uses a lightweight summary of the active conversation without modifying agent history or interrupting a running turn.')
                .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
                .textAlign(TextAlign.Center)
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_XL)
            .backgroundColor(this.store.palette.listCard)
            .borderRadius(OrbitTokens.RADIUS_LG)
            .alignItems(HorizontalAlign.Center)
          } else {
            if (this.store.sideQuestionPrompt.length > 0) {
              Column({ space: 4 }) {
                Text('Question')
                  .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
                Text(this.store.sideQuestionPrompt)
                  .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Medium))
                  .width('100%')
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_MD)
              .backgroundColor(this.store.palette.subtleSurface)
              .borderRadius(OrbitTokens.RADIUS_MD)
              .alignItems(HorizontalAlign.Start)
            }

            if (this.store.sideQuestionError.length > 0) {
              Text(this.store.sideQuestionError)
                .attributeModifier(orbitTextSubhead(this.store.palette.statusDanger))
                .width('100%')
                .padding(OrbitTokens.SPACE_MD)
                .backgroundColor(this.store.palette.statusDangerSoft)
                .borderRadius(OrbitTokens.RADIUS_MD)
            }

            if (this.store.sideQuestionAnswer.length > 0 || this.store.sideQuestionStreaming) {
              Column({ space: OrbitTokens.SPACE_SM }) {
                Row({ space: OrbitTokens.SPACE_SM }) {
                  Text('Answer')
                    .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
                  if (this.store.sideQuestionStreaming) {
                    LoadingProgress()
                      .width(14)
                      .height(14)
                      .color(this.store.palette.accentPrimary)
                  }
                }
                .width('100%')
                .alignItems(VerticalAlign.Center)

                Text(
                  this.store.sideQuestionAnswer.length > 0
                    ? this.store.sideQuestionAnswer
                    : 'Thinking…'
                )
                  .attributeModifier(orbitTextBody(this.store.palette.fgPrimary))
                  .copyOption(CopyOptions.LocalDevice)
                  .width('100%')
              }
              .width('100%')
              .padding(OrbitTokens.SPACE_LG)
              .backgroundColor(this.store.palette.listCard)
              .borderRadius(OrbitTokens.RADIUS_LG)
              .alignItems(HorizontalAlign.Start)
            }
          }
        }
        .width('100%')
      }
      .width('100%')
      .layoutWeight(1)
      .scrollBar(BarState.Auto)
      .edgeEffect(EdgeEffect.Spring)

      Row({ space: OrbitTokens.SPACE_SM }) {
        TextInput({ placeholder: 'Ask a quick side question…', text: this.questionDraft })
          .attributeModifier(
            orbitInputSubhead(
              this.store.palette.fgPrimary,
              this.store.palette.fgMuted,
              this.store.palette.listCard
            )
          )
          .height(OrbitTokens.HIT_MIN)
          .borderRadius(OrbitTokens.RADIUS_XL)
          .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_MD })
          .layoutWeight(1)
          .onChange((val: string) => {
            this.questionDraft = val;
          })
          .onSubmit(() => {
            this.submitQuestion();
          })

        Button({ type: ButtonType.Circle, stateEffect: true }) {
          SymbolGlyph($r('sys.symbol.arrow_up'))
            .fontSize(16)
            .fontColor([
              this.questionDraft.trim().length > 0
                ? this.store.palette.onAccent
                : this.store.palette.fgMuted
            ])
        }
        .width(OrbitTokens.HIT_MIN)
        .height(OrbitTokens.HIT_MIN)
        .padding(0)
        .backgroundColor(
          this.questionDraft.trim().length > 0
            ? this.store.palette.accentPrimaryFill
            : this.store.palette.subtleSurface
        )
        .accessibilityText('Ask side question')
        .onClick(() => {
          this.submitQuestion();
        })
      }
      .width('100%')
      .alignItems(VerticalAlign.Center)
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
