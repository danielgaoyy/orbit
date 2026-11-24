import {
  Annotation,
  EditorSelection,
  EditorState,
  Prec,
  StateEffect,
  StateField,
  type Extension,
  type Transaction,
  type TransactionSpec,
} from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
  keymap,
} from "@codemirror/view";
import type { FimConfig, FimMode, InlineEditResult } from "./orbit-ipc.ts";

export const FIM_PREFIX_MAX_BYTES = 16 * 1024;
export const FIM_SUFFIX_MAX_BYTES = 8 * 1024;
export const FIM_API_KEY_ENV_PREFIX = "ORBIT_FIM_";

export const FIM_DEBOUNCE_MS: Readonly<Record<FimMode, number>> = {
  off: 0,
  subtle: 650,
  eager: 250,
};

export const DEFAULT_FIM_CONFIG: Readonly<FimConfig> = {
  mode: "off",
  endpoint_url: null,
  model: null,
  api_key_env: null,
  max_tokens: 128,
  timeout_ms: 1800,
  allow_fast_cli_fallback: false,
};

export interface GhostTextSuggestion {
  text: string;
  pos: number;
  requestId?: string;
  source?: string;
  latencyMs?: number;
}

export type FimRuntimePhase = "idle" | "pending" | "active" | "error";

export interface FimRuntimeStatus {
  phase: FimRuntimePhase;
  source?: string;
  latencyMs?: number;
  error?: string;
}

export interface FimContextWindow {
  prefix: string;
  suffix: string;
  cursor_line: number;
  cursor_col: number;
}

export interface FimEditorLike {
  readonly state: EditorState;
  readonly composing: boolean;
  dispatch: (tr: TransactionSpec) => void;
}

export interface FimConfigDraft {
  mode: FimMode;
  endpointUrl: string;
  model: string;
  apiKeyEnv: string;
  maxTokens: string | number;
  timeoutMs: string | number;
  allowFastCliFallback: boolean;
}

export type FimConfigValidationErrorCode =
  | "invalid_endpoint_url"
  | "invalid_endpoint_path"
  | "invalid_api_key_env"
  | "insecure_api_key_transport"
  | "ollama_model_required"
  | "invalid_model";

export interface FimConfigValidationResult {
  valid: boolean;
  errorCode?: FimConfigValidationErrorCode;
  normalized?: FimConfig;
}

const textEncoder = new TextEncoder();

export function getFimDebounceMs(mode: FimMode): number {
  return FIM_DEBOUNCE_MS[mode] ?? 0;
}

/**
 * Compute UTF-8 byte length of a single Unicode code point.
 */
function utf8ByteLengthOfCodePoint(cp: number): number {
  if (cp <= 0x7f) return 1;
  if (cp <= 0x7ff) return 2;
  if (cp <= 0xffff) return 3;
  return 4;
}

/**
 * Slice the trailing window of `text` whose UTF-8 encoded length is strictly `<= maxBytes`
 * without ever splitting a UTF-16 surrogate pair.
 */
export function sliceTrailingUtf8Bytes(text: string, maxBytes: number): string {
  if (maxBytes <= 0 || text.length === 0) return "";
  if (text.length * 3 <= maxBytes && textEncoder.encode(text).length <= maxBytes) {
    return text;
  }

  let bytes = 0;
  let start = text.length;
  while (start > 0) {
    let charStart = start - 1;
    const code = text.charCodeAt(charStart);
    let cp = code;
    if (code >= 0xdc00 && code <= 0xdfff && charStart > 0) {
      const high = text.charCodeAt(charStart - 1);
      if (high >= 0xd800 && high <= 0xdbff) {
        charStart -= 1;
        cp = ((high - 0xd800) << 10) + (code - 0xdc00) + 0x10000;
      }
    }
    const need = utf8ByteLengthOfCodePoint(cp);
    if (bytes + need > maxBytes) {
      break;
    }
    bytes += need;
    start = charStart;
  }
  return text.slice(start);
}

/**
 * Slice the leading window of `text` whose UTF-8 encoded length is strictly `<= maxBytes`
 * without ever splitting a UTF-16 surrogate pair.
 */
