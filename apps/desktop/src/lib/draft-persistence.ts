import type { SlashDirective } from "@/lib/orbit-ipc";
import type { RichMediaKind } from "@/lib/media-utils";

export const DRAFTS_STORAGE_KEY = "orbit.drafts.v1";
export const DRAFT_SCHEMA_VERSION = 1 as const;
export const DRAFT_SAVE_DEBOUNCE_MS = 350;
export const DRAFT_TTL_MS = 30 * 24 * 60 * 60 * 1000;
export const MAX_FUTURE_TIMESTAMP_SKEW_MS = 60_000;
export const MAX_TOTAL_DRAFT_BYTES = 512 * 1024;
export const MAX_ENTRY_DRAFT_BYTES = 128 * 1024;
export const MAX_PASTE_ATTACHMENT_BYTES = 64 * 1024;
export const MAX_TEXT_DRAFT_BYTES = 64 * 1024;
export const MAX_DIFF_QUOTE_CODE_BYTES = 16 * 1024;
export const MAX_PASTES_PER_SCOPE = 16;
export const MAX_QUOTES_PER_SCOPE = 32;
export const MAX_CONTEXT_ITEMS_PER_SCOPE = 32;

const VALID_RICH_MEDIA_KINDS: ReadonlySet<RichMediaKind> = new Set<RichMediaKind>([
  "pdf",
  "audio",
  "video",
]);

const VALID_SLASH_DIRECTIVES: ReadonlySet<SlashDirective> = new Set<SlashDirective>([
  "plan",
  "review",
  "compact",
  "orbit",
  "orbit-handoff",
  "orbit-committee",
  "orbit-advisor",
]);

const FORBIDDEN_KEYS = new Set(["__proto__", "constructor", "prototype"]);

const textEncoder = typeof TextEncoder !== "undefined" ? new TextEncoder() : null;

export interface PersistedPastedAttachment {
  id: string;
  label: string;
  filename?: string;
  lineCount: number;
  content: string;
}

export interface PersistedDiffQuote {
  id: string;
  path: string;
  lineNumber: number | null;
  code: string;
}

export type PersistedContextItem =
  | {
      id: string;
      kind: "image";
      filename: string;
      path: string;
      previewUrl: string;
      sizeBytes: number;
      stagedHostId?: string;
    }
  | {
      id: string;
      kind: "file";
      path: string;
      filename?: string;
      sizeBytes?: number;
      stagedHostId?: string;
      mediaKind?: RichMediaKind;
      mimeType?: string;
      pdfPageCount?: number | null;
    }
  | {
      id: string;
      kind: "git_diff";
      staged: boolean;
      label: string;
    }
  | {
      id: string;
      kind: "terminal_output";
      label: string;
      lineCount: number;
      content: string;
    }
  | {
      id: string;
      kind: "code_symbol";
      path: string;
      startLine: number;
      endLine: number;
      symbolName: string;
      symbolKind: string;
      signature?: string | null;
      container?: string | null;
      language: string;
      snippet?: string | null;
    };

export interface PersistedScopeDraft {
  updatedAt: number;
  text?: string;
  pastedAttachments?: PersistedPastedAttachment[];
  diffQuotes?: PersistedDiffQuote[];
  contextItems?: PersistedContextItem[];
  slashDirective?: SlashDirective;
  sideQuestionDraft?: string;
}

export interface PersistedDraftPayloadV1 {
  version: typeof DRAFT_SCHEMA_VERSION;
  drafts: Record<string, PersistedScopeDraft>;
}

export interface SerializedDraftPayload {
  payload: PersistedDraftPayloadV1;
  json: string | null;
  keptKeys: string[];
  evictedKeys: string[];
}

export interface DraftStateSlices {
  composerDrafts: Record<string, string>;
  pastedAttachments: Record<string, PersistedPastedAttachment[]>;
  diffQuotes: Record<string, PersistedDiffQuote[]>;
  contextItems?: Record<string, readonly unknown[]>;
  slashDirectives: Record<string, SlashDirective | null>;
  sideQuestionDrafts: Record<string, string>;
}

export interface HydratedDraftSlices extends Omit<DraftStateSlices, "contextItems"> {
  contextItems: Record<string, PersistedContextItem[]>;
  updatedAtByScope: Record<string, number>;
  prunedCount: number;
}

export interface StorageLike {
  getItem(key: string): string | null;
  setItem(key: string, value: string): void;
  removeItem(key: string): void;
}

export interface WindowEventTargetLike {
  addEventListener(type: string, listener: EventListenerOrEventListenerObject): void;
  removeEventListener(type: string, listener: EventListenerOrEventListenerObject): void;
}

export interface DocumentEventTargetLike extends WindowEventTargetLike {
  readonly visibilityState: DocumentVisibilityState;
}

export interface DraftBudgetOptions {
  nowMs?: number;
  ttlMs?: number;
  maxTotalBytes?: number;
  maxEntryBytes?: number;
  maxPasteBytes?: number;
  maxTextBytes?: number;
  maxQuoteCodeBytes?: number;
  maxPastesPerScope?: number;
  maxQuotesPerScope?: number;
}

export interface DraftPersistenceOptions extends Omit<DraftBudgetOptions, "nowMs"> {
  storage?: StorageLike | null;
  storageKey?: string;
  debounceMs?: number;
  now?: () => number;
  windowTarget?: WindowEventTargetLike | null;
  documentTarget?: DocumentEventTargetLike | null;
}

export interface DraftPersistenceController {
  hydrate(excludeScopes?: ReadonlySet<string> | Record<string, unknown>): HydratedDraftSlices;
  markDirty(scopeKeys: string | Iterable<string>): void;
  removeScopes(scopeKeys: Iterable<string>, flushImmediately?: boolean): void;
  flushNow(force?: boolean): void;
  exportUpdatedAtByScope(): Record<string, number>;
  restoreUpdatedAtByScope(map: Record<string, number>): void;
  dispose(): void;
}

