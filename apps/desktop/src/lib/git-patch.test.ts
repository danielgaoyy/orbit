import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  NO_NEWLINE_MARKER,
  buildSingleHunkPatch,
  deriveDefaultPrTitle,
  extractFileHunks,
  normalizeAndValidatePatchPath,
  parseHunkHeader,
  type DiffHunkView,
} from "./git-patch.ts";
import type { DiffFileView, DiffLineView } from "./orbit-ipc.ts";

function line(
  kind: DiffLineView["kind"],
  content: string,
  oldLine: number | null = null,
  newLine: number | null = null,
): DiffLineView {
  return { kind, content, old_line: oldLine, new_line: newLine };
}

describe("parseHunkHeader", () => {
  it("parses standard headers, implicit single-line counts, and section headings", () => {
    assert.deepEqual(parseHunkHeader("@@ -10,6 +10,8 @@ export function demo()"), {
      oldStart: 10,
      oldCount: 6,
      newStart: 10,
      newCount: 8,
      sectionHeading: " export function demo()",
    });

    assert.deepEqual(parseHunkHeader("@@ -1 +1,3 @@"), {
      oldStart: 1,
      oldCount: 1,
      newStart: 1,
      newCount: 3,
      sectionHeading: "",
    });

    assert.deepEqual(parseHunkHeader("@@ -0,0 +1,4 @@\r"), {
      oldStart: 0,
      oldCount: 0,
      newStart: 1,
      newCount: 4,
      sectionHeading: "",
    });

    assert.equal(parseHunkHeader("not a header"), null);
    assert.equal(parseHunkHeader("@@ -a,1 +1,1 @@"), null);
  });
});

describe("normalizeAndValidatePatchPath", () => {
  it("normalizes safe relative paths while preserving literal spaces and special characters", () => {
    assert.equal(normalizeAndValidatePatchPath("src/app.ts"), "src/app.ts");
    assert.equal(normalizeAndValidatePatchPath("./src/./nested/file.ts"), "src/nested/file.ts");
    assert.equal(
      normalizeAndValidatePatchPath("docs/my design doc.md"),
      "docs/my design doc.md",
    );
    // Preserve trailing space verbatim (matches B2-01 no-trim security rule)
    assert.equal(normalizeAndValidatePatchPath("notes/report "), "notes/report ");
    assert.equal(
      normalizeAndValidatePatchPath("src/组件/file[1]?*.tsx"),
      "src/组件/file[1]?*.tsx",
    );
  });

  it("rejects traversal, absolute paths, .git internals, flags, quotes, backslashes, and control chars", () => {
    const invalid = [
      "",
      ".",
      "./",
      "../escape.ts",
      "src/../../etc/passwd",
      "/etc/passwd",
      "~/secret.txt",
      "C:/windows/system.ini",
      "-flag.ts",
      "./-flag.ts",
      ".git/config",
      "nested/.GIT/hooks/pre-commit",
      "src\\windows\\file.ts",
      '"src/quoted.ts"',
      "src/line1\nline2.ts",
      "src/tab\tfile.ts",
      "src/nul\0file.ts",
    ];
    for (const bad of invalid) {
      assert.throws(
        () => normalizeAndValidatePatchPath(bad),
        /Invalid patch path/,
        `Expected path to be rejected: ${JSON.stringify(bad)}`,
      );
    }
  });
});

