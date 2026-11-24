/**
 * Strict Sandboxed HTML CSP builder, SEP-1865 MCP Apps JSON-RPC 2.0 `postMessage` Host Bridge,
 * and safe CommonMark-aware inline artifact directive parser.
 *
 * Security boundaries:
 * - Iframe MUST use `sandbox="allow-scripts"` ONLY (no `allow-same-origin`, no `allow-top-navigation`, no `allow-popups`).
 * - Strict CSP (`default-src 'none'; base-uri 'none'; form-action 'none'; ...`) is emitted UNCONDITIONALLY at byte 0
 *   BEFORE any untrusted HTML bytes.
 * - Host bridge validates BOTH `event.source === expectedSource` AND `event.origin === "null"` (when present).
 * - `ui/open-link` accepts strictly `https://` URLs with no raw whitespace/control characters or embedded credentials,
 *   and defers its JSON-RPC reply until explicit user approval/cancellation.
 * - `tools/call`, `ui/open-link`, `ui/message`, and `ui/update-model-context` NEVER mutate host state silently and
 *   NEVER overwrite an already-open confirmation prompt (returns JSON-RPC error `-32002`).
 * - Payload caps (16 KiB text, 32 KiB JSON args) and per-bridge sliding-window rate limiting (20 req/s, coalescing
 *   `size-changed`) protect host IPC and UI responsiveness.
 */

export type SandboxTheme = "light" | "dark";

export const ARTIFACT_SANDBOX_CSP =
  "default-src 'none'; base-uri 'none'; form-action 'none'; frame-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data: blob:; font-src data:;";

export const SEP_1865_PROTOCOL_VERSION = "2026-01-26";

export const SANDBOX_MIN_HEIGHT_PX = 160;
export const SANDBOX_MAX_HEIGHT_PX = 1600;

export const MAX_BRIDGE_TEXT_BYTES = 16 * 1024;
export const MAX_BRIDGE_ARGS_BYTES = 32 * 1024;
export const DEFAULT_BRIDGE_MAX_REQ_PER_SEC = 20;
export const BRIDGE_RATE_WINDOW_MS = 1000;

export const JSON_RPC_ERR_INVALID_PARAMS = -32602;
export const JSON_RPC_ERR_METHOD_NOT_FOUND = -32601;
export const JSON_RPC_ERR_USER_DENIED = -32001;
export const JSON_RPC_ERR_CONFIRMATION_PENDING = -32002;
export const JSON_RPC_ERR_RATE_LIMITED = -32029;

export interface JsonRpcRequest {
  jsonrpc: "2.0";
  id?: string | number | null;
  method: string;
  params?: Record<string, unknown>;
}

export interface JsonRpcResponse {
  jsonrpc: "2.0";
  id: string | number | null;
  result?: unknown;
  error?: {
    code: number;
    message: string;
    data?: unknown;
  };
}

export interface JsonRpcNotification {
  jsonrpc: "2.0";
  method: string;
  params?: Record<string, unknown>;
}

export interface PendingToolCallRequest {
  id: string | number | null;
  name: string;
  arguments: Record<string, unknown>;
  argsPreview: string;
  navigationEpoch: number;
}

export interface PendingOpenLinkRequest {
  id: string | number | null;
  url: string;
}

export interface PendingMessageRequest {
  id: string | number | null;
  text: string;
}

export interface PendingModelContextRequest {
  id: string | number | null;
  note: string;
}

export interface PendingReadResourceRequest {
  id: string | number | null;
  uri: string;
  serverName: string;
  navigationEpoch: number;
}

export interface McpResourceReadResult {
  uri: string;
  mimeType?: string | null;
  text?: string | null;
  blob?: string | null;
}

export interface McpToolCallBridgeOutcome {
  ok: boolean;
  isError?: boolean;
  content?: unknown[];
  structuredContent?: unknown;
  error?: string | null;
}

export interface PostMessageTargetLike {
  postMessage: (message: unknown, targetOrigin: string) => void;
}

export interface MessageEventLike {
  source: unknown;
  origin?: string;
  data: unknown;
}