interface ResolvedBudget {
  ttlMs: number;
  maxTotalBytes: number;
  maxEntryBytes: number;
  maxPasteBytes: number;
  maxTextBytes: number;
  maxQuoteCodeBytes: number;
  maxPastesPerScope: number;
  maxQuotesPerScope: number;
}

function resolveBudget(options?: DraftBudgetOptions): ResolvedBudget {
  const maxTotalBytes = Math.max(256, options?.maxTotalBytes ?? MAX_TOTAL_DRAFT_BYTES);
  const maxEntryBytes = Math.min(
    maxTotalBytes,
    Math.max(128, options?.maxEntryBytes ?? MAX_ENTRY_DRAFT_BYTES),
  );
  return {
    ttlMs: Math.max(0, options?.ttlMs ?? DRAFT_TTL_MS),
    maxTotalBytes,
    maxEntryBytes,
    maxPasteBytes: Math.min(
      maxEntryBytes,
      Math.max(64, options?.maxPasteBytes ?? MAX_PASTE_ATTACHMENT_BYTES),
    ),
    maxTextBytes: Math.min(
      maxEntryBytes,
      Math.max(64, options?.maxTextBytes ?? MAX_TEXT_DRAFT_BYTES),
    ),
    maxQuoteCodeBytes: Math.min(
      maxEntryBytes,
      Math.max(64, options?.maxQuoteCodeBytes ?? MAX_DIFF_QUOTE_CODE_BYTES),
    ),
    maxPastesPerScope: Math.max(1, options?.maxPastesPerScope ?? MAX_PASTES_PER_SCOPE),
    maxQuotesPerScope: Math.max(1, options?.maxQuotesPerScope ?? MAX_QUOTES_PER_SCOPE),
  };
}

export function utf8ByteLength(str: string): number {
  if (str.length === 0) return 0;
  if (textEncoder) {
    return textEncoder.encode(str).byteLength;
  }
  let bytes = 0;
  for (let i = 0; i < str.length; ) {
    const cp = str.codePointAt(i) ?? 0;
    if (cp <= 0x7f) bytes += 1;
    else if (cp <= 0x7ff) bytes += 2;
    else if (cp <= 0xffff) bytes += 3;
    else bytes += 4;
    i += cp > 0xffff ? 2 : 1;
  }
  return bytes;
}

/**
 * Truncate a string so its UTF-8 byte length is at most `maxBytes` without splitting
 * UTF-16 surrogate pairs.
 */
export function truncateUtf8(str: string, maxBytes: number): string {
  if (maxBytes <= 0 || str.length === 0) return "";
  if (str.length * 3 <= maxBytes || utf8ByteLength(str) <= maxBytes) {
    return str;
  }
  let bytes = 0;
  let endIndex = 0;
  for (let i = 0; i < str.length; ) {
    const cp = str.codePointAt(i) ?? 0;
    const step = cp > 0xffff ? 2 : 1;
    const cost = cp <= 0x7f ? 1 : cp <= 0x7ff ? 2 : cp <= 0xffff ? 3 : 4;
    if (bytes + cost > maxBytes) break;
    bytes += cost;
    i += step;
    endIndex = i;
  }
  return str.slice(0, endIndex);
}

function isPlainRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function isValidScopeKey(key: unknown): key is string {
  return typeof key === "string" && key.length > 0 && !FORBIDDEN_KEYS.has(key);
}

function isValidSlashDirective(value: unknown): value is SlashDirective {
  return typeof value === "string" && VALID_SLASH_DIRECTIVES.has(value as SlashDirective);
}

function normalizePastedAttachment(
  raw: unknown,
  maxPasteBytes: number,
): PersistedPastedAttachment | null {
  if (!isPlainRecord(raw)) return null;
  if (typeof raw.id !== "string" || raw.id.length === 0) return null;
  if (typeof raw.content !== "string" || raw.content.length === 0) return null;
  if (utf8ByteLength(raw.content) > maxPasteBytes) return null;

  const derivedLines = raw.content.split("\n").length;
  const lineCount =
    typeof raw.lineCount === "number" && Number.isFinite(raw.lineCount) && raw.lineCount >= 1
      ? Math.floor(raw.lineCount)
      : derivedLines;
  const label =
    typeof raw.label === "string" && raw.label.length > 0
      ? truncateUtf8(raw.label, 512)
      : `Pasted text · ${lineCount} lines`;
  const item: PersistedPastedAttachment = {
    id: truncateUtf8(raw.id, 128),
    label,
    lineCount,
    content: raw.content,
  };
  if (typeof raw.filename === "string" && raw.filename.length > 0) {
    item.filename = truncateUtf8(raw.filename, 512);
  }
  return item;
}

function normalizeDiffQuote(
  raw: unknown,
  maxQuoteCodeBytes: number,
): PersistedDiffQuote | null {
  if (!isPlainRecord(raw)) return null;
  if (typeof raw.id !== "string" || raw.id.length === 0) return null;
  if (typeof raw.path !== "string" || raw.path.length === 0) return null;
  if (typeof raw.code !== "string") return null;

  const lineNumber =
    raw.lineNumber === null || raw.lineNumber === undefined
      ? null
      : typeof raw.lineNumber === "number" && Number.isFinite(raw.lineNumber)
        ? Math.floor(raw.lineNumber)
        : null;

  return {
    id: truncateUtf8(raw.id, 128),
    path: truncateUtf8(raw.path, 1024),
    lineNumber,
    code: truncateUtf8(raw.code, maxQuoteCodeBytes),
  };
}

