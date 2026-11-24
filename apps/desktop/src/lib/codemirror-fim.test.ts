import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { EditorSelection, EditorState, type TransactionSpec } from "@codemirror/state";
import {
  FIM_PREFIX_MAX_BYTES,
  FIM_SUFFIX_MAX_BYTES,
  FimPredictionScheduler,
  acceptGhostText,
  acceptGhostTextWord,
  canShowGhostText,
  clearGhostTextEffect,
  createFimGhostTextExtension,
  dismissGhostText,
  extractFimContext,
  getFimDebounceMs,
  getGhostText,
  ghostTextField,
  setGhostTextEffect,
  sliceLeadingUtf8Bytes,
  sliceTrailingUtf8Bytes,
  takeNextGhostToken,
  validateFimConfigDraft,
  type FimEditorLike,
  type FimRuntimeStatus,
  type GhostTextSuggestion,
} from "./codemirror-fim.ts";
import type { InlineEditResult } from "./orbit-ipc.ts";

function createTestEditor(
  doc: string,
  options?: {
    selection?: EditorSelection;
    readOnly?: boolean;
    allowMultipleSelections?: boolean;
    composing?: boolean;
  },
): FimEditorLike & { setComposing: (value: boolean) => void } {
  let state = EditorState.create({
    doc,
    selection: options?.selection ?? EditorSelection.cursor(doc.length),
    extensions: [
      ghostTextField,
      ...(options?.readOnly ? [EditorState.readOnly.of(true)] : []),
      ...(options?.allowMultipleSelections ? [EditorState.allowMultipleSelections.of(true)] : []),
    ],
  });
  let composing = options?.composing ?? false;
  return {
    get state() {
      return state;
    },
    get composing() {
      return composing;
    },
    setComposing(value: boolean) {
      composing = value;
    },
    dispatch(spec: TransactionSpec) {
      state = state.update(spec).state;
    },
  };
}

