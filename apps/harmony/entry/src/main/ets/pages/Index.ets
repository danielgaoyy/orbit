import { OrbitHarmonyStore } from '../model/OrbitHarmonyStore';
import { AdaptiveWorkspace } from '../components/shell/AdaptiveWorkspace';
import { OrbitTokens } from '../theme/OrbitTokens';

@Entry
@ComponentV2
struct Index {
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  onBackPress(): boolean {
    if (this.store.windowWidthVp < OrbitTokens.BP_FOLDABLE_MIN && this.store.sidebarOpen) {
      this.store.sidebarOpen = false;
      return true;
    }
    if (this.store.showModelPickerSheet) {
      this.store.showModelPickerSheet = false;
      return true;
    }
    if (this.store.showSideQuestionSheet) {
      this.store.showSideQuestionSheet = false;
      return true;
    }
    if (this.store.showWorkspaceBoardSheet) {
      this.store.showWorkspaceBoardSheet = false;
      return true;
    }
    if (this.store.showAutomationsSheet) {
      this.store.showAutomationsSheet = false;
      return true;
    }
    if (this.store.showArchivedSessionsSheet) {
      this.store.showArchivedSessionsSheet = false;
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
  }
}
