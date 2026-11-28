import { common, Want, wantAgent } from '@kit.AbilityKit';
import { pasteboard } from '@kit.BasicServicesKit';
import { fileIo } from '@kit.CoreFileKit';
import { i18n } from '@kit.LocalizationKit';
import { notificationManager } from '@kit.NotificationKit';
import { pushService } from '@kit.PushKit';
import { scanBarcode, scanCore } from '@kit.ScanKit';
import { vibrator } from '@kit.SensorServiceKit';
import { orbitLogWarn } from './OrbitLogger';

export const MAX_ATTACHMENT_BYTES: number = 20 * 1024 * 1024;
const SCAN_USER_CANCEL_CODE: number = 1000500002;

export interface PushTokenOutcome {
  available: boolean;
  token: string;
  errorMessage: string;
}

export interface LoadedUriBytes {
  filename: string;
  mimeType: string;
  bytes: Uint8Array;
}

export interface ScanQrOutcome {
  uri: string;
  cancelled: boolean;
  errorMessage: string;
}

export type OrbitHapticPreset = 'selection' | 'action' | 'success' | 'warning';

export interface OrbitNativeHapticDriver {
  isAvailable: () => boolean;
  isSupportEffectSync: (effectId: string) => boolean;
  startPresetVibration: (effectId: string) => Promise<void>;
  startTimeVibration: (durationMs: number) => Promise<void>;
}

const FALLBACK_HAPTIC_DURATION_MS: number = 10;

export function normalizeOrbitHapticPreset(
  presetOrMs: OrbitHapticPreset | number = 'selection'
): OrbitHapticPreset {
  if (typeof presetOrMs === 'string') {
    if (
      presetOrMs === 'selection' ||
      presetOrMs === 'action' ||
      presetOrMs === 'success' ||
      presetOrMs === 'warning'
    ) {
      return presetOrMs;
    }
    return 'selection';
  }
  if (presetOrMs >= 16) {
    return 'warning';
  }
  if (presetOrMs >= 12) {
    return 'success';
  }
  if (presetOrMs >= 9) {
    return 'action';
  }
  return 'selection';
}

export function resolveNativeHapticEffectId(preset: OrbitHapticPreset): string {
  if (preset === 'success') {
    return 'haptic.notice.success';
  }
  if (preset === 'warning') {
    return 'haptic.notice.warning';
  }
  if (preset === 'action') {
    return 'haptic.effect.soft';
  }
  return 'haptic.effect.soft';
}

function triggerTimeFallback(driver: OrbitNativeHapticDriver): void {
  try {
    driver.startTimeVibration(FALLBACK_HAPTIC_DURATION_MS).catch((err: Object) => {
      orbitLogWarn('OrbitHaptic', 'Fallback time haptic failed', err);
    });
  } catch (err) {
    orbitLogWarn('OrbitHaptic', 'Fallback time haptic unavailable', err as Object);
  }
}

export function triggerNativeHapticWithDriver(
  presetOrMs: OrbitHapticPreset | number,
  driver: OrbitNativeHapticDriver
): void {
  let available = false;
  try {
    available = driver.isAvailable();
  } catch (_err) {
    available = false;
  }
  if (!available) {
    return;
  }
  const preset = normalizeOrbitHapticPreset(presetOrMs);
  const effectId = resolveNativeHapticEffectId(preset);
  let supported = false;
  try {
    supported = driver.isSupportEffectSync(effectId);
  } catch (err) {
    orbitLogWarn('OrbitHaptic', 'Preset haptic support probe failed; falling back to 10ms time vibration', err as Object);
    supported = false;
  }
  if (supported) {
    try {
      driver.startPresetVibration(effectId).catch((err: Object) => {
        orbitLogWarn('OrbitHaptic', 'Preset haptic failed; falling back to 10ms time vibration', err);
        triggerTimeFallback(driver);
      });
      return;
    } catch (err) {
      orbitLogWarn('OrbitHaptic', 'Preset haptic threw synchronously; falling back to 10ms time vibration', err as Object);
    }
  }
  triggerTimeFallback(driver);
}

const DEFAULT_NATIVE_HAPTIC_DRIVER: OrbitNativeHapticDriver = {
  isAvailable: (): boolean => canIUse('SystemCapability.Sensors.MiscDevice'),
  isSupportEffectSync: (effectId: string): boolean => vibrator.isSupportEffectSync(effectId),
  startPresetVibration: (effectId: string): Promise<void> =>
    vibrator.startVibration(
      { type: 'preset', effectId: effectId, count: 1 },
      { id: 0, usage: 'touch' }
    ),
  startTimeVibration: (durationMs: number): Promise<void> =>
    vibrator.startVibration(
      { type: 'time', duration: durationMs },
      { id: 0, usage: 'touch' }
    )
};

