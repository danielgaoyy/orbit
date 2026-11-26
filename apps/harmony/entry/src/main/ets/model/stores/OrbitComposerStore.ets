import {
  PersistedContextItemDto,
  PersistedDiffQuoteDto,
  PersistedDraftPayloadV1Dto,
  PersistedInlineMentionDto,
  PersistedPastedAttachmentDto,
  PersistedScopeDraftDto
} from '../OrbitTypes';
import { OrbitComposerDraftBridge } from '../OrbitCoreBridge';
import { orbitLogWarn } from '../OrbitLogger';

export const ORBIT_DRAFTS_STORAGE_KEY: string = 'orbit.drafts.v1';
export const ORBIT_DRAFT_SCHEMA_VERSION: number = 1;
export const ORBIT_DRAFT_SAVE_DEBOUNCE_MS: number = 350;
export const ORBIT_DRAFT_TTL_MS: number = 30 * 24 * 60 * 60 * 1000;
export const ORBIT_DRAFT_MAX_TOTAL_BYTES: number = 512 * 1024;
export const ORBIT_DRAFT_MAX_ENTRY_BYTES: number = 128 * 1024;
export const ORBIT_DRAFT_MAX_TEXT_BYTES: number = 64 * 1024;
export const ORBIT_DRAFT_MAX_PASTE_BYTES: number = 64 * 1024;
export const ORBIT_DRAFT_MAX_QUOTE_CODE_BYTES: number = 16 * 1024;
export const ORBIT_DRAFT_MAX_PASTES_PER_SCOPE: number = 16;
export const ORBIT_DRAFT_MAX_QUOTES_PER_SCOPE: number = 32;
export const ORBIT_DRAFT_MAX_CONTEXT_ITEMS_PER_SCOPE: number = 32;
export const ORBIT_DRAFT_MAX_INLINE_MENTIONS_PER_SCOPE: number = 32;

const FORBIDDEN_SCOPE_KEYS: Set<string> = new Set<string>(['__proto__', 'constructor', 'prototype']);

export function composerDraftScopeKey(hostId: string, agentId: string): string {
  const cleanHost = hostId.trim();
  const cleanAgent = agentId.trim();
  if (cleanHost.length === 0) {
    return '';
  }
  if (cleanAgent.length > 0) {
    return `${cleanHost}::${cleanAgent}`;
  }
  return `draft::${cleanHost}`;
}

export function utf8ByteLength(str: string): number {
  if (str.length === 0) {
    return 0;
  }
  let bytes = 0;
  for (let i = 0; i < str.length; ) {
    const cp = str.codePointAt(i) ?? 0;
    if (cp <= 0x7F) {
      bytes += 1;
    } else if (cp <= 0x7FF) {
      bytes += 2;
    } else if (cp <= 0xFFFF) {
      bytes += 3;
    } else {
      bytes += 4;
    }
    i += cp > 0xFFFF ? 2 : 1;
  }
  return bytes;
}

export function truncateUtf8(str: string, maxBytes: number): string {
  if (maxBytes <= 0 || str.length === 0) {
    return '';
  }
  if (str.length * 3 <= maxBytes || utf8ByteLength(str) <= maxBytes) {
    return str;
  }
  let bytes = 0;
  let endIndex = 0;
  for (let i = 0; i < str.length; ) {
    const cp = str.codePointAt(i) ?? 0;
    const step = cp > 0xFFFF ? 2 : 1;
    const cost = cp <= 0x7F ? 1 : (cp <= 0x7FF ? 2 : (cp <= 0xFFFF ? 3 : 4));
    if (bytes + cost > maxBytes) {
      break;
    }
    bytes += cost;
    i += step;
    endIndex = i;
  }
  return str.slice(0, endIndex);
}

function isValidScopeKey(key: string): boolean {
  return key.length > 0 && !FORBIDDEN_SCOPE_KEYS.has(key);
}

