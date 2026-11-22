import type { SlashDirective } from "@/lib/orbit-ipc";

export const DRAFTS_STORAGE_KEY = "orbit.drafts.v1";
export const DRAFT_SCHEMA_VERSION = 1 as const;
export const DRAFT_SAVE_DEBOUNCE_MS = 350;
export const DRAFT_TTL_MS = 30 * 24 * 60 * 60 * 1000;
export const MAX_TOTAL_DRAFT_BYTES = 512 * 1024;
export const MAX_ENTRY_DRAFT_BYTES = 128 * 1024;
export const MAX_PASTE_ATTACHMENT_BYTES = 64 * 1024;
export const MAX_TEXT_DRAFT_BYTES = 64 * 1024;
export const MAX_DIFF_QUOTE_CODE_BYTES = 16 * 1024;
export const MAX_PASTES_PER_SCOPE = 16;
export const MAX_QUOTES_PER_SCOPE = 32;

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

export interface PersistedScopeDraft {
  updatedAt: number;
  text?: string;
  pastedAttachments?: PersistedPastedAttachment[];
  diffQuotes?: PersistedDiffQuote[];
  slashDirective?: SlashDirective;
  sideQuestionDraft?: string;
}

export interface PersistedDraftPayloadV1 {
  version: typeof DRAFT_SCHEMA_VERSION;
  drafts: Record<string, PersistedScopeDraft>;
}

export interface DraftStateSlices {
  composerDrafts: Record<string, string>;
  pastedAttachments: Record<string, PersistedPastedAttachment[]>;
  diffQuotes: Record<string, PersistedDiffQuote[]>;
  slashDirectives: Record<string, SlashDirective | null>;
  sideQuestionDrafts: Record<string, string>;
}