export function triggerNativeHaptic(presetOrMs: OrbitHapticPreset | number = 'selection'): void {
  triggerNativeHapticWithDriver(presetOrMs, DEFAULT_NATIVE_HAPTIC_DRIVER);
}

export function normalizeOrbitAppLanguage(lang: string): string {
  const clean = lang.trim();
  if (clean === 'en' || clean === 'zh_CN' || clean === 'ja_JP') {
    return clean;
  }
  const lower = clean.toLowerCase();
  if (lower.startsWith('zh')) {
    return 'zh_CN';
  }
  if (lower.startsWith('ja')) {
    return 'ja_JP';
  }
  if (lower.startsWith('en')) {
    return 'en';
  }
  return 'system';
}

export function resolveEffectiveOrbitLanguage(
  appLanguage: string,
  systemLanguageOverride?: string
): 'en' | 'zh_CN' | 'ja_JP' {
  const pref = normalizeOrbitAppLanguage(appLanguage);
  if (pref === 'en' || pref === 'zh_CN' || pref === 'ja_JP') {
    return pref;
  }
  let rawSystem = '';
  if (systemLanguageOverride !== undefined) {
    rawSystem = systemLanguageOverride.trim();
  } else {
    try {
      if (canIUse('SystemCapability.Global.I18n')) {
        rawSystem = i18n.System.getSystemLanguage();
      }
    } catch (_err) {
      rawSystem = '';
    }
  }
  const sysNorm = normalizeOrbitAppLanguage(rawSystem);
  if (sysNorm === 'zh_CN' || sysNorm === 'ja_JP') {
    return sysNorm;
  }
  return 'en';
}

export function mapOrbitAppLanguageToBcp47(lang: string): string {
  const normalized = normalizeOrbitAppLanguage(lang);
  if (normalized === 'en') {
    return 'en-US';
  }
  if (normalized === 'zh_CN') {
    return 'zh-Hans';
  }
  if (normalized === 'ja_JP') {
    return 'ja-JP';
  }
  return 'default';
}

export function applyNativeAppLanguage(
  lang: string,
  appContext: common.ApplicationContext | null
): boolean {
  const normalized = normalizeOrbitAppLanguage(lang);
  const bcp47 = mapOrbitAppLanguageToBcp47(normalized);
  let applied = false;
  try {
    if (canIUse('SystemCapability.Global.I18n')) {
      i18n.System.setAppPreferredLanguage(bcp47);
      applied = true;
    }
  } catch (err) {
    orbitLogWarn('OrbitI18n', 'setAppPreferredLanguage failed', err as Object);
  }
  if (appContext !== null) {
    try {
      if (normalized === 'system') {
        const sysLang = canIUse('SystemCapability.Global.I18n')
          ? i18n.System.getSystemLanguage()
          : '';
        if (sysLang.length > 0) {
          appContext.setLanguage(sysLang);
          applied = true;
        }
      } else {
        appContext.setLanguage(bcp47);
        applied = true;
      }
    } catch (err) {
      orbitLogWarn('OrbitI18n', 'ApplicationContext.setLanguage failed', err as Object);
    }
  }
  return applied;
}

export function writeClipboardText(text: string): boolean {
  try {
    const data = pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, text);
    pasteboard.getSystemPasteboard().setDataSync(data);
    return true;
  } catch (err) {
    orbitLogWarn('OrbitClipboard', 'Failed to copy to clipboard', err as Object);
    return false;
  }
}

export async function readAuthorizedClipboardText(): Promise<string> {
  try {
    const pb = pasteboard.getSystemPasteboard();
    // Exclusively invoked inside HarmonyOS PasteButton PasteButtonOnClickResult.SUCCESS
    // (one-shot security-control authorization without requiring restricted READ_PASTEBOARD ACL).
    // @SuppressWarnings permission
    const data = await pb.getData();
    return data.getPrimaryText().trim();
  } catch (err) {
    orbitLogWarn('OrbitClipboard', 'Failed to read pasteboard', err as Object);
    return '';
  }
}

export async function scanPairingQrCode(context: common.Context | undefined): Promise<ScanQrOutcome> {
  if (context === undefined) {
    return {
      uri: '',
      cancelled: false,
      errorMessage: 'Scanner requires an active ability context.'
    };
  }
  if (canIUse('SystemCapability.Multimedia.Scan.ScanBarcode')) {
    if (canIUse('SystemCapability.Multimedia.Scan.Core')) {
      try {
        const result = await scanBarcode.startScanForResult(context, {
          scanTypes: [scanCore.ScanType.QR_CODE],
          enableMultiMode: false,
          enableAlbum: true
        });
        const text = result.originalValue?.trim() ?? '';
        return { uri: text, cancelled: false, errorMessage: '' };
      } catch (err) {
        const code = (err as Record<string, Object>)['code'];
        if (typeof code === 'number' && code === SCAN_USER_CANCEL_CODE) {
          return { uri: '', cancelled: true, errorMessage: '' };
        }
        orbitLogWarn('OrbitScan', 'ScanKit failed', err as Object);
        return {
          uri: '',
          cancelled: false,
          errorMessage: 'QR scan did not complete. You can paste an orbit://pair URI in Settings.'
        };
      }
    }
  }
  return {
    uri: '',
    cancelled: false,
    errorMessage: 'Camera QR scanning is not available on this device. Paste an orbit://pair URI instead.'
  };
}