function sanitizeScopeDraft(
  draft: PersistedScopeDraftDto,
  nowMs: number
): PersistedScopeDraftDto | null {
  const updatedAt = Number.isFinite(draft.updatedAt) && draft.updatedAt > 0
    ? Math.floor(draft.updatedAt)
    : nowMs;
  if (nowMs - updatedAt > ORBIT_DRAFT_TTL_MS) {
    return null;
  }

  const out: PersistedScopeDraftDto = {
    updatedAt: updatedAt
  };
  let hasContent = false;

  if (draft.text !== undefined && draft.text.length > 0) {
    out.text = truncateUtf8(draft.text, ORBIT_DRAFT_MAX_TEXT_BYTES);
    if (out.text.length > 0) {
      hasContent = true;
    }
  }

  if (draft.slashDirective !== undefined && draft.slashDirective.trim().length > 0) {
    out.slashDirective = truncateUtf8(draft.slashDirective.trim(), 128);
    hasContent = true;
  }

  if (draft.sideQuestionDraft !== undefined && draft.sideQuestionDraft.length > 0) {
    out.sideQuestionDraft = truncateUtf8(draft.sideQuestionDraft, ORBIT_DRAFT_MAX_TEXT_BYTES);
    if (out.sideQuestionDraft.length > 0) {
      hasContent = true;
    }
  }

  if (draft.pastedAttachments !== undefined && draft.pastedAttachments.length > 0) {
    const pastes: PersistedPastedAttachmentDto[] = [];
    for (
      let i = 0;
      i < draft.pastedAttachments.length && pastes.length < ORBIT_DRAFT_MAX_PASTES_PER_SCOPE;
      i++
    ) {
      const p = draft.pastedAttachments[i];
      if (
        p.id.length > 0 &&
        p.content.length > 0 &&
        utf8ByteLength(p.content) <= ORBIT_DRAFT_MAX_PASTE_BYTES
      ) {
        const cleanPaste: PersistedPastedAttachmentDto = {
          id: truncateUtf8(p.id, 128),
          label: truncateUtf8(p.label, 512),
          lineCount: p.lineCount >= 1 ? Math.floor(p.lineCount) : p.content.split('\n').length,
          content: p.content
        };
        if (p.filename !== undefined && p.filename.length > 0) {
          cleanPaste.filename = truncateUtf8(p.filename, 512);
        }
        pastes.push(cleanPaste);
      }
    }
    if (pastes.length > 0) {
      out.pastedAttachments = pastes;
      hasContent = true;
    }
  }

  if (draft.diffQuotes !== undefined && draft.diffQuotes.length > 0) {
    const quotes: PersistedDiffQuoteDto[] = [];
    for (
      let i = 0;
      i < draft.diffQuotes.length && quotes.length < ORBIT_DRAFT_MAX_QUOTES_PER_SCOPE;
      i++
    ) {
      const q = draft.diffQuotes[i];
      if (q.id.length > 0 && q.path.length > 0) {
        quotes.push({
          id: truncateUtf8(q.id, 128),
          path: truncateUtf8(q.path, 1024),
          lineNumber: q.lineNumber !== null && Number.isFinite(q.lineNumber)
            ? Math.floor(q.lineNumber)
            : null,
          code: truncateUtf8(q.code, ORBIT_DRAFT_MAX_QUOTE_CODE_BYTES)
        });
      }
    }
    if (quotes.length > 0) {
      out.diffQuotes = quotes;
      hasContent = true;
    }
  }

  if (draft.contextItems !== undefined && draft.contextItems.length > 0) {
    const ctxs: PersistedContextItemDto[] = [];
    for (
      let i = 0;
      i < draft.contextItems.length && ctxs.length < ORBIT_DRAFT_MAX_CONTEXT_ITEMS_PER_SCOPE;
      i++
    ) {
      const c = draft.contextItems[i];
      if (c.id.length > 0 && c.kind.length > 0) {
        if (
          c.kind === 'terminal_output' &&
          c.content !== undefined &&
          utf8ByteLength(c.content) > ORBIT_DRAFT_MAX_PASTE_BYTES
        ) {
          continue;
        }
        ctxs.push(c);
      }
    }
    if (ctxs.length > 0) {
      out.contextItems = ctxs;
      hasContent = true;
    }
  }

  if (draft.inlineMentions !== undefined && draft.inlineMentions.length > 0) {
    const mentions: PersistedInlineMentionDto[] = [];
    for (
      let i = 0;
      i < draft.inlineMentions.length && mentions.length < ORBIT_DRAFT_MAX_INLINE_MENTIONS_PER_SCOPE;
      i++
    ) {
      const m = draft.inlineMentions[i];
      if (m.token.length > 0 && m.kind.length > 0) {
        if (
          m.kind === 'terminal_output' &&
          m.content !== undefined &&
          utf8ByteLength(m.content) > ORBIT_DRAFT_MAX_PASTE_BYTES
        ) {
          continue;
        }
        mentions.push(m);
      }
    }
    if (mentions.length > 0) {
      out.inlineMentions = mentions;
      hasContent = true;
    }
  }

  if (!hasContent) {
    return null;
  }

  // Enforce per-entry 128 KB budget by dropping lowest-priority blobs if necessary.
  if (utf8ByteLength(JSON.stringify(out)) > ORBIT_DRAFT_MAX_ENTRY_BYTES) {
    out.pastedAttachments = undefined;
  }
  if (utf8ByteLength(JSON.stringify(out)) > ORBIT_DRAFT_MAX_ENTRY_BYTES) {
    out.contextItems = undefined;
  }
  if (utf8ByteLength(JSON.stringify(out)) > ORBIT_DRAFT_MAX_ENTRY_BYTES) {
    out.diffQuotes = undefined;
  }
  if (utf8ByteLength(JSON.stringify(out)) > ORBIT_DRAFT_MAX_ENTRY_BYTES) {
    out.inlineMentions = undefined;
  }
  if (utf8ByteLength(JSON.stringify(out)) > ORBIT_DRAFT_MAX_ENTRY_BYTES) {
    return null;
  }
  return out;
}