export function sliceLeadingUtf8Bytes(text: string, maxBytes: number): string {
  if (maxBytes <= 0 || text.length === 0) return "";
  if (text.length * 3 <= maxBytes && textEncoder.encode(text).length <= maxBytes) {
    return text;
  }

  let bytes = 0;
  let end = 0;
  while (end < text.length) {
    const code = text.charCodeAt(end);
    let step = 1;
    let cp = code;
    if (code >= 0xd800 && code <= 0xdbff && end + 1 < text.length) {
      const low = text.charCodeAt(end + 1);
      if (low >= 0xdc00 && low <= 0xdfff) {
        step = 2;
        cp = ((code - 0xd800) << 10) + (low - 0xdc00) + 0x10000;
      }
    }
    const need = utf8ByteLengthOfCodePoint(cp);
    if (bytes + need > maxBytes) {
      break;
    }
    bytes += need;
    end += step;
  }
  return text.slice(0, end);
}

/**
 * Extract daemon-compliant bounded UTF-8 prefix (<= 16 KiB), suffix (<= 8 KiB),
 * and 1-based `(cursor_line, cursor_col)` coordinates at `offset`.
 */
export function extractFimContext(docText: string, offset: number): FimContextWindow {
  const clamped = Math.max(0, Math.min(Math.floor(offset), docText.length));
  const rawPrefix = docText.slice(0, clamped);
  const rawSuffix = docText.slice(clamped);

  let line = 1;
  let lastNewline = -1;
  for (let i = 0; i < clamped; i += 1) {
    if (docText.charCodeAt(i) === 10) {
      line += 1;
      lastNewline = i;
    }
  }

  return {
    prefix: sliceTrailingUtf8Bytes(rawPrefix, FIM_PREFIX_MAX_BYTES),
    suffix: sliceLeadingUtf8Bytes(rawSuffix, FIM_SUFFIX_MAX_BYTES),
    cursor_line: line,
    cursor_col: clamped - lastNewline,
  };
}

/**
 * Split the next logical word/token chunk off a ghost suggestion for progressive `Mod-ArrowRight`
 * acceptance. Consumes leading horizontal whitespace + one identifier/number/punctuation run, or
 * a single newline break + leading indentation of the next line.
 */
export function takeNextGhostToken(ghostText: string): { accepted: string; remaining: string } {
  if (!ghostText) {
    return { accepted: "", remaining: "" };
  }

  // Leading newline: accept the newline plus indentation of the following line
  const newlineMatch = ghostText.match(/^\r?\n[ \t]*/);
  if (newlineMatch) {
    const accepted = newlineMatch[0];
    return { accepted, remaining: ghostText.slice(accepted.length) };
  }

  // Leading horizontal whitespace followed by one word or one non-word run on the same line
  const tokenMatch = ghostText.match(/^[ \t]*(?:[\p{L}\p{N}_$]+|[^\s\p{L}\p{N}_$]+)/u);
  if (tokenMatch && tokenMatch[0].length > 0) {
    const accepted = tokenMatch[0];
    return { accepted, remaining: ghostText.slice(accepted.length) };
  }

  // Trailing whitespace only on current line
  const wsMatch = ghostText.match(/^[ \t]+/);
  if (wsMatch) {
    const accepted = wsMatch[0];
    return { accepted, remaining: ghostText.slice(accepted.length) };
  }

  // Fallback: single code point
  const firstCp = Array.from(ghostText)[0] ?? "";
  return { accepted: firstCp, remaining: ghostText.slice(firstCp.length) };
}

function isExplicitLoopbackHost(url: URL): boolean {
  const host = url.hostname.replace(/\.$/, "").replace(/^\[|\]$/g, "").toLowerCase();
  return host === "localhost" || host === "127.0.0.1" || host === "::1";
}

/**
 * Validate and normalize a host-scoped FIM configuration form draft, mirroring the hardened
 * daemon checks in `crates/orbit-daemon/src/fim.rs`.
 */
