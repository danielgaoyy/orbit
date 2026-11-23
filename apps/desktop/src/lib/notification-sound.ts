/**
 * Restrained pure-WebAudio soft notification chime for Orbit Desktop.
 *
 * Synthesizes a subtle two-note harmonic bell without bundled audio files or external packages.
 * Safely degrades to a no-op in headless/Node environments or when AudioContext is blocked.
 */

export type NotificationSoundTone = "default" | "approval" | "error";

interface AudioContextLike {
  state?: string;
  currentTime: number;
  destination: unknown;
  resume?: () => Promise<void> | void;
  createOscillator: () => {
    type: string;
    frequency: { setValueAtTime: (value: number, startTime: number) => void };
    connect: (dest: unknown) => void;
    start: (when?: number) => void;
    stop: (when?: number) => void;
  };
  createGain: () => {
    gain: {
      setValueAtTime: (value: number, startTime: number) => void;
      linearRampToValueAtTime: (value: number, endTime: number) => void;
      exponentialRampToValueAtTime: (value: number, endTime: number) => void;
    };
    connect: (dest: unknown) => void;
  };
}

type AudioContextCtor = new () => AudioContextLike;

interface NoteSpec {
  freqHz: number;
  delaySec: number;
  durationSec: number;
  peakGain: number;
}

const TONE_NOTES: Record<NotificationSoundTone, readonly NoteSpec[]> = {
  // Subtle ascending fifth (E5 -> B5)
  default: [
    { freqHz: 659.25, delaySec: 0, durationSec: 0.24, peakGain: 0.055 },
    { freqHz: 987.77, delaySec: 0.085, durationSec: 0.34, peakGain: 0.045 },
  ],
  // Warm gentle attention interval (F5 -> A5)
  approval: [
    { freqHz: 698.46, delaySec: 0, durationSec: 0.24, peakGain: 0.06 },
    { freqHz: 880.0, delaySec: 0.095, durationSec: 0.36, peakGain: 0.055 },
  ],
  // Soft descending minor third (E5 -> C#5)
  error: [
    { freqHz: 659.25, delaySec: 0, durationSec: 0.22, peakGain: 0.05 },
    { freqHz: 554.37, delaySec: 0.1, durationSec: 0.32, peakGain: 0.045 },
  ],
};

let sharedContext: AudioContextLike | null = null;

function resolveAudioContextCtor(): AudioContextCtor | null {
  if (typeof globalThis === "undefined") return null;
  const g = globalThis as unknown as {
    AudioContext?: AudioContextCtor;
    webkitAudioContext?: AudioContextCtor;
  };
  return g.AudioContext ?? g.webkitAudioContext ?? null;
}

function getOrCreateAudioContext(ctorOverride?: AudioContextCtor | null): AudioContextLike | null {
  if (ctorOverride !== undefined) {
    if (!ctorOverride) return null;
    try {
      return new ctorOverride();
    } catch {
      return null;
    }
  }
  if (sharedContext) return sharedContext;
  const Ctor = resolveAudioContextCtor();
  if (!Ctor) return null;
  try {
    sharedContext = new Ctor();
    return sharedContext;
  } catch {
    return null;
  }
}

/**
 * Play a soft, restrained synthesized chime.
 * Returns `true` when audio nodes were scheduled cleanly, or `false` if WebAudio is unavailable.
 */
export function playSoftNotificationChime(
  tone: NotificationSoundTone = "default",
  opts?: { audioContextCtor?: AudioContextCtor | null },
): boolean {
  const ctx = getOrCreateAudioContext(opts?.audioContextCtor);
  if (!ctx) return false;

  try {
    if (ctx.state === "suspended" && typeof ctx.resume === "function") {
      const resumed = ctx.resume();
      if (resumed && typeof (resumed as Promise<void>).catch === "function") {
        void (resumed as Promise<void>).catch(() => undefined);
      }
    }

    const baseTime = ctx.currentTime;
    const notes = TONE_NOTES[tone] ?? TONE_NOTES.default;

    for (const note of notes) {
      const start = baseTime + note.delaySec;
      const end = start + note.durationSec;

      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.setValueAtTime(note.freqHz, start);

      gain.gain.setValueAtTime(0.0001, start);
      gain.gain.linearRampToValueAtTime(note.peakGain, start + 0.012);
      gain.gain.exponentialRampToValueAtTime(0.0001, end);

      osc.connect(gain);
      gain.connect(ctx.destination);
      osc.start(start);
      osc.stop(end + 0.015);
    }

    return ctx.state !== "closed";
  } catch {
    return false;
  }
}

export function resetNotificationAudioContextForTests(): void {
  sharedContext = null;
}
