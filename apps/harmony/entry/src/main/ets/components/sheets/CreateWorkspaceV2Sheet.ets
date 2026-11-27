import {
  CreateWorkspaceV2RequestDto,
  basenameOfPath
} from '../../model/OrbitTypes';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitTokens,
  orbitButtonMeta,
  orbitButtonSubhead,
  orbitInputMono,
  orbitInputSubhead,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { OrbitTheme } from '../../theme/OrbitTheme';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

/**
 * Create Workspace v2 Sheet (PR-5).
 * Supports both Local and Git Worktree workspaces (branch_off / checkout_branch / checkout_pr).
 */
@ComponentV2
export struct CreateWorkspaceV2Sheet {
  @Param hostId: string = '';
  @Param sourceCwd: string = '';
  @Param projectTitle: string = '';
  @Event onClose: () => void = () => {};

  @Local private titleInput: string = '';
  @Local private isolation: string = 'worktree';
  @Local private worktreeMode: string = 'branch_off';
  @Local private branchInput: string = '';
  @Local private baseRefInput: string = '';
  @Local private prNumberInput: string = '';

  private readonly store: OrbitHarmonyStore = orbitRuntime();

  private submitCreate(): void {
    const cleanHost = this.hostId.trim();
    const cleanCwd = this.sourceCwd;
    if (cleanHost.length === 0 || cleanCwd.trim().length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const parsedPr = this.prNumberInput.trim().length > 0 ? Number.parseInt(this.prNumberInput.trim(), 10) : NaN;
    const req: CreateWorkspaceV2RequestDto = {
      host_id: cleanHost,
      source_cwd: cleanCwd,
      title: this.titleInput.trim().length > 0 ? this.titleInput.trim() : null,
      isolation: this.isolation,
      worktree_mode: this.isolation === 'worktree' ? this.worktreeMode : 'branch_off',
      worktree_branch: this.isolation === 'worktree' && this.branchInput.trim().length > 0
        ? this.branchInput.trim()
        : null,
      base_ref: this.isolation === 'worktree' && this.baseRefInput.trim().length > 0
        ? this.baseRefInput.trim()
        : null,
      pr_number: this.isolation === 'worktree' && this.worktreeMode === 'checkout_pr' && Number.isFinite(parsedPr) && parsedPr > 0
        ? parsedPr
        : null
    };
    const ok = this.store.createWorkspaceV2(req);
    if (ok) {
      this.onClose();
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text($r('app.string.workspace_new_v2'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
          Text(this.projectTitle.length > 0 ? this.projectTitle : basenameOfPath(this.sourceCwd))
            .attributeModifier(orbitTextMono(this.store.palette.fgMuted))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
        }
        .alignItems(HorizontalAlign.Start)
        .layoutWeight(1)

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
          TextInput({
            placeholder: $r('app.string.workspace_title_placeholder'),
            text: this.titleInput
          })
            .attributeModifier(
              orbitInputSubhead(
                this.store.palette.fgPrimary,
                this.store.palette.fgMuted,
                this.store.palette.recessed
              )
            )
            .height(40)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .width('100%')
            .onChange((val: string) => {
              this.titleInput = val;
            })

          Row({ space: OrbitTokens.SPACE_XS }) {
            Button($r('app.string.workspace_isolation_worktree'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(
                  this.isolation === 'worktree' ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                  this.isolation === 'worktree'
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.subtleSurface
                )
              )
              .height(34)
              .layoutWeight(1)
              .onClick(() => {
                this.isolation = 'worktree';
              })

            Button($r('app.string.workspace_isolation_local'), { type: ButtonType.Capsule, stateEffect: true })
              .attributeModifier(
                orbitButtonMeta(
                  this.isolation === 'local' ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                  this.isolation === 'local'
                    ? this.store.palette.accentPrimaryFill
                    : this.store.palette.subtleSurface
                )
              )
              .height(34)
              .layoutWeight(1)
              .onClick(() => {
                this.isolation = 'local';
              })
          }
          .width('100%')

          if (this.isolation === 'worktree') {
            Column({ space: OrbitTokens.SPACE_SM }) {
              Row({ space: OrbitTokens.SPACE_XS }) {
                Button($r('app.string.workspace_mode_branch_off'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(
                      this.worktreeMode === 'branch_off' ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                      this.worktreeMode === 'branch_off'
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.subtleSurface
                    )
                  )
                  .height(30)
                  .layoutWeight(1)
                  .onClick(() => {
                    this.worktreeMode = 'branch_off';
                  })

                Button($r('app.string.workspace_mode_checkout_branch'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(
                      this.worktreeMode === 'checkout_branch' ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                      this.worktreeMode === 'checkout_branch'
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.subtleSurface
                    )
                  )
                  .height(30)
                  .layoutWeight(1)
                  .onClick(() => {
                    this.worktreeMode = 'checkout_branch';
                  })

                Button($r('app.string.workspace_mode_checkout_pr'), { type: ButtonType.Capsule, stateEffect: true })
                  .attributeModifier(
                    orbitButtonMeta(
                      this.worktreeMode === 'checkout_pr' ? this.store.palette.onAccent : this.store.palette.fgPrimary,
                      this.worktreeMode === 'checkout_pr'
                        ? this.store.palette.accentPrimaryFill
                        : this.store.palette.subtleSurface
                    )
                  )
                  .height(30)
                  .layoutWeight(1)
                  .onClick(() => {
                    this.worktreeMode = 'checkout_pr';
                  })
              }
              .width('100%')

              if (this.worktreeMode === 'checkout_pr') {
                TextInput({
                  placeholder: $r('app.string.workspace_pr_number_placeholder'),
                  text: this.prNumberInput
                })
                  .attributeModifier(
                    orbitInputMono(
                      this.store.palette.fgPrimary,
                      this.store.palette.fgMuted,
                      this.store.palette.recessed
                    )
                  )
                  .type(InputType.Number)
                  .height(38)
                  .borderRadius(OrbitTokens.RADIUS_MD)
                  .width('100%')
                  .onChange((val: string) => {
                    this.prNumberInput = val;
                  })
              } else {
                TextInput({
                  placeholder: $r('app.string.workspace_branch_placeholder'),
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
                  .onChange((val: string) => {
                    this.branchInput = val;
                  })

                TextInput({
                  placeholder: $r('app.string.workspace_base_ref_placeholder'),
                  text: this.baseRefInput
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
                  .onChange((val: string) => {
                    this.baseRefInput = val;
                  })
              }
            }
            .width('100%')
            .padding(OrbitTokens.SPACE_MD)
            .borderRadius(OrbitTokens.RADIUS_MD)
            .attributeModifier(OrbitTheme.raisedCard(this.store.palette))
          }
        }
        .width('100%')
        .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })
      }
      .width('100%')
      .layoutWeight(1)

      Row({ space: OrbitTokens.SPACE_SM }) {
        Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonSubhead(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
          .height(42)
          .layoutWeight(1)
          .onClick(() => {
            this.onClose();
          })

        Button($r('app.string.action_create'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
          .height(42)
          .layoutWeight(2)
          .onClick(() => {
            this.submitCreate();
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