function normalizeContextItem(
  raw: unknown,
  maxPasteBytes: number,
  maxQuoteCodeBytes: number,
): PersistedContextItem | null {
  if (!isPlainRecord(raw)) return null;
  if (typeof raw.id !== "string" || raw.id.length === 0) return null;
  const id = truncateUtf8(raw.id, 128);

  if (raw.kind === "image") {
    if (typeof raw.path !== "string" || raw.path.trim().length === 0) return null;
    const path = truncateUtf8(raw.path.trim(), 2048);
    const filename =
      typeof raw.filename === "string" && raw.filename.trim().length > 0
        ? truncateUtf8(raw.filename.trim(), 512)
        : path.split("/").pop() || "image.png";
    const sizeBytes =
      typeof raw.sizeBytes === "number" && Number.isFinite(raw.sizeBytes) && raw.sizeBytes >= 0
        ? Math.floor(raw.sizeBytes)
        : 0;
    const item: Extract<PersistedContextItem, { kind: "image" }> = {
      id,
      kind: "image",
      filename,
      path,
      // Never persist ephemeral `blob:` URLs; ComposerImageThumb hydrates from staged host path.
      previewUrl: "",
      sizeBytes,
    };
    if (typeof raw.stagedHostId === "string" && raw.stagedHostId.trim().length > 0) {
      item.stagedHostId = truncateUtf8(raw.stagedHostId.trim(), 256);
    }
    return item;
  }

  if (raw.kind === "file") {
    if (typeof raw.path !== "string" || raw.path.trim().length === 0) return null;
    const item: Extract<PersistedContextItem, { kind: "file" }> = {
      id,
      kind: "file",
      path: truncateUtf8(raw.path.trim(), 2048),
    };
    if (typeof raw.filename === "string" && raw.filename.trim().length > 0) {
      item.filename = truncateUtf8(raw.filename.trim(), 512);
    }
    if (typeof raw.sizeBytes === "number" && Number.isFinite(raw.sizeBytes) && raw.sizeBytes >= 0) {
      item.sizeBytes = Math.floor(raw.sizeBytes);
    }
    if (typeof raw.stagedHostId === "string" && raw.stagedHostId.trim().length > 0) {
      item.stagedHostId = truncateUtf8(raw.stagedHostId.trim(), 256);
    }
    if (
      typeof raw.mediaKind === "string" &&
      VALID_RICH_MEDIA_KINDS.has(raw.mediaKind as RichMediaKind)
    ) {
      item.mediaKind = raw.mediaKind as RichMediaKind;
    }
    if (typeof raw.mimeType === "string" && raw.mimeType.trim().length > 0) {
      item.mimeType = truncateUtf8(raw.mimeType.trim(), 128);
    }
    if (
      typeof raw.pdfPageCount === "number" &&
      Number.isFinite(raw.pdfPageCount) &&
      raw.pdfPageCount >= 0
    ) {
      item.pdfPageCount = Math.floor(raw.pdfPageCount);
    }
    return item;
  }

  if (raw.kind === "git_diff") {
    if (typeof raw.staged !== "boolean") return null;
    const label =
      typeof raw.label === "string" && raw.label.trim().length > 0
        ? truncateUtf8(raw.label.trim(), 256)
        : raw.staged
          ? "Staged Diff"
          : "Working Tree Diff";
    return {
      id,
      kind: "git_diff",
      staged: raw.staged,
      label,
    };
  }

  if (raw.kind === "terminal_output") {
    if (typeof raw.content !== "string" || raw.content.length === 0) return null;
    if (utf8ByteLength(raw.content) > maxPasteBytes) return null;
    const derivedLines = raw.content.split("\n").length;
    const lineCount =
      typeof raw.lineCount === "number" && Number.isFinite(raw.lineCount) && raw.lineCount >= 1
        ? Math.floor(raw.lineCount)
        : derivedLines;
    const label =
      typeof raw.label === "string" && raw.label.trim().length > 0
        ? truncateUtf8(raw.label.trim(), 256)
        : `Terminal · ${lineCount} lines`;
    return {
      id,
      kind: "terminal_output",
      label,
      lineCount,
      content: raw.content,
    };
  }

  if (raw.kind === "code_symbol") {
    if (typeof raw.path !== "string" || raw.path.trim().length === 0) return null;
    if (typeof raw.symbolName !== "string" || raw.symbolName.trim().length === 0) return null;
    if (typeof raw.startLine !== "number" || !Number.isFinite(raw.startLine)) return null;
    if (typeof raw.endLine !== "number" || !Number.isFinite(raw.endLine)) return null;
    const item: Extract<PersistedContextItem, { kind: "code_symbol" }> = {
      id,
      kind: "code_symbol",
      path: truncateUtf8(raw.path.trim(), 1024),
      startLine: Math.max(1, Math.floor(raw.startLine)),
      endLine: Math.max(1, Math.floor(raw.endLine)),
      symbolName: truncateUtf8(raw.symbolName.trim(), 256),
      symbolKind:
        typeof raw.symbolKind === "string" && raw.symbolKind.trim().length > 0
          ? truncateUtf8(raw.symbolKind.trim(), 64)
          : "symbol",
      language:
        typeof raw.language === "string" && raw.language.trim().length > 0
          ? truncateUtf8(raw.language.trim(), 64)
          : "text",
    };
    if (typeof raw.signature === "string" && raw.signature.length > 0) {
      item.signature = truncateUtf8(raw.signature, 512);
    }
    if (typeof raw.container === "string" && raw.container.length > 0) {
      item.container = truncateUtf8(raw.container, 256);
    }
    if (typeof raw.snippet === "string" && raw.snippet.length > 0) {
      item.snippet = truncateUtf8(raw.snippet, maxQuoteCodeBytes);
    }
    return item;
  }

  return null;
}

