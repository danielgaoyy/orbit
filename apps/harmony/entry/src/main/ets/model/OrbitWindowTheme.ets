import { ConfigurationConstant, common } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { deviceInfo } from '@kit.BasicServicesKit';
import { DARK_PALETTE, LIGHT_PALETTE } from '../theme/OrbitTokens';
import { orbitLogWarn } from './OrbitLogger';

let boundAppContext: common.ApplicationContext | null = null;
let boundMainWindow: window.Window | null = null;
let preferredThemeMode: string = 'system';

export function orbitBindThemeApplicationContext(ctx: common.ApplicationContext | null): void {
  boundAppContext = ctx;
}

export function orbitBindThemeWindow(win: window.Window | null): void {
  boundMainWindow = win;
}

export function orbitNormalizeThemeMode(mode: string): string {
  if (mode === 'light') {
    return 'light';
  }
  if (mode === 'dark') {
    return 'dark';
  }
  return 'system';
}

export function orbitSetThemeModePreference(mode: string): void {
  preferredThemeMode = orbitNormalizeThemeMode(mode);
}

function resolveTargetColorMode(): ConfigurationConstant.ColorMode {
  if (preferredThemeMode === 'light') {
    return ConfigurationConstant.ColorMode.COLOR_MODE_LIGHT;
  }
  if (preferredThemeMode === 'dark') {
    return ConfigurationConstant.ColorMode.COLOR_MODE_DARK;
  }
  return ConfigurationConstant.ColorMode.COLOR_MODE_NOT_SET;
}

/**
 * Synchronizes HarmonyOS application color mode and window system bar content colors
 * in strict order: `appContext.setColorMode(target)` first, then
 * `mainWindow.setWindowSystemBarProperties(...)`.
 */
export function orbitApplySystemBarTheme(isDark: boolean): void {
  if (boundAppContext !== null) {
    try {
      boundAppContext.setColorMode(resolveTargetColorMode());
    } catch (err) {
      orbitLogWarn('OrbitWindowTheme', 'setColorMode failed', err as Object);
    }
  }

  if (boundMainWindow === null) {
    return;
  }

  // `setWindowSystemBarProperties` is unavailable on 2in1 free-window devices; guard explicitly.
  if (
    deviceInfo.deviceType === '2in1' ||
    !canIUse('SystemCapability.WindowManager.WindowManager.Core')
  ) {
    return;
  }

  try {
    const contentHex = isDark
      ? DARK_PALETTE.statusBarContentHex
      : LIGHT_PALETTE.statusBarContentHex;
    const barProps: window.SystemBarProperties = {
      statusBarColor: '#00000000',
      navigationBarColor: '#00000000',
      statusBarContentColor: contentHex,
      navigationBarContentColor: contentHex
    };
    boundMainWindow.setWindowSystemBarProperties(barProps).catch((err: Object) => {
      orbitLogWarn('OrbitWindowTheme', 'setWindowSystemBarProperties failed', err);
    });
  } catch (err) {
    orbitLogWarn('OrbitWindowTheme', 'System bar theme sync unavailable', err as Object);
  }
}