describe("CodeMirror 6 Inline FIM Ghost Text", () => {
  it("maps FIM modes to exact debounce timings", () => {
    assert.equal(getFimDebounceMs("eager"), 250);
    assert.equal(getFimDebounceMs("subtle"), 650);
    assert.equal(getFimDebounceMs("off"), 0);
  });

  it("extracts daemon-bounded UTF-8 prefix (<=16 KiB) and suffix (<=8 KiB) without splitting surrogate pairs", () => {
    const encoder = new TextEncoder();
    const emojiChunk = "🚀中文abc"; // 4 + 6 + 3 = 13 UTF-8 bytes
    const hugePrefix = emojiChunk.repeat(2000); // ~26 KiB
    const hugeSuffix = emojiChunk.repeat(1000); // ~13 KiB
    const doc = `${hugePrefix}\nline2_here${hugeSuffix}`;
    const offset = hugePrefix.length + 1 + "line2_".length;

    const ctx = extractFimContext(doc, offset);
    assert.equal(ctx.cursor_line, 2);
    assert.equal(ctx.cursor_col, 7);
    assert.ok(encoder.encode(ctx.prefix).length <= FIM_PREFIX_MAX_BYTES);
    assert.ok(encoder.encode(ctx.suffix).length <= FIM_SUFFIX_MAX_BYTES);
    assert.ok(ctx.prefix.endsWith("\nline2_"));
    assert.ok(ctx.suffix.startsWith("here"));
    // Verify no broken UTF-16 surrogates at boundaries
    assert.equal(ctx.prefix.includes("\uFFFD"), false);
    assert.equal(ctx.suffix.includes("\uFFFD"), false);

    // Boundary edge cases for UTF-8 helpers
    const emoji4 = "🚀"; // 4 UTF-8 bytes, 2 UTF-16 code units
    assert.equal(sliceTrailingUtf8Bytes(`${emoji4}a`, 3), "a");
    assert.equal(sliceTrailingUtf8Bytes(`${emoji4}a`, 5), `${emoji4}a`);
    assert.equal(sliceLeadingUtf8Bytes(`${emoji4}a`, 3), "");
    assert.equal(sliceLeadingUtf8Bytes(`${emoji4}a`, 4), emoji4);
  });

  it("splits progressive word/token chunks cleanly for Mod-ArrowRight acceptance", () => {
    let text = "  const result = compute(a, b);\n  return result;";
    const acceptedChunks: string[] = [];
    while (text.length > 0) {
      const step = takeNextGhostToken(text);
      assert.ok(step.accepted.length > 0);
      acceptedChunks.push(step.accepted);
      text = step.remaining;
    }
    assert.deepEqual(acceptedChunks, [
      "  const",
      " result",
      " =",
      " compute",
      "(",
      "a",
      ",",
      " b",
      ");",
      "\n  ",
      "return",
      " result",
      ";",
    ]);
  });

  it("enforces safety guards for read-only buffers, IME composition, multi-cursor, and non-empty selections", () => {
    const readOnlyEditor = createTestEditor("const x = 1;", { readOnly: true });
    assert.equal(canShowGhostText(readOnlyEditor.state), false);
    readOnlyEditor.dispatch({
      effects: setGhostTextEffect.of({ text: " + 2", pos: 12 }),
    });
    assert.equal(getGhostText(readOnlyEditor.state), null);
    assert.equal(acceptGhostText(readOnlyEditor), false);

    const selectedEditor = createTestEditor("const x = 1;", {
      selection: EditorSelection.create([EditorSelection.range(0, 5)]),
    });
    assert.equal(canShowGhostText(selectedEditor.state), false);
    selectedEditor.dispatch({
      effects: setGhostTextEffect.of({ text: " + 2", pos: 5 }),
    });
    assert.equal(getGhostText(selectedEditor.state), null);

    const multiCursorEditor = createTestEditor("foo\nbar", {
      allowMultipleSelections: true,
      selection: EditorSelection.create([EditorSelection.cursor(3), EditorSelection.cursor(7)]),
    });
    assert.equal(canShowGhostText(multiCursorEditor.state), false);
    multiCursorEditor.dispatch({
      effects: setGhostTextEffect.of({ text: "!", pos: 3 }),
    });
    assert.equal(getGhostText(multiCursorEditor.state), null);

    const composingEditor = createTestEditor("const x = ");
    composingEditor.dispatch({
      effects: setGhostTextEffect.of({ text: "42;", pos: 10 }),
    });
    assert.equal(getGhostText(composingEditor.state)?.text, "42;");
    composingEditor.setComposing(true);
    assert.equal(canShowGhostText(composingEditor.state, true), false);
    assert.equal(acceptGhostText(composingEditor), false);
    assert.equal(acceptGhostTextWord(composingEditor), false);
    assert.equal(composingEditor.state.doc.toString(), "const x = ");
  });

  it("accepts full ghost text with Tab, partial words with Mod-ArrowRight, and dismisses with Escape", () => {
    // Full acceptance (Tab)
    const fullEd = createTestEditor("function add(a: number, b: number) {");
    assert.equal(acceptGhostText(fullEd), false);
    assert.equal(dismissGhostText(fullEd), false);

    fullEd.dispatch({
      effects: setGhostTextEffect.of({ text: "\n  return a + b;\n}", pos: 36 }),
    });
    assert.equal(getGhostText(fullEd.state)?.text, "\n  return a + b;\n}");
    assert.equal(acceptGhostText(fullEd), true);
    assert.equal(fullEd.state.doc.toString(), "function add(a: number, b: number) {\n  return a + b;\n}");
    assert.equal(fullEd.state.selection.main.head, fullEd.state.doc.length);
    assert.equal(getGhostText(fullEd.state), null);

    // Progressive word acceptance (Mod-ArrowRight)
    const wordEd = createTestEditor("let total = ");
    wordEd.dispatch({
      effects: setGhostTextEffect.of({ text: "price * qty;", pos: 12 }),
    });
    assert.equal(acceptGhostTextWord(wordEd), true);
    assert.equal(wordEd.state.doc.toString(), "let total = price");
    assert.equal(getGhostText(wordEd.state)?.text, " * qty;");
    assert.equal(getGhostText(wordEd.state)?.pos, 17);

    assert.equal(acceptGhostTextWord(wordEd), true);
    assert.equal(wordEd.state.doc.toString(), "let total = price *");
    assert.equal(getGhostText(wordEd.state)?.text, " qty;");

    // Dismiss remaining with Escape
    assert.equal(dismissGhostText(wordEd), true);
    assert.equal(getGhostText(wordEd.state), null);
    assert.equal(dismissGhostText(wordEd), false);
  });

  it("advances ghost text on matching type-through and clears on diverging edits or cursor moves", () => {
    const ed = createTestEditor("return ");
    ed.dispatch({
      effects: setGhostTextEffect.of({ text: "result;", pos: 7 }),
    });

    // Type-through matching prefix shrinks ghost text seamlessly
    ed.dispatch({
      changes: { from: 7, to: 7, insert: "res" },
      selection: EditorSelection.cursor(10),
    });
    assert.equal(getGhostText(ed.state)?.text, "ult;");
    assert.equal(getGhostText(ed.state)?.pos, 10);

    // Diverging edit clears immediately
    ed.dispatch({
      changes: { from: 10, to: 10, insert: "x" },
      selection: EditorSelection.cursor(11),
    });
    assert.equal(getGhostText(ed.state), null);

    // Cursor movement clears immediately
    ed.dispatch({
      effects: setGhostTextEffect.of({ text: ";", pos: 11 }),
    });
    assert.equal(getGhostText(ed.state)?.text, ";");
    ed.dispatch({ selection: EditorSelection.cursor(0) });
    assert.equal(getGhostText(ed.state), null);

    // Explicit clear effect works
    ed.dispatch({
      effects: setGhostTextEffect.of({ text: "// ", pos: 0 }),
    });
    ed.dispatch({ effects: clearGhostTextEffect.of(null) });
    assert.equal(getGhostText(ed.state), null);
  });

  it("cancels stale in-flight FIM predictions via monotonic request sequencing", async () => {
    const timers: Array<{ id: number; ms: number; fn: () => void; cleared: boolean }> = [];
    let nextTimerId = 1;
    const resolvers: Array<(value: InlineEditResult | null) => void> = [];
    const received: GhostTextSuggestion[] = [];
    const statuses: FimRuntimeStatus[] = [];

    let docText = "const a = ";
    let offset = docText.length;
    let allowed = true;

    const scheduler = new FimPredictionScheduler({
      mode: "eager",
      setTimer(fn, ms) {
        const entry = { id: nextTimerId++, ms, fn, cleared: false };
        timers.push(entry);
        return entry.id as unknown as ReturnType<typeof setTimeout>;
      },
      clearTimer(handle) {
        const id = handle as unknown as number;
        const found = timers.find((t) => t.id === id);
        if (found) found.cleared = true;
      },
      requestPrediction() {
        return new Promise<InlineEditResult | null>((resolve) => {
          resolvers.push(resolve);
        });
      },
      onResult(suggestion) {
        received.push(suggestion);
      },
      onStatusChange(status) {
        statuses.push(status);
      },
    });

    // First edit schedules at 250ms
    assert.equal(
      scheduler.schedule(() => ({ docText, offset, allowed })),
      true,
    );
    assert.equal(timers[0].ms, 250);
    timers[0].fn();
    assert.equal(resolvers.length, 1);

    // Second edit supersedes the first while first is still in flight
    docText = "const a = 1 + ";
    offset = docText.length;
    assert.equal(
      scheduler.schedule(() => ({ docText, offset, allowed })),
      true,
    );
    timers[1].fn();
    assert.equal(resolvers.length, 2);

    // Stale first response resolves AFTER second request started -> MUST be dropped
    resolvers[0]({
      request_id: "req-1",
      completion: "STALE",
      latency_ms: 40,
      source: "ollama",
    });
    await Promise.resolve();
    assert.equal(received.length, 0);

    // Fresh second response resolves -> applied cleanly
    resolvers[1]({
      request_id: "req-2",
      completion: "2;",
      latency_ms: 25,
      source: "ollama",
    });
    await Promise.resolve();
    assert.equal(received.length, 1);
    assert.equal(received[0].text, "2;");
    assert.equal(received[0].pos, offset);
    assert.equal(statuses[statuses.length - 1]?.phase, "active");

    // Switching to Off never schedules
    scheduler.setMode("off");
    assert.equal(
      scheduler.schedule(() => ({ docText, offset, allowed })),
      false,
    );

    // Guard disallowed (e.g. composing) never schedules even in subtle mode
    scheduler.setMode("subtle");
    allowed = false;
    assert.equal(
      scheduler.schedule(() => ({ docText, offset, allowed })),
      false,
    );

    // Returns empty extension array when Off
    const offExt = createFimGhostTextExtension({
      mode: "off",
      requestPrediction: async () => null,
    });
    assert.deepEqual(offExt, []);
  });

  it("validates FIM config drafts against hardened daemon rules (ORBIT_FIM_ env, endpoints, TLS/loopback)", () => {
    // Reject non-ORBIT_FIM_ env var
    const badEnv = validateFimConfigDraft({
      mode: "subtle",
      endpointUrl: "https://api.example.com/v1/completions",
      model: "qwen2.5-coder",
      apiKeyEnv: "OPENAI_API_KEY",
      maxTokens: 128,
      timeoutMs: 1800,
      allowFastCliFallback: false,
    });
    assert.equal(badEnv.valid, false);
    assert.equal(badEnv.errorCode, "invalid_api_key_env");

    // Reject plaintext http:// remote endpoint when using API key env
    const insecureHttp = validateFimConfigDraft({
      mode: "subtle",
      endpointUrl: "http://10.0.0.5:8080/v1/completions",
      model: "qwen2.5-coder",
      apiKeyEnv: "ORBIT_FIM_TOKEN",
      maxTokens: 128,
      timeoutMs: 1800,
      allowFastCliFallback: false,
    });
    assert.equal(insecureHttp.valid, false);
    assert.equal(insecureHttp.errorCode, "insecure_api_key_transport");

    // Allow http://127.0.0.1 loopback with ORBIT_FIM_ key
    const loopbackOk = validateFimConfigDraft({
      mode: "eager",
      endpointUrl: "http://127.0.0.1:11434/api/generate",
      model: "qwen2.5-coder:1.5b",
      apiKeyEnv: "ORBIT_FIM_LOCAL_KEY",
      maxTokens: 5000,
      timeoutMs: 100,
      allowFastCliFallback: true,
    });
    assert.equal(loopbackOk.valid, true);
    assert.deepEqual(loopbackOk.normalized, {
      mode: "eager",
      endpoint_url: "http://127.0.0.1:11434/api/generate",
      model: "qwen2.5-coder:1.5b",
      api_key_env: "ORBIT_FIM_LOCAL_KEY",
      max_tokens: 2048,
      timeout_ms: 200,
      allow_fast_cli_fallback: true,
    });

    // Reject Ollama endpoint without model
    const missingOllamaModel = validateFimConfigDraft({
      mode: "subtle",
      endpointUrl: "http://localhost:11434/api/generate",
      model: "",
      apiKeyEnv: "",
      maxTokens: 128,
      timeoutMs: 1800,
      allowFastCliFallback: false,
    });
    assert.equal(missingOllamaModel.valid, false);
    assert.equal(missingOllamaModel.errorCode, "ollama_model_required");

    // Reject invalid endpoint path or query
    const badPath = validateFimConfigDraft({
      mode: "subtle",
      endpointUrl: "https://api.example.com/v1/chat/completions",
      model: "codestral",
      apiKeyEnv: "",
      maxTokens: 128,
      timeoutMs: 1800,
      allowFastCliFallback: false,
    });
    assert.equal(badPath.valid, false);
    assert.equal(badPath.errorCode, "invalid_endpoint_path");
  });
});