export function validateFimConfigDraft(draft: FimConfigDraft): FimConfigValidationResult {
  const endpointRaw = draft.endpointUrl.trim();
  const modelRaw = draft.model.trim();
  const apiKeyEnvRaw = draft.apiKeyEnv.trim();

  let parsedEndpoint: URL | null = null;
  if (endpointRaw.length > 0) {
    try {
      parsedEndpoint = new URL(endpointRaw);
    } catch {
      return { valid: false, errorCode: "invalid_endpoint_url" };
    }
    if (parsedEndpoint.protocol !== "http:" && parsedEndpoint.protocol !== "https:") {
      return { valid: false, errorCode: "invalid_endpoint_url" };
    }
    const validPath =
      parsedEndpoint.pathname.endsWith("/api/generate") ||
      parsedEndpoint.pathname.endsWith("/v1/completions");
    if (!validPath || parsedEndpoint.search.length > 0 || parsedEndpoint.hash.length > 0) {
      return { valid: false, errorCode: "invalid_endpoint_path" };
    }
  }

  if (apiKeyEnvRaw.length > 0) {
    if (
      apiKeyEnvRaw.length <= FIM_API_KEY_ENV_PREFIX.length ||
      apiKeyEnvRaw.length > 128 ||
      !apiKeyEnvRaw.startsWith(FIM_API_KEY_ENV_PREFIX) ||
      !/^[A-Z0-9_]+$/.test(apiKeyEnvRaw)
    ) {
      return { valid: false, errorCode: "invalid_api_key_env" };
    }
    if (
      parsedEndpoint &&
      parsedEndpoint.protocol !== "https:" &&
      !isExplicitLoopbackHost(parsedEndpoint)
    ) {
      return { valid: false, errorCode: "insecure_api_key_transport" };
    }
  }

  if (modelRaw.length > 0) {
    const hasControlOrSpace = /[\x00-\x1f\x7f\s]/.test(modelRaw);
    if (modelRaw.length > 256 || modelRaw.startsWith("-") || hasControlOrSpace) {
      return { valid: false, errorCode: "invalid_model" };
    }
  } else if (parsedEndpoint && parsedEndpoint.pathname.endsWith("/api/generate")) {
    return { valid: false, errorCode: "ollama_model_required" };
  }

  const parsedMaxTokens = Number(draft.maxTokens);
  const maxTokens = Number.isFinite(parsedMaxTokens)
    ? Math.max(1, Math.min(2048, Math.round(parsedMaxTokens)))
    : DEFAULT_FIM_CONFIG.max_tokens;

  const parsedTimeout = Number(draft.timeoutMs);
  const timeoutMs = Number.isFinite(parsedTimeout)
    ? Math.max(200, Math.min(5000, Math.round(parsedTimeout)))
    : DEFAULT_FIM_CONFIG.timeout_ms;

  return {
    valid: true,
    normalized: {
      mode: draft.mode,
      endpoint_url: endpointRaw.length > 0 ? endpointRaw : null,
      model: modelRaw.length > 0 ? modelRaw : null,
      api_key_env: apiKeyEnvRaw.length > 0 ? apiKeyEnvRaw : null,
      max_tokens: maxTokens,
      timeout_ms: timeoutMs,
      allow_fast_cli_fallback: Boolean(draft.allowFastCliFallback),
    },
  };
}

/**
 * Safety predicate enforced before scheduling, storing, rendering, or accepting ghost text.
 * Blocks read-only buffers, IME composition, multi-cursor, and non-empty selections.
 */
export function canShowGhostText(state: EditorState, composing = false): boolean {
  if (composing) return false;
  if (state.readOnly) return false;
  if (state.selection.ranges.length !== 1) return false;
  if (!state.selection.main.empty) return false;
  return true;
}

export const setGhostTextEffect = StateEffect.define<GhostTextSuggestion | null>();
export const clearGhostTextEffect = StateEffect.define<null>();
export const fimGhostActionAnnotation = Annotation.define<"accept_all" | "accept_word" | "dismiss">();

class GhostTextWidget extends WidgetType {
  private readonly text: string;

  constructor(text: string) {
    super();
    this.text = text;
  }

  eq(other: GhostTextWidget): boolean {
    return other.text === this.text;
  }

  toDOM(): HTMLElement {
    const span = document.createElement("span");
    span.className = this.text.includes("\n")
      ? "orbit-cm-ghost-text orbit-cm-ghost-text-multiline"
      : "orbit-cm-ghost-text";
    span.setAttribute("aria-hidden", "true");
    span.textContent = this.text;
    return span;
  }

  ignoreEvent(): boolean {
    return true;
  }
}

function tryAdvanceGhostOnTypeThrough(
  prev: GhostTextSuggestion,
  tr: Transaction,
): GhostTextSuggestion | null {
  if (!canShowGhostText(tr.state)) return null;
  let insertedText: string | null = null;
  let validSingleInsert = true;
  let changeCount = 0;

  tr.changes.iterChanges((fromA, toA, _fromB, _toB, inserted) => {
    changeCount += 1;
    if (changeCount > 1 || fromA !== prev.pos || toA !== prev.pos) {
      validSingleInsert = false;
      return;
    }
    insertedText = inserted.toString();
  });

  if (!validSingleInsert || !insertedText) return null;
  const typed: string = insertedText;
  if (typed.length === 0 || !prev.text.startsWith(typed)) return null;

  const remaining = prev.text.slice(typed.length);
  const nextPos = prev.pos + typed.length;
  if (remaining.length === 0 || tr.state.selection.main.head !== nextPos) {
    return null;
  }
  return {
    ...prev,
    text: remaining,
    pos: nextPos,
  };
}

