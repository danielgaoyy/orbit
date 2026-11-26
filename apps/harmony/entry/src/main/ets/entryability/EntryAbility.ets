import { AbilityConstant, Configuration, ConfigurationConstant, UIAbility, Want } from '@kit.AbilityKit';
import { display, FrameCallback, KeyboardAvoidMode, ThemeControl, window } from '@kit.ArkUI';
import { fileIo } from '@kit.CoreFileKit';
import { connection } from '@kit.NetworkKit';
import { OrbitHarmonyStore } from '../model/OrbitHarmonyStore';
import { OrbitHuksMasterKey, OrbitMasterKeyResult } from '../model/OrbitHuksMasterKey';
import { orbitLogError, orbitLogInfo, orbitLogWarn } from '../model/OrbitLogger';
import { OrbitPerfProbe } from '../model/OrbitPerfProbe';
import { OrbitPreferencesStore } from '../model/OrbitPreferences';
import {
  orbitApplySystemBarTheme,
  orbitBindThemeApplicationContext,
  orbitBindThemeWindow
} from '../model/OrbitWindowTheme';
import { ORBIT_DEFAULT_THEME } from '../theme/OrbitTokens';

class OrbitFirstFrameCallback extends FrameCallback {
  private readonly onFirstFrame: () => void;
  private fired: boolean = false;

  constructor(onFirstFrame: () => void) {
    super();
    this.onFirstFrame = onFirstFrame;
  }

  onFrame(_frameTimeInNano: number): void {
    if (this.fired) {
      return;
    }
    this.fired = true;
    this.onFirstFrame();
  }
}

interface OrbitEdgeInsetsPx {
  top: number;
  bottom: number;
  left: number;
  right: number;
}

interface OrbitPushExtraPayload {
  deep_link?: string;
  orbit_open_uri?: string;
  host_id?: string;
  orbit_host_id?: string;
  agent_id?: string;
  orbit_agent_id?: string;
  event_kind?: string;
}

function zeroInsets(): OrbitEdgeInsetsPx {
  return { top: 0, bottom: 0, left: 0, right: 0 };
}

function mergeInsets(a: OrbitEdgeInsetsPx, b: OrbitEdgeInsetsPx): OrbitEdgeInsetsPx {
  return {
    top: Math.max(a.top, b.top),
    bottom: Math.max(a.bottom, b.bottom),
    left: Math.max(a.left, b.left),
    right: Math.max(a.right, b.right)
  };
}

function extractAvoidInsetsPx(
  area: window.AvoidArea,
  screenWidthPx: number,
  screenHeightPx: number
): OrbitEdgeInsetsPx {
  return {
    top: area.topRect.height > 0 ? area.topRect.top + area.topRect.height : 0,
    bottom: area.bottomRect.height > 0
      ? (screenHeightPx > 0
        ? Math.max(area.bottomRect.height, screenHeightPx - area.bottomRect.top)
        : area.bottomRect.height)
      : 0,
    left: area.leftRect.width > 0 ? area.leftRect.left + area.leftRect.width : 0,
    right: area.rightRect.width > 0
      ? (screenWidthPx > 0
        ? Math.max(area.rightRect.width, screenWidthPx - area.rightRect.left)
        : area.rightRect.width)
      : 0
  };
}

function mapFoldStatusToSlug(status: display.FoldStatus): string {
  if (
    status === display.FoldStatus.FOLD_STATUS_EXPANDED ||
    status === display.FoldStatus.FOLD_STATUS_EXPANDED_WITH_SECOND_EXPANDED
  ) {
    return 'expanded';
  }
  if (
    status === display.FoldStatus.FOLD_STATUS_HALF_FOLDED ||
    status === display.FoldStatus.FOLD_STATUS_EXPANDED_WITH_SECOND_HALF_FOLDED
  ) {
    return 'half_folded';
  }
  if (status === display.FoldStatus.FOLD_STATUS_FOLDED) {
    return 'folded';
  }
  return 'unknown';
}

function readParamString(params: Record<string, Object> | undefined, key: string): string {
  if (params === undefined) {
    return '';
  }
  const raw = params[key];
  if (typeof raw === 'string') {
    return raw.trim();
  }
  return '';
}

export default class EntryAbility extends UIAbility {
  private netConn: connection.NetConnection | null = null;
  private mainWin: window.Window | null = null;
  private lastBearerSig: string = '';
  private systemInsets: OrbitEdgeInsetsPx = zeroInsets();
  private navIndicatorInsets: OrbitEdgeInsetsPx = zeroInsets();
  private cutoutInsets: OrbitEdgeInsetsPx = zeroInsets();
  private floatNavInsets: OrbitEdgeInsetsPx = zeroInsets();
  private keyboardVp: number = 0;