function isScopeDraftPopulated(entry: PersistedScopeDraft): boolean {
  if (entry.text && entry.text.trim().length > 0) return true;
  if (entry.sideQuestionDraft && entry.sideQuestionDraft.trim().length > 0) return true;
  if (entry.slashDirective) return true;
  if (entry.pastedAttachments && entry.pastedAttachments.length > 0) return true;
  if (entry.diffQuotes && entry.diffQuotes.length > 0) return true;
  if (entry.contextItems && entry.contextItems.length > 0) return true;
  return false;
}

function estimateEntryRecordBytes(scopeKey: string, entry: PersistedScopeDraft): number {
  return utf8ByteLength(JSON.stringify(scopeKey)) + 1 + utf8ByteLength(JSON.stringify(entry)) + 1;
}

function capScopeDraftToBudget(
  scopeKey: string,
  entry: PersistedScopeDraft,
  budget: ResolvedBudget,
): PersistedScopeDraft | null {
  const next: PersistedScopeDraft = { updatedAt: entry.updatedAt };
  if (entry.text && entry.text.trim().length > 0) {
    const capped = truncateUtf8(entry.text, budget.maxTextBytes);
    if (capped.trim().length > 0) {
      next.text = capped;
    }
  }
  if (entry.sideQuestionDraft && entry.sideQuestionDraft.trim().length > 0) {
    const capped = truncateUtf8(entry.sideQuestionDraft, budget.maxTextBytes);
    if (capped.trim().length > 0) {
      next.sideQuestionDraft = capped;
    }
  }
  if (entry.slashDirective && isValidSlashDirective(entry.slashDirective)) {
    next.slashDirective = entry.slashDirective;
  }
  if (entry.pastedAttachments && entry.pastedAttachments.length > 0) {
    const validPastes: PersistedPastedAttachment[] = [];
    for (const p of entry.pastedAttachments) {
      const normalized = normalizePastedAttachment(p, budget.maxPasteBytes);
      if (normalized) validPastes.push(normalized);
    }
    if (validPastes.length > budget.maxPastesPerScope) {
      validPastes.splice(0, validPastes.length - budget.maxPastesPerScope);
    }
    if (validPastes.length > 0) {
      next.pastedAttachments = validPastes;
    }
  }
  if (entry.diffQuotes && entry.diffQuotes.length > 0) {
    const validQuotes: PersistedDiffQuote[] = [];
    for (const q of entry.diffQuotes) {
      const normalized = normalizeDiffQuote(q, budget.maxQuoteCodeBytes);
      if (normalized) validQuotes.push(normalized);
    }
    if (validQuotes.length > budget.maxQuotesPerScope) {
      validQuotes.splice(0, validQuotes.length - budget.maxQuotesPerScope);
    }
    if (validQuotes.length > 0) {
      next.diffQuotes = validQuotes;
    }
  }
  if (entry.contextItems && entry.contextItems.length > 0) {
    const validContexts: PersistedContextItem[] = [];
    for (const ctx of entry.contextItems) {
      const normalized = normalizeContextItem(ctx, budget.maxPasteBytes, budget.maxQuoteCodeBytes);
      if (normalized) validContexts.push(normalized);
    }
    if (validContexts.length > MAX_CONTEXT_ITEMS_PER_SCOPE) {
      validContexts.splice(0, validContexts.length - MAX_CONTEXT_ITEMS_PER_SCOPE);
    }
    if (validContexts.length > 0) {
      next.contextItems = validContexts;
    }
  }

  if (!isScopeDraftPopulated(next)) {
    return null;
  }

  while (true) {
    const currentBytes = estimateEntryRecordBytes(scopeKey, next);
    if (currentBytes <= budget.maxEntryBytes) {
      break;
    }
    if (next.pastedAttachments && next.pastedAttachments.length > 0) {
      next.pastedAttachments.shift();
      if (next.pastedAttachments.length === 0) {
        delete next.pastedAttachments;
      }
      continue;
    }
    if (next.diffQuotes && next.diffQuotes.length > 0) {
      next.diffQuotes.shift();
      if (next.diffQuotes.length === 0) {
        delete next.diffQuotes;
      }
      continue;
    }
    if (next.contextItems && next.contextItems.length > 0) {
      // Shed heavy terminal_output contexts first before lightweight staged image/file refs
      const heavyIdx = next.contextItems.findIndex((c) => c.kind === "terminal_output");
      if (heavyIdx >= 0) {
        next.contextItems.splice(heavyIdx, 1);
      } else {
        next.contextItems.shift();
      }
      if (next.contextItems.length === 0) {
        delete next.contextItems;
      }
      continue;
    }
    const textBytes = next.text ? utf8ByteLength(next.text) : 0;
    const sqBytes = next.sideQuestionDraft ? utf8ByteLength(next.sideQuestionDraft) : 0;
    if (textBytes === 0 && sqBytes === 0) {
      return null;
    }
    const excessBytes = currentBytes - budget.maxEntryBytes;
    if (sqBytes >= textBytes && next.sideQuestionDraft) {
      const target = Math.max(0, sqBytes - excessBytes);
      const shortened = truncateUtf8(next.sideQuestionDraft, target);
      if (shortened.trim().length === 0) {
        delete next.sideQuestionDraft;
      } else {
        next.sideQuestionDraft = shortened;
      }
    } else if (next.text) {
      const target = Math.max(0, textBytes - excessBytes);
      const shortened = truncateUtf8(next.text, target);
      if (shortened.trim().length === 0) {
        delete next.text;
      } else {
        next.text = shortened;
      }
    }
  }

  return isScopeDraftPopulated(next) ? next : null;
}

