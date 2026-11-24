import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  buildFixFailingCiPrompt,
  buildFixPrCommentsPrompt,
  countTotalConflictBlocks,
  defaultManualResolutionText,
  extractRangeSnippetFromDiffFile,
  formatLineRangeTag,
  formatPathLineRange,
  formatReviewCommentsPrompt,
  getUnresolvedPrComments,
  hasActiveConflictOrRebase,
  isLineWithinSelection,
  normalizeLineRange,
  type ReviewCommentDraft,
} from "./git-collab.ts";
import type {
  DiffFileView,
  GitConflictBlock,
  GitConflictFile,
  GitPrStatus,
  GitRebaseStatus,
} from "./orbit-ipc.ts";

describe("git-collab multi-line review comments", () => {
  it("normalizes upward and downward Shift+Click ranges", () => {
    assert.deepEqual(normalizeLineRange(10, 24), { startLine: 10, endLine: 24 });
    assert.deepEqual(normalizeLineRange(24, 10), { startLine: 10, endLine: 24 });
    assert.deepEqual(normalizeLineRange(7, 7), { startLine: 7, endLine: 7 });
  });

  it("formats single-line and multi-line range locations cleanly", () => {
    assert.equal(formatLineRangeTag(10, 24, "new"), "L10-L24");
    assert.equal(formatLineRangeTag(24, 10, "new"), "L10-L24");
    assert.equal(formatLineRangeTag(12, 12, "new"), "L12");
    assert.equal(formatLineRangeTag(null, null, "old"), "old");

    assert.equal(formatPathLineRange("src/app.ts", 10, 24, "new"), "src/app.ts:L10-L24");
    assert.equal(formatPathLineRange("src/app.ts", 10, 10, "new"), "src/app.ts:L10");
    assert.equal(formatPathLineRange("src/app.ts", null, null, "old"), "src/app.ts (old)");
  });

  it("checks line membership inside active range strictly per side", () => {
    const sel = { side: "new" as const, startLine: 20, endLine: 15 };
    assert.equal(isLineWithinSelection(15, "new", sel), true);
    assert.equal(isLineWithinSelection(18, "new", sel), true);
    assert.equal(isLineWithinSelection(20, "new", sel), true);
    assert.equal(isLineWithinSelection(14, "new", sel), false);
    assert.equal(isLineWithinSelection(21, "new", sel), false);
    assert.equal(isLineWithinSelection(18, "old", sel), false);
    assert.equal(isLineWithinSelection(null, "new", sel), false);
  });

  it("extracts range code snippets from DiffFileView without meta or hunk headers", () => {
    const file: DiffFileView = {
      path: "src/math.ts",
      additions: 3,
      deletions: 1,
      lines: [
        { kind: "meta", old_line: null, new_line: null, content: "--- a/src/math.ts" },
        { kind: "hunk_header", old_line: null, new_line: null, content: "@@ -8,3 +8,5 @@" },
        { kind: "context", old_line: 8, new_line: 8, content: "export function add(a: number, b: number) {" },
        { kind: "removed", old_line: 9, new_line: null, content: "  return a - b;" },
        { kind: "added", old_line: null, new_line: 9, content: "  if (!Number.isFinite(a)) return 0;" },
        { kind: "added", old_line: null, new_line: 10, content: "  if (!Number.isFinite(b)) return 0;" },
        { kind: "added", old_line: null, new_line: 11, content: "  return a + b;" },
        { kind: "context", old_line: 10, new_line: 12, content: "}" },
      ],
    };

    assert.equal(
      extractRangeSnippetFromDiffFile(file, "new", 9, 11),
      "  if (!Number.isFinite(a)) return 0;\n  if (!Number.isFinite(b)) return 0;\n  return a + b;",
    );
    assert.equal(extractRangeSnippetFromDiffFile(file, "old", 9, 9), "  return a - b;");
  });

  it("formats single-line and multi-line draft comments as path:Lstart-Lend in Composer prompt", () => {
    const comments: ReviewCommentDraft[] = [
      {
        id: "c1",
        path: "src/review.ts",
        side: "new",
        lineNumber: 24,
        startLine: 10,
        endLine: 24,
        code: "const a = 1;\nconst b = 2;",
        comment: "Extract this loop into a pure helper.",
      },
      {
        id: "c2",
        path: "src/review.ts",
        side: "new",
        lineNumber: 42,
        startLine: 42,
        endLine: 42,
        code: "return result;",
        comment: "Handle null case.",
      },
    ];

    const prompt = formatReviewCommentsPrompt(comments);
    assert.match(prompt, /2 code review comments:/);
    assert.match(prompt, /### `src\/review\.ts`/);
    assert.match(prompt, /\*\*`src\/review\.ts:L10-L24`\*\*/);
    assert.match(prompt, /\*\*`src\/review\.ts:L42`\*\*/);
    assert.match(prompt, /Extract this loop into a pure helper\./);
  });
});

describe("git-collab PR auto-fix prompts & conflict models", () => {
  const samplePr: GitPrStatus = {
    availability: "available",
    number: 128,
    title: "Deep Git Collaboration Loop",
    url: "https://github.com/danielgaoyy/orbit/pull/128",
    state: "OPEN",
    is_draft: false,
    head_ref: "feat/git-collab",
    base_ref: "main",
    checks: [
      { name: "cargo clippy", state: "passing", workflow: "CI" },
      {
        name: "desktop unit tests",
        state: "failing",
        workflow: "Desktop CI",
        raw_conclusion: "FAILURE",
        details_url: "https://github.com/danielgaoyy/orbit/actions/runs/999",
      },
      { name: "ios build", state: "pending", workflow: "Mobile CI" },
    ],
    passing_checks: 1,
    failing_checks: 1,
    pending_checks: 1,
    review_comments: [
      {
        id: "resolved_1",
        author: "alice",
        body: "Looks good now",
        path: "src/a.ts",
        line: 5,
        is_resolved: true,
        is_outdated: false,
        kind: "review_thread",
      },
      {
        id: "open_1",
        author: "bob",
        body: "Reject leading `-` in rebase target.",
        path: "crates/orbit-daemon/src/git_collab.rs",
        start_line: 82,
        line: 96,
        is_resolved: false,
        is_outdated: false,
        kind: "review_thread",
      },
    ],
    unresolved_comment_count: 1,
  };

  it("builds a structured Fix Failing CI prompt containing only failing checks", () => {
    const prompt = buildFixFailingCiPrompt(samplePr);
    assert.match(prompt, /PR #128/);
    assert.match(prompt, /desktop unit tests/);
    assert.match(prompt, /Desktop CI/);
    assert.match(prompt, /actions\/runs\/999/);
    assert.doesNotMatch(prompt, /cargo clippy/);
    assert.doesNotMatch(prompt, /ios build/);
  });

  it("filters resolved comments and formats multi-line PR thread ranges", () => {
    const unresolved = getUnresolvedPrComments(samplePr);
    assert.equal(unresolved.length, 1);
    assert.equal(unresolved[0].id, "open_1");

    const prompt = buildFixPrCommentsPrompt(samplePr);
    assert.match(prompt, /PR #128/);
    assert.match(prompt, /crates\/orbit-daemon\/src\/git_collab\.rs:L82-L96/);
    assert.match(prompt, /@bob/);
    assert.doesNotMatch(prompt, /@alice/);
  });

  it("detects active conflict/rebase state and builds default manual merge text", () => {
    const idleRebase: GitRebaseStatus = {
      in_progress: false,
      kind: "none",
      conflicted_files: [],
      merge_in_progress: false,
      cherry_pick_in_progress: false,
    };
    assert.equal(hasActiveConflictOrRebase(idleRebase, []), false);

    const activeRebase: GitRebaseStatus = {
      ...idleRebase,
      in_progress: true,
      kind: "merge",
      current_step: 1,
      total_steps: 3,
    };
    assert.equal(hasActiveConflictOrRebase(activeRebase, []), true);

    const block: GitConflictBlock = {
      index: 0,
      start_line: 3,
      end_line: 10,
      ours_label: "HEAD",
      base_label: "base",
      theirs_label: "feat",
      ours: "const x = 1;",
      base: "const x = 0;",
      theirs: "const x = 2;",
      context_before: [],
      context_after: [],
    };
    const conflictFile: GitConflictFile = {
      path: "src/x.ts",
      blocks: [block],
      is_binary: false,
      unmerged_in_index: true,
      total_blocks: 1,
    };
    assert.equal(hasActiveConflictOrRebase(idleRebase, [conflictFile]), true);
    assert.equal(countTotalConflictBlocks([conflictFile]), 1);
    assert.equal(defaultManualResolutionText(block), "const x = 1;\nconst x = 2;");
    assert.equal(defaultManualResolutionText({ ...block, ours: "" }), "const x = 2;");
    assert.equal(defaultManualResolutionText({ ...block, theirs: "" }), "const x = 1;");
  });
});
