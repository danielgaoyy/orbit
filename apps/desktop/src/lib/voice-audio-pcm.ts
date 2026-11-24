export const TARGET_VOICE_SAMPLE_RATE = 16000;

export interface ParsedVoiceRpcError {
  code: string | null;
  message: string;
}

export function parseVoiceRpcError(err: unknown): ParsedVoiceRpcError {
  if (err && typeof err === "object") {
    const rec = err as { code?: unknown; message?: unknown };
    if (typeof rec.code === "string" && rec.code.length > 0) {
      return {
        code: rec.code,
        message: typeof rec.message === "string" ? rec.message : String(err),
      };
    }
  }
  const raw = err instanceof Error ? err.message : String(err ?? "");
  const trimmed = raw.trim();
  if (trimmed.startsWith("{") && trimmed.endsWith("}")) {
    try {
      const parsed = JSON.parse(trimmed) as { code?: unknown; message?: unknown };
      if (typeof parsed.code === "string" && parsed.code.length > 0) {
        return {
          code: parsed.code,
          message: typeof parsed.message === "string" ? parsed.message : raw,
        };
      }
    } catch {
      // Fall through to bracket parsing
    }
  }
  const bracketMatch = trimmed.match(/^\[([a-z0-9_]+)\]\s*(.*)$/i);
  if (bracketMatch) {
    return {
      code: bracketMatch[1],
      message: bracketMatch[2] || raw,
    };
  }
  return { code: null, message: raw };
}

/**
 * Linearly resample a mono Float32 buffer to `targetRate` Hz.
 */
export function resampleMonoFloat32(
  samples: Float32Array,
  fromRate: number,
  targetRate: number = TARGET_VOICE_SAMPLE_RATE,
): Float32Array {
  if (samples.length === 0 || fromRate <= 0 || targetRate <= 0) {
    return new Float32Array(0);
  }
  if (fromRate === targetRate) {
    return new Float32Array(samples);
  }
  const ratio = fromRate / targetRate;
  const outLength = Math.max(1, Math.round(samples.length / ratio));
  const out = new Float32Array(outLength);
  for (let i = 0; i < outLength; i += 1) {
    const srcPos = i * ratio;
    const idx = Math.floor(srcPos);
    const frac = srcPos - idx;
    const s0 = samples[Math.min(idx, samples.length - 1)] ?? 0;
    const s1 = samples[Math.min(idx + 1, samples.length - 1)] ?? s0;
    out[i] = s0 + (s1 - s0) * frac;
  }
  return out;
}

/**
 * Encode mono Float32 audio (`[-1.0, 1.0]`) to little-endian signed 16-bit PCM base64.
 */
export function encodePcm16LeBase64(samples: Float32Array): string {
  const bytes = new Uint8Array(samples.length * 2);
  const view = new DataView(bytes.buffer);
  for (let i = 0; i < samples.length; i += 1) {
    const clamped = Math.max(-1, Math.min(1, samples[i] ?? 0));
    const int16 = clamped < 0 ? Math.round(clamped * 0x8000) : Math.round(clamped * 0x7fff);
    view.setInt16(i * 2, int16, true);
  }
  let binary = "";
  const chunkSize = 0x2000;
  for (let i = 0; i < bytes.length; i += chunkSize) {
    const sub = bytes.subarray(i, Math.min(i + chunkSize, bytes.length));
    binary += String.fromCharCode(...sub);
  }
  if (typeof btoa === "function") {
    return btoa(binary);
  }
  return Buffer.from(bytes).toString("base64");
}