interface ScopeSortEntry {
  key: string;
  updatedAt: number;
  draft: PersistedScopeDraftDto;
}

export interface OrbitComposerToastSink {
  onDraftToast(message: string, isError: boolean): void;
}

/**
 * Per-session SQLite-persisted composer draft store using the desktop-compatible
 * single storage key `orbit.drafts.v1` (`version: 1`).
 */
@ObservedV2
export class OrbitComposerStore {
  @Trace public activeScopeKey: string = '';
  @Trace public text: string = '';
  @Trace public slashDirective: string = '';
  @Trace public sideQuestionDraft: string = '';
  @Trace public pastedAttachments: PersistedPastedAttachmentDto[] = [];
  @Trace public diffQuotes: PersistedDiffQuoteDto[] = [];
  @Trace public contextItems: PersistedContextItemDto[] = [];
  @Trace public inlineMentions: PersistedInlineMentionDto[] = [];

  // Non-@Trace backing map of all persisted scope drafts.
  private readonly draftsByScope: Map<string, PersistedScopeDraftDto> =
    new Map<string, PersistedScopeDraftDto>();

  private bridge: OrbitComposerDraftBridge | null = null;
  private toastSink: OrbitComposerToastSink | null = null;
  private debounceTimerId: number = -1;
  private dirty: boolean = false;
  private hydrated: boolean = false;
  private writeErrorToasted: boolean = false;

  public bindBridge(bridge: OrbitComposerDraftBridge | null): void {
    this.bridge = bridge;
  }

  public setToastSink(sink: OrbitComposerToastSink | null): void {
    this.toastSink = sink;
  }

