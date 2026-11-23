import { common, Want, wantAgent } from '@kit.AbilityKit';
import { pasteboard } from '@kit.BasicServicesKit';
import { fileIo } from '@kit.CoreFileKit';
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

export function triggerNativeHaptic(durationMs: number = 10): void {
  if (!canIUse('SystemCapability.Sensors.MiscDevice')) {
    return;
  }
  try {
    const presetId = durationMs >= 14 ? 'haptic.effect.hard' : 'haptic.effect.soft';
    if (vibrator.isSupportEffectSync(presetId)) {
      vibrator.startVibration(
        { type: 'preset', effectId: presetId, count: 1 },
        { id: 0, usage: 'touch' }
      ).catch((err: Object) => {
        orbitLogWarn('OrbitHaptic', 'Preset haptic failed', err);
      });
      return;
    }
    vibrator.startVibration(
      { type: 'time', duration: Math.max(5, Math.min(durationMs, 30)) },
      { id: 0, usage: 'touch' }
    ).catch((err: Object) => {
      orbitLogWarn('OrbitHaptic', 'Time haptic failed', err);
    });
  } catch (err) {
    orbitLogWarn('OrbitHaptic', 'Haptic unavailable', err as Object);
  }
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
    const data = await pb.getData();
    return data.getPrimaryText().trim();
  } catch (err) {
    orbitLogWarn('OrbitClipboard', 'Failed to read pasteboard', err as Object);
    return '';
  }
}

export async function scanPairingQrCode(context: common.Context | undefined): Promise<ScanQrOutcome> {
  if (!canIUse('SystemCapability.Multimedia.Scan.ScanBarcode') ||
    !canIUse('SystemCapability.Multimedia.Scan.Core')) {
    return {
      uri: '',
      cancelled: false,
      errorMessage: 'Camera QR scanning is not available on this device. Paste an orbit://pair URI instead.'
    };
  }
  if (context === undefined) {
    return {
      uri: '',
      cancelled: false,
      errorMessage: 'Scanner requires an active ability context.'
    };
  }
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

export async function publishOrbitDeepLinkNotification(
  context: common.Context | undefined,
  notificationId: number,
  title: string,
  body: string,
  hostId: string,
  agentId: string
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
    const deepLinkUri = `orbit://open?host=${encodeURIComponent(hostId)}&agent=${encodeURIComponent(agentId)}`;
    const targetWant: Want = {
      bundleName: bundleName,
      abilityName: abilityName,
      uri: deepLinkUri,
      parameters: {
        'orbit_open_uri': deepLinkUri,
        'orbit_host_id': hostId,
        'orbit_agent_id': agentId
      }
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