  private avoidAreaHandler: ((opts: window.AvoidAreaOptions) => void) | null = null;
  private windowSizeHandler: ((size: window.Size) => void) | null = null;
  private foldStatusHandler: ((status: display.FoldStatus) => void) | null = null;
  private coldLoadContentMarked: boolean = false;
  private coldFirstFrameMarked: boolean = false;

  onCreate(want: Want, _launchParam: AbilityConstant.LaunchParam): void {
    OrbitPerfProbe.mark('cold.create_start');
    const store = OrbitHarmonyStore.getInstance();
    try {
      store.bindContext(this.context);
      const appCtx = this.context.getApplicationContext();
      orbitBindThemeApplicationContext(appCtx);
      store.bindApplicationContext(appCtx);
      const mode = this.context.config?.colorMode;
      if (mode !== undefined) {
        store.setSystemDark(mode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK);
      }
      store.applyPersistedThemeMode();
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Color mode / preferences setup warning', err as Object);
    }
    OrbitPerfProbe.mark('cold.prefs_ready');

    const dbPath = `${this.context.databaseDir}/orbit_harmony_v2.sqlite`;
    OrbitHuksMasterKey.resolveOrCreateMasterKey(this.context).then((keyRes: OrbitMasterKeyResult) => {
      OrbitPerfProbe.mark('cold.huks_ready');
      store.keyProtectionMode = keyRes.protectionMode;
      const routerPrefs = new OrbitPreferencesStore();
      routerPrefs.init(this.context);
      store.primaryRouterHostId = routerPrefs.getPrimaryRouterHostId();
      store.initialize(dbPath, keyRes.masterKeyB64);
      OrbitPerfProbe.mark('cold.core_ready');
      OrbitPerfProbe.flushIfDue();
      orbitLogInfo('OrbitEntry', `OrbitHarmonyCore ready at ${dbPath}`);
    }).catch((err: Object) => {
      orbitLogError('OrbitEntry', 'OrbitHarmonyCore HUKS init failed', err);
    });

    this.applyLaunchWant(want);
    this.registerNetworkObserver();
    this.registerFoldableObserver();
    store.registerPushTokenSilently();
  }

  onNewWant(want: Want, _launchParam: AbilityConstant.LaunchParam): void {
    this.applyLaunchWant(want);
  }

  private applyLaunchWant(want: Want): void {
    const store = OrbitHarmonyStore.getInstance();
    const params = want.parameters;
    let eventKind = readParamString(params, 'event_kind');
    let deepLink = want.uri !== undefined ? want.uri.trim() : '';
    let hostId = readParamString(params, 'orbit_host_id');
    if (hostId.length === 0) {
      hostId = readParamString(params, 'host_id');
    }
    let agentId = readParamString(params, 'orbit_agent_id');
    if (agentId.length === 0) {
      agentId = readParamString(params, 'agent_id');
    }

    if (deepLink.length === 0) {
      deepLink = readParamString(params, 'deep_link');
    }
    if (deepLink.length === 0) {
      deepLink = readParamString(params, 'orbit_open_uri');
    }
    if (deepLink.length === 0) {
      deepLink = readParamString(params, 'orbit_pair_uri');
    }

    // Parse HarmonyOS Push Kit nested extraData JSON when present
    const extraRaw = readParamString(params, 'extraData');
    if (extraRaw.length > 0 && extraRaw.startsWith('{')) {
      try {
        const parsed = JSON.parse(extraRaw) as OrbitPushExtraPayload;
        if (eventKind.length === 0 && typeof parsed.event_kind === 'string') {
          eventKind = parsed.event_kind.trim();
        }
        if (deepLink.length === 0 && typeof parsed.deep_link === 'string') {
          deepLink = parsed.deep_link.trim();
        }
        if (deepLink.length === 0 && typeof parsed.orbit_open_uri === 'string') {
          deepLink = parsed.orbit_open_uri.trim();
        }
        if (hostId.length === 0 && typeof parsed.host_id === 'string') {
          hostId = parsed.host_id.trim();
        }
        if (hostId.length === 0 && typeof parsed.orbit_host_id === 'string') {
          hostId = parsed.orbit_host_id.trim();
        }
        if (agentId.length === 0 && typeof parsed.agent_id === 'string') {
          agentId = parsed.agent_id.trim();
        }
        if (agentId.length === 0 && typeof parsed.orbit_agent_id === 'string') {
          agentId = parsed.orbit_agent_id.trim();
        }
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'Failed to parse push extraData JSON', err as Object);
      }
    }

