import { AbilityConstant, Configuration, ConfigurationConstant, UIAbility, Want } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { connection } from '@kit.NetworkKit';
import { hilog } from '@kit.PerformanceAnalysisKit';
import { OrbitHarmonyStore } from '../model/OrbitHarmonyStore';

const DOMAIN: number = 0x0B17;

export default class EntryAbility extends UIAbility {
  private netConn: connection.NetConnection | null = null;
  private mainWin: window.Window | null = null;
  private lastBearerSig: string = '';
  private statusTopVp: number = 0;
  private navBottomVp: number = 0;
  private keyboardVp: number = 0;

  onCreate(want: Want, _launchParam: AbilityConstant.LaunchParam): void {
    const store = OrbitHarmonyStore.getInstance();
    try {
      const mode = this.context.config?.colorMode;
      if (mode !== undefined) {
        store.setSystemDark(mode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK);
      }
      this.context.getApplicationContext().setColorMode(
        ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET
      );
    } catch (_) {
      // Ignore color mode configuration failure on previewer
    }

    const dbPath = `${this.context.databaseDir}/orbit_harmony_v2.sqlite`;
    store.initialize(dbPath);
    hilog.info(DOMAIN, 'OrbitEntry', 'OrbitHarmonyCore initialized at %{public}s', dbPath);

    this.applyLaunchWant(want);
    this.registerNetworkObserver();
  }

  onNewWant(want: Want, _launchParam: AbilityConstant.LaunchParam): void {
    this.applyLaunchWant(want);
  }

  private applyLaunchWant(want: Want): void {
    const store = OrbitHarmonyStore.getInstance();
    if (want.uri !== undefined && want.uri.length > 0) {
      store.handleDeepLinkUri(want.uri);
    }
    const paramUri = want.parameters?.['orbit_pair_uri'];
    if (typeof paramUri === 'string' && paramUri.length > 0) {
      store.handleDeepLinkUri(paramUri);
    }
  }

  onConfigurationUpdate(newConfig: Configuration): void {
    if (newConfig.colorMode !== undefined) {
      OrbitHarmonyStore.getInstance().setSystemDark(
        newConfig.colorMode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK
      );
    }
  }

  onForeground(): void {
    OrbitHarmonyStore.getInstance().onAppForeground();
  }

  onBackground(): void {
    OrbitHarmonyStore.getInstance().onAppBackground();
  }

  onDestroy(): void {
    if (this.netConn !== null) {
      try {
        this.netConn.unregister(() => {});
      } catch (_) {
        // Ignore unregister error
      }
      this.netConn = null;
    }
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        hilog.error(DOMAIN, 'OrbitEntry', 'Failed to load content: %{public}s', JSON.stringify(err));
        return;
      }
      this.setupImmersiveWindow(windowStage);
    });
  }

  onWindowStageDestroy(): void {
    if (this.mainWin !== null) {
      try {
        this.mainWin.off('avoidAreaChange');
      } catch (_) {
        // Ignore listener unregister error
      }
      this.mainWin = null;
    }
  }

  private registerNetworkObserver(): void {
    try {
      const conn = connection.createNetConnection();
      conn.register(() => {});
      conn.on('netAvailable', (_handle: connection.NetHandle) => {
        OrbitHarmonyStore.getInstance().onNetworkChanged();
      });
      conn.on('netCapabilitiesChange', (info: connection.NetCapabilityInfo) => {
        const bearers = info.netCap?.bearerTypes ?? [];
        const sig = bearers.slice().sort().join(',');
        if (sig.length > 0 && sig !== this.lastBearerSig) {
          this.lastBearerSig = sig;
          OrbitHarmonyStore.getInstance().onNetworkChanged();
        }
      });
      this.netConn = conn;
    } catch (_) {
      // Ignore network listener failure when permission unavailable on previewer
    }
  }

  private setupImmersiveWindow(windowStage: window.WindowStage): void {
    try {
      const mainWin = windowStage.getMainWindowSync();
      this.mainWin = mainWin;
      mainWin.setWindowLayoutFullScreen(true).catch(() => {});
      const uiCtx = mainWin.getUIContext();

      const sysArea = mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM);
      this.statusTopVp = uiCtx.px2vp(sysArea.topRect.height);

      const navArea = mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR);
      this.navBottomVp = uiCtx.px2vp(navArea.bottomRect.height);

      const kbArea = mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_KEYBOARD);
      this.keyboardVp = kbArea.visible ? uiCtx.px2vp(kbArea.bottomRect.height) : 0;

      OrbitHarmonyStore.getInstance().updateSafeArea(
        this.statusTopVp,
        this.navBottomVp,
        this.keyboardVp
      );

      mainWin.on('avoidAreaChange', (opts: window.AvoidAreaOptions) => {
        try {
          if (opts.type === window.AvoidAreaType.TYPE_SYSTEM) {
            this.statusTopVp = uiCtx.px2vp(opts.area.topRect.height);
          } else if (opts.type === window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR) {
            this.navBottomVp = uiCtx.px2vp(opts.area.bottomRect.height);
          } else if (opts.type === window.AvoidAreaType.TYPE_KEYBOARD) {
            this.keyboardVp = opts.area.visible ? uiCtx.px2vp(opts.area.bottomRect.height) : 0;
          }
          OrbitHarmonyStore.getInstance().updateSafeArea(
            this.statusTopVp,
            this.navBottomVp,
            this.keyboardVp
          );
        } catch (_) {
          // Ignore conversion failure
        }
      });
    } catch (_) {
      // Safe fallback on DevEco previewer
    }
  }
}