  public hydrate(nowMs: number = Date.now()): void {
    this.draftsByScope.clear();
    this.hydrated = true;
    if (this.bridge === null) {
      return;
    }
    try {
      const raw = this.bridge.getComposerDraftJson(ORBIT_DRAFTS_STORAGE_KEY);
      if (raw === null || raw.trim().length === 0) {
        if (this.activeScopeKey.length > 0) {
          this.loadScopeIntoMemory(this.activeScopeKey);
        }
        return;
      }
      const parsed = JSON.parse(raw) as PersistedDraftPayloadV1Dto;
      if (
        parsed === null ||
        typeof parsed !== 'object' ||
        parsed.version !== ORBIT_DRAFT_SCHEMA_VERSION ||
        parsed.drafts === null ||
        parsed.drafts === undefined ||
        typeof parsed.drafts !== 'object' ||
        Array.isArray(parsed.drafts)
      ) {
        orbitLogWarn('OrbitComposerStore', 'Invalid orbit.drafts.v1 payload; discarding', raw.length);
        return;
      }
      const keys = Object.keys(parsed.drafts);
      for (let i = 0; i < keys.length; i++) {
        const k = keys[i];
        if (!isValidScopeKey(k)) {
          continue;
        }
        const entry = parsed.drafts[k];
        if (entry === undefined || entry === null || typeof entry !== 'object') {
          continue;
        }
        const clean = sanitizeScopeDraft(entry, nowMs);
        if (clean !== null) {
          this.draftsByScope.set(k, clean);
        }
      }
      if (this.activeScopeKey.length > 0) {
        this.loadScopeIntoMemory(this.activeScopeKey);
      }
    } catch (err) {
      orbitLogWarn('OrbitComposerStore', 'Corrupt orbit.drafts.v1 JSON; discarding payload', err as Object);
      this.draftsByScope.clear();
    }
  }

  public setActiveScope(hostId: string, agentId: string, nowMs: number = Date.now()): void {
    const nextScope = composerDraftScopeKey(hostId, agentId);
    if (nextScope === this.activeScopeKey) {
      return;
    }
    if (this.activeScopeKey.length > 0) {
      this.syncActiveScopeIntoMap(nowMs);
      this.flushNow(nowMs);
    }
    this.activeScopeKey = nextScope;
    if (nextScope.length > 0) {
      this.loadScopeIntoMemory(nextScope);
    } else {
      this.resetInMemoryFields();
    }
  }

  public remapScope(
    oldHostId: string,
    oldAgentId: string,
    newHostId: string,
    newAgentId: string,
    nowMs: number = Date.now()
  ): void {
    const oldKey = composerDraftScopeKey(oldHostId, oldAgentId);
    const newKey = composerDraftScopeKey(newHostId, newAgentId);
    if (oldKey.length === 0 || newKey.length === 0 || oldKey === newKey) {
      return;
    }
    if (this.activeScopeKey === oldKey) {
      this.syncActiveScopeIntoMap(nowMs);
      const moved = this.draftsByScope.get(oldKey);
      this.draftsByScope.delete(oldKey);
      if (moved !== undefined) {
        moved.updatedAt = nowMs;
        this.draftsByScope.set(newKey, moved);
      }
      this.activeScopeKey = newKey;
      this.dirty = true;
      this.flushNow(nowMs);
      return;
    }
    const existing = this.draftsByScope.get(oldKey);
    if (existing !== undefined) {
      this.draftsByScope.delete(oldKey);
      existing.updatedAt = nowMs;
      this.draftsByScope.set(newKey, existing);
      this.dirty = true;
      this.flushNow(nowMs);
    }
  }

  public setText(nextText: string): void {
    if (this.text === nextText) {
      return;
    }
    this.text = nextText;
    this.markDirtyAndSchedule();
  }

  public setSlashDirective(nextDirective: string): void {
    const clean = nextDirective.trim();
    if (this.slashDirective === clean) {
      return;
    }
    this.slashDirective = clean;
    this.markDirtyAndSchedule();
  }

  public setSideQuestionDraft(nextDraft: string): void {
    if (this.sideQuestionDraft === nextDraft) {
      return;
    }
    this.sideQuestionDraft = nextDraft;
    this.markDirtyAndSchedule();
  }

  public setPastedAttachments(items: PersistedPastedAttachmentDto[]): void {
    this.pastedAttachments = items;
    this.markDirtyAndSchedule();
  }

  public setDiffQuotes(items: PersistedDiffQuoteDto[]): void {
    this.diffQuotes = items;
    this.markDirtyAndSchedule();
  }