export interface McpAppBridgeCallbacks {
  getExpectedSource: () => PostMessageTargetLike | null;
  getTheme: () => SandboxTheme;
  getOwningMcpServerName?: () => string | null;
  now?: () => number;
  maxRequestsPerSecond?: number;
  onInitialized?: () => void;
  onSizeChanged?: (heightPx: number) => void;
  onOpenLinkRequest?: (req: PendingOpenLinkRequest) => void;
  onMessageRequest?: (req: PendingMessageRequest) => void;
  onUpdateModelContextRequest?: (req: PendingModelContextRequest) => void;
  onToolCallRequest?: (req: PendingToolCallRequest) => void;
  onReadResourceRequest?: (req: PendingReadResourceRequest) => void;
}

const utf8Encoder = typeof TextEncoder !== "undefined" ? new TextEncoder() : null;

export function utf8ByteLen(value: string): number {
  if (utf8Encoder) return utf8Encoder.encode(value).length;
  return value.length * 3;
}

/**
 * Safely serialize an unknown tool-arguments value into JSON, guarding against
 * circular references, BigInt values, or throwing getters.
 */
export function safeSerializeJson(value: unknown, space?: number): string | null {
  try {
    const seen = new WeakSet<object>();
    const result = JSON.stringify(
      value,
      (_key, val: unknown) => {
        if (typeof val === "bigint") {
          return val.toString();
        }
        if (typeof val === "object" && val !== null) {
          if (seen.has(val)) {
            return "[Circular]";
          }
          seen.add(val);
        }
        return val;
      },
      space,
    );
    return typeof result === "string" ? result : null;
  } catch {
    return null;
  }
}

/**
 * Safely format a bounded preview string of `tools/call` arguments for user confirmation.
 */
export function safeFormatToolArgsPreview(args: unknown, maxChars = 600): string {
  const serialized = safeSerializeJson(args, 2);
  if (!serialized) return "[Unserializable arguments]";
  if (serialized.length <= maxChars) return serialized;
  return `${serialized.slice(0, maxChars)}\n… (truncated)`;
}

/**
 * Validate that a URL requested by `ui/open-link` is strictly `https://` with NO raw
 * control characters, NO raw whitespace, and NO embedded credentials.
 */
export function validateExternalHttpsUrl(rawUrl: unknown): string | null {
  if (typeof rawUrl !== "string") return null;
  if (rawUrl.length === 0 || rawUrl.length > 2048) return null;
  // Reject any ASCII control characters (0x00-0x1F, 0x7F) or whitespace BEFORE `new URL()`
  if (/[\u0000-\u0020\u007F]|\s/.test(rawUrl)) return null;
  try {
    const parsed = new URL(rawUrl);
    if (parsed.protocol !== "https:") return null;
    if (!parsed.hostname) return null;
    if (parsed.username || parsed.password) return null;
    return parsed.toString();
  } catch {
    return null;
  }
}

/**
 * Validate that a URI requested by SEP-1865 `resources/read` is strictly `ui://...`
 * with NO control characters, NO whitespace, and a non-empty authority/path.
 */
export function validateUiResourceUri(rawUri: unknown): string | null {
  if (typeof rawUri !== "string") return null;
  if (rawUri.length === 0 || rawUri.length > 2048) return null;
  if (/[\u0000-\u0020\u007F]|\s/.test(rawUri)) return null;
  if (!rawUri.startsWith("ui://")) return null;
  const rest = rawUri.slice("ui://".length);
  if (!rest || rest.startsWith("/")) return null;
  return rawUri;
}

/**
 * Extract an optional explicit server override attempted by an iframe payload so the bridge
 * can strictly reject cross-server calls.
 */
function extractAttemptedServerOverride(params: Record<string, unknown>): string | null {
  for (const key of ["server", "serverName", "server_name", "mcpServerName", "mcp_server_name"]) {
    const val = params[key];
    if (typeof val === "string" && val.trim().length > 0) {
      return val.trim();
    }
  }
  return null;
}

/**
 * Clamp requested iframe height within safe visual bounds.
 */
export function clampSandboxHeight(
  rawHeight: unknown,
  minPx = SANDBOX_MIN_HEIGHT_PX,
  maxPx = SANDBOX_MAX_HEIGHT_PX,
): number | null {
  if (typeof rawHeight !== "number" || !Number.isFinite(rawHeight)) return null;
  return Math.round(Math.min(maxPx, Math.max(minPx, rawHeight)));
}