export function inferFilenameFromUri(uri: string, fallbackName: string): string {
  const clean = uri.split('?')[0];
  const lastSlash = clean.lastIndexOf('/');
  if (lastSlash >= 0 && lastSlash + 1 < clean.length) {
    const raw = clean.substring(lastSlash + 1);
    try {
      const decoded = decodeURIComponent(raw).trim();
      if (decoded.length > 0) {
        return decoded;
      }
    } catch (_e) {
      if (raw.trim().length > 0) {
        return raw.trim();
      }
    }
  }
  return fallbackName;
}

export function inferMimeFromFilename(filename: string, fallbackMime: string): string {
  const dot = filename.lastIndexOf('.');
  const ext = dot >= 0 ? filename.substring(dot + 1).toLowerCase() : '';
  if (ext === 'png') {
    return 'image/png';
  }
  if (ext === 'jpg' || ext === 'jpeg') {
    return 'image/jpeg';
  }
  if (ext === 'webp') {
    return 'image/webp';
  }
  if (ext === 'gif') {
    return 'image/gif';
  }
  if (ext === 'heic' || ext === 'heif') {
    return 'image/heic';
  }
  if (ext === 'svg') {
    return 'image/svg+xml';
  }
  if (ext === 'pdf') {
    return 'application/pdf';
  }
  if (ext === 'json') {
    return 'application/json';
  }
  if (ext === 'md' || ext === 'txt' || ext === 'log' || ext === 'csv' || ext === 'rs' || ext === 'ts' || ext === 'ets') {
    return 'text/plain';
  }
  return fallbackMime;
}

export async function readUriBytesAsync(
  uri: string,
  fallbackFilename: string,
  fallbackMime: string
): Promise<LoadedUriBytes> {
  const resolvedName = inferFilenameFromUri(uri, fallbackFilename);
  const resolvedMime = inferMimeFromFilename(resolvedName, fallbackMime);
  let file: fileIo.File | null = null;
  try {
    file = await fileIo.open(uri, fileIo.OpenMode.READ_ONLY);
    const stat = await fileIo.stat(file.fd);
    if (stat.size <= 0) {
      throw new Error('Selected file is empty.');
    }
    if (stat.size > MAX_ATTACHMENT_BYTES) {
      const mb = Math.round(MAX_ATTACHMENT_BYTES / (1024 * 1024));
      throw new Error(`Attachment exceeds ${mb} MB limit.`);
    }
    const buf = new ArrayBuffer(stat.size);
    const bytesRead = await fileIo.read(file.fd, buf);
    const bytes = new Uint8Array(buf, 0, bytesRead);
    return {
      filename: resolvedName,
      mimeType: resolvedMime,
      bytes: bytes
    };
  } catch (err) {
    throw err as Error;
  } finally {
    if (file !== null) {
      try {
        await fileIo.close(file);
      } catch (err) {
        orbitLogWarn('OrbitFile', 'Failed to close attachment file handle', err as Object);
      }
    }
  }
}

export function openExternalWebUrl(context: common.Context | undefined, rawUrl: string): boolean {
  const trimmed = rawUrl.trim();
  if (!trimmed.startsWith('https://') && !trimmed.startsWith('http://')) {
    return false;
  }
  if (context === undefined) {
    return false;
  }
  try {
    const uiAbilityCtx = context as common.UIAbilityContext;
    const want: Want = {
      action: 'ohos.want.action.viewData',
      entities: ['entity.system.browsable'],
      uri: trimmed
    };
    uiAbilityCtx.startAbility(want).catch((err: Object) => {
      orbitLogWarn('OrbitLink', 'Failed to open external URL', err);
    });
    return true;
  } catch (err) {
    orbitLogWarn('OrbitLink', 'External URL launch unavailable', err as Object);
    return false;
  }
}

export async function registerHarmonyPushToken(): Promise<PushTokenOutcome> {
  if (!canIUse('SystemCapability.Push.PushService')) {
    return {
      available: false,
      token: '',
      errorMessage: 'Push Kit is unavailable on this device.'
    };
  }
  try {
    const token = await pushService.getToken();
    const clean = token !== undefined && token !== null ? token.trim() : '';
    return {
      available: true,
      token: clean,
      errorMessage: clean.length > 0 ? '' : 'Push Kit returned an empty token.'
    };
  } catch (err) {
    orbitLogWarn('OrbitPush', 'Push Kit getToken failed', err as Object);
    const msg = (err as Error).message;
    return {
      available: true,
      token: '',
      errorMessage: msg !== undefined && msg.length > 0 ? msg : 'Push Kit registration requires a signed HMS profile.'
    };
  }
}