export const ghostTextField = StateField.define<GhostTextSuggestion | null>({
  create() {
    return null;
  },
  update(value, tr) {
    if (!canShowGhostText(tr.state)) {
      return null;
    }

    let next: GhostTextSuggestion | null | undefined = undefined;
    for (const effect of tr.effects) {
      if (effect.is(clearGhostTextEffect)) {
        next = null;
      } else if (effect.is(setGhostTextEffect)) {
        const candidate = effect.value;
        if (
          candidate &&
          candidate.text.length > 0 &&
          candidate.pos === tr.state.selection.main.head &&
          candidate.pos >= 0 &&
          candidate.pos <= tr.state.doc.length
        ) {
          next = candidate;
        } else {
          next = null;
        }
      }
    }

    if (next !== undefined) {
      return next;
    }

    if (!value) {
      return null;
    }

    if (tr.docChanged) {
      return tryAdvanceGhostOnTypeThrough(value, tr);
    }

    if (tr.selection) {
      return tr.state.selection.main.head === value.pos ? value : null;
    }

    return value;
  },
  provide(field) {
    return EditorView.decorations.from(field, (suggestion): DecorationSet => {
      if (!suggestion || suggestion.text.length === 0) {
        return Decoration.none;
      }
      return Decoration.set([
        Decoration.widget({
          widget: new GhostTextWidget(suggestion.text),
          side: 1,
        }).range(suggestion.pos),
      ]);
    });
  },
});

export function getGhostText(state: EditorState): GhostTextSuggestion | null {
  return state.field(ghostTextField, false) ?? null;
}

/**
 * Accept the entire active ghost suggestion cleanly at the cursor (`Tab`).
 * Returns `false` when no ghost text is active or safety guards block acceptance,
 * allowing normal Tab indentation or autocomplete to proceed.
 */
export function acceptGhostText(view: FimEditorLike): boolean {
  if (!canShowGhostText(view.state, view.composing)) return false;
  const ghost = getGhostText(view.state);
  if (!ghost || ghost.text.length === 0 || ghost.pos !== view.state.selection.main.head) {
    return false;
  }
  const endPos = ghost.pos + ghost.text.length;
  view.dispatch({
    changes: { from: ghost.pos, to: ghost.pos, insert: ghost.text },
    selection: EditorSelection.cursor(endPos),
    effects: clearGhostTextEffect.of(null),
    annotations: fimGhostActionAnnotation.of("accept_all"),
    userEvent: "input.complete.fim",
  });
  return true;
}

/**
 * Accept the next word/token of the active ghost suggestion (`Mod-ArrowRight`),
 * keeping the remainder inline for progressive acceptance.
 */
export function acceptGhostTextWord(view: FimEditorLike): boolean {
  if (!canShowGhostText(view.state, view.composing)) return false;
  const ghost = getGhostText(view.state);
  if (!ghost || ghost.text.length === 0 || ghost.pos !== view.state.selection.main.head) {
    return false;
  }
  const { accepted, remaining } = takeNextGhostToken(ghost.text);
  if (!accepted) return false;

  const nextPos = ghost.pos + accepted.length;
  view.dispatch({
    changes: { from: ghost.pos, to: ghost.pos, insert: accepted },
    selection: EditorSelection.cursor(nextPos),
    effects:
      remaining.length > 0
        ? setGhostTextEffect.of({ ...ghost, text: remaining, pos: nextPos })
        : clearGhostTextEffect.of(null),
    annotations: fimGhostActionAnnotation.of("accept_word"),
    userEvent: "input.complete.fim.word",
  });
  return true;
}

/**
 * Dismiss active ghost text immediately (`Escape`).
 * Returns `false` when no ghost text is active so Escape continues to close panels/popovers.
 */
export function dismissGhostText(view: FimEditorLike): boolean {
  const ghost = getGhostText(view.state);
  if (!ghost) return false;
  view.dispatch({
    effects: clearGhostTextEffect.of(null),
    annotations: fimGhostActionAnnotation.of("dismiss"),
  });
  return true;
}

