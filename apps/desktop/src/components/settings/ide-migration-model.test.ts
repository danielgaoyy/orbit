import assert from "node:assert/strict";
import { describe, it } from "node:test";
import type { ExternalIdeMigrationItem } from "../../lib/orbit-ipc.ts";
import {
  countOverwrites,
  migrationPreviewKey,
  overwriteDestinations,
  unexpectedOverwrites,
} from "./ide-migration-model.ts";

function item(
  decision: string,
  destination: string,
  name = destination,
): ExternalIdeMigrationItem {
  return { source: "claude", kind: "rule", name, destination, decision };
}

describe("ide migration preview key", () => {
  it("is stable for identical inputs so a fresh preview stays usable", () => {
    assert.equal(
      migrationPreviewKey("claude", "/repo", "skip"),
      migrationPreviewKey("claude", "/repo", "skip"),
    );
  });

  it("changes when source, cwd, or conflict strategy changes", () => {
    const base = migrationPreviewKey("claude", "/repo", "skip");
    assert.notEqual(base, migrationPreviewKey("codex", "/repo", "skip"));
    assert.notEqual(base, migrationPreviewKey("claude", "/other", "skip"));
    assert.notEqual(base, migrationPreviewKey("claude", null, "skip"));
    assert.notEqual(base, migrationPreviewKey("claude", "/repo", "overwrite"));
    assert.notEqual(base, migrationPreviewKey("claude", "/repo", "rename"));
  });

  it("does not collide across differently-split inputs", () => {
    assert.notEqual(
      migrationPreviewKey("a", "b", "skip"),
      migrationPreviewKey("a,b", null, "skip"),
    );
  });
});

describe("overwrite accounting", () => {
  it("counts and lists only overwrite decisions", () => {
    const items = [
      item("create", "/home/a.md"),
      item("overwrite", "/home/b.md"),
      item("skip", "/home/c.md"),
      item("overwrite", "/home/d.md"),
      item("unchanged", "/home/e.md"),
      item("rename", "/home/f.md"),
    ];
    assert.equal(countOverwrites(items), 2);
    assert.deepEqual(overwriteDestinations(items), ["/home/b.md", "/home/d.md"]);
  });

  it("reports zero for an empty or overwrite-free run", () => {
    assert.equal(countOverwrites([]), 0);
    assert.equal(countOverwrites([item("create", "/a"), item("skip", "/b")]), 0);
    assert.deepEqual(overwriteDestinations([]), []);
  });
});

describe("unexpected overwrites", () => {
  it("flags a destination the preview showed as create but the run overwrote", () => {
    const preview = [item("create", "/home/rules.md")];
    const result = [item("overwrite", "/home/rules.md")];
    assert.deepEqual(unexpectedOverwrites(preview, result), ["/home/rules.md"]);
  });

  it("stays silent when the overwrite was already confirmed in the preview", () => {
    const preview = [item("overwrite", "/home/rules.md")];
    const result = [item("overwrite", "/home/rules.md")];
    assert.deepEqual(unexpectedOverwrites(preview, result), []);
  });

  it("ignores non-overwrite drift such as create becoming skip", () => {
    const preview = [item("create", "/home/a.md"), item("overwrite", "/home/b.md")];
    const result = [item("skip", "/home/a.md"), item("overwrite", "/home/b.md")];
    assert.deepEqual(unexpectedOverwrites(preview, result), []);
  });

  it("reports each unconfirmed destination once, preserving run order", () => {
    const preview = [item("create", "/home/a.md"), item("create", "/home/b.md")];
    const result = [
      item("overwrite", "/home/b.md"),
      item("overwrite", "/home/a.md"),
      item("overwrite", "/home/b.md", "dup"),
    ];
    assert.deepEqual(unexpectedOverwrites(preview, result), ["/home/b.md", "/home/a.md"]);
  });

  it("matches on destination, not display name, so renames are tracked by path", () => {
    const preview = [item("overwrite", "/home/rules.md", "rules")];
    const result = [item("overwrite", "/home/rules.md", "rules-claude-1")];
    assert.deepEqual(unexpectedOverwrites(preview, result), []);
  });

  it("treats an empty preview as confirming nothing", () => {
    assert.deepEqual(unexpectedOverwrites([], [item("overwrite", "/home/a.md")]), [
      "/home/a.md",
    ]);
  });
});