  public setContextItems(items: PersistedContextItemDto[]): void {
    this.contextItems = items;
    this.markDirtyAndSchedule();
  }

  public setInlineMentions(items: PersistedInlineMentionDto[]): void {
    this.inlineMentions = items;
    this.markDirtyAndSchedule();
  }

  public clearActiveDraft(flushImmediately: boolean = true, nowMs: number = Date.now()): void {
    this.resetInMemoryFields();
    if (this.activeScopeKey.length > 0) {
      this.draftsByScope.delete(this.activeScopeKey);
    }
    this.dirty = true;
    if (flushImmediately) {
      this.flushNow(nowMs);
    } else {
      this.scheduleDebouncedFlush();
    }
  }

  public getDraftForScope(scopeKey: string): PersistedScopeDraftDto | null {
    const found = this.draftsByScope.get(scopeKey);
    return found !== undefined ? found : null;
  }

  public isHydrated(): boolean {
    return this.hydrated;
  }

  public flushNow(nowMs: number = Date.now()): boolean {
    this.cancelDebounceTimer();
    if (this.activeScopeKey.length > 0) {
      this.syncActiveScopeIntoMap(nowMs);
    }
    if (!this.dirty || this.bridge === null) {
      return true;
    }

    // Build budget-compliant payload (TTL 30d, per-entry 128 KB, total 512 KB).
    const sortedEntries: ScopeSortEntry[] = [];
    this.draftsByScope.forEach((rawDraft: PersistedScopeDraftDto, scopeKey: string): void => {
      if (!isValidScopeKey(scopeKey)) {
        return;
      }
      const clean = sanitizeScopeDraft(rawDraft, nowMs);
      if (clean !== null) {
        sortedEntries.push({
          key: scopeKey,
          updatedAt: clean.updatedAt,
          draft: clean
        });
      }
    });

    // Sort newest first (keep active scope highest priority on tie / eviction).
    sortedEntries.sort((a: ScopeSortEntry, b: ScopeSortEntry): number => {
      if (a.key === this.activeScopeKey && b.key !== this.activeScopeKey) {
        return -1;
      }
      if (b.key === this.activeScopeKey && a.key !== this.activeScopeKey) {
        return 1;
      }
      return b.updatedAt - a.updatedAt;
    });

    const keptList: ScopeSortEntry[] = [];
    let evictedAny = false;
    for (let i = 0; i < sortedEntries.length; i++) {
      const entry = sortedEntries[i];
      const candidateDrafts: Record<string, PersistedScopeDraftDto> = {};
      for (let j = 0; j < keptList.length; j++) {
        candidateDrafts[keptList[j].key] = keptList[j].draft;
      }
      candidateDrafts[entry.key] = entry.draft;
      const candidatePayload: PersistedDraftPayloadV1Dto = {
        version: ORBIT_DRAFT_SCHEMA_VERSION,
        drafts: candidateDrafts
      };
      const candidateBytes = utf8ByteLength(JSON.stringify(candidatePayload));
      if (candidateBytes > ORBIT_DRAFT_MAX_TOTAL_BYTES) {
        evictedAny = true;
        continue;
      }
      keptList.push(entry);
    }

    const keptDrafts: Record<string, PersistedScopeDraftDto> = {};
    this.draftsByScope.clear();
    for (let i = 0; i < keptList.length; i++) {
      const item = keptList[i];
      keptDrafts[item.key] = item.draft;
      this.draftsByScope.set(item.key, item.draft);
    }

    try {
      if (keptList.length === 0) {
        this.bridge.deleteComposerDraft(ORBIT_DRAFTS_STORAGE_KEY);
      } else {
        const finalPayload: PersistedDraftPayloadV1Dto = {
          version: ORBIT_DRAFT_SCHEMA_VERSION,
          drafts: keptDrafts
        };
        const serialized = JSON.stringify(finalPayload);
        if (utf8ByteLength(serialized) > ORBIT_DRAFT_MAX_TOTAL_BYTES) {
          this.notifyWriteFailureOnce('Draft exceeds 512 KB storage limit');
          return false;
        }
        this.bridge.setComposerDraft(ORBIT_DRAFTS_STORAGE_KEY, serialized);
      }
      this.dirty = false;
      this.writeErrorToasted = false;
      if (evictedAny) {
        orbitLogWarn('OrbitComposerStore', 'Evicted oldest drafts to stay within 512 KB budget');
      }
      return true;
    } catch (err) {
      orbitLogWarn('OrbitComposerStore', 'Failed to persist composer draft to SQLite', err as Object);
      this.notifyWriteFailureOnce('Failed to save composer draft');
      return false;
    }
  }

