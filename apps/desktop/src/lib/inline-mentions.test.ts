import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  buildFolderTreeSummary,
  expandClaudeCommandArguments,
  findInlineMentionSpans,
  isValidHttpsReferenceUrl,
  removeInlineMentionFromDraft,
  resolveAtomicMentionBackspace,
  resolveAtomicMentionDelete,
  shortWebUrlLabel,
} from "./inline-mentions.ts";
import type { InlineMentionTarget } from "../state/store-types.ts";

describe("inline-mentions", () => {
  const mentions: InlineMentionTarget[] = [
    { token: "@src/app.ts", kind: "file", path: "src/app.ts" },
    { token: "@folder:src/components", kind: "folder", path: "src/components", fileCount: 12 },
    {
      token: "@web:https://docs.rs/tokio",
      kind: "web",
      url: "https://docs.rs/tokio",
      label: "docs.rs/tokio",
    },
    {
      token: "@pr#42",
      kind: "pr",
      number: 42,
      title: "Add terminal splits",
    },
  ];

  it("locates boundary-valid inline mention spans without matching partial identifiers", () => {
    const text = "Check @src/app.ts and @folder:src/components plus @pr#42 vs foo@src/app.tsbar";
    const spans = findInlineMentionSpans(text, mentions);
    assert.equal(spans.length, 3);
    assert.equal(spans[0].mention.token, "@src/app.ts");
    assert.equal(spans[1].mention.token, "@folder:src/components");
    assert.equal(spans[2].mention.token, "@pr#42");
  });

  it("deletes entire mention tokens atomically on Backspace and Delete without leaving broken text", () => {
    const text = "Inspect @src/app.ts carefully";
    const endOfToken = "Inspect @src/app.ts".length;

    const backspaced = resolveAtomicMentionBackspace(text, endOfToken, endOfToken, mentions);
    assert.ok(backspaced);
    assert.equal(backspaced.removedToken, "@src/app.ts");
    assert.equal(backspaced.nextText, "Inspect carefully");
    assert.equal(backspaced.nextCursor, "Inspect ".length);

    // Caret in the middle of the pill also removes the entire chip cleanly
    const midToken = "Inspect @src/a".length;
    const backspacedMid = resolveAtomicMentionBackspace(text, midToken, midToken, mentions);
    assert.ok(backspacedMid);
    assert.equal(backspacedMid.nextText, "Inspect carefully");

    // Caret outside mention returns null (falls through to native Backspace)
    assert.equal(resolveAtomicMentionBackspace(text, text.length, text.length, mentions), null);

    // Forward-delete at leading edge removes whole chip
    const startOfToken = "Inspect ".length;
    const deleted = resolveAtomicMentionDelete(text, startOfToken, startOfToken, mentions);
    assert.ok(deleted);
    assert.equal(deleted.removedToken, "@src/app.ts");
    assert.equal(deleted.nextText, "Inspect carefully");
  });

  it("removes all boundary-matched occurrences cleanly via removeInlineMentionFromDraft", () => {
    assert.equal(
      removeInlineMentionFromDraft("Compare @src/app.ts with @src/app.ts now", "@src/app.ts"),
      "Compare with now",
    );
    assert.equal(removeInlineMentionFromDraft("@pr#42 please review", "@pr#42"), "please review");
  });

  it("validates explicit https:// URLs strictly (rejecting http, file, javascript, whitespace, userinfo)", () => {
    assert.equal(isValidHttpsReferenceUrl("https://docs.rs/tokio"), true);
    assert.equal(isValidHttpsReferenceUrl("https://github.com/org/repo/pull/42#issue-1"), true);
    assert.equal(isValidHttpsReferenceUrl("http://example.com"), false);
    assert.equal(isValidHttpsReferenceUrl("file:///etc/passwd"), false);
    assert.equal(isValidHttpsReferenceUrl("javascript:alert(1)"), false);
    assert.equal(isValidHttpsReferenceUrl("https://user:pass@example.com"), false);
    assert.equal(isValidHttpsReferenceUrl("https://example.com/has space"), false);
    assert.equal(isValidHttpsReferenceUrl("https://notadomain"), false);
    assert.equal(shortWebUrlLabel("https://docs.rs/tokio/"), "docs.rs/tokio");
  });

  it("builds deterministic structured folder tree summaries from workspace files", () => {
    const files = [
      "apps/desktop/src/main.tsx",
      "apps/desktop/src/App.tsx",
      "apps/desktop/src/lib/a.ts",
      "crates/orbit-cli/src/main.rs",
    ];
    const summary = buildFolderTreeSummary("apps/desktop/src", files, 2);
    assert.equal(summary.fileCount, 3);
    assert.match(summary.treeSummary, /- App\.tsx/);
    assert.match(summary.treeSummary, /- lib\/a\.ts/);
    assert.match(summary.treeSummary, /\+1 more files/);
  });

  it("strips YAML frontmatter and expands $ARGUMENTS for .claude/commands/*.md templates", () => {
    const templateWithPlaceholder = `---
description: Deploy target environment
argument-hint: <env>
---

Deploy service to $ARGUMENTS and verify health checks for $ARGUMENTS.`;
    assert.equal(
      expandClaudeCommandArguments(templateWithPlaceholder, "staging"),
      "Deploy service to staging and verify health checks for staging.",
    );

    const templateWithoutPlaceholder = `---
description: Review diff
---

Inspect the active git diff carefully.`;
    assert.equal(
      expandClaudeCommandArguments(templateWithoutPlaceholder, "focus on security"),
      "Inspect the active git diff carefully.\n\nfocus on security",
    );
    assert.equal(
      expandClaudeCommandArguments(templateWithoutPlaceholder, "   "),
      "Inspect the active git diff carefully.",
    );
  });
});
