import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitTokens,
  orbitButtonSubhead,
  orbitInputSubhead,
  orbitTextMono,
  orbitTextSubhead
} from '../../theme/OrbitTokens';
import { dismissHarmonyKeyboard } from '../common/OrbitCommonControls';

/**
 * Single-modal Workspace Rename Sheet (`route.kind === 'rename_workspace'`).
 * Captures immutable `(hostId, workspaceId)` and waits for authoritative daemon `Ack`
 * via `store.renameWorkspace(...)` before showing feedback or closing (R2-P1-2 / R2-P1-4).
 */
@ComponentV2
export struct RenameWorkspaceSheet {
  @Param hostId: string = '';
  @Param workspaceId: string = '';
  @Param initialTitle: string = '';
  @Event onClose: () => void = () => {};

  @Local private titleDraft: string = '';
  @Local private submitting: boolean = false;

  private lastTargetSig: string = '';
  private submitSeq: number = 0;
  private disposed: boolean = false;
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  private targetSig(): string {
    return `${this.hostId.trim()}::${this.workspaceId.trim()}`;
  }

  aboutToAppear(): void {
    this.disposed = false;
    this.lastTargetSig = this.targetSig();
    this.titleDraft = this.initialTitle.trim();
  }

  aboutToDisappear(): void {
    this.disposed = true;
    this.submitSeq += 1;
  }

  @Monitor('hostId', 'workspaceId', 'initialTitle')
  onTargetChanged(_mon: IMonitor): void {
    const nextSig = this.targetSig();
    if (nextSig !== this.lastTargetSig) {
      this.lastTargetSig = nextSig;
      this.submitSeq += 1;
      this.submitting = false;
      this.titleDraft = this.initialTitle.trim();
    }
  }

  private async submit(): Promise<void> {
    const cleanHost = this.hostId.trim();
    const cleanWs = this.workspaceId.trim();
    const cleanTitle = this.titleDraft.trim();
    if (this.submitting || cleanHost.length === 0 || cleanWs.length === 0 || cleanTitle.length === 0) {
      return;
    }
    dismissHarmonyKeyboard(this.getUIContext());
    const sig = this.targetSig();
    const originActiveHostId = this.store.selectedHostId;
    const originHostEpoch = this.store.hostSelectionEpoch;
    this.submitSeq += 1;
    const seq = this.submitSeq;
    this.submitting = true;
    const outcome = await this.store.renameWorkspace(cleanHost, cleanWs, cleanTitle);
    if (this.disposed || seq !== this.submitSeq) {
      return;
    }
    this.submitting = false;
    // R3-P1-4: Never surface toast, haptic, error, or close sheet if the user switched host
    // (`h1 -> h2` or `h1 -> h2 -> h1`) or rebound the sheet while the rename RPC was in flight.
    if (
      sig !== this.targetSig() ||
      this.store.selectedHostId !== originActiveHostId ||
      this.store.hostSelectionEpoch !== originHostEpoch
    ) {
      return;
    }
    if (outcome.ok) {
      this.store.showToastResource($r('app.string.workspace_renamed_toast'), false);
      this.store.triggerHapticPreset('success');
      this.onClose();
    } else {
      this.store.showToastResource(
        outcome.errorDetail.length > 0
          ? $r('app.string.workspace_rename_failed_detail', outcome.errorDetail)
          : $r('app.string.workspace_rename_failed'),
        true,
        outcome.errorDetail
      );
    }
  }

  build() {
    Column({ space: OrbitTokens.SPACE_MD }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text($r('app.string.workspace_rename_title'))
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
          Text(this.workspaceId)
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
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_MD
      })
      .alignItems(VerticalAlign.Center)

      Column({ space: OrbitTokens.SPACE_SM }) {
        TextInput({
          placeholder: $r('app.string.workspace_rename_placeholder'),
          text: this.titleDraft
        })
          .attributeModifier(
            orbitInputSubhead(
              this.store.palette.fgPrimary,
              this.store.palette.fgMuted,
              this.store.palette.recessed
            )
          )
          .height(42)
          .borderRadius(OrbitTokens.RADIUS_MD)
          .width('100%')
          .enabled(!this.submitting)
          .onFocus(() => {
            this.store.shellStore.setTextInputFocused(true);
          })
          .onBlur(() => {
            this.store.shellStore.setTextInputFocused(false);
          })
          .onChange((val: string) => {
            this.titleDraft = val;
          })
          .onSubmit(() => {
            void this.submit();
          })
      }
      .width('100%')
      .padding({ left: OrbitTokens.SPACE_LG, right: OrbitTokens.SPACE_LG })

      Blank()

      Row({ space: OrbitTokens.SPACE_SM }) {
        Button($r('app.string.action_cancel'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonSubhead(this.store.palette.fgSecondary, this.store.palette.subtleSurface))
          .height(42)
          .layoutWeight(1)
          .enabled(!this.submitting)
          .onClick(() => {
            this.onClose();
          })

        Button($r('app.string.workspace_rename_confirm'), { type: ButtonType.Capsule, stateEffect: true })
          .attributeModifier(orbitButtonSubhead(this.store.palette.onAccent, this.store.palette.accentPrimaryFill))
          .height(42)
          .layoutWeight(2)
          .enabled(!this.submitting && this.titleDraft.trim().length > 0)
          .onClick(() => {
            void this.submit();
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
