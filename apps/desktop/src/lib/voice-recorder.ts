import { orbitIpc, parseVoiceRpcError } from "@/lib/orbit-ipc";
import {
  TARGET_VOICE_SAMPLE_RATE,
  encodePcm16LeBase64,
  resampleMonoFloat32,
} from "@/lib/voice-audio-pcm";

export { TARGET_VOICE_SAMPLE_RATE, encodePcm16LeBase64, resampleMonoFloat32 };

export interface VoiceRecordingPayload {
  pcm16LeBase64: string;
  sampleRateHz: number;
  durationMs: number;
  browserTranscript: string;
}

interface SpeechRecognitionAlternativeLike {
  transcript: string;
}

interface SpeechRecognitionResultLike {
  isFinal: boolean;
  length: number;
  [index: number]: SpeechRecognitionAlternativeLike | undefined;
}

interface SpeechRecognitionEventLike {
  resultIndex: number;
  results: {
    length: number;
    [index: number]: SpeechRecognitionResultLike | undefined;
  };
}

interface SpeechRecognitionLike {
  lang: string;
  continuous: boolean;
  interimResults: boolean;
  maxAlternatives: number;
  onresult: ((ev: SpeechRecognitionEventLike) => void) | null;
  onerror: ((ev: { error?: string }) => void) | null;
  onend: (() => void) | null;
  start(): void;
  stop(): void;
  abort(): void;
}

type SpeechRecognitionCtor = new () => SpeechRecognitionLike;

function getSpeechRecognitionCtor(): SpeechRecognitionCtor | null {
  if (typeof window === "undefined") return null;
  const w = window as unknown as {
    SpeechRecognition?: SpeechRecognitionCtor;
    webkitSpeechRecognition?: SpeechRecognitionCtor;
  };
  return w.SpeechRecognition ?? w.webkitSpeechRecognition ?? null;
}

export function isBrowserSpeechRecognitionAvailable(): boolean {
  return getSpeechRecognitionCtor() !== null;
}

export function mapVoiceLanguageToBcp47(language: string | null | undefined): string {
  switch (language) {
    case "zh":
      return "zh-CN";
    case "ja":
      return "ja-JP";
    case "en":
      return "en-US";
    default:
      if (typeof navigator !== "undefined" && navigator.language) {
        return navigator.language;
      }
      return "en-US";
  }
}

function decodeBase64ToBytes(b64: string): Uint8Array {
  const binary = atob(b64);
  const bytes = new Uint8Array(binary.length);
  for (let i = 0; i < binary.length; i += 1) {
    bytes[i] = binary.charCodeAt(i);
  }
  return bytes;
}

export interface AudioInputDeviceInfo {
  deviceId: string;
  label: string;
}

export async function listAudioInputDevices(): Promise<{
  devices: AudioInputDeviceInfo[];
  needsPermission: boolean;
  supported: boolean;
}> {
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.enumerateDevices) {
    return { devices: [], needsPermission: false, supported: false };
  }
  const all = await navigator.mediaDevices.enumerateDevices();
  const seen = new Set<string>();
  const devices: AudioInputDeviceInfo[] = [];
  let hasUnlabeledInput = false;

  for (const dev of all) {
    if (dev.kind !== "audioinput") continue;
    const id = dev.deviceId?.trim() ?? "";
    if (!dev.label.trim()) hasUnlabeledInput = true;
    if (!id || id === "default" || id === "communications" || seen.has(id)) continue;
    seen.add(id);
    devices.push({ deviceId: id, label: dev.label.trim() });
  }

  return {
    devices,
    needsPermission: hasUnlabeledInput && devices.every((d) => !d.label),
    supported: true,
  };
}

