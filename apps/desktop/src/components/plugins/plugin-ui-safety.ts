export interface SafeMarkdownInlineToken {
  kind: "text" | "code" | "strong" | "link";
  text: string;
  href?: string;
}

/**
 * Strictly validate external URLs inside plugin UI blocks.
 * Only `https://` and `http://` schemes are permitted.
 */
export function isSafePluginUrl(raw: string): boolean {
  const trimmed = raw.trim();
  if (!trimmed || trimmed.length > 2048) return false;
  try {
    const parsed = new URL(trimmed);
    return parsed.protocol === "https:" || parsed.protocol === "http:";
  } catch {
    return false;
  }
}

/**
 * Pure, inert inline Markdown tokenizer (zero HTML parsing, zero `dangerouslySetInnerHTML`).
 */
export function tokenizeSafePluginMarkdownLine(line: string): SafeMarkdownInlineToken[] {
  const tokens: SafeMarkdownInlineToken[] = [];
  const pattern = /(`[^`]+`|\*\*[^*]+\*\*|\[([^\]]+)\]\(((?:[^()\s]+|\([^()\s]*\))+)\))/g;
  let lastIndex = 0;
  for (const match of line.matchAll(pattern)) {
    const idx = match.index ?? 0;
    if (idx > lastIndex) {
      tokens.push({ kind: "text", text: line.slice(lastIndex, idx) });
    }
    const full = match[0];
    if (full.startsWith("`") && full.endsWith("`")) {
      tokens.push({ kind: "code", text: full.slice(1, -1) });
    } else if (full.startsWith("**") && full.endsWith("**")) {
      tokens.push({ kind: "strong", text: full.slice(2, -2) });
    } else if (match[2] !== undefined && match[3] !== undefined) {
      const label = match[2];
      const url = match[3];
      if (isSafePluginUrl(url)) {
        tokens.push({ kind: "link", text: label, href: url });
      } else {
        tokens.push({ kind: "text", text: label });
      }
    }
    lastIndex = idx + full.length;
  }
  if (lastIndex < line.length) {
    tokens.push({ kind: "text", text: line.slice(lastIndex) });
  }
  return tokens;
}