describe("buildSingleHunkPatch", () => {
  it("formats a context + deletion + addition hunk with exact line counts and prefixes", () => {
    const hunk: DiffHunkView = {
      header: "@@ -10,4 +10,5 @@ function greet()",
      oldStart: 10,
      oldCount: 4,
      newStart: 10,
      newCount: 5,
      lines: [
        line("context", "function greet(name: string) {", 10, 10),
        line("removed", '  return "hi " + name;', 11, null),
        line("added", '  const clean = name.trim();', null, 11),
        line("added", '  return `hello ${clean}`;', null, 12),
        line("context", "}", 12, 13),
        line("context", "", 13, 14),
      ],
    };

    const patch = buildSingleHunkPatch("src/greet.ts", hunk);
    assert.equal(
      patch,
      [
        "diff --git a/src/greet.ts b/src/greet.ts",
        "--- a/src/greet.ts",
        "+++ b/src/greet.ts",
        "@@ -10,4 +10,5 @@ function greet()",
        " function greet(name: string) {",
        '-  return "hi " + name;',
        "+  const clean = name.trim();",
        "+  return `hello ${clean}`;",
        " }",
        " ",
        "",
      ].join("\n"),
    );
  });

  it("recomputes exact oldCount and newCount from body lines even if caller counts are stale", () => {
    const hunk: DiffHunkView = {
      oldStart: 5,
      oldCount: 999,
      newStart: 5,
      newCount: 999,
      lines: [
        line("context", "const a = 1;", 5, 5),
        line("added", "const b = 2;", null, 6),
        line("added", "const c = 3;", null, 7),
      ],
    };

    const patch = buildSingleHunkPatch("src/calc.ts", hunk);
    assert.ok(patch.includes("@@ -5,1 +5,3 @@\n"));
  });

  it("handles `\\ No newline at end of file` cleanly without inflating hunk counts", () => {
    const hunk: DiffHunkView = {
      header: "@@ -1,2 +1,2 @@",
      oldStart: 1,
      oldCount: 2,
      newStart: 1,
      newCount: 2,
      lines: [
        line("context", "first", 1, 1),
        line("removed", "old-last", 2, null),
        // Rust `parse_unified_diff` emits no-newline markers as `context`
        line("context", NO_NEWLINE_MARKER, 3, 2),
        line("added", "new-last", null, 2),
        // Also support explicit `meta` representation
        line("meta", NO_NEWLINE_MARKER, null, null),
      ],
    };

    const patch = buildSingleHunkPatch("README.md", hunk);
    assert.equal(
      patch,
      [
        "diff --git a/README.md b/README.md",
        "--- a/README.md",
        "+++ b/README.md",
        "@@ -1,2 +1,2 @@",
        " first",
        "-old-last",
        "\\ No newline at end of file",
        "+new-last",
        "\\ No newline at end of file",
        "",
      ].join("\n"),
    );
  });

  it("supports new-file and deleted-file hunks with `/dev/null` headers", () => {
    const addedPatch = buildSingleHunkPatch("src/new file.ts", {
      oldStart: 0,
      oldCount: 0,
      newStart: 1,
      newCount: 2,
      isNewFile: true,
      lines: [line("added", "export const x = 1;", null, 1), line("added", "export const y = 2;", null, 2)],
    });
    assert.equal(
      addedPatch,
      [
        "diff --git a/src/new file.ts b/src/new file.ts",
        "--- /dev/null",
        "+++ b/src/new file.ts",
        "@@ -0,0 +1,2 @@",
        "+export const x = 1;",
        "+export const y = 2;",
        "",
      ].join("\n"),
    );

    const deletedPatch = buildSingleHunkPatch("src/old.ts", {
      oldStart: 1,
      oldCount: 1,
      newStart: 0,
      newCount: 0,
      isDeletedFile: true,
      lines: [line("removed", "export const obsolete = true;", 1, null)],
    });
    assert.equal(
      deletedPatch,
      [
        "diff --git a/src/old.ts b/src/old.ts",
        "--- a/src/old.ts",
        "+++ /dev/null",
        "@@ -1,1 +0,0 @@",
        "-export const obsolete = true;",
        "",
      ].join("\n"),
    );
  });

  it("handles special characters in paths cleanly without breaking diff --git parity", () => {
    const patch = buildSingleHunkPatch("./docs/Q&A (draft) [v2]/笔记.md", {
      oldStart: 1,
      oldCount: 1,
      newStart: 1,
      newCount: 1,
      lines: [line("removed", "old", 1, null), line("added", "new", null, 1)],
    });
    assert.ok(patch.startsWith("diff --git a/docs/Q&A (draft) [v2]/笔记.md b/docs/Q&A (draft) [v2]/笔记.md\n"));
    assert.ok(patch.includes("--- a/docs/Q&A (draft) [v2]/笔记.md\n+++ b/docs/Q&A (draft) [v2]/笔记.md\n"));
  });

  it("rejects empty hunks, context-only hunks, truncated hunks, and embedded newlines", () => {
    assert.throws(
      () =>
        buildSingleHunkPatch("a.ts", {
          oldStart: 1,
          oldCount: 0,
          newStart: 1,
          newCount: 0,
          lines: [],
        }),
      /empty hunk/,
    );

    assert.throws(
      () =>
        buildSingleHunkPatch("a.ts", {
          oldStart: 1,
          oldCount: 1,
          newStart: 1,
          newCount: 1,
          lines: [line("context", "unchanged", 1, 1)],
        }),
      /no additions or deletions/,
    );

    assert.throws(
      () =>
        buildSingleHunkPatch("a.ts", {
          oldStart: 1,
          oldCount: 5,
          newStart: 1,
          newCount: 6,
          truncated: true,
          lines: [line("added", "x", null, 1)],
        }),
      /truncated/,
    );

    assert.throws(
      () =>
        buildSingleHunkPatch("a.ts", {
          oldStart: 1,
          oldCount: 0,
          newStart: 1,
          newCount: 1,
          lines: [line("added", "ok\n--- a/evil", null, 1)],
        }),
      /embedded newline/,
    );
  });
});

