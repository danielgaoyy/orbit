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

const LEADING_PROTOCOL_TAGS: readonly [string, string][] = [
  ["orbit-context", "</orbit-context>"],
  ["orbit-workspace", "</orbit-workspace>"],
  ["orbit-workspace-delta", "</orbit-workspace-delta>"],
  ["orbit-fleet-state", "</orbit-fleet-state>"],
  ["orbit-fleet-delta", "</orbit-fleet-delta>"],
  ["WORKSPACE_RULES", "</WORKSPACE_RULES>"],
  ["WORKFLOW_INSTRUCTION", "</WORKFLOW_INSTRUCTION>"],
  ["ATTACHED_CONTEXT", "</ATTACHED_CONTEXT>"],
];

const SYSTEM_NOISE_EVENTS = new Set([
  "finished",
  "completed",
  "canceled",
  "cancelled",
  "stopped",
  "heartbeat",
  "finish_notification",
]);

export interface ParsedOrbitSystemEnvelope {
  kind: string | null;
  sourceAgentId: string | null;
  sourceHostId: string | null;
  sourceTitle: string | null;
  event: string | null;
  messageId: string | null;
  body: string;
}

export interface DirectAgentMessagePresentation {
  cleanText: string;
  isSystemNoise: boolean;
  strippedEnvelope: boolean;
  sourceAgentId: string | null;
  sourceHostId: string | null;
  sourceTitle: string | null;
  event: string | null;
}

/**
 * Strips leading Orbit internal context/fleet protocol envelopes (`<orbit-fleet-state>`,
 * `<orbit-fleet-delta>`, `<orbit-workspace>`, `<orbit-workspace-delta>`, `<orbit-context>`,
 * `<WORKSPACE_RULES>`, `<WORKFLOW_INSTRUCTION>`, `<ATTACHED_CONTEXT>`) while preserving
 * mentions of those tags inside actual user/agent prose.
 */
export function stripLeadingOrbitProtocolBlocks(raw: string): {
  text: string;
  stripped: boolean;
} {
  let rest = raw.replace(/\r\n?/g, "\n").trim();
  let stripped = false;
  while (rest.startsWith("<")) {
    let matched = false;
    for (const [tagName, closeTag] of LEADING_PROTOCOL_TAGS) {
      if (!rest.startsWith(`<${tagName}`)) continue;
      const afterName = rest[tagName.length + 1];
      if (afterName !== ">" && afterName !== " " && afterName !== "\n" && afterName !== "\t") {
        continue;
      }
      const closeIdx = rest.indexOf(closeTag);
      if (closeIdx >= 0) {
        rest = rest.slice(closeIdx + closeTag.length).trimStart();
      } else {
        // Unclosed trailing pure-protocol block (e.g. truncated fleet state/delta)
        rest = "";
      }
      stripped = true;
      matched = true;
      break;
    }
    if (!matched) break;
  }

  const userReqMatch = /^<USER_REQUEST>\s*([\s\S]*?)\s*<\/USER_REQUEST>$/.exec(rest);
  if (userReqMatch) {
    rest = (userReqMatch[1] ?? "").trim();
    stripped = true;
  }

  return { text: rest.trim(), stripped };
}

function parseOrbitSystemAttrs(attrText: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  const attrRe = /([a-zA-Z0-9_-]+)\s*=\s*"([^"]*)"/g;
  let match: RegExpExecArray | null;
  while ((match = attrRe.exec(attrText)) !== null) {
    const key = match[1];
    const val = unescapeBasicXml(match[2] ?? "");
    if (key) attrs[key] = val;
  }
  return attrs;
}

/**
 * Parses a leading `<orbit-system ...>...</orbit-system>` envelope, extracting source
 * metadata and XML-unescaping the readable inner body.
 */
