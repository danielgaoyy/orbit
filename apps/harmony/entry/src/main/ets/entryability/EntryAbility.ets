import { AbilityConstant, ConfigurationConstant, UIAbility, Want } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { OrbitHarmonyStore } from '../model/OrbitHarmonyStore';

const DOMAIN = 0x0B17;

export default class EntryAbility extends UIAbility {
  onCreate(_want: Want, _launchParam: AbilityConstant.LaunchParam): void {
    this.context.getApplicationContext().setColorMode(ConfigurationConstant.ColorMode.COLOR_MODE_DARK);
    const dbPath = `${this.context.databaseDir}/orbit_harmony_v2.sqlite`;
    OrbitHarmonyStore.getInstance().initialize(dbPath);
    hilog.info(DOMAIN, 'OrbitEntry', 'OrbitHarmonyCore initialized at %{public}s', dbPath);
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        hilog.error(DOMAIN, 'OrbitEntry', 'Failed to load content: %{public}s', JSON.stringify(err));
        return;
      }
      try {
        const mainWin = windowStage.getMainWindowSync();
        mainWin.setWindowBackgroundColor('#090D16');
      } catch (_) {
        // Ignore window chrome styling failure on previewer
      }
    });
  }
}
