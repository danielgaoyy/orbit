import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { MentionCandidateView, WorkspaceSymbolEntry } from "./orbit-ipc.ts";
import {
  containsInlineMentionToken,
  extractLineSnippet,
  formatSymbolMentionToken,
  mergeMentionCandidates,
  parseSymbolMentionQuery,
  resolveDisambiguatedSymbolToken,
  symbolKindGlyph,
  workspaceSymbolToMentionCandidate,
} from "./symbol-mentions.ts";

describe("symbol-mentions parsing, ranking merge, and token formatting", () => {
  it("parses @sym: and @# as symbol-only triggers and plain @query as unified", () => {
    assert.deepEqual(parseSymbolMentionQuery("sym:searchSym"), {
      symbolOnly: true,
      query: "searchSym",
    });
    assert.deepEqual(parseSymbolMentionQuery("SYM: "), {
      symbolOnly: true,
      query: "",
    });
    assert.deepEqual(parseSymbolMentionQuery("#OmniComposer"), {
      symbolOnly: true,
      query: "OmniComposer",
    });
    assert.deepEqual(parseSymbolMentionQuery("#"), {
      symbolOnly: true,
      query: "",
    });
    assert.deepEqual(parseSymbolMentionQuery("rankMention"), {
      symbolOnly: false,
      query: "rankMention",
    });
  });

  it("maps symbol kinds to crisp monochrome glyphs", () => {
    assert.equal(symbolKindGlyph("fn").glyph, "ƒ");
    assert.equal(symbolKindGlyph("method").glyph, "ƒ");
    assert.equal(symbolKindGlyph("struct").glyph, "S");
    assert.equal(symbolKindGlyph("class").glyph, "C");
    assert.equal(symbolKindGlyph("enum").glyph, "E");
    assert.equal(symbolKindGlyph("trait").glyph, "T");
    assert.equal(symbolKindGlyph("interface").glyph, "T");
    assert.equal(symbolKindGlyph("const").glyph, "K");
  });

  it("converts daemon entries to MentionCandidateView and merges with files cleanly", () => {
    const entry: WorkspaceSymbolEntry = {
      name: "search_workspace_symbols",
      kind: "fn",
      path: "crates/orbit-daemon/src/symbols.rs",
      start_line: 42,
      end_line: 88,
      signature: "pub async fn search_workspace_symbols(...)",
      container: "WorkspaceSymbolIndex",
      language: "rust",
    };

    const symCandidate = workspaceSymbolToMentionCandidate(entry, 0);
    assert.equal(symCandidate.kind, "code_symbol");
    assert.equal(symCandidate.token_label, "@search_workspace_symbols");
    assert.equal(symCandidate.subtitle, "crates/orbit-daemon/src/symbols.rs:L42");
    assert.equal(symCandidate.start_line, 42);
    assert.equal(symCandidate.end_line, 88);
    assert.equal(formatSymbolMentionToken(" OrbitEngine "), "@OrbitEngine");

    const fileCandidate: MentionCandidateView = {
      key: "file::crates/orbit-daemon/src/symbols.rs",
      kind: "file",
      token_label: "@symbols.rs",
      title: "symbols.rs",
      subtitle: "crates/orbit-daemon/src",
      file_path: "crates/orbit-daemon/src/symbols.rs",
      git_status: "M",
      score: 100,
    };

    const unified = mergeMentionCandidates([fileCandidate], [symCandidate], false, 12);
    assert.equal(unified.length, 2);
    assert.equal(unified[0]?.kind, "file");
    assert.equal(unified[1]?.kind, "code_symbol");

    const symbolOnly = mergeMentionCandidates([fileCandidate], [symCandidate], true, 12);
    assert.equal(symbolOnly.length, 1);
    assert.equal(symbolOnly[0]?.kind, "code_symbol");
  });

  it("extracts exact 1-based line-numbered snippets and truncates cleanly", () => {
    const src = [
      "line one",
      "pub fn demo() {",
      "    println!(\"hello\");",
      "}",
      "tail",
    ].join("\n");

    const exact = extractLineSnippet(src, 2, 4, 2000);
    assert.equal(exact, "2 | pub fn demo() {\n3 |     println!(\"hello\");\n4 | }");

    const clipped = extractLineSnippet(src, 2, 4, 24);
    assert.ok(clipped.includes("2 | pub fn demo() {"));
    assert.ok(clipped.includes("truncated"));
  });

  it("matches inline @tokens strictly on token boundaries", () => {
    assert.equal(containsInlineMentionToken("Explain @run_turn logic", "@run"), false);
    assert.equal(containsInlineMentionToken("Explain @run:L12 logic", "@run"), false);
    assert.equal(containsInlineMentionToken("Explain @run::inner logic", "@run"), false);
    assert.equal(containsInlineMentionToken("Explain @run logic", "@run"), true);
    assert.equal(containsInlineMentionToken("Explain (@run), please", "@run"), true);
    assert.equal(containsInlineMentionToken("Explain @run: why?", "@run"), true);
    assert.equal(containsInlineMentionToken("Explain @run:L12 logic", "@run:L12"), true);
  });

  it("disambiguates colliding same-named symbols without clobbering earlier citations", () => {
    const first = {
      token: "@new",
      kind: "code_symbol",
      path: "src/a.rs",
      symbolName: "new",
      startLine: 10,
    };
    const draftWithFirst = "Compare @new with ";

    // Re-selecting the exact same symbol reuses `@new`
    assert.equal(
      resolveDisambiguatedSymbolToken(
        { symbolName: "new", path: "src/a.rs", startLine: 10, container: "Alpha" },
        [first],
        draftWithFirst,
      ),
      "@new",
    );

    // Selecting a different `new` with a container disambiguates to `@Beta::new`
    assert.equal(
      resolveDisambiguatedSymbolToken(
        { symbolName: "new", path: "src/b.rs", startLine: 42, container: "Beta" },
        [first],
        draftWithFirst,
      ),
      "@Beta::new",
    );

    // Selecting a different `new` without container disambiguates to `@new:L42`
    assert.equal(
      resolveDisambiguatedSymbolToken(
        { symbolName: "new", path: "src/c.rs", startLine: 42, container: null },
        [first],
        draftWithFirst,
      ),
      "@new:L42",
    );
  });
});
