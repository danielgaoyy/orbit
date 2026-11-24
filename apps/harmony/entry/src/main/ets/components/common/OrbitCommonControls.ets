import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { HostCardDto } from '../../model/OrbitTypes';
import { orbitLogWarn } from '../../model/OrbitLogger';
import { OrbitTokens, orbitTextMeta, orbitTextSubhead } from '../../theme/OrbitTokens';

export function dismissHarmonyKeyboard(uiContext: UIContext): void {
  try {
    uiContext.getFocusController().clearFocus();
  } catch (err) {
    orbitLogWarn('OrbitUI', 'Failed to clear focus', err as Object);
  }
}

export function resolveHostStatusColor(host: HostCardDto | null, store: OrbitHarmonyStore): ResourceColor {
  if (host === null) {
    return store.palette.fgMuted;
  }
  if (host.connection_state === 'online_direct') {
    return store.palette.statusDirect;
  }
  if (host.connection_state === 'online_relay') {
    return store.palette.statusRelay;
  }
  if (
    host.connection_state === 'connecting_direct' ||
    host.connection_state === 'connecting_relay'
  ) {
    return store.palette.statusWarning;
  }
  return store.palette.statusDanger;
}

export function resolveSelectedHost(store: OrbitHarmonyStore): HostCardDto | null {
  if (store.currentHostDetail !== null) {
    return store.currentHostDetail.host;
  }
  for (let i = 0; i < store.hosts.length; i++) {
    if (store.hosts[i].host_id === store.selectedHostId) {
      return store.hosts[i];
    }
  }
  return null;
}

@ComponentV2
export struct OrbitSectionHeader {
  @Param @Require title: ResourceStr;
  @Param isFirst: boolean = false;

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  build() {
    Row() {
      Text(this.title)
        .attributeModifier(orbitTextMeta(this.store.palette.fgMuted, FontWeight.Medium))
        .maxLines(1)
    }
    .width('100%')
    .padding({
      left: OrbitTokens.SPACE_XS,
      right: OrbitTokens.SPACE_XS,
      top: this.isFirst ? 6 : 20,
      bottom: 6
    })
  }
}

@ComponentV2
export struct OrbitSheetHeader {
  @Param @Require title: ResourceStr;
  @Param subtitle: string = '';
  @Event onClose: () => void = () => {};

  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  build() {
    Column({ space: 0 }) {
      Row({ space: OrbitTokens.SPACE_SM }) {
        Column({ space: 2 }) {
          Text(this.title)
            .attributeModifier(orbitTextSubhead(this.store.palette.fgPrimary, FontWeight.Bold))
            .maxLines(1)
            .textOverflow({ overflow: TextOverflow.Ellipsis })
          if (this.subtitle.length > 0) {
            Text(this.subtitle)
              .attributeModifier(orbitTextMeta(this.store.palette.fgMuted))
              .maxLines(1)
              .textOverflow({ overflow: TextOverflow.Ellipsis })
          }
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
        .accessibilityText('Close')
        .onClick(() => {
          this.onClose();
        })
      }
      .width('100%')
      .padding({
        left: OrbitTokens.SPACE_LG,
        right: OrbitTokens.SPACE_LG,
        top: OrbitTokens.SPACE_MD,
        bottom: OrbitTokens.SPACE_SM
      })
      .alignItems(VerticalAlign.Center)

      Divider()
        .strokeWidth(OrbitTokens.HAIRLINE)
        .color(this.store.palette.hairline)
        .width('100%')
    }
    .width('100%')
  }
}

@ComponentV2
export struct OrbitCopyIconButton {
  @Param @Require content: string;
  @Param toastLabel: string = 'Copied to clipboard';
  @Param accessibilityLabel: string = 'Copy';
  @Param buttonSize: number = OrbitTokens.HIT_COMPACT;

  @Local copied: boolean = false;
  private timerId: number = -1;
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  aboutToDisappear(): void {
    if (this.timerId !== -1) {
      clearTimeout(this.timerId);
      this.timerId = -1;
    }
  }

  build() {
    Button({ type: ButtonType.Circle, stateEffect: true }) {
      SymbolGlyph(this.copied ? $r('sys.symbol.checkmark') : $r('sys.symbol.square_and_square'))
        .fontSize(16)
        .renderingStrategy(SymbolRenderingStrategy.SINGLE)
        .fontColor([
          this.copied ? this.store.palette.statusDirect : this.store.palette.iconSecondary
        ])
    }
    .width(this.buttonSize)
    .height(this.buttonSize)
    .padding(0)
    .backgroundColor(Color.Transparent)
    .accessibilityText(this.accessibilityLabel)
    .onClick(() => {
      if (this.content.length === 0) {
        return;
      }
      this.store.copyToClipboard(this.content, this.toastLabel);
      this.copied = true;
      if (this.timerId !== -1) {
        clearTimeout(this.timerId);
      }
      this.timerId = setTimeout(() => {
        this.copied = false;
        this.timerId = -1;
      }, 1600);
    })
  }
}