describe("extractFileHunks", () => {
  it("extracts multiple hunks accurately and round-trips through buildSingleHunkPatch", () => {
    const file: DiffFileView = {
      path: "src/multi.ts",
      additions: 2,
      deletions: 1,
      lines: [
        line("meta", "index 1111111..2222222 100644"),
        line("meta", "--- a/src/multi.ts"),
        line("hunk_header", "@@ -1,3 +1,3 @@"),
        line("context", "line 1", 1, 1),
        line("removed", "line 2", 2, null),
        line("added", "line 2 edited", null, 2),
        line("context", "line 3", 3, 3),
        line("hunk_header", "@@ -20,2 +20,3 @@ fn tail()"),
        line("context", "line 20", 20, 20),
        line("added", "line 21 inserted", null, 21),
        line("context", "line 22", 21, 22),
      ],
    };

    const hunks = extractFileHunks(file);
    assert.equal(hunks.length, 2);
    assert.equal(hunks[0].headerLineIndex, 2);
    assert.equal(hunks[0].truncated, undefined);
    assert.equal(hunks[1].headerLineIndex, 7);
    assert.equal(hunks[1].sectionHeading, " fn tail()");

    const secondPatch = buildSingleHunkPatch(file.path, hunks[1]);
    assert.equal(
      secondPatch,
      [
        "diff --git a/src/multi.ts b/src/multi.ts",
        "--- a/src/multi.ts",
        "+++ b/src/multi.ts",
        "@@ -20,2 +20,3 @@ fn tail()",
        " line 20",
        "+line 21 inserted",
        " line 22",
        "",
      ].join("\n"),
    );
  });

  it("marks hunks cut off by the diff preview line cap as truncated", () => {
    const file: DiffFileView = {
      path: "src/huge.ts",
      additions: 50,
      deletions: 0,
      lines: [
        line("hunk_header", "@@ -1,2 +1,10 @@"),
        line("context", "a", 1, 1),
        line("added", "b", null, 2),
        line("meta", "… 42 more diff lines omitted"),
      ],
    };

    const hunks = extractFileHunks(file);
    assert.equal(hunks.length, 1);
    assert.equal(hunks[0].truncated, true);
  });
});

describe("deriveDefaultPrTitle", () => {
  it("prefers the latest commit summary and otherwise humanizes feature branch names", () => {
    assert.equal(
      deriveDefaultPrTitle("feat/something", "feat(review): add hunk staging\n\nBody details"),
      "feat(review): add hunk staging",
    );
    assert.equal(
      deriveDefaultPrTitle("batch2/b2-02-review-tab-git-write-ui"),
      "B2 02 review tab git write ui",
    );
    assert.equal(deriveDefaultPrTitle("fix_login_redirect"), "Fix login redirect");
    assert.equal(deriveDefaultPrTitle("main"), "");
    assert.equal(deriveDefaultPrTitle("HEAD"), "");
    assert.equal(deriveDefaultPrTitle(null), "");
  });
});