export const ghostTextKeymap: Extension = Prec.highest(
  keymap.of([
    {
      key: "Tab",
      run: acceptGhostText,
    },
    {
      key: "Mod-ArrowRight",
      run: acceptGhostTextWord,
    },
    {
      key: "Escape",
      run: dismissGhostText,
    },
  ]),
);

export interface FimRequestPayload {
  prefix: string;
  suffix: string;
  cursor_line: number;
  cursor_col: number;
  offset: number;
  docText: string;
}

export interface FimSchedulerOptions {
  mode: FimMode;
  requestPrediction: (payload: FimRequestPayload) => Promise<InlineEditResult | null>;
  onResult: (suggestion: GhostTextSuggestion) => void;
  onStatusChange?: (status: FimRuntimeStatus) => void;
  setTimer?: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>;
  clearTimer?: (handle: ReturnType<typeof setTimeout>) => void;
}

/**
 * Monotonic, debounced FIM prediction scheduler.
 * Guarantees stale in-flight responses are discarded if the user edits, moves the cursor,
 * starts IME composition, blurs the editor, or changes FIM mode before resolution.
 */
export class FimPredictionScheduler {
  private mode: FimMode;
  private readonly requestPrediction: FimSchedulerOptions["requestPrediction"];
  private readonly onResult: FimSchedulerOptions["onResult"];
  private readonly onStatusChange?: FimSchedulerOptions["onStatusChange"];
  private readonly setTimer: (fn: () => void, ms: number) => ReturnType<typeof setTimeout>;
  private readonly clearTimer: (handle: ReturnType<typeof setTimeout>) => void;
  private timer: ReturnType<typeof setTimeout> | null = null;
  private seq = 0;
  private disposed = false;

  constructor(options: FimSchedulerOptions) {
    this.mode = options.mode;
    this.requestPrediction = options.requestPrediction;
    this.onResult = options.onResult;
    this.onStatusChange = options.onStatusChange;
    this.setTimer = options.setTimer ?? ((fn, ms) => setTimeout(fn, ms));
    this.clearTimer = options.clearTimer ?? ((handle) => clearTimeout(handle));
  }

  get currentSeq(): number {
    return this.seq;
  }

  setMode(mode: FimMode): void {
    this.mode = mode;
    if (mode === "off") {
      this.cancel();
    }
  }

  cancel(emitIdle = true): void {
    this.seq += 1;
    if (this.timer !== null) {
      this.clearTimer(this.timer);
      this.timer = null;
    }
    if (emitIdle && !this.disposed) {
      this.onStatusChange?.({ phase: "idle" });
    }
  }

  dispose(): void {
    this.disposed = true;
    this.cancel(false);
  }

  schedule(getSnapshot: () => { docText: string; offset: number; allowed: boolean }): boolean {
    this.cancel(false);
    const delayMs = getFimDebounceMs(this.mode);
    if (this.disposed || delayMs <= 0) {
      this.onStatusChange?.({ phase: "idle" });
      return false;
    }

    const initial = getSnapshot();
    if (!initial.allowed) {
      this.onStatusChange?.({ phase: "idle" });
      return false;
    }

    const reqSeq = this.seq;
    this.timer = this.setTimer(() => {
      this.timer = null;
      if (this.disposed || reqSeq !== this.seq || this.mode === "off") {
        return;
      }

      const snap = getSnapshot();
      if (!snap.allowed || snap.docText !== initial.docText || snap.offset !== initial.offset) {
        this.onStatusChange?.({ phase: "idle" });
        return;
      }

      const ctx = extractFimContext(snap.docText, snap.offset);
      if (ctx.prefix.trim().length === 0 && ctx.suffix.trim().length === 0) {
        this.onStatusChange?.({ phase: "idle" });
        return;
      }

      this.onStatusChange?.({ phase: "pending" });

      void this.requestPrediction({
        prefix: ctx.prefix,
        suffix: ctx.suffix,
        cursor_line: ctx.cursor_line,
        cursor_col: ctx.cursor_col,
        offset: snap.offset,
        docText: snap.docText,
      })
        .then((res) => {
          if (this.disposed || reqSeq !== this.seq || this.mode === "off") {
            return;
          }
          const latest = getSnapshot();
          if (
            !latest.allowed ||
            latest.docText !== snap.docText ||
            latest.offset !== snap.offset
          ) {
            this.onStatusChange?.({ phase: "idle" });
            return;
          }

          const completion = res?.completion ?? "";
          if (!completion) {
            this.onStatusChange?.({
              phase: "idle",
              source: res?.source,
              latencyMs: res?.latency_ms,
            });
            return;
          }

          this.onResult({
            text: completion,
            pos: latest.offset,
            requestId: res?.request_id,
            source: res?.source,
            latencyMs: res?.latency_ms,
          });
          this.onStatusChange?.({
            phase: "active",
            source: res?.source,
            latencyMs: res?.latency_ms,
          });
        })
        .catch((err) => {
          if (this.disposed || reqSeq !== this.seq) {
            return;
          }
          this.onStatusChange?.({
            phase: "error",
            error: err instanceof Error ? err.message : String(err),
          });
        });
    }, delayMs);

    return true;
  }
}

