import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  VOICE_DEVICE_LABEL_PREFIX,
  findMatchingWebDeviceId,
  mergeAudioInputDevices,
} from "./voice-devices.ts";

describe("voice audio input device reconciliation", () => {
  it("surfaces newly connected Bluetooth headsets from OS CoreAudio when WebKit cache is stale", () => {
    const merged = mergeAudioInputDevices(
      [{ deviceId: "web-builtin-1", label: "MacBook Pro麦克风" }],
      [
        { name: "MacBook Pro麦克风", is_default: false, transport: "builtin" },
        { name: "Xiaomi Buds 6", is_default: true, transport: "bluetooth" },
      ],
    );

    assert.equal(merged.defaultSystemDeviceName, "Xiaomi Buds 6");
    assert.equal(merged.hasMissingSystemDevice, true);
    assert.deepEqual(merged.devices, [
      { deviceId: "web-builtin-1", label: "MacBook Pro麦克风" },
      { deviceId: `${VOICE_DEVICE_LABEL_PREFIX}Xiaomi Buds 6`, label: "Xiaomi Buds 6" },
    ]);
  });

  it("deduplicates browser-decorated labels against macOS CoreAudio device names", () => {
    const merged = mergeAudioInputDevices(
      [
        { deviceId: "web-builtin-1", label: "MacBook Pro麦克风 (Built-in)" },
        { deviceId: "web-bt-2", label: "Xiaomi Buds 6 (Bluetooth)" },
      ],
      [
        { name: "MacBook Pro麦克风", is_default: false, transport: "builtin" },
        { name: "Xiaomi Buds 6", is_default: true, transport: "bluetooth" },
      ],
    );

    assert.equal(merged.defaultSystemDeviceName, "Xiaomi Buds 6");
    assert.equal(merged.hasMissingSystemDevice, false);
    assert.equal(merged.devices.length, 2);
  });

  it("resolves label-prefixed device selections to live WebView deviceIds", () => {
    const webDevices = [
      { deviceId: "web-builtin-1", label: "MacBook Pro麦克风" },
      { deviceId: "web-bt-2", label: "Xiaomi Buds 6 (Bluetooth)" },
    ];
    assert.equal(
      findMatchingWebDeviceId(webDevices, `${VOICE_DEVICE_LABEL_PREFIX}Xiaomi Buds 6`),
      "web-bt-2",
    );
    assert.equal(findMatchingWebDeviceId(webDevices, "web-builtin-1"), "web-builtin-1");
    assert.equal(findMatchingWebDeviceId(webDevices, "default"), null);
  });
});
