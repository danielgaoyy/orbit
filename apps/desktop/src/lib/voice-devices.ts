export interface AudioInputDeviceInfo {
  deviceId: string;
  label: string;
}

export interface SystemAudioInputLike {
  name: string;
  is_default: boolean;
  transport?: string | null;
}

export const VOICE_DEVICE_LABEL_PREFIX = "label:";

export function normalizeAudioDeviceName(name: string): string {
  return name.trim().toLowerCase().replace(/\s+/g, " ");
}

export function audioDeviceLabelsMatch(webLabel: string, systemName: string): boolean {
  const web = normalizeAudioDeviceName(webLabel);
  const sys = normalizeAudioDeviceName(systemName);
  if (!web || !sys) return false;
  return (
    web === sys ||
    web.startsWith(`${sys} (`) ||
    sys.startsWith(`${web} (`) ||
    web.includes(sys) ||
    sys.includes(web)
  );
}

export function parseLabelPrefixedDeviceId(deviceId: string | null | undefined): string | null {
  if (!deviceId || !deviceId.startsWith(VOICE_DEVICE_LABEL_PREFIX)) return null;
  const label = deviceId.slice(VOICE_DEVICE_LABEL_PREFIX.length).trim();
  return label.length > 0 ? label : null;
}

export function findMatchingWebDeviceId(
  webDevices: readonly AudioInputDeviceInfo[],
  preferredDeviceId: string | null | undefined,
): string | null {
  const trimmed = preferredDeviceId?.trim() ?? "";
  if (!trimmed || trimmed === "default") return null;

  const direct = webDevices.find((dev) => dev.deviceId === trimmed);
  if (direct) return direct.deviceId;

  const targetLabel = parseLabelPrefixedDeviceId(trimmed);
  if (!targetLabel) return null;

  const byLabel = webDevices.find((dev) => audioDeviceLabelsMatch(dev.label, targetLabel));
  return byLabel?.deviceId ?? null;
}

export function mergeAudioInputDevices(
  webDevices: readonly AudioInputDeviceInfo[],
  systemDevices: readonly SystemAudioInputLike[],
): {
  devices: AudioInputDeviceInfo[];
  defaultSystemDeviceName: string | null;
  hasMissingSystemDevice: boolean;
} {
  const devices: AudioInputDeviceInfo[] = [];
  const seenIds = new Set<string>();
  const webLabels: string[] = [];

  for (const dev of webDevices) {
    const id = dev.deviceId.trim();
    if (!id || seenIds.has(id)) continue;
    seenIds.add(id);
    const label = dev.label.trim();
    if (label) webLabels.push(label);
    devices.push({ deviceId: id, label });
  }

  let defaultSystemDeviceName: string | null = null;
  let hasMissingSystemDevice = false;
  const seenSystemKeys = new Set<string>();

  for (const sys of systemDevices) {
    const name = sys.name.trim();
    if (!name) continue;
    if (sys.is_default && !defaultSystemDeviceName) {
      defaultSystemDeviceName = name;
    }
    const key = normalizeAudioDeviceName(name);
    if (seenSystemKeys.has(key)) continue;
    seenSystemKeys.add(key);

    const matchedInWeb = webLabels.some((label) => audioDeviceLabelsMatch(label, name));
    if (!matchedInWeb) {
      hasMissingSystemDevice = true;
      devices.push({
        deviceId: `${VOICE_DEVICE_LABEL_PREFIX}${name}`,
        label: name,
      });
    }
  }

  return {
    devices,
    defaultSystemDeviceName,
    hasMissingSystemDevice,
  };
}
