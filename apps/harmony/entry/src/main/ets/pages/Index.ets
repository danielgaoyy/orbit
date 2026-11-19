import { OrbitHarmonyStore } from '../model/OrbitHarmonyStore';
import { AdaptiveWorkspace } from '../components/shell/AdaptiveWorkspace';

@Entry
@ComponentV2
struct Index {
  private store: OrbitHarmonyStore = OrbitHarmonyStore.getInstance();

  build() {
    Column() {
      AdaptiveWorkspace()
    }
    .width('100%')
    .height('100%')
    .backgroundColor(this.store.palette.canvas)
  }
}
