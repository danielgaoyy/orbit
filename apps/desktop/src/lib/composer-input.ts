/**
 * Inspect the token to the left of the caret.
 * - `@` mentions activate at start-of-input or after any whitespace (mid-sentence `@file` / `@peer`).
 * - `/` slash commands activate ONLY at the start of input (allowing leading whitespace),
 *   matching Claude Code / Antigravity so mid-sentence `/goal` or Unix paths never open or
 *   hijack the composer.
 */
export function detectActiveTriggerKind(text: string, cursorPos: number): "slash" | "mention" | null {
  const end = Math.min(cursorPos, text.length);
  let hasSlash = false;
  let hasAt = false;
  for (let i = end - 1; i >= 0; i--) {
    const char = text[i];
    if (/\s/.test(char)) break;
    if (i === 0 || /\s/.test(text[i - 1])) {
      if (char === "/" && !hasSlash) {
        return !/\S/.test(text.slice(0, i)) ? "slash" : null;
      }
      if (char === "@" && !hasAt) return "mention";
      break;
    }
    hasSlash ||= char === "/";
    hasAt ||= char === "@";
  }
  return null;
}

export interface CappedTextarea {
  prevText: string;
  width: number;
  indent: number;
  scopeKey: string;
}

export type ComposerActionMode =
  | "stop"
  | "voice_recording"
  | "submitting"
  | "send"
  | "send_disabled"
  | "voice_idle";

export function resolveComposerActionMode(opts: {
  voiceRecording: boolean;
  showStopButton: boolean;
  submitBusy: boolean;
  voiceTranscribing: boolean;
  hasPayload: boolean;
  voiceEnabled: boolean;
}): ComposerActionMode {
  if (opts.voiceRecording) return "voice_recording";
  if (opts.showStopButton) return "stop";
  if (opts.submitBusy || opts.voiceTranscribing) return "submitting";
  if (opts.hasPayload) return "send";
  return opts.voiceEnabled ? "voice_idle" : "send_disabled";
}

export function canSkipCappedMeasurement(
  capped: CappedTextarea | null,
  text: string,
  width: number,
  indent: number,
  scopeKey: string,
): boolean {
  return (
    capped !== null &&
    capped.width === width &&
    capped.indent === indent &&
    capped.scopeKey === scopeKey &&
    text.length >= capped.prevText.length &&
    text.startsWith(capped.prevText)
  );
}