function emptyHydratedSlices(prunedCount = 0): HydratedDraftSlices {
  return {
    composerDrafts: {},
    pastedAttachments: {},
    diffQuotes: {},
    contextItems: {},
    slashDirectives: {},
    sideQuestionDrafts: {},
    updatedAtByScope: {},
    prunedCount,
  };
}

export function hasNonEmptyScopeDraft(slices: DraftStateSlices, scopeKey: string): boolean {
  if (!scopeKey) return false;
  if ((slices.composerDrafts[scopeKey] ?? "").trim().length > 0) return true;
  if ((slices.pastedAttachments[scopeKey]?.length ?? 0) > 0) return true;
  if ((slices.diffQuotes[scopeKey]?.length ?? 0) > 0) return true;
  const ctxs = slices.contextItems?.[scopeKey];
  if (Array.isArray(ctxs) && ctxs.length > 0) {
    for (const raw of ctxs) {
      if (isPlainRecord(raw)) {
        if (raw.kind === "image" || raw.kind === "file") {
          if (typeof raw.path === "string" && raw.path.trim().length > 0) return true;
        } else if (typeof raw.kind === "string") {
          return true;
        }
      }
    }
  }
  if (slices.slashDirectives[scopeKey]) return true;
  if ((slices.sideQuestionDrafts[scopeKey] ?? "").trim().length > 0) return true;
  return false;
}

const EMPTY_PAYLOAD_BYTES = utf8ByteLength(
  JSON.stringify({ version: DRAFT_SCHEMA_VERSION, drafts: {} }),
);

function normalizeTimestamp(raw: number, nowMs: number): number {
  return raw > nowMs + MAX_FUTURE_TIMESTAMP_SKEW_MS ? nowMs : raw;
}

function evictLruToBudget(
  drafts: Record<string, PersistedScopeDraft>,
  maxTotalBytes: number,
): SerializedDraftPayload {
  const keys = Object.keys(drafts);
  if (keys.length === 0) {
    return {
      payload: { version: DRAFT_SCHEMA_VERSION, drafts: {} },
      json: null,
      keptKeys: [],
      evictedKeys: [],
    };
  }

  const working: Record<string, PersistedScopeDraft> = { ...drafts };
  let payload: PersistedDraftPayloadV1 = { version: DRAFT_SCHEMA_VERSION, drafts: working };
  let json = JSON.stringify(payload);
  if (utf8ByteLength(json) <= maxTotalBytes) {
    return { payload, json, keptKeys: keys, evictedKeys: [] };
  }

  // Pre-measure per-entry serialized UTF-8 byte sizes once (O(N)) to avoid O(N^2) re-stringification.
  const entryBytes = new Map<string, number>();
  let estimatedBytes = EMPTY_PAYLOAD_BYTES + Math.max(0, keys.length - 1);
  for (const key of keys) {
    const size =
      utf8ByteLength(JSON.stringify(key)) + 1 + utf8ByteLength(JSON.stringify(working[key]));
    entryBytes.set(key, size);
    estimatedBytes += size;
  }

  // Sort oldest-first by `updatedAt` (deterministic lexicographical tie-break) for LRU eviction.
  const orderedOldestFirst = keys.slice().sort((a, b) => {
    const diff = working[a].updatedAt - working[b].updatedAt;
    return diff !== 0 ? diff : a.localeCompare(b);
  });

  const evictedKeys: string[] = [];
  let remainingCount = orderedOldestFirst.length;
  let nextIdx = 0;

  while (nextIdx < orderedOldestFirst.length && estimatedBytes > maxTotalBytes) {
    const oldestKey = orderedOldestFirst[nextIdx++];
    delete working[oldestKey];
    evictedKeys.push(oldestKey);
    remainingCount -= 1;
    if (remainingCount === 0) {
      return {
        payload: { version: DRAFT_SCHEMA_VERSION, drafts: {} },
        json: null,
        keptKeys: [],
        evictedKeys,
      };
    }
    estimatedBytes -= (entryBytes.get(oldestKey) ?? 0) + 1;
  }

  // Verify exact serialized payload once (with defensive fallback if still over budget).
  payload = { version: DRAFT_SCHEMA_VERSION, drafts: working };
  json = JSON.stringify(payload);
  while (utf8ByteLength(json) > maxTotalBytes) {
    if (nextIdx >= orderedOldestFirst.length) {
      return {
        payload: { version: DRAFT_SCHEMA_VERSION, drafts: {} },
        json: null,
        keptKeys: [],
        evictedKeys,
      };
    }
    const oldestKey = orderedOldestFirst[nextIdx++];
    delete working[oldestKey];
    evictedKeys.push(oldestKey);
    remainingCount -= 1;
    if (remainingCount === 0) {
      return {
        payload: { version: DRAFT_SCHEMA_VERSION, drafts: {} },
        json: null,
        keptKeys: [],
        evictedKeys,
      };
    }
    payload = { version: DRAFT_SCHEMA_VERSION, drafts: working };
    json = JSON.stringify(payload);
  }

  return { payload, json, keptKeys: Object.keys(working), evictedKeys };
}

function isReadonlyMap<K, V>(value: unknown): value is ReadonlyMap<K, V> {
  return value instanceof Map;
}

function isReadonlySet<T>(value: unknown): value is ReadonlySet<T> {
  return value instanceof Set;
}

