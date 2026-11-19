import { AbilityConstant, Configuration, ConfigurationConstant, UIAbility, Want } from '@kit.AbilityKit';
import { display, KeyboardAvoidMode, ThemeControl, window } from '@kit.ArkUI';
import { connection } from '@kit.NetworkKit';
import { OrbitHarmonyStore } from '../model/OrbitHarmonyStore';
import { orbitLogError, orbitLogInfo, orbitLogWarn } from '../model/OrbitLogger';
import { ORBIT_DEFAULT_THEME } from '../theme/OrbitTokens';

interface EdgeInsetsPx {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

function zeroInsets(): EdgeInsetsPx {
  return { top: 0, bottom: 0, left: 0, right: 0 };
}

function mergeInsets(a: EdgeInsetsPx, b: EdgeInsetsPx): EdgeInsetsPx {
  return {
    top: Math.max(a.top, b.top),
    bottom: Math.max(a.bottom, b.bottom),
    left: Math.max(a.left, b.left),
    right: Math.max(a.right, b.right)
  };
}

function extractAvoidInsetsPx(area: window.AvoidArea, screenWidthPx: number, screenHeightPx: number): EdgeInsetsPx {
  return {
    top: area.topRect.height > 0 ? area.topRect.top + area.topRect.height : 0,
    bottom: area.bottomRect.height > 0
      ? (screenHeightPx > 0 ? Math.max(area.bottomRect.height, screenHeightPx - area.bottomRect.top) : area.bottomRect.height)
      : 0,
    left: area.leftRect.width > 0 ? area.leftRect.left + area.leftRect.width : 0,
    right: area.rightRect.width > 0
      ? (screenWidthPx > 0 ? Math.max(area.rightRect.width, screenWidthPx - area.rightRect.left) : area.rightRect.width)
      : 0
  };
}

export default class EntryAbility extends UIAbility {
  private netConn: connection.NetConnection | null = null;
  private mainWin: window.Window | null = null;
  private lastBearerSig: string = '';
  private systemInsets: EdgeInsetsPx = zeroInsets();
  private navIndicatorInsets: EdgeInsetsPx = zeroInsets();
  private cutoutInsets: EdgeInsetsPx = zeroInsets();
  private floatNavInsets: EdgeInsetsPx = zeroInsets();
  private keyboardVp: number = 0;

  private avoidAreaHandler: ((opts: window.AvoidAreaOptions) => void) | null = null;
  private windowSizeHandler: ((size: window.Size) => void) | null = null;

  onCreate(want: Want, _launchParam: AbilityConstant.LaunchParam): void {
    const store = OrbitHarmonyStore.getInstance();
    try {
      store.bindContext(this.context);
      const appCtx = this.context.getApplicationContext();
      store.bindApplicationContext(appCtx);
      const mode = this.context.config?.colorMode;
      if (mode !== undefined) {
        store.setSystemDark(mode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK);
      }
      store.applyPersistedThemeMode();
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Color mode / preferences setup warning', err as Object);
    }

    const dbPath = `${this.context.databaseDir}/orbit_harmony_v2.sqlite`;
    store.initialize(dbPath);
    orbitLogInfo('OrbitEntry', `OrbitHarmonyCore initialized at ${dbPath}`);

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

  onMemoryLevel(level: AbilityConstant.MemoryLevel): void {
    if (level === AbilityConstant.MemoryLevel.MEMORY_LEVEL_CRITICAL ||
      level === AbilityConstant.MemoryLevel.MEMORY_LEVEL_LOW) {
      orbitLogWarn('OrbitEntry', `Memory pressure level ${level}, trimming caches`);
      OrbitHarmonyStore.getInstance().onMemoryPressure();
    }
  }

  onDestroy(): void {
    if (this.netConn !== null) {
      try {
        this.netConn.unregister((err) => {
          if (err !== undefined && err !== null) {
            orbitLogWarn('OrbitEntry', 'Network unregister warning', err as Object);
          }
        });
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'Network unregister failed', err as Object);
      }
      this.netConn = null;
    }
    OrbitHarmonyStore.getInstance().dispose();
  }

  onWindowStageCreate(windowStage: window.WindowStage): void {
    try {
      ThemeControl.setDefaultTheme(ORBIT_DEFAULT_THEME);
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'ThemeControl.setDefaultTheme unavailable', err as Object);
    }
    windowStage.loadContent('pages/Index', (err) => {
      if (err.code) {
        orbitLogError('OrbitEntry', 'Failed to load content', err as Object);
        return;
      }
      this.setupImmersiveWindow(windowStage);
    });
  }