export async function checkHarmonyNotificationEnabled(): Promise<boolean> {
  if (!canIUse('SystemCapability.Notification.Notification')) {
    return false;
  }
  try {
    return await notificationManager.isNotificationEnabled();
  } catch (err) {
    orbitLogWarn('OrbitNotify', 'Notification status probe failed', err as Object);
    return false;
  }
}

export async function requestHarmonyNotificationPermission(
  context: common.Context | undefined
): Promise<boolean> {
  if (!canIUse('SystemCapability.Notification.Notification')) {
    return false;
  }
  try {
    const enabled = await notificationManager.isNotificationEnabled();
    if (enabled) {
      return true;
    }
    if (context !== undefined) {
      const uiCtx = context as common.UIAbilityContext;
      await notificationManager.requestEnableNotification(uiCtx);
      return await notificationManager.isNotificationEnabled();
    }
    return false;
  } catch (err) {
    orbitLogWarn('OrbitNotify', 'Notification permission request failed', err as Object);
    return false;
  }
}

export async function setHarmonyAppBadgeNumber(count: number): Promise<void> {
  if (!canIUse('SystemCapability.Notification.Notification')) {
    return;
  }
  try {
    const safeCount = Number.isFinite(count) ? Math.max(0, Math.floor(count)) : 0;
    await notificationManager.setBadgeNumber(safeCount);
  } catch (err) {
    orbitLogWarn('OrbitNotify', 'Failed to update app badge number', err as Object);
  }
}

export async function publishOrbitDeepLinkNotification(
  context: common.Context | undefined,
  notificationId: number,
  title: string,
  body: string,
  hostId: string,
  agentId: string,
  eventKind: string = '',
  requestId: string = '',
  targetSeq: number = 0
): Promise<boolean> {
  if (
    !canIUse('SystemCapability.Notification.Notification') ||
    !canIUse('SystemCapability.Ability.AbilityRuntime.Core') ||
    context === undefined
  ) {
    return false;
  }
  try {
    const uiCtx = context as common.UIAbilityContext;
    const bundleName = uiCtx.abilityInfo?.bundleName ?? 'dev.orbit.harmony';
    const abilityName = uiCtx.abilityInfo?.name ?? 'EntryAbility';
    const cleanEvent = eventKind.trim();
    const cleanReqId = requestId.trim();
    const safeSeq = Number.isFinite(targetSeq) && targetSeq > 0 ? Math.floor(targetSeq) : 0;
    let deepLinkUri = `orbit://open?host=${encodeURIComponent(hostId)}&agent=${encodeURIComponent(agentId)}`;
    if (cleanEvent.length > 0) {
      deepLinkUri += `&event_kind=${encodeURIComponent(cleanEvent)}`;
    }
    if (cleanReqId.length > 0) {
      deepLinkUri += `&request_id=${encodeURIComponent(cleanReqId)}`;
    }
    if (safeSeq > 0) {
      deepLinkUri += `&seq=${safeSeq}`;
    }
    const params: Record<string, Object> = {
      'orbit_open_uri': deepLinkUri,
      'orbit_host_id': hostId,
      'orbit_agent_id': agentId
    };
    if (cleanEvent.length > 0) {
      params['event_kind'] = cleanEvent;
    }
    if (cleanReqId.length > 0) {
      params['orbit_request_id'] = cleanReqId;
    }
    if (safeSeq > 0) {
      params['orbit_target_seq'] = String(safeSeq);
    }
    const targetWant: Want = {
      bundleName: bundleName,
      abilityName: abilityName,
      uri: deepLinkUri,
      parameters: params
    };
    const agentInfo: wantAgent.WantAgentInfo = {
      wants: [targetWant],
      actionType: wantAgent.OperationType.START_ABILITY,
      requestCode: notificationId,
      actionFlags: [wantAgent.WantAgentFlags.UPDATE_PRESENT_FLAG]
    };
    const agentObj = await wantAgent.getWantAgent(agentInfo);
    const req: notificationManager.NotificationRequest = {
      id: notificationId,
      content: {
        notificationContentType: notificationManager.ContentType.NOTIFICATION_CONTENT_BASIC_TEXT,
        normal: {
          title: title,
          text: body
        }
      },
      wantAgent: agentObj
    };
    await notificationManager.publish(req);
    return true;
  } catch (err) {
    orbitLogWarn('OrbitNotify', 'Failed to publish local notification', err as Object);
    return false;
  }
}
