import { orbitRuntime } from '../../model/OrbitRuntime';
import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { OrbitShellStore } from '../../model/stores/OrbitShellStore';
import {
  OrbitSubagentAvatarAsset,
  resolveOrbitSubagentAvatar
} from '../../model/OrbitSubagentAvatarModel';

export {
  ORBIT_SUBAGENT_AVATAR_IDS,
  ORBIT_SUBAGENT_AVATAR_ROSTER,
  OrbitAvatarSeedKeyed,
  OrbitSubagentAvatarAsset,
  assignSubagentCharacters,
  getSubagentCharacterById,
  hashSubagentAvatarSeed,
  resolveOrbitSubagentAvatar,
  resolveSubagentCharacter
} from '../../model/OrbitSubagentAvatarModel';

const BREATH_INTERVAL_MS: number = 1200;

@ComponentV2
export struct OrbitSubagentAvatar {
  @Param characterId: string = '';
  @Param seedKey: string = 'subagent';
  @Param status: string = 'completed';
  @Param avatarSize: number = 24;
  @Param showStatusBadge: boolean = true;

  @Local private breathOn: boolean = false;
  @Local private store: OrbitHarmonyStore = orbitRuntime();
  @Local private shellStore: OrbitShellStore = orbitRuntime().shellStore;
  private breathTimerId: number = -1;

  aboutToAppear(): void {
    this.syncBreathTimer();
  }

  aboutToDisappear(): void {
    this.stopBreathTimer();
  }

  @Monitor('status', 'shellStore.reducedMotion')
  onMotionStateChanged(_mon: IMonitor): void {
    this.syncBreathTimer();
  }

  private isActiveStatus(): boolean {
    return this.status === 'running' || this.status === 'awaiting_permission';
  }

  private shouldAnimate(): boolean {
    return this.isActiveStatus() && !this.shellStore.reducedMotion;
  }

  private syncBreathTimer(): void {
    if (!this.shouldAnimate()) {
      this.stopBreathTimer();
      this.breathOn = false;
      return;
    }
    if (this.breathTimerId !== -1) {
      return;
    }
    this.breathOn = true;
    this.breathTimerId = setInterval((): void => {
      if (!this.shouldAnimate()) {
        this.stopBreathTimer();
        this.breathOn = false;
        return;
      }
      this.breathOn = !this.breathOn;
    }, BREATH_INTERVAL_MS);
  }

  private stopBreathTimer(): void {
    if (this.breathTimerId !== -1) {
      clearInterval(this.breathTimerId);
      this.breathTimerId = -1;
    }
  }

  private clampedSize(): number {
    if (this.avatarSize <= 18) {
      return 18;
    }
    if (this.avatarSize <= 20) {
      return 20;
    }
    if (this.avatarSize <= 24) {
      return 24;
    }
    return 28;
  }

  private showTerminalBadge(): boolean {
    return (
      this.showStatusBadge &&
      (this.status === 'completed' || this.status === 'failed' || this.status === 'canceled')
    );
  }

  private terminalBadgeColor(): ResourceColor {
    if (this.status === 'completed') {
      return this.store.palette.statusDirect;
    }
    if (this.status === 'failed') {
      return this.store.palette.statusDanger;
    }
    return this.store.palette.fgMuted;
  }

  private resolvedAsset(): OrbitSubagentAvatarAsset {
    return resolveOrbitSubagentAvatar(this.characterId, this.seedKey);
  }

  build() {
    Stack({ alignContent: Alignment.BottomEnd }) {
      Stack({ alignContent: Alignment.Center }) {
        Image(this.resolvedAsset().media)
          .width(this.clampedSize())
          .height(this.clampedSize())
          .objectFit(ImageFit.Contain)
          .draggable(false)
      }
      .width(this.clampedSize())
      .height(this.clampedSize())
      .borderRadius(this.clampedSize() / 2)
      .backgroundColor(this.resolvedAsset().bgColor)
      .border(
        this.status === 'awaiting_permission'
          ? { width: 1.5, color: this.store.palette.statusWarning }
          : { width: 0.5, color: this.store.palette.hairline }
      )
      .clip(true)
      .opacity(this.shouldAnimate() && this.breathOn ? 0.78 : 1.0)
      .scale(
        this.shouldAnimate() && this.breathOn
          ? { x: 1.04, y: 1.04 }
          : { x: 1.0, y: 1.0 }
      )
      .animation(
        this.shouldAnimate()
          ? { duration: 600, curve: Curve.EaseInOut }
          : { duration: 0 }
      )

      if (this.showTerminalBadge()) {
        Row() {
          SymbolGlyph(
            this.status === 'completed'
              ? $r('sys.symbol.checkmark')
              : $r('sys.symbol.xmark')
          )
            .fontSize(6)
            .fontColor([$r('sys.color.font_on_primary')])
        }
        .width(8)
        .height(8)
        .borderRadius(4)
        .justifyContent(FlexAlign.Center)
        .alignItems(VerticalAlign.Center)
        .backgroundColor(this.terminalBadgeColor())
        .border({ width: 1, color: this.store.palette.canvas })
        .offset({ x: 1, y: 1 })
      }
    }
    .width(this.clampedSize())
    .height(this.clampedSize())
    .flexShrink(0)
    .accessibilityText(this.resolvedAsset().nameResource)
  }
}