export function parseOrbitSystemEnvelope(raw: string): ParsedOrbitSystemEnvelope | null {
  if (!raw) return null;
  const { text: afterLeading } = stripLeadingOrbitProtocolBlocks(raw);
  const openMatch = /^<orbit-system(?:\s+([^>]*))?>/i.exec(afterLeading);
  if (!openMatch) return null;

  const attrs = parseOrbitSystemAttrs(openMatch[1] ?? "");
  let inner = afterLeading.slice(openMatch[0].length);
  inner = inner.replace(/\s*<\/orbit-system>\s*$/i, "");
  const unescaped = unescapeBasicXml(inner).trim();
  const { text: cleanBody } = stripLeadingOrbitProtocolBlocks(unescaped);

  return {
    kind: attrs.kind ?? null,
    sourceAgentId: attrs["source-agent-id"] || null,
    sourceHostId: attrs["source-host-id"] || null,
    sourceTitle: attrs["source-agent-title"] || null,
    event: attrs.event || null,
    messageId: attrs["message-id"] || null,
    body: cleanBody,
  };
}

/**
 * Cleans an incoming direct agent dispatch message (`create_agent`, `spawn_workspace_peer`,
 * `send_agent_prompt`, `message_peer`) for rendering inside the shared `UserPromptBubble`.
 *
 * Unwraps `<orbit-system>` envelopes, strips `<orbit-fleet-state>` / `<orbit-fleet-delta>`
 * and other leading Orbit protocol blocks, and flags pure system/control noise so protocol
 * envelopes are never rendered as chat bubbles.
 */
export function extractDirectAgentMessagePresentation(
  raw: string,
  meta?: {
    source_agent_id?: string | null;
    source_host_id?: string | null;
    source_title?: string | null;
    event?: string | null;
  },
): DirectAgentMessagePresentation {
  const baseSourceAgentId = meta?.source_agent_id ?? null;
  const baseSourceHostId = meta?.source_host_id ?? null;
  const baseSourceTitle = meta?.source_title ?? null;
  const baseEvent = meta?.event ?? null;

  if (!raw || !raw.trim()) {
    return {
      cleanText: "",
      isSystemNoise: true,
      strippedEnvelope: false,
      sourceAgentId: baseSourceAgentId,
      sourceHostId: baseSourceHostId,
      sourceTitle: baseSourceTitle,
      event: baseEvent,
    };
  }

  const envelope = parseOrbitSystemEnvelope(raw);
  const sourceAgentId = baseSourceAgentId || envelope?.sourceAgentId || null;
  const sourceHostId = baseSourceHostId || envelope?.sourceHostId || null;
  const sourceTitle = baseSourceTitle || envelope?.sourceTitle || null;
  const event = baseEvent || envelope?.event || null;

  const normalizedEvent = event?.trim().toLowerCase() ?? "";
  if (
    envelope?.kind === "agent-notification" ||
    envelope?.kind === "heartbeat" ||
    SYSTEM_NOISE_EVENTS.has(normalizedEvent)
  ) {
    return {
      cleanText: "",
      isSystemNoise: true,
      strippedEnvelope: true,
      sourceAgentId,
      sourceHostId,
      sourceTitle,
      event,
    };
  }

  let cleanText: string;
  let strippedEnvelope: boolean;
  if (envelope) {
    cleanText = envelope.body;
    strippedEnvelope = true;
  } else {
    const stripped = stripLeadingOrbitProtocolBlocks(raw);
    cleanText = stripped.text;
    strippedEnvelope = stripped.stripped;
  }

  // Strip stray structured permission-request XML if any survived outside permission_context
  if (/<permission-request\b/i.test(cleanText)) {
    cleanText = cleanText
      .replace(/<permission-request\b[^>]*>[\s\S]*?<\/permission-request>/gi, "")
      .replace(/<permission-request\b[^>]*>[\s\S]*$/gi, "")
      .trim();
    strippedEnvelope = true;
  }

  if (!cleanText) {
    return {
      cleanText: "",
      isSystemNoise: true,
      strippedEnvelope,
      sourceAgentId,
      sourceHostId,
      sourceTitle,
      event,
    };
  }

  return {
    cleanText,
    isSystemNoise: false,
    strippedEnvelope,
    sourceAgentId,
    sourceHostId,
    sourceTitle,
    event,
  };
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