export function buildSerializedDraftPayload(
  slices: DraftStateSlices,
  updatedAtByScope: ReadonlyMap<string, number> | Record<string, number>,
  options?: DraftBudgetOptions,
): SerializedDraftPayload {
  const nowMs = options?.nowMs ?? Date.now();
  const budget = resolveBudget(options);

  const scopeKeys = new Set<string>();
  for (const k of Object.keys(slices.composerDrafts)) scopeKeys.add(k);
  for (const k of Object.keys(slices.pastedAttachments)) scopeKeys.add(k);
  for (const k of Object.keys(slices.diffQuotes)) scopeKeys.add(k);
  if (slices.contextItems) {
    for (const k of Object.keys(slices.contextItems)) scopeKeys.add(k);
  }
  for (const k of Object.keys(slices.slashDirectives)) scopeKeys.add(k);
  for (const k of Object.keys(slices.sideQuestionDrafts)) scopeKeys.add(k);

  const getTimestamp = (key: string): number => {
    const raw = isReadonlyMap<string, number>(updatedAtByScope)
      ? updatedAtByScope.get(key)
      : updatedAtByScope[key];
    if (typeof raw !== "number" || !Number.isFinite(raw) || raw <= 0) {
      return nowMs;
    }
    return normalizeTimestamp(raw, nowMs);
  };

  const drafts: Record<string, PersistedScopeDraft> = {};
  for (const key of scopeKeys) {
    if (!isValidScopeKey(key)) continue;
    if (!hasNonEmptyScopeDraft(slices, key)) continue;

    const updatedAt = getTimestamp(key);
    if (nowMs - updatedAt > budget.ttlMs) continue;

    const candidate: PersistedScopeDraft = { updatedAt };
    const text = slices.composerDrafts[key];
    if (typeof text === "string" && text.trim().length > 0) {
      candidate.text = text;
    }
    const pastes = slices.pastedAttachments[key];
    if (Array.isArray(pastes) && pastes.length > 0) {
      candidate.pastedAttachments = pastes;
    }
    const quotes = slices.diffQuotes[key];
    if (Array.isArray(quotes) && quotes.length > 0) {
      candidate.diffQuotes = quotes;
    }
    const contexts = slices.contextItems?.[key];
    if (Array.isArray(contexts) && contexts.length > 0) {
      const normalizedContexts: PersistedContextItem[] = [];
      for (const rawCtx of contexts) {
        const normalized = normalizeContextItem(
          rawCtx,
          budget.maxPasteBytes,
          budget.maxQuoteCodeBytes,
        );
        if (normalized) normalizedContexts.push(normalized);
      }
      if (normalizedContexts.length > 0) {
        candidate.contextItems = normalizedContexts;
      }
    }
    const directive = slices.slashDirectives[key];
    if (isValidSlashDirective(directive)) {
      candidate.slashDirective = directive;
    }
    const sideDraft = slices.sideQuestionDrafts[key];
    if (typeof sideDraft === "string" && sideDraft.trim().length > 0) {
      candidate.sideQuestionDraft = sideDraft;
    }

    const capped = capScopeDraftToBudget(key, candidate, budget);
    if (capped) {
      drafts[key] = capped;
    }
  }

  return evictLruToBudget(drafts, budget.maxTotalBytes);
}

export function buildPersistedDraftPayload(
  slices: DraftStateSlices,
  updatedAtByScope: ReadonlyMap<string, number> | Record<string, number>,
  options?: DraftBudgetOptions,
): PersistedDraftPayloadV1 {
  return buildSerializedDraftPayload(slices, updatedAtByScope, options).payload;
}