    const replayPathRaw = readParamString(params, 'orbit_perf_replay_path');
    if (replayPathRaw.length > 0) {
      const replayHost = readParamString(params, 'orbit_perf_replay_host') || 'replay-host';
      const replayAgent = readParamString(params, 'orbit_perf_replay_agent') || 'replay-agent';
      const resolvedPath = replayPathRaw.startsWith('files://')
        ? `${this.context.filesDir}/${replayPathRaw.substring('files://'.length)}`
        : replayPathRaw;
      try {
        const framesJson = fileIo.readTextSync(resolvedPath);
        store.scheduleDebugStageReplay(replayHost, replayAgent, framesJson);
        orbitLogInfo('OrbitEntry', `Scheduled stage perf replay from ${resolvedPath}`);
      } catch (err) {
        orbitLogError('OrbitEntry', `Failed to read perf replay fixture at ${resolvedPath}`, err as Object);
      }
      return;
    }

    if (hostId.length > 0) {
      store.openSessionFromDeepLink(hostId, agentId, eventKind);
      return;
    }
    if (deepLink.length > 0) {
      store.handleDeepLinkUri(deepLink, eventKind);
    }
  }

  onConfigurationUpdate(newConfig: Configuration): void {
    if (newConfig.colorMode !== undefined) {
      const store = OrbitHarmonyStore.getInstance();
      store.setSystemDark(newConfig.colorMode === ConfigurationConstant.ColorMode.COLOR_MODE_DARK);
      orbitApplySystemBarTheme(store.palette.isDark);
    }
  }

  onForeground(): void {
    OrbitHarmonyStore.getInstance().onAppForeground();
  }

  onBackground(): void {
    OrbitHarmonyStore.getInstance().onAppBackground();
  }

  onMemoryLevel(level: AbilityConstant.MemoryLevel): void {
    if (
      level === AbilityConstant.MemoryLevel.MEMORY_LEVEL_CRITICAL ||
      level === AbilityConstant.MemoryLevel.MEMORY_LEVEL_LOW
    ) {
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
    if (this.foldStatusHandler !== null) {
      try {
        display.off('foldStatusChange', this.foldStatusHandler);
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'foldStatusChange unregister failed', err as Object);
      }
      this.foldStatusHandler = null;
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
      if (!this.coldLoadContentMarked) {
        this.coldLoadContentMarked = true;
        OrbitPerfProbe.mark('cold.load_content');
      }
      this.scheduleFirstFrameMark(windowStage);
      this.setupImmersiveWindow(windowStage);
    });
  }

  private scheduleFirstFrameMark(windowStage: window.WindowStage): void {
    if (this.coldFirstFrameMarked) {
      return;
    }
    try {
      const uiCtx = windowStage.getMainWindowSync().getUIContext();
      const firstFrameCallback = new OrbitFirstFrameCallback((): void => {
        if (!this.coldFirstFrameMarked) {
          this.coldFirstFrameMarked = true;
          OrbitPerfProbe.mark('cold.first_frame');
          OrbitPerfProbe.flushIfDue();
        }
      });
      uiCtx.postFrameCallback(firstFrameCallback);
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Failed to schedule cold.first_frame callback', err as Object);
    }
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
      orbitBindThemeWindow(null);
      OrbitHarmonyStore.getInstance().bindUIContext(null);
    }
  }

  private registerFoldableObserver(): void {
    if (!canIUse('SystemCapability.Window.SessionManager')) {
      return;
    }
    try {
      const foldable = display.isFoldable();
      const initialStatus = foldable
        ? mapFoldStatusToSlug(display.getFoldStatus())
        : 'unknown';
      OrbitHarmonyStore.getInstance().updateFoldPosture(foldable, initialStatus);

      if (foldable) {
        this.foldStatusHandler = (nextStatus: display.FoldStatus): void => {
          const slug = mapFoldStatusToSlug(nextStatus);
          OrbitHarmonyStore.getInstance().updateFoldPosture(true, slug);
          if (this.mainWin !== null) {
            try {
              const uiCtx = this.mainWin.getUIContext();
              this.queryAndPublishAllAvoidAreas(this.mainWin, uiCtx);
            } catch (err) {
              orbitLogWarn('OrbitEntry', 'Avoid area refresh on foldStatusChange failed', err as Object);
            }
          }
        };
        display.on('foldStatusChange', this.foldStatusHandler);
      }
    } catch (err) {
      orbitLogWarn('OrbitEntry', 'Foldable display observer unavailable', err as Object);
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
      orbitBindThemeWindow(mainWin);
      try {
        await mainWin.setWindowLayoutFullScreen(true);
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'setWindowLayoutFullScreen not applied', err as Object);
      }

      orbitApplySystemBarTheme(OrbitHarmonyStore.getInstance().palette.isDark);

      try {
        await mainWin.setFloatNavigationAvoidAreaEnabled(true);
      } catch (err) {
        orbitLogWarn('OrbitEntry', 'setFloatNavigationAvoidAreaEnabled unavailable', err as Object);
      }

      const uiCtx = mainWin.getUIContext();
      OrbitHarmonyStore.getInstance().bindUIContext(uiCtx);
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