/**
 * UNCONDITIONALLY emit the HTML5 document header + strict CSP `<meta>` at byte 0
 * BEFORE any untrusted HTML bytes so comments (`<!-- <head> -->`), attribute payloads
 * (`<div title="<head>">`), or leading `<script>` tags can NEVER bypass CSP enforcement.
 */
export function injectSandboxCsp(rawHtml: string, theme: SandboxTheme): string {
  const cspMeta = `<meta http-equiv="Content-Security-Policy" content="${ARTIFACT_SANDBOX_CSP}">`;
  const themeStyle = `<style id="orbit-sandbox-theme">:root{color-scheme:${theme};--orbit-theme:${theme};}html,body{margin:0;padding:0;box-sizing:border-box;background:${theme === "dark" ? "#0f1115" : "#ffffff"};color:${theme === "dark" ? "#e4e4e7" : "#18181b"};font-family:-apple-system,BlinkMacSystemFont,"Segoe UI",Roboto,sans-serif;}</style>`;
  return `<!doctype html>
<html data-theme="${theme}">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
${cspMeta}
${themeStyle}
</head>
<body>
${rawHtml}
</body>
</html>`;
}

/**
 * Build the SEP-1865 `ui/notifications/host-context-changed` notification payload
 * pushed to the sandboxed app whenever Orbit switches Light <-> Dark mode.
 */
export function buildHostContextChangedNotification(theme: SandboxTheme): JsonRpcNotification {
  return {
    jsonrpc: "2.0",
    method: "ui/notifications/host-context-changed",
    params: {
      theme,
      displayMode: "inline",
      platform: "desktop",
    },
  };
}

/**
 * Build a JSON-RPC 2.0 response for resolving a user-confirmed `tools/call` request.
 */
export function buildToolCallResolutionResponse(
  id: string | number | null,
  approved: boolean,
  detail?: string,
): JsonRpcResponse {
  if (approved) {
    return {
      jsonrpc: "2.0",
      id,
      result: {
        content: [
          {
            type: "text",
            text: detail ?? "Tool request approved and forwarded to the active Orbit conversation.",
          },
        ],
        isError: false,
      },
    };
  }
  return {
    jsonrpc: "2.0",
    id,
    error: {
      code: JSON_RPC_ERR_USER_DENIED,
      message: detail ?? "Tool request denied by user",
    },
  };
}

/**
 * Extract readable text from MCP `content` field (string or array of `{ type: "text", text }`).
 */
function extractMcpTextContent(raw: unknown): string | null {
  if (typeof raw === "string") {
    const t = raw.trim();
    return t.length > 0 ? t : null;
  }
  if (Array.isArray(raw)) {
    const parts: string[] = [];
    for (const item of raw) {
      if (typeof item === "string" && item.trim()) {
        parts.push(item.trim());
      } else if (
        item &&
        typeof item === "object" &&
        "text" in item &&
        typeof (item as { text: unknown }).text === "string"
      ) {
        const text = (item as { text: string }).text.trim();
        if (text) parts.push(text);
      }
    }
    return parts.length > 0 ? parts.join("\n") : null;
  }
  if (
    raw &&
    typeof raw === "object" &&
    "text" in raw &&
    typeof (raw as { text: unknown }).text === "string"
  ) {
    const t = (raw as { text: string }).text.trim();
    return t.length > 0 ? t : null;
  }
  return null;
}

/**
 * Format a concise structured inline comment referencing an Artifact, version, and optional line/section.
 */