  public dispose(): void {
    this.flushNow();
    this.cancelDebounceTimer();
  }

  private markDirtyAndSchedule(): void {
    this.dirty = true;
    if (this.activeScopeKey.length > 0) {
      this.syncActiveScopeIntoMap(Date.now());
    }
    this.scheduleDebouncedFlush();
  }

  private scheduleDebouncedFlush(): void {
    this.cancelDebounceTimer();
    this.debounceTimerId = setTimeout((): void => {
      this.debounceTimerId = -1;
      this.flushNow();
    }, ORBIT_DRAFT_SAVE_DEBOUNCE_MS);
  }

  private cancelDebounceTimer(): void {
    if (this.debounceTimerId !== -1) {
      clearTimeout(this.debounceTimerId);
      this.debounceTimerId = -1;
    }
  }

  private resetInMemoryFields(): void {
    this.text = '';
    this.slashDirective = '';
    this.sideQuestionDraft = '';
    this.pastedAttachments = [];
    this.diffQuotes = [];
    this.contextItems = [];
    this.inlineMentions = [];
  }

  private loadScopeIntoMemory(scopeKey: string): void {
    const saved = this.draftsByScope.get(scopeKey);
    if (saved === undefined) {
      this.resetInMemoryFields();
      return;
    }
    this.text = saved.text ?? '';
    this.slashDirective = saved.slashDirective ?? '';
    this.sideQuestionDraft = saved.sideQuestionDraft ?? '';
    this.pastedAttachments = saved.pastedAttachments !== undefined ? saved.pastedAttachments.slice() : [];
    this.diffQuotes = saved.diffQuotes !== undefined ? saved.diffQuotes.slice() : [];
    this.contextItems = saved.contextItems !== undefined ? saved.contextItems.slice() : [];
    this.inlineMentions = saved.inlineMentions !== undefined ? saved.inlineMentions.slice() : [];
  }

  private syncActiveScopeIntoMap(nowMs: number): void {
    if (!isValidScopeKey(this.activeScopeKey)) {
      return;
    }
    const candidate: PersistedScopeDraftDto = {
      updatedAt: nowMs,
      text: this.text.length > 0 ? this.text : undefined,
      slashDirective: this.slashDirective.length > 0 ? this.slashDirective : undefined,
      sideQuestionDraft: this.sideQuestionDraft.length > 0 ? this.sideQuestionDraft : undefined,
      pastedAttachments: this.pastedAttachments.length > 0 ? this.pastedAttachments.slice() : undefined,
      diffQuotes: this.diffQuotes.length > 0 ? this.diffQuotes.slice() : undefined,
      contextItems: this.contextItems.length > 0 ? this.contextItems.slice() : undefined,
      inlineMentions: this.inlineMentions.length > 0 ? this.inlineMentions.slice() : undefined
    };
    const sanitized = sanitizeScopeDraft(candidate, nowMs);
    if (sanitized === null) {
      if (this.draftsByScope.has(this.activeScopeKey)) {
        this.draftsByScope.delete(this.activeScopeKey);
        this.dirty = true;
      }
    } else {
      this.draftsByScope.set(this.activeScopeKey, sanitized);
    }
  }

  private notifyWriteFailureOnce(message: string): void {
    if (this.writeErrorToasted) {
      return;
    }
    this.writeErrorToasted = true;
    if (this.toastSink !== null) {
      this.toastSink.onDraftToast(message, true);
    }
  }
}