export interface HydratedDraftSlices extends DraftStateSlices {
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

function isScopeDraftPopulated(entry: PersistedScopeDraft): boolean {
  if (entry.text && entry.text.trim().length > 0) return true;
  if (entry.sideQuestionDraft && entry.sideQuestionDraft.trim().length > 0) return true;
  if (entry.slashDirective) return true;
  if (entry.pastedAttachments && entry.pastedAttachments.length > 0) return true;
  if (entry.diffQuotes && entry.diffQuotes.length > 0) return true;
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

  if (!isScopeDraftPopulated(next)) {
    return null;
  }

  while (estimateEntryRecordBytes(scopeKey, next) > budget.maxEntryBytes) {
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
    const textBytes = next.text ? utf8ByteLength(next.text) : 0;
    const sqBytes = next.sideQuestionDraft ? utf8ByteLength(next.sideQuestionDraft) : 0;
    if (textBytes === 0 && sqBytes === 0) {
      return null;
    }
    if (sqBytes >= textBytes && next.sideQuestionDraft) {
      const target = Math.floor(sqBytes / 2);
      const shortened = truncateUtf8(next.sideQuestionDraft, target);
      if (shortened.trim().length === 0) {
        delete next.sideQuestionDraft;
      } else {
        next.sideQuestionDraft = shortened;
      }
    } else if (next.text) {
      const target = Math.floor(textBytes / 2);
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
  if (slices.slashDirectives[scopeKey]) return true;
  if ((slices.sideQuestionDrafts[scopeKey] ?? "").trim().length > 0) return true;
  return false;
}

function evictLruToBudget(
  drafts: Record<string, PersistedScopeDraft>,
  maxTotalBytes: number,
): {
  payload: PersistedDraftPayloadV1;
  json: string | null;
  keptKeys: string[];
  evictedKeys: string[];
} {
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

  // Sort oldest-first by `updatedAt` (deterministic lexicographical tie-break) for LRU eviction.
  const orderedOldestFirst = keys.slice().sort((a, b) => {
    const diff = working[a].updatedAt - working[b].updatedAt;
    return diff !== 0 ? diff : a.localeCompare(b);
  });

  const evictedKeys: string[] = [];
  for (const oldestKey of orderedOldestFirst) {
    delete working[oldestKey];
    evictedKeys.push(oldestKey);
    const remainingKeys = Object.keys(working);
    if (remainingKeys.length === 0) {
      return {
        payload: { version: DRAFT_SCHEMA_VERSION, drafts: {} },
        json: null,
        keptKeys: [],
        evictedKeys,
      };
    }
    payload = { version: DRAFT_SCHEMA_VERSION, drafts: working };
    json = JSON.stringify(payload);
    if (utf8ByteLength(json) <= maxTotalBytes) {
      return { payload, json, keptKeys: remainingKeys, evictedKeys };
    }
  }

  return {
    payload: { version: DRAFT_SCHEMA_VERSION, drafts: {} },
    json: null,
    keptKeys: [],
    evictedKeys,
  };
}

function isReadonlyMap<K, V>(value: unknown): value is ReadonlyMap<K, V> {
  return value instanceof Map;
}

function isReadonlySet<T>(value: unknown): value is ReadonlySet<T> {
  return value instanceof Set;
}

export function buildPersistedDraftPayload(
  slices: DraftStateSlices,
  updatedAtByScope: ReadonlyMap<string, number> | Record<string, number>,
  options?: DraftBudgetOptions,
): PersistedDraftPayloadV1 {
  const nowMs = options?.nowMs ?? Date.now();
  const budget = resolveBudget(options);

  const scopeKeys = new Set<string>();
  for (const k of Object.keys(slices.composerDrafts)) scopeKeys.add(k);
  for (const k of Object.keys(slices.pastedAttachments)) scopeKeys.add(k);
  for (const k of Object.keys(slices.diffQuotes)) scopeKeys.add(k);
  for (const k of Object.keys(slices.slashDirectives)) scopeKeys.add(k);
  for (const k of Object.keys(slices.sideQuestionDrafts)) scopeKeys.add(k);

  const getTimestamp = (key: string): number => {
    const raw = isReadonlyMap<string, number>(updatedAtByScope)
      ? updatedAtByScope.get(key)
      : updatedAtByScope[key];
    return typeof raw === "number" && Number.isFinite(raw) && raw > 0 ? raw : nowMs;
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

  return evictLruToBudget(drafts, budget.maxTotalBytes).payload;
}

export function serializePersistedDraftPayload(
  payload: PersistedDraftPayloadV1,
  maxTotalBytes = MAX_TOTAL_DRAFT_BYTES,
): { json: string | null; keptKeys: string[]; evictedKeys: string[] } {
  const { json, keptKeys, evictedKeys } = evictLruToBudget(payload.drafts, maxTotalBytes);
  return { json, keptKeys, evictedKeys };
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

    const candidate: PersistedScopeDraft = { updatedAt: rawUpdatedAt };
    let entryHadInvalidField = false;

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
  payload: PersistedDraftPayloadV1,
  maxTotalBytes: number,
): string[] {
  const initial = evictLruToBudget(payload.drafts, maxTotalBytes);
  if (!initial.json || initial.keptKeys.length === 0) {
    try {
      storage.removeItem(storageKey);
    } catch {
      // Ignore storage access errors
    }
    return [];
  }

  try {
    storage.setItem(storageKey, initial.json);
    return initial.keptKeys;
  } catch {
    // Storage quota exceeded or restricted — progressively shed oldest drafts by updatedAt
    const working: Record<string, PersistedScopeDraft> = { ...initial.payload.drafts };
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
        return Object.keys(working);
      } catch {
        // Continue shedding
      }
    }

    try {
      storage.removeItem(storageKey);
    } catch {
      // Ignore storage access errors
    }
    return [];
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

    const payload = buildPersistedDraftPayload(getSnapshot(), updatedAtByScope, {
      ...budget,
      nowMs: now(),
    });
    const keptKeys = writePayloadWithQuotaFallback(
      storage,
      storageKey,
      payload,
      budget.maxTotalBytes,
    );
    const keptSet = new Set(keptKeys);
    for (const k of Array.from(updatedAtByScope.keys())) {
      if (!keptSet.has(k)) {
        updatedAtByScope.delete(k);
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
        const cleanedPayload = buildPersistedDraftPayload(hydrated, updatedAtByScope, {
          ...budget,
          nowMs: now(),
        });
        writePayloadWithQuotaFallback(storage, storageKey, cleanedPayload, budget.maxTotalBytes);
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
      for (const [key, ts] of Object.entries(map)) {
        if (isValidScopeKey(key) && typeof ts === "number" && Number.isFinite(ts) && ts > 0) {
          updatedAtByScope.set(key, ts);
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