export interface CreateFimGhostTextExtensionOptions {
  mode: FimMode;
  enabled?: boolean;
  requestPrediction: (payload: FimRequestPayload) => Promise<InlineEditResult | null>;
  onStatusChange?: (status: FimRuntimeStatus) => void;
}

/**
 * Create the complete CodeMirror 6 Inline FIM Ghost Text extension bundle suitable
 * for mounting inside a reactive `Compartment`.
 */
export function createFimGhostTextExtension(
  options: CreateFimGhostTextExtensionOptions,
): Extension {
  const effectiveMode: FimMode = options.enabled === false ? "off" : options.mode;

  if (effectiveMode === "off") {
    return [];
  }

  const plugin = ViewPlugin.fromClass(
    class FimGhostTextPlugin {
      private readonly scheduler: FimPredictionScheduler;
      private readonly view: EditorView;

      constructor(view: EditorView) {
        this.view = view;
        this.scheduler = new FimPredictionScheduler({
          mode: effectiveMode,
          requestPrediction: options.requestPrediction,
          onStatusChange: options.onStatusChange,
          onResult: (suggestion) => {
            if (!canShowGhostText(this.view.state, this.view.composing)) return;
            if (suggestion.pos !== this.view.state.selection.main.head) return;
            this.view.dispatch({
              effects: setGhostTextEffect.of(suggestion),
            });
          },
        });
      }

      private getSnapshot(): { docText: string; offset: number; allowed: boolean } {
        const state = this.view.state;
        const allowed = canShowGhostText(state, this.view.composing) && this.view.hasFocus;
        return {
          docText: state.doc.toString(),
          offset: state.selection.main.head,
          allowed,
        };
      }

      update(update: ViewUpdate): void {
        if (this.view.composing || !canShowGhostText(update.state, this.view.composing)) {
          this.scheduler.cancel();
          if (getGhostText(update.state)) {
            queueMicrotask(() => {
              if (getGhostText(this.view.state)) {
                this.view.dispatch({ effects: clearGhostTextEffect.of(null) });
              }
            });
          }
          return;
        }

        const isFimAction = update.transactions.some(
          (tr) => tr.annotation(fimGhostActionAnnotation) !== undefined,
        );
        if (isFimAction) {
          this.scheduler.cancel(false);
          const stillHasGhost = Boolean(getGhostText(update.state));
          options.onStatusChange?.({ phase: stillHasGhost ? "active" : "idle" });
          return;
        }

        if (update.docChanged) {
          // Ignore external/programmatic full-buffer reloads (non-user events without input/delete)
          const isUserEdit = update.transactions.some(
            (tr) =>
              tr.isUserEvent("input") ||
              tr.isUserEvent("delete") ||
              tr.isUserEvent("undo") ||
              tr.isUserEvent("redo"),
          );
          if (!isUserEdit) {
            this.scheduler.cancel();
            return;
          }
          // If type-through kept a valid ghost suffix, keep showing it cleanly without flickering
          if (getGhostText(update.state)) {
            this.scheduler.cancel(false);
            options.onStatusChange?.({ phase: "active" });
            return;
          }
          this.scheduler.schedule(() => this.getSnapshot());
          return;
        }

        if (update.selectionSet || (update.focusChanged && !this.view.hasFocus)) {
          this.scheduler.cancel();
        }
      }

      destroy(): void {
        this.scheduler.dispose();
      }
    },
  );

  const domGuards = EditorView.domEventHandlers({
    compositionstart(_event, view) {
      if (getGhostText(view.state)) {
        view.dispatch({ effects: clearGhostTextEffect.of(null) });
      }
      return false;
    },
    blur(_event, view) {
      if (getGhostText(view.state)) {
        view.dispatch({ effects: clearGhostTextEffect.of(null) });
      }
      return false;
    },
  });

  return [ghostTextField, ghostTextKeymap, plugin, domGuards];
}