export function parseAndValidatePersistedDrafts(
  raw: string | null,
  options?: DraftBudgetOptions & {
    excludeScopes?: ReadonlySet<string> | Record<string, unknown>;
  },
): HydratedDraftSlices {
  if (raw === null || raw.length === 0) {
    return emptyHydratedSlices(0);
  }

  let parsed: unknown;
  try {
    parsed = JSON.parse(raw);
  } catch {
    return emptyHydratedSlices(1);
  }

  if (!isPlainRecord(parsed)) {
    return emptyHydratedSlices(1);
  }
  if (parsed.version !== DRAFT_SCHEMA_VERSION || !isPlainRecord(parsed.drafts)) {
    return emptyHydratedSlices(1);
  }

  const nowMs = options?.nowMs ?? Date.now();
  const budget = resolveBudget(options);
  const excludeScopes = options?.excludeScopes;
  const isExcluded = (key: string): boolean => {
    if (!excludeScopes) return false;
    if (isReadonlySet<string>(excludeScopes)) return excludeScopes.has(key);
    return Object.prototype.hasOwnProperty.call(excludeScopes, key) && Boolean(excludeScopes[key]);
  };

  let prunedCount = 0;
  const validatedDrafts: Record<string, PersistedScopeDraft> = {};

  for (const [scopeKey, rawEntry] of Object.entries(parsed.drafts)) {
    if (!isValidScopeKey(scopeKey) || isExcluded(scopeKey) || !isPlainRecord(rawEntry)) {
      prunedCount += 1;
      continue;
    }

    const rawUpdatedAt = rawEntry.updatedAt;
    if (
      typeof rawUpdatedAt !== "number" ||
      !Number.isFinite(rawUpdatedAt) ||
      rawUpdatedAt <= 0 ||
      nowMs - rawUpdatedAt > budget.ttlMs
    ) {
      prunedCount += 1;
      continue;
    }

    const updatedAt = normalizeTimestamp(rawUpdatedAt, nowMs);
    const candidate: PersistedScopeDraft = { updatedAt };
    let entryHadInvalidField = updatedAt !== rawUpdatedAt;

    if (rawEntry.text !== undefined) {
      if (typeof rawEntry.text === "string" && rawEntry.text.trim().length > 0) {
        candidate.text = rawEntry.text;
      } else {
        entryHadInvalidField = true;
      }
    }

    if (rawEntry.sideQuestionDraft !== undefined) {
      if (
        typeof rawEntry.sideQuestionDraft === "string" &&
        rawEntry.sideQuestionDraft.trim().length > 0
      ) {
        candidate.sideQuestionDraft = rawEntry.sideQuestionDraft;
      } else {
        entryHadInvalidField = true;
      }
    }

    if (rawEntry.slashDirective !== undefined && rawEntry.slashDirective !== null) {
      if (isValidSlashDirective(rawEntry.slashDirective)) {
        candidate.slashDirective = rawEntry.slashDirective;
      } else {
        entryHadInvalidField = true;
      }
    }

    if (rawEntry.pastedAttachments !== undefined) {
      if (Array.isArray(rawEntry.pastedAttachments)) {
        const cleanPastes: PersistedPastedAttachment[] = [];
        for (const item of rawEntry.pastedAttachments) {
          const normalized = normalizePastedAttachment(item, budget.maxPasteBytes);
          if (normalized) {
            cleanPastes.push(normalized);
          } else {
            entryHadInvalidField = true;
          }
        }
        if (cleanPastes.length > 0) {
          candidate.pastedAttachments = cleanPastes;
        }
      } else {
        entryHadInvalidField = true;
      }
    }

    if (rawEntry.diffQuotes !== undefined) {
      if (Array.isArray(rawEntry.diffQuotes)) {
        const cleanQuotes: PersistedDiffQuote[] = [];
        for (const item of rawEntry.diffQuotes) {
          const normalized = normalizeDiffQuote(item, budget.maxQuoteCodeBytes);
          if (normalized) {
            cleanQuotes.push(normalized);
          } else {
            entryHadInvalidField = true;
          }
        }
        if (cleanQuotes.length > 0) {
          candidate.diffQuotes = cleanQuotes;
        }
      } else {
        entryHadInvalidField = true;
      }
    }

    if (rawEntry.contextItems !== undefined) {
      if (Array.isArray(rawEntry.contextItems)) {
        const cleanContexts: PersistedContextItem[] = [];
        for (const item of rawEntry.contextItems) {
          const normalized = normalizeContextItem(
            item,
            budget.maxPasteBytes,
            budget.maxQuoteCodeBytes,
          );
          if (normalized) {
            cleanContexts.push(normalized);
          } else {
            entryHadInvalidField = true;
          }
        }
        if (cleanContexts.length > 0) {
          candidate.contextItems = cleanContexts;
        }
      } else {
        entryHadInvalidField = true;
      }
    }

    const capped = capScopeDraftToBudget(scopeKey, candidate, budget);
    if (!capped) {
      prunedCount += 1;
      continue;
    }
    if (entryHadInvalidField) {
      prunedCount += 1;
    }
    validatedDrafts[scopeKey] = capped;
  }

  const bounded = evictLruToBudget(validatedDrafts, budget.maxTotalBytes);
  prunedCount += bounded.evictedKeys.length;

  const result = emptyHydratedSlices(prunedCount);
  for (const [scopeKey, entry] of Object.entries(bounded.payload.drafts)) {
    result.updatedAtByScope[scopeKey] = entry.updatedAt;
    if (entry.text !== undefined) {
      result.composerDrafts[scopeKey] = entry.text;
    }
    if (entry.pastedAttachments !== undefined) {
      result.pastedAttachments[scopeKey] = entry.pastedAttachments;
    }
    if (entry.diffQuotes !== undefined) {
      result.diffQuotes[scopeKey] = entry.diffQuotes;
    }
    if (entry.contextItems !== undefined) {
      result.contextItems[scopeKey] = entry.contextItems;
    }
    if (entry.slashDirective !== undefined) {
      result.slashDirectives[scopeKey] = entry.slashDirective;
    }
    if (entry.sideQuestionDraft !== undefined) {
      result.sideQuestionDrafts[scopeKey] = entry.sideQuestionDraft;
    }
  }
  return result;
}

function writePayloadWithQuotaFallback(
  storage: StorageLike,
  storageKey: string,
  serialized: SerializedDraftPayload,
): void {
  if (!serialized.json || serialized.keptKeys.length === 0) {
    try {
      storage.removeItem(storageKey);
    } catch {
      // Ignore storage access errors
    }
    return;
  }

  try {
    storage.setItem(storageKey, serialized.json);
    return;
  } catch {
    // Storage quota exceeded or restricted — progressively shed oldest drafts by updatedAt
    const working: Record<string, PersistedScopeDraft> = { ...serialized.payload.drafts };
    const orderedOldestFirst = Object.keys(working).sort((a, b) => {
      const diff = working[a].updatedAt - working[b].updatedAt;
      return diff !== 0 ? diff : a.localeCompare(b);
    });

    while (orderedOldestFirst.length > 1) {
      const dropCount = Math.max(1, Math.ceil(orderedOldestFirst.length / 2));
      for (let i = 0; i < dropCount; i++) {
        const dropKey = orderedOldestFirst.shift();
        if (dropKey) delete working[dropKey];
      }
      if (orderedOldestFirst.length === 0) break;
      try {
        storage.setItem(
          storageKey,
          JSON.stringify({ version: DRAFT_SCHEMA_VERSION, drafts: working }),
        );
        return;
      } catch {
        // Continue shedding
      }
    }

    // Do not call storage.removeItem(storageKey) on quota failure so previously
    // persisted valid drafts survive transient quota errors.
  }
}

function getDefaultStorage(): StorageLike | null {
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage;
  } catch {
    return null;
  }
}

