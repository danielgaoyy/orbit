import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  buildInlineAiEditPrompt,
  convertLspTextEditsToChanges,
  extractIdentifierAtOffset,
  lineColToOffset,
  mapLspCompletionItems,
  mapLspDiagnosticsToCodeMirror,
  offsetToLineCol,
  parseHoverContentBlocks,
  resolveCodeMirrorLanguageKey,
} from "./codemirror-helpers.ts";
import type { LspCompletionItem, LspDiagnostic, LspTextEdit } from "./orbit-ipc.ts";

describe("CodeMirror 6 editor helpers", () => {
  it("resolves language packs accurately from language id and file path", () => {
    assert.equal(resolveCodeMirrorLanguageKey("rs", "crates/orbit/src/lib.rs"), "rust");
    assert.equal(resolveCodeMirrorLanguageKey("tsx", "src/App.tsx"), "tsx");
    assert.equal(resolveCodeMirrorLanguageKey("ts", "src/index.ts"), "typescript");
    assert.equal(resolveCodeMirrorLanguageKey("jsx", "src/Widget.jsx"), "jsx");
    assert.equal(resolveCodeMirrorLanguageKey("js", "scripts/smoke.mjs"), "javascript");
    assert.equal(resolveCodeMirrorLanguageKey("py", "train.py"), "python");
    assert.equal(resolveCodeMirrorLanguageKey("go", "cmd/main.go"), "go");
    assert.equal(resolveCodeMirrorLanguageKey("cpp", "engine.hpp"), "cpp");
    assert.equal(resolveCodeMirrorLanguageKey("yaml", ".github/workflows/ci.yml"), "yaml");
    assert.equal(resolveCodeMirrorLanguageKey("json", "package.json"), "json");
    assert.equal(resolveCodeMirrorLanguageKey("css", "index.css"), "css");
    assert.equal(resolveCodeMirrorLanguageKey("html", "index.html"), "html");
    assert.equal(resolveCodeMirrorLanguageKey("markdown", "README.md"), "markdown");
    // Falls back to file extension when language is "text"
    assert.equal(resolveCodeMirrorLanguageKey("text", "config.yml"), "yaml");
    assert.equal(resolveCodeMirrorLanguageKey("text", "notes.txt"), "plain");
  });

  it("converts between 1-based UTF-16 line/col and CodeMirror offsets cleanly", () => {
    const doc = "let s = \"🚀\";\r\nconst x = 42;\n";
    // Line 1: `let s = "🚀";` -> `"🚀"` is 2 UTF-16 code units at offsets 9..11
    assert.equal(lineColToOffset(doc, 1, 1), 0);
    assert.equal(lineColToOffset(doc, 1, 10), 9);
    assert.equal(lineColToOffset(doc, 1, 12), 11);
    // Clamps past EOL before \r\n
    assert.equal(lineColToOffset(doc, 1, 999), 13);
    // Line 2 starts right after \r\n (offset 15)
    assert.equal(lineColToOffset(doc, 2, 1), 15);
    assert.equal(lineColToOffset(doc, 2, 7), 21);

    assert.deepEqual(offsetToLineCol(doc, 0), { line: 1, col: 1 });
    assert.deepEqual(offsetToLineCol(doc, 15), { line: 2, col: 1 });
    assert.deepEqual(offsetToLineCol(doc, 21), { line: 2, col: 7 });
  });

  it("maps LSP diagnostics to clamped CodeMirror ranges with visible non-empty spans", () => {
    const doc = "fn main() {\n  let x = unknown_var;\n}\n";
    const diags: LspDiagnostic[] = [
      {
        path: "src/main.rs",
        line: 2,
        col: 11,
        end_line: 2,
        end_col: 22,
        severity: "error",
        message: "cannot find value `unknown_var`",
        source: "rustc",
        code: "E0425",
      },
      {
        // Zero-width range expands cleanly by 1 char so underline is visible
        path: "src/main.rs",
        line: 1,
        col: 4,
        end_line: 1,
        end_col: 4,
        severity: "warning",
        message: "unused item",
        source: "clippy",
        code: null,
      },
    ];

    const mapped = mapLspDiagnosticsToCodeMirror(doc, diags);
    assert.equal(mapped.length, 2);
    assert.equal(doc.slice(mapped[0].from, mapped[0].to), "unknown_var");
    assert.equal(mapped[0].severity, "error");
    assert.equal(mapped[0].source, "rustc E0425");
    assert.equal(mapped[1].to - mapped[1].from, 1);
    assert.equal(mapped[1].severity, "warning");
    assert.equal(mapped[1].source, "clippy");
  });

  it("maps LSP completion items with kinds, details, docs, insert_text, and sort priority", () => {
    const items: LspCompletionItem[] = [
      {
        label: "compute_answer",
        kind: "function",
        detail: "fn() -> u32",
        documentation: "Returns 42",
        insert_text: "compute_answer()",
        sort_text: "0001",
        filter_text: "compute_answer",
      },
      {
        label: "Engine",
        kind: "struct",
        detail: "struct Engine",
        documentation: null,
        insert_text: "Engine",
        sort_text: "0002",
        filter_text: null,
      },
    ];

    const mapped = mapLspCompletionItems(items);
    assert.equal(mapped.length, 2);
    assert.equal(mapped[0].label, "compute_answer");
    assert.equal(mapped[0].type, "function");
    assert.equal(mapped[0].detail, "fn() -> u32");
    assert.equal(mapped[0].info, "Returns 42");
    assert.equal(mapped[0].apply, "compute_answer()");
    assert.equal(mapped[1].type, "type");
    assert.equal(mapped[1].apply, undefined);
    assert.ok((mapped[0].boost ?? 0) > (mapped[1].boost ?? 0));
  });

  it("converts same-file LSP text edits into sorted CodeMirror ChangeSpecs", () => {
    const doc = "let a = 1;\nlet b = 2;\n";
    const edits: LspTextEdit[] = [
      { line: 2, col: 5, end_line: 2, end_col: 6, new_text: "beta" },
      { line: 1, col: 5, end_line: 1, end_col: 6, new_text: "alpha" },
    ];
    const changes = convertLspTextEditsToChanges(doc, edits);
    assert.equal(changes.length, 2);
    assert.equal(doc.slice(changes[0].from, changes[0].to), "a");
    assert.equal(changes[0].insert, "alpha");
    assert.equal(doc.slice(changes[1].from, changes[1].to), "b");
    assert.equal(changes[1].insert, "beta");
  });

  it("extracts the identifier under cursor for F2 Rename", () => {
    const doc = "pub fn compute_answer(val: u32) {}";
    const hit = extractIdentifierAtOffset(doc, 12);
    assert.deepEqual(hit, { word: "compute_answer", from: 7, to: 21 });
    // Right at trailing boundary of identifier still resolves the identifier
    assert.equal(extractIdentifierAtOffset(doc, 21)?.word, "compute_answer");
    // Punctuation/whitespace returns null
    assert.equal(extractIdentifierAtOffset("   ", 1), null);
  });

  it("builds focused inline Cmd+K AI edit prompts for both selections and cursor-only context", () => {
    const withSel = buildInlineAiEditPrompt({
      filePath: "src/lsp.rs",
      language: "rust",
      startLine: 12,
      endLine: 28,
      selectedCode: "fn old() {\n    todo!()\n}",
      instruction: "Add proper error propagation",
    });
    assert.equal(withSel.rangeLabel, "L12-L28");
    assert.equal(
      withSel.displayPrompt,
      "Inline edit `src/lsp.rs:L12-L28`: Add proper error propagation",
    );
    assert.ok(withSel.wirePrompt.includes("Edit `src/lsp.rs` (L12-L28): Add proper error propagation"));
    assert.ok(withSel.wirePrompt.includes("```rust\nfn old() {\n    todo!()\n}\n```"));
    assert.ok(withSel.wirePrompt.includes("Target selection (`src/lsp.rs:L12-L28`):"));

    const cursorOnly = buildInlineAiEditPrompt({
      filePath: "src/app.ts",
      language: "typescript",
      startLine: 5,
      endLine: 5,
      selectedCode: "",
      surroundingContext: "const count = 0;",
      instruction: "Initialize from props",
    });
    assert.equal(cursorOnly.rangeLabel, "L5");
    assert.ok(cursorOnly.wirePrompt.includes("Cursor context (`src/app.ts:L5`):"));
    assert.ok(cursorOnly.wirePrompt.includes("const count = 0;"));
  });

  it("parses LSP hover markdown into safe structured code, divider, and paragraph blocks", () => {
    const blocks = parseHoverContentBlocks(
      "```rust\npub fn compute() -> u32\n```\n\n---\n\nComputes the value.\nSecond line.",
    );
    assert.deepEqual(blocks, [
      { kind: "code", language: "rust", code: "pub fn compute() -> u32" },
      { kind: "divider" },
      { kind: "paragraph", text: "Computes the value.\nSecond line." },
    ]);
  });
});