export async function requestMicrophoneLabels(): Promise<{
  devices: AudioInputDeviceInfo[];
  granted: boolean;
}> {
  if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) {
    return { devices: [], granted: false };
  }
  let stream: MediaStream | null = null;
  try {
    stream = await navigator.mediaDevices.getUserMedia({ audio: true });
  } catch {
    return { devices: [], granted: false };
  } finally {
    if (stream) {
      for (const track of stream.getTracks()) {
        try {
          track.stop();
        } catch {
          // Ignore track stop errors
        }
      }
    }
  }
  const listed = await listAudioInputDevices();
  return { devices: listed.devices, granted: true };
}

export class VoiceRecorderSession {
  private stream: MediaStream | null = null;
  private audioCtx: AudioContext | null = null;
  private sourceNode: MediaStreamAudioSourceNode | null = null;
  private processorNode: ScriptProcessorNode | null = null;
  private silentGainNode: GainNode | null = null;
  private chunks: Float32Array[] = [];
  private totalSamples = 0;
  private nativeSampleRate = TARGET_VOICE_SAMPLE_RATE;
  private startedAtMs = 0;
  private recognition: SpeechRecognitionLike | null = null;
  private recognizedFinalParts: string[] = [];
  private recognizedInterim = "";
  private active = false;

  async start(options: {
    language?: string | null;
    deviceId?: string | null;
    onLevel?: (level: number) => void;
    onInterimTranscript?: (text: string) => void;
  }): Promise<void> {
    if (typeof navigator === "undefined" || !navigator.mediaDevices?.getUserMedia) {
      throw new Error("Microphone capture is unavailable in this environment");
    }
    const preferredDeviceId =
      options.deviceId && options.deviceId !== "default" ? options.deviceId : null;
    const stream = await navigator.mediaDevices.getUserMedia({
      audio: {
        ...(preferredDeviceId ? { deviceId: { ideal: preferredDeviceId } } : {}),
        channelCount: 1,
        echoCancellation: true,
        noiseSuppression: true,
        autoGainControl: true,
      },
    });
    const AudioCtx =
      window.AudioContext ||
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioCtx) {
      for (const track of stream.getTracks()) track.stop();
      throw new Error("Web Audio API is unavailable");
    }
    let audioCtx: AudioContext;
    try {
      audioCtx = new AudioCtx({ sampleRate: TARGET_VOICE_SAMPLE_RATE });
    } catch {
      audioCtx = new AudioCtx();
    }
    if (audioCtx.state === "suspended") {
      await audioCtx.resume().catch(() => {});
    }

    this.stream = stream;
    this.audioCtx = audioCtx;
    this.nativeSampleRate = audioCtx.sampleRate || TARGET_VOICE_SAMPLE_RATE;
    this.chunks = [];
    this.totalSamples = 0;
    this.startedAtMs = Date.now();
    this.recognizedFinalParts = [];
    this.recognizedInterim = "";
    this.active = true;

    const source = audioCtx.createMediaStreamSource(stream);
    const processor = audioCtx.createScriptProcessor(4096, 1, 1);
    const silentGain = audioCtx.createGain();
    silentGain.gain.value = 0;

    processor.onaudioprocess = (ev) => {
      if (!this.active) return;
      const input = ev.inputBuffer.getChannelData(0);
      const copy = new Float32Array(input.length);
      copy.set(input);
      this.chunks.push(copy);
      this.totalSamples += copy.length;

      if (options.onLevel) {
        let sumSq = 0;
        for (let i = 0; i < copy.length; i += 1) {
          const v = copy[i] ?? 0;
          sumSq += v * v;
        }
        const rms = Math.sqrt(sumSq / Math.max(1, copy.length));
        const normalized = Math.min(1, rms * 5.2);
        options.onLevel(normalized);
      }
    };

    source.connect(processor);
    processor.connect(silentGain);
    silentGain.connect(audioCtx.destination);
    this.sourceNode = source;
    this.processorNode = processor;
    this.silentGainNode = silentGain;