export function formatArtifactInlineComment(params: {
  title: string;
  version: number;
  startLine?: number | null;
  endLine?: number | null;
  section?: string | null;
  comment: string;
}): string {
  const cleanTitle = params.title.trim().replace(/"/g, "'") || "Artifact";
  const cleanComment = params.comment.trim();
  const ver = Math.max(1, Math.trunc(params.version || 1));

  let location = `v${ver}`;
  if (typeof params.startLine === "number" && params.startLine > 0) {
    const start = Math.trunc(params.startLine);
    const end =
      typeof params.endLine === "number" && params.endLine > start
        ? Math.trunc(params.endLine)
        : null;
    location = end ? `v${ver}, L${start}-L${end}` : `v${ver}, L${start}`;
  } else if (params.section && params.section.trim().length > 0) {
    location = `v${ver}, ${params.section.trim()}`;
  }

  return `Artifact "${cleanTitle}" (${location}): ${cleanComment}`;
}

export interface ParsedInlineArtifactDirective {
  id?: string;
  title?: string;
  resourceUri?: string;
  index: number;
}

/**
 * Strip CommonMark fenced code blocks (`` ``` `` / `~~~`) and inline backtick code spans
 * so documentation examples containing `<orbit-artifact>` / `<agent-embed>` are ignored.
 */
function stripMarkdownCodeRegions(text: string): string {
  const lines = text.split(/(?<=\n)/);
  const kept: string[] = [];
  let fenceChar = "";
  let fenceLen = 0;

  for (const line of lines) {
    const trimmed = line.trimStart();
    if (fenceChar) {
      let run = 0;
      while (run < trimmed.length && trimmed[run] === fenceChar) run += 1;
      if (run >= fenceLen && trimmed.slice(run).trim().length === 0) {
        fenceChar = "";
        fenceLen = 0;
      }
      kept.push(" ".repeat(line.length));
      continue;
    }
    const ch = trimmed[0];
    if (ch === "`" || ch === "~") {
      let run = 0;
      while (run < trimmed.length && trimmed[run] === ch) run += 1;
      const info = trimmed.slice(run);
      // CommonMark: backtick fence info strings cannot contain backticks
      if (run >= 3 && !(ch === "`" && info.includes("`"))) {
        fenceChar = ch;
        fenceLen = run;
        kept.push(" ".repeat(line.length));
        continue;
      }
    }
    kept.push(line);
  }

  const withoutFences = kept.join("");
  // Mask inline backtick spans with spaces while preserving indices
  let out = "";
  let i = 0;
  while (i < withoutFences.length) {
    if (withoutFences[i] !== "`") {
      out += withoutFences[i];
      i += 1;
      continue;
    }
    const openStart = i;
    while (i < withoutFences.length && withoutFences[i] === "`") i += 1;
    const tickLen = i - openStart;
    let scan = i;
    let closeEnd = -1;
    while (scan < withoutFences.length) {
      if (withoutFences[scan] === "`") {
        const runStart = scan;
        while (scan < withoutFences.length && withoutFences[scan] === "`") scan += 1;
        if (scan - runStart === tickLen) {
          closeEnd = scan;
          break;
        }
      } else {
        scan += 1;
      }
    }
    if (closeEnd !== -1) {
      out += " ".repeat(closeEnd - openStart);
      i = closeEnd;
    } else {
      out += withoutFences.slice(openStart, i);
    }
  }
  return out;
}

function findQuotedTagEnd(text: string, start: number, maxLen = 4096): number {
  let quote: string | null = null;
  const end = Math.min(text.length, start + maxLen);
  for (let i = start; i < end; i += 1) {
    const ch = text[i];
    if (quote) {
      if (ch === quote) quote = null;
    } else if (ch === '"' || ch === "'") {
      quote = ch;
    } else if (ch === ">") {
      return i;
    }
  }
  return -1;
}

function parseDirectiveAttributes(rawAttrs: string): Record<string, string> {
  const attrs: Record<string, string> = {};
  let i = 0;
  const n = rawAttrs.length;
  while (i < n) {
    while (i < n && /\s/.test(rawAttrs[i])) i += 1;
    if (i >= n) break;
    const keyStart = i;
    while (i < n && !/\s/.test(rawAttrs[i]) && rawAttrs[i] !== "=" && rawAttrs[i] !== "/") i += 1;
    const key = rawAttrs.slice(keyStart, i).trim().toLowerCase();
    if (!key) {
      i += 1;
      continue;
    }
    while (i < n && /\s/.test(rawAttrs[i])) i += 1;
    if (i >= n || rawAttrs[i] !== "=") {
      attrs[key] = "true";
      continue;
    }
    i += 1;
    while (i < n && /\s/.test(rawAttrs[i])) i += 1;
    if (i >= n) break;
    let val = "";
    if (rawAttrs[i] === '"' || rawAttrs[i] === "'") {
      const q = rawAttrs[i];
      i += 1;
      const valStart = i;
      while (i < n && rawAttrs[i] !== q) i += 1;
      val = rawAttrs.slice(valStart, i);
      if (i < n) i += 1;
    } else {
      const valStart = i;
      while (i < n && !/\s/.test(rawAttrs[i])) i += 1;
      val = rawAttrs.slice(valStart, i);
    }
    attrs[key] = val;
  }
  return attrs;
}

/**
 * Safely scan an assistant message for `<orbit-artifact>` / `<agent-embed>` directives
 * while ignoring fenced code blocks & inline code spans and handling `>` inside quoted attributes.
 */
export function extractInlineArtifactDirectives(text: string): ParsedInlineArtifactDirective[] {
  const lowerRaw = text.toLowerCase();
  if (!lowerRaw.includes("<orbit-artifact") && !lowerRaw.includes("<agent-embed")) {
    return [];
  }

  const scrubbed = stripMarkdownCodeRegions(text);
  const lower = scrubbed.toLowerCase();
  const out: ParsedInlineArtifactDirective[] = [];
  const tags = ["orbit-artifact", "agent-embed"] as const;
  let cursor = 0;

  while (cursor < lower.length && out.length < 16) {
    let bestIdx = -1;
    let bestTag = "";
    for (const tag of tags) {
      const needle = `<${tag}`;
      let from = cursor;
      while (true) {
        const idx = lower.indexOf(needle, from);
        if (idx === -1) break;
        const after = lower[idx + needle.length];
        if (after === undefined || /\s/.test(after) || after === ">" || after === "/") {
          if (bestIdx === -1 || idx < bestIdx) {
            bestIdx = idx;
            bestTag = tag;
          }
          break;
        }
        from = idx + needle.length;
      }
    }
    if (bestIdx === -1) break;

    const attrStart = bestIdx + 1 + bestTag.length;
    const tagEnd = findQuotedTagEnd(scrubbed, attrStart);
    if (tagEnd === -1) {
      cursor = attrStart;
      continue;
    }

    const attrs = parseDirectiveAttributes(scrubbed.slice(attrStart, tagEnd));
    out.push({
      id: (attrs.id || attrs.artifact_id || "").trim() || undefined,
      title: (attrs.title || attrs.name || "").trim() || undefined,
      resourceUri: (attrs.resource_uri || attrs.resourceuri || attrs.uri || attrs.src || "").trim() || undefined,
      index: bestIdx,
    });
    cursor = tagEnd + 1;
  }

  return out;
}

type ConfirmationKind = "tool_call" | "open_link" | "message" | "model_context";

interface ActiveConfirmation {
  kind: ConfirmationKind;
  id: string | number | null;
}

/**
 * Create a hardened SEP-1865 MCP Apps JSON-RPC 2.0 host bridge dispatcher.
 */
export function createMcpAppHostBridge(callbacks: McpAppBridgeCallbacks) {
  const nowFn = callbacks.now ?? (() => Date.now());
  const maxReqPerSec = callbacks.maxRequestsPerSecond ?? DEFAULT_BRIDGE_MAX_REQ_PER_SEC;
  const recentTimestamps: number[] = [];
  let activeConfirmation: ActiveConfirmation | null = null;
  let lastEmittedHeight: number | null = null;
  let lastSizeChangedAtMs = -Infinity;
  let navigationEpoch = 0;

  function postToIframe(payload: JsonRpcResponse | JsonRpcNotification): boolean {
    const target = callbacks.getExpectedSource();
    if (!target) return false;
    // Sandboxed iframe without `allow-same-origin` has opaque `null` origin, so targetOrigin is "*"
    // while inbound messages are strictly authenticated via `event.source === iframe.contentWindow`
    // and `event.origin === "null"`.
    target.postMessage(payload, "*");
    return true;
  }

  function postError(id: string | number | null | undefined, code: number, message: string): void {
    if (id !== undefined) {
      postToIframe({
        jsonrpc: "2.0",
        id,
        error: { code, message },
      });
    }
  }

  function checkRateLimit(): boolean {
    const now = nowFn();
    while (recentTimestamps.length > 0 && now - recentTimestamps[0] >= BRIDGE_RATE_WINDOW_MS) {
      recentTimestamps.shift();
    }
    if (recentTimestamps.length >= maxReqPerSec) {
      return false;
    }
    recentTimestamps.push(now);
    return true;
  }

  function clearConfirmation(kind: ConfirmationKind, id: string | number | null): void {
    if (activeConfirmation && activeConfirmation.kind === kind && activeConfirmation.id === id) {
      activeConfirmation = null;
    }
  }

  function handleMessageEvent(event: MessageEventLike): boolean {
    const expected = callbacks.getExpectedSource();
    if (!expected || event.source !== expected) {
      return false;
    }
    // Strictly require opaque `"null"` origin whenever `event.origin` is provided
    if (event.origin !== undefined && event.origin !== "null") {
      return false;
    }

    const data = event.data;
    if (!data || typeof data !== "object") {
      return false;
    }

    const msg = data as Partial<JsonRpcRequest>;
    if (msg.jsonrpc !== "2.0" || typeof msg.method !== "string") {
      return false;
    }

    const id = msg.id !== undefined ? msg.id : undefined;
    const params =
      msg.params && typeof msg.params === "object" && !Array.isArray(msg.params)
        ? (msg.params as Record<string, unknown>)
        : {};

    // Coalesce `ui/notifications/size-changed` and `ui/size-change` separately so ResizeObserver bursts never starve RPC quota
    if (msg.method === "ui/notifications/size-changed" || msg.method === "ui/size-change") {
      const clamped = clampSandboxHeight(params.height);
      if (clamped !== null) {
        const now = nowFn();
        if (clamped !== lastEmittedHeight || now - lastSizeChangedAtMs >= 100) {
          lastEmittedHeight = clamped;
          lastSizeChangedAtMs = now;
          callbacks.onSizeChanged?.(clamped);
        }
      }
      return true;
    }

    if (!checkRateLimit()) {
      postError(id, JSON_RPC_ERR_RATE_LIMITED, "Rate limit exceeded");
      return true;
    }

    switch (msg.method) {
      case "ui/initialize": {
        if (id !== undefined) {
          const theme = callbacks.getTheme();
          postToIframe({
            jsonrpc: "2.0",
            id,
            result: {
              protocolVersion: SEP_1865_PROTOCOL_VERSION,
              hostInfo: {
                name: "Orbit",
                version: "0.3",
              },
              hostCapabilities: {
                openLinks: {},
                message: {},
                updateModelContext: {},
                serverTools: {},
                serverResources: {},
              },
              hostContext: {
                theme,
                displayMode: "inline",
                platform: "desktop",
              },
            },
          });
        }
        return true;
      }

      case "ui/notifications/initialized": {
        callbacks.onInitialized?.();
        return true;
      }

      case "ui/open-link": {
        const safeUrl = validateExternalHttpsUrl(params.url);
        if (!safeUrl) {
          postError(
            id,
            JSON_RPC_ERR_INVALID_PARAMS,
            "Only https:// URLs without control characters or whitespace are permitted",
          );
          return true;
        }
        if (activeConfirmation !== null) {
          postError(
            id,
            JSON_RPC_ERR_CONFIRMATION_PENDING,
            "Another user confirmation prompt is already pending",
          );
          return true;
        }
        activeConfirmation = { kind: "open_link", id: id ?? null };
        callbacks.onOpenLinkRequest?.({ id: id ?? null, url: safeUrl });
        return true;
      }

      case "ui/message": {
        const text =
          extractMcpTextContent(params.content) ?? extractMcpTextContent(params.message);
        if (!text) {
          postError(id, JSON_RPC_ERR_INVALID_PARAMS, "ui/message requires non-empty text content");
          return true;
        }
        if (utf8ByteLen(text) > MAX_BRIDGE_TEXT_BYTES) {
          postError(
            id,
            JSON_RPC_ERR_INVALID_PARAMS,
            `ui/message exceeds maximum size of ${MAX_BRIDGE_TEXT_BYTES} bytes`,
          );
          return true;
        }
        if (activeConfirmation !== null) {
          postError(
            id,
            JSON_RPC_ERR_CONFIRMATION_PENDING,
            "Another user confirmation prompt is already pending",
          );
          return true;
        }
        activeConfirmation = { kind: "message", id: id ?? null };
        callbacks.onMessageRequest?.({ id: id ?? null, text });
        return true;
      }

      case "ui/update-model-context": {
        const note =
          extractMcpTextContent(params.content) ?? extractMcpTextContent(params.context);
        if (!note) {
          postError(
            id,
            JSON_RPC_ERR_INVALID_PARAMS,
            "ui/update-model-context requires non-empty content",
          );
          return true;
        }
        if (utf8ByteLen(note) > MAX_BRIDGE_TEXT_BYTES) {
          postError(
            id,
            JSON_RPC_ERR_INVALID_PARAMS,
            `ui/update-model-context exceeds maximum size of ${MAX_BRIDGE_TEXT_BYTES} bytes`,
          );
          return true;
        }
        if (activeConfirmation !== null) {
          postError(
            id,
            JSON_RPC_ERR_CONFIRMATION_PENDING,
            "Another user confirmation prompt is already pending",
          );
          return true;
        }
        activeConfirmation = { kind: "model_context", id: id ?? null };
        callbacks.onUpdateModelContextRequest?.({ id: id ?? null, note });
        return true;
      }

      case "resources/read": {
        const safeUri = validateUiResourceUri(params.uri);
        if (!safeUri) {
          postError(
            id,
            JSON_RPC_ERR_INVALID_PARAMS,
            "resources/read only permits valid `ui://` resource URIs",
          );
          return true;
        }
        const owningServer = callbacks.getOwningMcpServerName?.()?.trim() || null;
        if (!owningServer) {
          postError(
            id,
            JSON_RPC_ERR_USER_DENIED,
            "Artifact is not bound to an owning MCP server for resources/read",
          );
          return true;
        }
        const attemptedServer = extractAttemptedServerOverride(params);
        if (attemptedServer && attemptedServer !== owningServer) {
          postError(
            id,
            JSON_RPC_ERR_USER_DENIED,
            "Cross-server resources/read requests are forbidden",
          );
          return true;
        }
        if (!callbacks.onReadResourceRequest) {
          postError(
            id,
            JSON_RPC_ERR_METHOD_NOT_FOUND,
            "resources/read is not enabled for this host frame",
          );
          return true;
        }
        callbacks.onReadResourceRequest({
          id: id ?? null,
          uri: safeUri,
          serverName: owningServer,
          navigationEpoch,
        });
        return true;
      }

      case "tools/call": {
        const rawName = typeof params.name === "string" ? params.name.trim() : "";
        if (!rawName || rawName.length > 128 || /[\u0000-\u0020\u007F]|\s/.test(rawName)) {
          postError(id, JSON_RPC_ERR_INVALID_PARAMS, "tools/call requires a valid tool `name`");
          return true;
        }
        const owningServer = callbacks.getOwningMcpServerName?.()?.trim() || null;
        const attemptedServer = extractAttemptedServerOverride(params);
        if (attemptedServer && (!owningServer || attemptedServer !== owningServer)) {
          postError(
            id,
            JSON_RPC_ERR_USER_DENIED,
            "Cross-server tools/call requests are forbidden",
          );
          return true;
        }
        const toolArgs =
          params.arguments &&
          typeof params.arguments === "object" &&
          !Array.isArray(params.arguments)
            ? (params.arguments as Record<string, unknown>)
            : {};
        const serializedArgs = safeSerializeJson(toolArgs);
        if (serializedArgs === null) {
          postError(id, JSON_RPC_ERR_INVALID_PARAMS, "tools/call arguments are not serializable");
          return true;
        }
        if (utf8ByteLen(serializedArgs) > MAX_BRIDGE_ARGS_BYTES) {
          postError(
            id,
            JSON_RPC_ERR_INVALID_PARAMS,
            `tools/call arguments exceed maximum size of ${MAX_BRIDGE_ARGS_BYTES} bytes`,
          );
          return true;
        }
        if (activeConfirmation !== null) {
          postError(
            id,
            JSON_RPC_ERR_CONFIRMATION_PENDING,
            "Another user confirmation prompt is already pending",
          );
          return true;
        }
        activeConfirmation = { kind: "tool_call", id: id ?? null };
        callbacks.onToolCallRequest?.({
          id: id ?? null,
          name: rawName,
          arguments: toolArgs,
          argsPreview: safeFormatToolArgsPreview(toolArgs, 600),
          navigationEpoch,
        });
        return true;
      }

      default: {
        postError(id, JSON_RPC_ERR_METHOD_NOT_FOUND, `Unsupported SEP-1865 method: ${msg.method}`);
        return true;
      }
    }
  }

  function pushThemeChange(theme: SandboxTheme): boolean {
    return postToIframe(buildHostContextChangedNotification(theme));
  }

  function resolveToolCall(
    id: string | number | null,
    approved: boolean,
    detail?: string,
    expectedEpoch?: number,
  ): boolean {
    if (expectedEpoch !== undefined && expectedEpoch !== navigationEpoch) {
      return false;
    }
    clearConfirmation("tool_call", id);
    return postToIframe(buildToolCallResolutionResponse(id, approved, detail));
  }

  function resolveToolCallResult(
    id: string | number | null,
    outcome: McpToolCallBridgeOutcome,
    expectedEpoch?: number,
  ): boolean {
    if (expectedEpoch !== undefined && expectedEpoch !== navigationEpoch) {
      return false;
    }
    clearConfirmation("tool_call", id);
    if (id === null) return true;
    const content = Array.isArray(outcome.content) ? outcome.content : [];
    if (!outcome.ok && content.length === 0) {
      return postToIframe({
        jsonrpc: "2.0",
        id,
        error: {
          code: JSON_RPC_ERR_USER_DENIED,
          message: outcome.error ?? "MCP tool execution failed",
        },
      });
    }
    const resultObj: Record<string, unknown> = {
      content,
      isError: Boolean(outcome.isError ?? !outcome.ok),
    };
    if (outcome.structuredContent !== undefined && outcome.structuredContent !== null) {
      resultObj.structuredContent = outcome.structuredContent;
    }
    return postToIframe({
      jsonrpc: "2.0",
      id,
      result: resultObj,
    });
  }

  function resolveResourceRead(
    id: string | number | null,
    resource: McpResourceReadResult | null,
    errorMessage?: string,
    expectedEpoch?: number,
  ): boolean {
    if (expectedEpoch !== undefined && expectedEpoch !== navigationEpoch) {
      return false;
    }
    if (id === null) return true;
    if (!resource) {
      return postToIframe({
        jsonrpc: "2.0",
        id,
        error: {
          code: JSON_RPC_ERR_USER_DENIED,
          message: errorMessage ?? "Unable to read MCP resource",
        },
      });
    }
    const entry: Record<string, unknown> = {
      uri: resource.uri,
    };
    if (resource.mimeType) entry.mimeType = resource.mimeType;
    if (resource.text !== undefined && resource.text !== null) entry.text = resource.text;
    if (resource.blob !== undefined && resource.blob !== null) entry.blob = resource.blob;
    return postToIframe({
      jsonrpc: "2.0",
      id,
      result: {
        contents: [entry],
      },
    });
  }

  function resolveOpenLink(id: string | number | null, approved: boolean, detail?: string): boolean {
    clearConfirmation("open_link", id);
    if (id === null) return true;
    return postToIframe(
      approved
        ? { jsonrpc: "2.0", id, result: { ok: true } }
        : {
            jsonrpc: "2.0",
            id,
            error: {
              code: JSON_RPC_ERR_USER_DENIED,
              message: detail ?? "Link open cancelled by user",
            },
          },
    );
  }

  function resolveMessage(id: string | number | null, approved: boolean, detail?: string): boolean {
    clearConfirmation("message", id);
    if (id === null) return true;
    return postToIframe(
      approved
        ? { jsonrpc: "2.0", id, result: { ok: true } }
        : {
            jsonrpc: "2.0",
            id,
            error: {
              code: JSON_RPC_ERR_USER_DENIED,
              message: detail ?? "Message dismissed by user",
            },
          },
    );
  }

  function resolveModelContext(
    id: string | number | null,
    approved: boolean,
    detail?: string,
  ): boolean {
    clearConfirmation("model_context", id);
    if (id === null) return true;
    return postToIframe(
      approved
        ? { jsonrpc: "2.0", id, result: { ok: true } }
        : {
            jsonrpc: "2.0",
            id,
            error: {
              code: JSON_RPC_ERR_USER_DENIED,
              message: detail ?? "Context update dismissed by user",
            },
          },
    );
  }

  function resetConfirmations(): void {
    activeConfirmation = null;
  }

  /**
   * Increment the active iframe navigation epoch and cancel any pending confirmation so
   * in-flight async replies from a previous document can NEVER be delivered after navigation.
   */
  function invalidateNavigation(): number {
    navigationEpoch += 1;
    activeConfirmation = null;
    return navigationEpoch;
  }

  function getNavigationEpoch(): number {
    return navigationEpoch;
  }

  return {
    handleMessageEvent,
    pushThemeChange,
    resolveToolCall,
    resolveToolCallResult,
    resolveResourceRead,
    resolveOpenLink,
    resolveMessage,
    resolveModelContext,
    resetConfirmations,
    invalidateNavigation,
    getNavigationEpoch,
  };
}