export function createDraftPersistenceController(
  getSnapshot: () => DraftStateSlices,
  options?: DraftPersistenceOptions,
): DraftPersistenceController {
  const storage = options && "storage" in options ? (options.storage ?? null) : getDefaultStorage();
  const storageKey = options?.storageKey ?? DRAFTS_STORAGE_KEY;
  const debounceMs = Math.max(0, options?.debounceMs ?? DRAFT_SAVE_DEBOUNCE_MS);
  const now = options?.now ?? (() => Date.now());
  const budget = resolveBudget(options);
  const windowTarget =
    options && "windowTarget" in options
      ? (options.windowTarget ?? null)
      : typeof window !== "undefined"
        ? window
        : null;
  const documentTarget =
    options && "documentTarget" in options
      ? (options.documentTarget ?? null)
      : typeof document !== "undefined"
        ? document
        : null;

  const updatedAtByScope = new Map<string, number>();
  let debounceTimer: ReturnType<typeof setTimeout> | null = null;
  let dirty = false;
  let disposed = false;

  const clearDebounceTimer = () => {
    if (debounceTimer !== null) {
      clearTimeout(debounceTimer);
      debounceTimer = null;
    }
  };

  const flushNow = (force = false): void => {
    clearDebounceTimer();
    if (disposed || !storage) return;
    if (!dirty && !force) return;
    dirty = false;

    const snapshot = getSnapshot();
    const nowMs = now();
    const serialized = buildSerializedDraftPayload(snapshot, updatedAtByScope, {
      ...budget,
      nowMs,
    });
    writePayloadWithQuotaFallback(storage, storageKey, serialized);

    // Only delete timestamps for scopes that are genuinely empty in memory;
    // preserve timestamps for still-in-memory LRU-evicted scopes to prevent LRU inversion.
    for (const [k, ts] of Array.from(updatedAtByScope.entries())) {
      if (!hasNonEmptyScopeDraft(snapshot, k)) {
        updatedAtByScope.delete(k);
      } else if (ts > nowMs + MAX_FUTURE_TIMESTAMP_SKEW_MS) {
        updatedAtByScope.set(k, nowMs);
      }
    }
  };

  const scheduleDebouncedFlush = (): void => {
    if (disposed || !storage) return;
    clearDebounceTimer();
    debounceTimer = setTimeout(() => {
      debounceTimer = null;
      flushNow();
    }, debounceMs);
  };

  const onUnload = (): void => {
    flushNow();
  };

  const onVisibilityChange = (): void => {
    if (documentTarget?.visibilityState === "hidden") {
      flushNow();
    }
  };

  if (windowTarget) {
    windowTarget.addEventListener("beforeunload", onUnload);
    windowTarget.addEventListener("pagehide", onUnload);
  }
  if (documentTarget) {
    documentTarget.addEventListener("visibilitychange", onVisibilityChange);
  }

  return {
    hydrate(excludeScopes?: ReadonlySet<string> | Record<string, unknown>): HydratedDraftSlices {
      if (!storage) return emptyHydratedSlices(0);
      let raw: string | null = null;
      try {
        raw = storage.getItem(storageKey);
      } catch {
        return emptyHydratedSlices(0);
      }

      const hydrated = parseAndValidatePersistedDrafts(raw, {
        ...budget,
        nowMs: now(),
        excludeScopes,
      });

      updatedAtByScope.clear();
      for (const [scopeKey, ts] of Object.entries(hydrated.updatedAtByScope)) {
        updatedAtByScope.set(scopeKey, ts);
      }

      // Persist cleanup immediately when hydration pruned expired, corrupt, or archived drafts
      if (hydrated.prunedCount > 0 && raw !== null) {
        const cleaned = buildSerializedDraftPayload(hydrated, updatedAtByScope, {
          ...budget,
          nowMs: now(),
        });
        writePayloadWithQuotaFallback(storage, storageKey, cleaned);
      }

      return hydrated;
    },

    markDirty(scopeKeys: string | Iterable<string>): void {
      if (disposed) return;
      const ts = now();
      let touched = false;
      if (typeof scopeKeys === "string") {
        if (isValidScopeKey(scopeKeys)) {
          updatedAtByScope.set(scopeKeys, ts);
          touched = true;
        }
      } else {
        for (const key of scopeKeys) {
          if (isValidScopeKey(key)) {
            updatedAtByScope.set(key, ts);
            touched = true;
          }
        }
      }
      if (!touched) return;
      dirty = true;
      scheduleDebouncedFlush();
    },

    removeScopes(scopeKeys: Iterable<string>, flushImmediately = true): void {
      if (disposed) return;
      let removedAny = false;
      for (const key of scopeKeys) {
        if (updatedAtByScope.delete(key)) {
          removedAny = true;
        }
      }
      if (!removedAny && !dirty) return;
      dirty = true;
      if (flushImmediately) {
        flushNow();
      } else {
        scheduleDebouncedFlush();
      }
    },

    flushNow,

    exportUpdatedAtByScope(): Record<string, number> {
      const out: Record<string, number> = {};
      for (const [key, ts] of updatedAtByScope.entries()) {
        out[key] = ts;
      }
      return out;
    },

    restoreUpdatedAtByScope(map: Record<string, number>): void {
      const nowMs = now();
      for (const [key, ts] of Object.entries(map)) {
        if (isValidScopeKey(key) && typeof ts === "number" && Number.isFinite(ts) && ts > 0) {
          updatedAtByScope.set(key, normalizeTimestamp(ts, nowMs));
        }
      }
    },

    dispose(): void {
      if (disposed) return;
      disposed = true;
      clearDebounceTimer();
      if (windowTarget) {
        windowTarget.removeEventListener("beforeunload", onUnload);
        windowTarget.removeEventListener("pagehide", onUnload);
      }
      if (documentTarget) {
        documentTarget.removeEventListener("visibilitychange", onVisibilityChange);
      }
    },
  };
}
