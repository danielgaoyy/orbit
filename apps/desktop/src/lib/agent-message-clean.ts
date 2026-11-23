function unescapeBasicXml(text: string): string {
  return text
    .replace(/&lt;/g, "<")
    .replace(/&gt;/g, ">")
    .replace(/&quot;/g, '"')
    .replace(/&apos;/g, "'")
    .replace(/&amp;/g, "&");
}

const AGENT_HEADER_LINE_RE =
  /^Agent\s+\S+(?:\s+\(.*\))?\s+(?:finished|completed|failed|canceled|cancelled|stopped|errored|exited|needs\s+permission)\.?\s*$/i;

const AGENT_HEADER_INLINE_PREFIX_RE =
  /^Agent\s+\S+(?:\s+\(.*?\))?\s+(?:finished|completed|failed|canceled|cancelled|stopped|errored|exited|needs\s+permission)\.\s*/i;

const PERMISSION_HINT_LINE_RE =
  /^Respond\s+with\s+`?respond_to_permission`?.*$/i;

const TRUNCATION_FOOTER_RE =
  /\n?\[truncated\s+\d+\s+chars;\s*use\s+get_agent_activity[^\]]*\]\s*$/i;

/**
 * Strips Orbit internal envelope / notification boilerplate (`<orbit-system ...>`,
 * `Agent ag_... (...) finished.`, `Host: ...`, `<agent-response>`, `<permission-request>`,
 * and internal truncation hints) so stage cards & side-panel previews show only
 * meaningful human-readable summary content.
 */
export function extractCleanAgentMessageBody(raw: string): string {
  if (!raw) return "";
  let text = raw.replace(/\r\n?/g, "\n").trim();
  if (!text) return "";

  // 1. Unwrap `<orbit-system ...>...</orbit-system>` envelope if present.
  let unwrappedOrbitEnvelope = false;
  const fullEnvelopeMatch = /^<orbit-system\b[^>]*>\s*\n?([\s\S]*?)\n?\s*<\/orbit-system>$/i.exec(
    text,
  );
  if (fullEnvelopeMatch) {
    text = unescapeBasicXml(fullEnvelopeMatch[1] ?? "").trim();
    unwrappedOrbitEnvelope = true;
  } else if (/^<orbit-system\b/i.test(text)) {
    text = text
      .replace(/^<orbit-system\b[^>]*>\s*/i, "")
      .replace(/\s*<\/orbit-system>\s*$/i, "");
    text = unescapeBasicXml(text).trim();
    unwrappedOrbitEnvelope = true;
  }

  // Strip any stray outer `<orbit-system>` tags that survived partial truncation.
  if (!unwrappedOrbitEnvelope && /<\/?orbit-system\b/i.test(text)) {
    text = text
      .replace(/<orbit-system\b[^>]*>/gi, "")
      .replace(/<\/orbit-system>/gi, "")
      .trim();
  }

  // 2. Remove structured `<permission-request>...</permission-request>` JSON blocks
  //    (rendered natively by the inline Allow/Deny checkpoint bar).
  text = text
    .replace(/<permission-request\b[^>]*>[\s\S]*?<\/permission-request>/gi, "")
    .replace(/<permission-request\b[^>]*>[\s\S]*$/gi, "")
    .trim();

  // 3. If `<agent-response>` is present, preserve any preceding `Error: ...` lines
  //    and extract the response payload directly.
  const openResponseMatch = /<agent-response\b[^>]*>/i.exec(text);
  if (openResponseMatch) {
    const before = text.slice(0, openResponseMatch.index);
    const afterOpen = text.slice(openResponseMatch.index + openResponseMatch[0].length);
    const closeIdx = afterOpen.search(/<\/agent-response>/i);
    const responseInner = (closeIdx >= 0 ? afterOpen.slice(0, closeIdx) : afterOpen).trim();

    const errorLines = before
      .split("\n")
      .map((line) => line.trim())
      .filter((line) => /^Error:\s+\S/i.test(line));

    const parts: string[] = [];
    if (errorLines.length > 0) {
      parts.push(errorLines.join("\n"));
    }
    if (responseInner.length > 0) {
      parts.push(responseInner);
    }
    text = parts.join("\n\n").trim();
  } else {
    // Strip any orphaned closing tag.
    text = text.replace(/<\/agent-response>/gi, "").trim();

    // Strip leading notification header lines (`Agent ag_... (...) finished.`, `Host: ...`).
    const lines = text.split("\n");
    let cursor = 0;
    let strippedHeader = false;

    while (cursor < lines.length) {
      const trimmed = lines[cursor].trim();
      if (!trimmed) {
        cursor += 1;
        continue;
      }
      if (!strippedHeader && AGENT_HEADER_LINE_RE.test(trimmed)) {
        strippedHeader = true;
        cursor += 1;
        continue;
      }
      if (strippedHeader && /^Host:\s+\S+$/i.test(trimmed)) {
        cursor += 1;
        continue;
      }
      if (strippedHeader && PERMISSION_HINT_LINE_RE.test(trimmed)) {
        cursor += 1;
        continue;
      }
      break;
    }

    text = lines.slice(cursor).join("\n").trim();

    if (!strippedHeader && AGENT_HEADER_INLINE_PREFIX_RE.test(text)) {
      text = text.replace(AGENT_HEADER_INLINE_PREFIX_RE, "").trim();
    }
  }

  // 4. Clean internal truncation instructions pointing at MCP tool names.
  if (TRUNCATION_FOOTER_RE.test(text)) {
    const withoutFooter = text.replace(TRUNCATION_FOOTER_RE, "").trimEnd();
    text =
      withoutFooter.length > 0 && !/(?:\.{3}|…)$/.test(withoutFooter)
        ? `${withoutFooter}…`
        : withoutFooter;
  }

  return text.replace(/\n{3,}/g, "\n\n").trim();
}

/**
 * Formats cleaned message body into a concise single-line preview for collapsed
 * disclosure headers and side-panel tab subtitles.
 */
export function formatCleanAgentMessagePreview(cleanBody: string): string {
  if (!cleanBody) return "";
  const firstMeaningfulLine = cleanBody
    .split("\n")
    .map((line) => line.trim())
    .find((line) => line.length > 0 && !/^[-*_]{3,}$/.test(line));
  if (!firstMeaningfulLine) return "";

  return firstMeaningfulLine
    .replace(/^#{1,6}\s+/, "")
    .replace(/^[-*+]\s+/, "")
    .replace(/^\d+\.\s+/, "")
    .replace(/\*\*([^*]+)\*\*/g, "$1")
    .replace(/`([^`]+)`/g, "$1")
    .replace(/\s+/g, " ")
    .trim();
}