    // Best-effort parallel SpeechRecognition so system_client_fallback needs zero second recording.
    const RecognitionCtor = getSpeechRecognitionCtor();
    if (RecognitionCtor) {
      try {
        const rec = new RecognitionCtor();
        rec.lang = mapVoiceLanguageToBcp47(options.language);
        rec.continuous = true;
        rec.interimResults = true;
        rec.maxAlternatives = 1;
        rec.onresult = (ev) => {
          const finals: string[] = [];
          let interim = "";
          for (let i = 0; i < ev.results.length; i += 1) {
            const res = ev.results[i];
            const text = res?.[0]?.transcript?.trim() ?? "";
            if (!text) continue;
            if (res?.isFinal) finals.push(text);
            else interim = interim ? `${interim} ${text}` : text;
          }
          this.recognizedFinalParts = finals;
          this.recognizedInterim = interim;
          const combined = [...finals, interim].filter(Boolean).join(" ").trim();
          if (combined) options.onInterimTranscript?.(combined);
        };
        rec.onerror = () => {
          // Ignore parallel SpeechRecognition errors; host STT remains primary.
        };
        rec.start();
        this.recognition = rec;
      } catch {
        this.recognition = null;
      }
    }
  }

  async stop(): Promise<VoiceRecordingPayload> {
    this.active = false;
    const rec = this.recognition;
    this.recognition = null;
    if (rec) {
      await new Promise<void>((resolve) => {
        let done = false;
        const finish = () => {
          if (done) return;
          done = true;
          resolve();
        };
        const timer = window.setTimeout(finish, 260);
        rec.onend = () => {
          window.clearTimeout(timer);
          finish();
        };
        try {
          rec.stop();
        } catch {
          window.clearTimeout(timer);
          finish();
        }
      });
    }

    this.teardownAudio();

    const merged = new Float32Array(this.totalSamples);
    let offset = 0;
    for (const chunk of this.chunks) {
      merged.set(chunk, offset);
      offset += chunk.length;
    }
    this.chunks = [];
    this.totalSamples = 0;

    const resampled = resampleMonoFloat32(merged, this.nativeSampleRate, TARGET_VOICE_SAMPLE_RATE);
    const pcm16LeBase64 = encodePcm16LeBase64(resampled);
    const durationMs = Math.max(
      Date.now() - this.startedAtMs,
      Math.round((resampled.length / TARGET_VOICE_SAMPLE_RATE) * 1000),
    );
    const browserTranscript = [...this.recognizedFinalParts, this.recognizedInterim]
      .filter(Boolean)
      .join(" ")
      .trim();

    return {
      pcm16LeBase64,
      sampleRateHz: TARGET_VOICE_SAMPLE_RATE,
      durationMs,
      browserTranscript,
    };
  }

  cancel(): void {
    this.active = false;
    if (this.recognition) {
      try {
        this.recognition.abort();
      } catch {
        // Ignore
      }
      this.recognition = null;
    }
    this.chunks = [];
    this.totalSamples = 0;
    this.teardownAudio();
  }

  private teardownAudio(): void {
    try {
      this.processorNode?.disconnect();
      this.sourceNode?.disconnect();
      this.silentGainNode?.disconnect();
    } catch {
      // Ignore disconnect errors
    }
    this.processorNode = null;
    this.sourceNode = null;
    this.silentGainNode = null;
    if (this.stream) {
      for (const track of this.stream.getTracks()) {
        try {
          track.stop();
        } catch {
          // Ignore
        }
      }
      this.stream = null;
    }
    if (this.audioCtx) {
      void this.audioCtx.close().catch(() => {});
      this.audioCtx = null;
    }
  }
}

/**
 * One-shot Web Speech API fallback when daemon STT returned `system_client_fallback`
 * and parallel recognition captured no text.
 */
