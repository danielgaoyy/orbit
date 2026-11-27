import { OrbitHarmonyStore } from '../../model/OrbitHarmonyStore';
import { orbitRuntime } from '../../model/OrbitRuntime';
import {
  OrbitSidePanelStore,
  OrbitSidePanelTab
} from '../../model/stores/OrbitSidePanelStore';
import {
  OrbitSerializedSheetHost,
  OrbitShellStore
} from '../../model/stores/OrbitShellStore';
import { OrbitTokens } from '../../theme/OrbitTokens';
import { OrbitSidePanelLaunchpad } from './OrbitSidePanelLaunchpad';
import { OrbitSidePanelTabBar } from './OrbitSidePanelTabBar';
import { OrbitSidePanelTabContent } from './OrbitSidePanelTabContent';

/**
 * Captures the immutable `presentationId` for each compact sheet mount so native `aboutToAppear`
 * and `aboutToDisappear` callbacks are strictly bound to their originating sheet instance (R3-P1-8).
 */
@ComponentV2
struct OrbitCompactSheetPanelContent {
  @Param presentationId: number = 0;
  @Param launchpadHostId: string = '';
  @Param launchpadAgentId: string | null = null;
  @Param launchpadCwd: string | null = null;
  @Event onNativeAppear: (id: number) => void = (_id: number) => {};
  @Event onNativeDisappear: (id: number) => void = (_id: number) => {};