  onWindowStageDestroy(): void {
    if (this.mainWin !== null) {
      try {
        if (this.avoidAreaHandler !== null) {
          this.mainWin.off('avoidAreaChange', this.avoidAreaHandler);
        }
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'Failed to unregister avoidAreaChange', err as Object);
      }
      try {
        if (this.windowSizeHandler !== null) {
          this.mainWin.off('windowSizeChange', this.windowSizeHandler);
        }
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'Failed to unregister windowSizeChange', err as Object);
      }
      this.avoidAreaHandler = null;
      this.windowSizeHandler = null;
      this.mainWin = null;
    }
  }

  private registerNetworkObserver(): void {
    if (!canIUse('SystemCapability.Communication.NetManager.Core')) {
      return;
    }
    try {
      const conn = connection.createNetConnection();
      conn.register((err) => {
        if (err !== undefined && err !== null) {
          orbitLogWarn('OrbitEntry', 'Network observer register failed', err as Object);
        }
      });
      conn.on('netAvailable', (_handle: connection.NetHandle) => {
        OrbitHarmonyStore.getInstance().onNetworkChanged();
      });
      conn.on('netLost', (_handle: connection.NetHandle) => {
        OrbitHarmonyStore.getInstance().onNetworkChanged();
      });
      conn.on('netUnavailable', () => {
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
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Network observer unavailable', err as Object);
    }
  }

  private queryAndPublishAllAvoidAreas(mainWin: window.Window, uiCtx: UIContext): void {
    let screenW = 0;
    let screenH = 0;
    try {
      const disp = display.getDefaultDisplaySync();
      screenW = disp.width;
      screenH = disp.height;
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Failed to query display dimensions', err as Object);
    }

    try {
      this.systemInsets = extractAvoidInsetsPx(
        mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM),
        screenW,
        screenH
      );
      this.navIndicatorInsets = extractAvoidInsetsPx(
        mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR),
        screenW,
        screenH
      );
      this.cutoutInsets = extractAvoidInsetsPx(
        mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_CUTOUT),
        screenW,
        screenH
      );
      try {
        this.floatNavInsets = extractAvoidInsetsPx(
          mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_FLOAT_NAVIGATION),
          screenW,
          screenH
        );
      } catch (_floatErr) {
        this.floatNavInsets = zeroInsets();
      }
      const kbArea = mainWin.getWindowAvoidArea(window.AvoidAreaType.TYPE_KEYBOARD);
      this.keyboardVp = kbArea.bottomRect.height > 0 ? uiCtx.px2vp(kbArea.bottomRect.height) : 0;
      this.publishMergedSafeArea(uiCtx);
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Failed to query avoid areas', err as Object);
    }
  }

  private publishMergedSafeArea(uiCtx: UIContext): void {
    const merged = mergeInsets(
      mergeInsets(this.systemInsets, this.navIndicatorInsets),
      mergeInsets(this.cutoutInsets, this.floatNavInsets)
    );
    OrbitHarmonyStore.getInstance().updateSafeArea(
      uiCtx.px2vp(merged.top),
      uiCtx.px2vp(merged.bottom),
      this.keyboardVp,
      uiCtx.px2vp(merged.left),
      uiCtx.px2vp(merged.right)
    );
  }

  private async setupImmersiveWindow(windowStage: window.WindowStage): Promise<void> {
    try {
      const mainWin = windowStage.getMainWindowSync();
      this.mainWin = mainWin;
      try {
        await mainWin.setWindowLayoutFullScreen(true);
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'setWindowLayoutFullScreen not applied (e.g. freeform window)', err as Object);
      }

      try {
        await mainWin.setFloatNavigationAvoidAreaEnabled(true);
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'setFloatNavigationAvoidAreaEnabled unavailable', err as Object);
      }

      const uiCtx = mainWin.getUIContext();
      uiCtx.setKeyboardAvoidMode(KeyboardAvoidMode.RESIZE);

      this.queryAndPublishAllAvoidAreas(mainWin, uiCtx);

      try {
        const props = mainWin.getWindowProperties();
        if (props.windowRect.width > 0 || props.windowRect.height > 0) {
          OrbitHarmonyStore.getInstance().updateWindowSize(
            uiCtx.px2vp(props.windowRect.width),
            uiCtx.px2vp(props.windowRect.height)
          );
        }
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'Initial windowRect query failed', err as Object);
      }

      this.avoidAreaHandler = (opts: window.AvoidAreaOptions): void => {
        try {
          let screenW = 0;
          let screenH = 0;
          try {
            const disp = display.getDefaultDisplaySync();
            screenW = disp.width;
            screenH = disp.height;
          } catch (_dErr) {
            // Keep 0 fallback
          }

          if (opts.type === window.AvoidAreaType.TYPE_SYSTEM) {
            this.systemInsets = extractAvoidInsetsPx(opts.area, screenW, screenH);
          } else if (opts.type === window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR) {
            this.navIndicatorInsets = extractAvoidInsetsPx(opts.area, screenW, screenH);
          } else if (opts.type === window.AvoidAreaType.TYPE_CUTOUT) {
            this.cutoutInsets = extractAvoidInsetsPx(opts.area, screenW, screenH);
          } else if (opts.type === window.AvoidAreaType.TYPE_FLOAT_NAVIGATION) {
            this.floatNavInsets = extractAvoidInsetsPx(opts.area, screenW, screenH);
          } else if (opts.type === window.AvoidAreaType.TYPE_KEYBOARD) {
            this.keyboardVp = opts.area.bottomRect.height > 0
              ? uiCtx.px2vp(opts.area.bottomRect.height)
              : 0;
          }
          this.publishMergedSafeArea(uiCtx);
        } catch (err) {
          orbitLogWarn('OrbitEntry', 'avoidAreaChange handling failed', err as Object);
        }
      };
      mainWin.on('avoidAreaChange', this.avoidAreaHandler);

      // Re-query all avoid areas atomically on rotation/fold to prevent intermediate-state jumps
      this.windowSizeHandler = (size: window.Size): void => {
        try {
          OrbitHarmonyStore.getInstance().updateWindowSize(
            uiCtx.px2vp(size.width),
            uiCtx.px2vp(size.height)
          );
          this.queryAndPublishAllAvoidAreas(mainWin, uiCtx);
        } catch (err) {
          orbitLogWarn('OrbitEntry', 'windowSizeChange handling failed', err as Object);
        }
      };
      mainWin.on('windowSizeChange', this.windowSizeHandler);
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Immersive window setup failed', err as Object);
    }
  }
}
