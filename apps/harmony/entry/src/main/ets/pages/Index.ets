import { OrbitHarmonyStore } from '../model/OrbitHarmonyStore';
import { orbitRuntime } from '../model/OrbitRuntime';
import { AdaptiveWorkspace } from '../components/shell/AdaptiveWorkspace';

@Entry
@ComponentV2
struct Index {
  private readonly store: OrbitHarmonyStore = orbitRuntime();

  onBackPress(): boolean {
    if (this.store.shellStore.isModalSheetPresented) {
      this.store.shellStore.closeModal();
      return true;
    }
    if (this.store.showModelPickerSheet) {
      this.store.showModelPickerSheet = false;
      return true;
    }
    if (this.store.shellStore.breakpoint === 'sm' && this.store.sidePanelStore.isOpen) {
      this.store.sidePanelStore.dismissPanel();
      return true;
    }
    if (this.store.shellStore.breakpoint === 'sm' && this.store.shellStore.sidebarOpen) {
      this.store.shellStore.sidebarOpen = false;
      return true;
    }
    return false;
  }

  build() {
    Column() {
      AdaptiveWorkspace()
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
    .expandSafeArea(
      [SafeAreaType.SYSTEM, SafeAreaType.CUTOUT],
      [SafeAreaEdge.TOP, SafeAreaEdge.BOTTOM, SafeAreaEdge.START, SafeAreaEdge.END]
    )
  }
}