  private capturedPresentationId: number = 0;
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    this.capturedPresentationId = this.presentationId;
    if (this.capturedPresentationId > 0) {
      this.onNativeAppear(this.capturedPresentationId);
    }
  }

  aboutToDisappear(): void {
    const id = this.capturedPresentationId > 0 ? this.capturedPresentationId : this.presentationId;
    if (id > 0) {
      this.onNativeDisappear(id);
    }
  }

  private get activeTab(): OrbitSidePanelTab | null {
    return this.store.sidePanelStore.activeSnapshot.getActiveTab();
  }

  build() {
    Column({ space: 0 }) {
      OrbitSidePanelTabBar({
        isCompactSheet: true,
        onClosePanel: () => {
          this.store.sidePanelStore.dismissPanel();
          this.store.triggerHaptic(6);
        }
      })

      if (this.activeTab !== null) {
        OrbitSidePanelTabContent({
          tab: this.activeTab,
          scopeKey: this.store.sidePanelStore.activeScopeKey
        })
      } else {
        OrbitSidePanelLaunchpad({
          boundHostId: this.launchpadHostId,
          boundAgentId: this.launchpadAgentId,
          boundCwd: this.launchpadCwd
        })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}

/**
 * Unified Right Side Panel Container:
 * - On compact sheet mode (`sm` or when safe insets force `sidePanelInline === false`):
 *   hosts the bottom Sheet with horizontal tab capsules + `+` Launchpad.
 *   Yields while a single-route modal is presented and gates `onDisappear` by immutable instance ID
 *   so stale or duplicate disappear callbacks from a yielded sheet never dismiss a newly opened panel.
 * - On `md` / `lg` inline mode: renders inline right pane with left hairline border and drag resize sash.
 */
@ComponentV2
export struct OrbitSidePanelContainer {
  @Param inlineWidthVp: number = 360;
  @Param sidePanelInline: boolean = false;
  @Param launchpadHostId: string = '';
  @Param launchpadAgentId: string | null = null;
  @Param launchpadCwd: string | null = null;

  @Local private smSheetOpen: boolean = false;
  @Local private smSheetPresentationId: number = 0;
  @Local private isDraggingWidth: boolean = false;
  @Local private sidePanelStore: OrbitSidePanelStore = orbitRuntime().sidePanelStore;
  @Local private shellStore: OrbitShellStore = orbitRuntime().shellStore;
  private dragStartWidthVp: number = 360;
  private smSheetHost: OrbitSerializedSheetHost = new OrbitSerializedSheetHost();

  private readonly store: OrbitHarmonyStore = orbitRuntime();

  aboutToAppear(): void {
    this.syncSmSheetBinding();
  }

  @Monitor(
    'sidePanelStore.stateRevision',
    'shellStore.breakpoint',
    'shellStore.isSheetModalPresented',
    'shellStore.isLightboxPresented',
    'shellStore.pendingModalRoute',
    'sidePanelInline'
  )
  onPanelVisibilityMonitor(_mon: IMonitor): void {
    this.syncSmSheetBinding();
  }

  private get useCompactSheetHost(): boolean {
    return this.store.shellStore.breakpoint === 'sm' || !this.sidePanelInline;
  }

  private syncSmSheetBinding(): void {
    const wantSmSheet =
      this.useCompactSheetHost &&
      this.store.sidePanelStore.isOpen &&
      !this.store.shellStore.isModalSheetPresented;
    if (wantSmSheet) {
      this.smSheetHost.requestOpen();
    } else {
      this.smSheetHost.requestClose();
    }
    this.smSheetOpen = this.smSheetHost.boundOpen;
    this.smSheetPresentationId = this.smSheetHost.activePresentationId;
  }

  private get activeTab(): OrbitSidePanelTab | null {
    return this.store.sidePanelStore.activeSnapshot.getActiveTab();
  }

  @Builder
  private renderPanelInner(isCompactSheet: boolean) {
    Column({ space: 0 }) {
      OrbitSidePanelTabBar({
        isCompactSheet: isCompactSheet,
        onClosePanel: () => {
          this.store.sidePanelStore.dismissPanel();
          this.store.triggerHaptic(6);
        }
      })

      if (this.activeTab !== null) {
        OrbitSidePanelTabContent({
          tab: this.activeTab,
          scopeKey: this.store.sidePanelStore.activeScopeKey
        })
      } else {
        OrbitSidePanelLaunchpad({
          boundHostId: this.launchpadHostId,
          boundAgentId: this.launchpadAgentId,
          boundCwd: this.launchpadCwd
        })
      }
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }

  private compactSheetPresentationIds(): number[] {
    return this.smSheetPresentationId > 0 ? [this.smSheetPresentationId] : [];
  }

  private handleCompactSheetDisappear(instanceId: number): void {
    const userDismissed = this.smSheetHost.onDisappear(instanceId);
    this.smSheetOpen = this.smSheetHost.boundOpen;
    this.smSheetPresentationId = this.smSheetHost.activePresentationId;
    if (
      userDismissed &&
      this.useCompactSheetHost &&
      !this.store.shellStore.isModalSheetPresented &&
      this.store.sidePanelStore.isOpen
    ) {
      this.store.sidePanelStore.dismissPanel();
    }
  }

  @Builder
  private renderCompactSheetBuilder() {
    ForEach(
      this.compactSheetPresentationIds(),
      (compactId: number) => {
        OrbitCompactSheetPanelContent({
          presentationId: compactId,
          launchpadHostId: this.launchpadHostId,
          launchpadAgentId: this.launchpadAgentId,
          launchpadCwd: this.launchpadCwd,
          onNativeAppear: (id: number) => {
            this.smSheetHost.onAppear(id);
          },
          onNativeDisappear: (id: number) => {
            this.handleCompactSheetDisappear(id);
          }
        })
      },
      (compactId: number): string => `compact_${compactId}`
    )
  }

  build() {
    if (this.useCompactSheetHost) {
      // Zero-footprint compact sheet host (avoids any layout impact on narrow phone Stage).
      // Exit confirmation is routed EXCLUSIVELY through `OrbitCompactSheetPanelContent`'s
      // immutable `presentationId` — NO secondary mutable `bindSheet.onDisappear` callback (R3-P1-8).
      Column()
        .width(0)
        .height(0)
        .hitTestBehavior(HitTestMode.None)
        .bindSheet($$this.smSheetOpen, this.renderCompactSheetBuilder(), {
          detents: [SheetSize.LARGE, SheetSize.MEDIUM],
          dragBar: true,
          showClose: false,
          backgroundColor: this.store.palette.canvas
        })
    } else if (this.store.sidePanelStore.isOpen && this.sidePanelInline) {
      Row({ space: 0 }) {
        // 6vp interactive resize sash with crisp monochrome hairline indicator
        Stack({ alignContent: Alignment.Center }) {
          Column()
            .width(this.isDraggingWidth ? 2 : OrbitTokens.HAIRLINE)
            .height('100%')
            .backgroundColor(
              this.isDraggingWidth
                ? this.store.palette.borderStrong
                : this.store.palette.hairline
            )
        }
        .width(6)
        .height('100%')
        .backgroundColor(Color.Transparent)
        .accessibilityText('Resize side panel')
        .gesture(
          PanGesture({ direction: PanDirection.Horizontal, distance: 2 })
            .onActionStart(() => {
              this.isDraggingWidth = true;
              this.dragStartWidthVp = this.inlineWidthVp;
            })
            .onActionUpdate((event: GestureEvent) => {
              // Dragging left (negative offsetX) expands the right panel.
              const targetW = this.dragStartWidthVp - event.offsetX;
              this.store.shellStore.setSidePanelWidthVp(targetW, false);
              this.store.sidePanelStore.setWidthVp(this.store.shellStore.sidePanelWidthVp);
            })
            .onActionEnd(() => {
              this.isDraggingWidth = false;
              this.store.shellStore.setSidePanelWidthVp(this.store.shellStore.sidePanelWidthVp, true);
              this.store.sidePanelStore.setWidthVp(this.store.shellStore.sidePanelWidthVp);
            })
            .onActionCancel(() => {
              this.isDraggingWidth = false;
            })
        )

        Column() {
          this.renderPanelInner(false)
        }
        .layoutWeight(1)
        .height('100%')
      }
      .width(this.inlineWidthVp)
      .height('100%')
      .backgroundColor(this.store.palette.canvas)
    }
  }
}