export function captureBrowserSpeechFallback(
  language?: string | null,
  timeoutMs = 7000,
): Promise<string> {
  const RecognitionCtor = getSpeechRecognitionCtor();
  if (!RecognitionCtor) return Promise.resolve("");
  return new Promise((resolve) => {
    const rec = new RecognitionCtor();
    let settled = false;
    let transcript = "";
    const finish = (value: string) => {
      if (settled) return;
      settled = true;
      window.clearTimeout(timer);
      resolve(value.trim());
    };
    const timer = window.setTimeout(() => {
      try {
        rec.stop();
      } catch {
        // Ignore
      }
      finish(transcript);
    }, timeoutMs);
    rec.lang = mapVoiceLanguageToBcp47(language);
    rec.continuous = false;
    rec.interimResults = false;
    rec.maxAlternatives = 1;
    rec.onresult = (ev) => {
      for (let i = ev.resultIndex; i < ev.results.length; i += 1) {
        const text = ev.results[i]?.[0]?.transcript?.trim();
        if (text) transcript = transcript ? `${transcript} ${text}` : text;
      }
    };
    rec.onerror = () => finish(transcript);
    rec.onend = () => finish(transcript);
    try {
      rec.start();
    } catch {
      finish("");
    }
  });
}

let activeVoiceAudio: HTMLAudioElement | null = null;
let activeVoiceAudioUrl: string | null = null;

export function stopActiveVoicePlayback(): void {
  if (activeVoiceAudio) {
    try {
      activeVoiceAudio.pause();
    } catch {
      // Ignore
    }
    activeVoiceAudio = null;
  }
  if (activeVoiceAudioUrl) {
    URL.revokeObjectURL(activeVoiceAudioUrl);
    activeVoiceAudioUrl = null;
  }
  if (typeof window !== "undefined" && "speechSynthesis" in window) {
    try {
      window.speechSynthesis.cancel();
    } catch {
      // Ignore
    }
  }
}

/**
 * Speak text using host TTS (`synthesizeVoiceSpeech`), playing WAV audio when returned,
 * respecting `voice_tts_disabled`, and falling back to `window.speechSynthesis` on
 * `voice_client_tts_required`.
 */
export async function speakVoiceText(
  hostId: string,
  text: string,
  options?: { language?: string | null; voiceId?: string | null },
): Promise<{ spoken: boolean; reason: "host_wav" | "client_tts" | "disabled" | "unavailable" }> {
  const trimmed = text.trim();
  if (!trimmed) return { spoken: false, reason: "unavailable" };

  stopActiveVoicePlayback();

  try {
    const res = await orbitIpc.synthesizeVoiceSpeech(hostId, trimmed, {
      language: options?.language ?? null,
      voiceId: options?.voiceId ?? null,
    });
    if (res.audio_wav_base64) {
      const bytes = decodeBase64ToBytes(res.audio_wav_base64);
      const blob = new Blob([bytes.buffer as ArrayBuffer], {
        type: res.mime_type || "audio/wav",
      });
      const url = URL.createObjectURL(blob);
      const audio = new Audio(url);
      activeVoiceAudio = audio;
      activeVoiceAudioUrl = url;
      const cleanup = () => {
        if (activeVoiceAudio === audio) activeVoiceAudio = null;
        if (activeVoiceAudioUrl === url) {
          URL.revokeObjectURL(url);
          activeVoiceAudioUrl = null;
        }
      };
      audio.onended = cleanup;
      audio.onerror = cleanup;
      await audio.play();
      return { spoken: true, reason: "host_wav" };
    }
  } catch (err) {
    const parsed = parseVoiceRpcError(err);
    if (parsed.code === "voice_tts_disabled") {
      return { spoken: false, reason: "disabled" };
    }
    if (parsed.code !== "voice_client_tts_required") {
      throw err;
    }
  }

  if (typeof window !== "undefined" && "speechSynthesis" in window) {
    const utterance = new SpeechSynthesisUtterance(trimmed.slice(0, 2000));
    utterance.lang = mapVoiceLanguageToBcp47(options?.language);
    window.speechSynthesis.speak(utterance);
    return { spoken: true, reason: "client_tts" };
  }
  return { spoken: false, reason: "unavailable" };
}
