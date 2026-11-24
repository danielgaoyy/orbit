/** Only inspect the word to the left of the caret. The other trigger character is legal in a query. */
export function detectActiveTriggerKind(text: string, cursorPos: number): "slash" | "mention" | null {
  const end = Math.min(cursorPos, text.length);
  let hasSlash = false;
  let hasAt = false;
  for (let i = end - 1; i >= 0; i--) {
    const char = text[i];
    if (/\s/.test(char)) break;
    if (i === 0 || /\s/.test(text[i - 1])) {
      if (char === "/" && !hasSlash) return "slash";
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
