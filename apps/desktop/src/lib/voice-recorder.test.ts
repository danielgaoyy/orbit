import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  TARGET_VOICE_SAMPLE_RATE,
  encodePcm16LeBase64,
  parseVoiceRpcError,
  resampleMonoFloat32,
} from "./voice-audio-pcm.ts";

describe("voice-recorder audio helpers", () => {
  it("resamples 48kHz mono audio cleanly down to 16kHz", () => {
    const input = new Float32Array(480);
    for (let i = 0; i < input.length; i += 1) {
      input[i] = Math.sin((2 * Math.PI * 440 * i) / 48000);
    }
    const resampled = resampleMonoFloat32(input, 48000, TARGET_VOICE_SAMPLE_RATE);
    assert.equal(resampled.length, 160);
    assert.ok(resampled.every((s) => Number.isFinite(s) && s >= -1 && s <= 1));
  });

  it("encodes PCM16 little-endian samples to valid base64 with clamping", () => {
    const samples = new Float32Array([0, 1, -1, 1.5, -2.0]);
    const b64 = encodePcm16LeBase64(samples);
    const buf = Buffer.from(b64, "base64");
    assert.equal(buf.length, 10);
    assert.equal(buf.readInt16LE(0), 0);
    assert.equal(buf.readInt16LE(2), 32767);
    assert.equal(buf.readInt16LE(4), -32768);
    assert.equal(buf.readInt16LE(6), 32767);
    assert.equal(buf.readInt16LE(8), -32768);
  });

  it("parses structured JSON and bracketed voice RPC errors cleanly", () => {
    const fromJson = parseVoiceRpcError(
      new Error('{"code":"voice_client_tts_required","message":"use client speech"}'),
    );
    assert.deepEqual(fromJson, {
      code: "voice_client_tts_required",
      message: "use client speech",
    });

    const fromBracket = parseVoiceRpcError(
      new Error("[voice_tts_disabled] speech synthesis disabled"),
    );
    assert.deepEqual(fromBracket, {
      code: "voice_tts_disabled",
      message: "speech synthesis disabled",
    });

    const plain = parseVoiceRpcError("network timeout");
    assert.deepEqual(plain, { code: null, message: "network timeout" });
  });
});
