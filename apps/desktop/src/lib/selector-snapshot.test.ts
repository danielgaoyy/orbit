import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import path from "node:path";
import { describe, it } from "node:test";
import { fileURLToPath } from "node:url";
import { createCachedStateSnapshot } from "./selector-snapshot.ts";

function collectSourceFiles(dir: string): string[] {
  const out: string[] = [];
  for (const entry of readdirSync(dir, { withFileTypes: true })) {
    const full = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      out.push(...collectSourceFiles(full));
    } else if (
      (entry.name.endsWith(".ts") || entry.name.endsWith(".tsx")) &&
      !entry.name.endsWith(".test.ts") &&
      !entry.name.endsWith(".test.tsx")
    ) {
      out.push(full);
    }
  }
  return out;
}

function extractSelectorCalls(source: string): Array<{ line: number; body: string }> {
  const results: Array<{ line: number; body: string }> = [];
  const callRe = /\buseOrbitSelector(?:<[^>(]*>)?\(/g;
  let match: RegExpExecArray | null;
  while ((match = callRe.exec(source)) !== null) {
    const startIdx = match.index + match[0].length;
    const line = source.slice(0, match.index).split("\n").length;
    let depth = 1;
    let i = startIdx;
    let quote: string | null = null;
    let inLineComment = false;
    let inBlockComment = false;

    while (i < source.length && depth > 0) {
      const ch = source[i];
      const next = source[i + 1];

      if (inLineComment) {
        if (ch === "\n") inLineComment = false;
        i += 1;
        continue;
      }
      if (inBlockComment) {
        if (ch === "*" && next === "/") {
          inBlockComment = false;
          i += 2;
          continue;
        }
        i += 1;
        continue;
      }
      if (quote) {
        if (ch === "\\") {
          i += 2;
          continue;
        }
        if (ch === quote) quote = null;
        i += 1;
        continue;
      }

      if (ch === "/" && next === "/") {
        inLineComment = true;
        i += 2;
        continue;
      }
      if (ch === "/" && next === "*") {
        inBlockComment = true;
        i += 2;
        continue;
      }
      if (ch === "'" || ch === '"' || ch === "`") {
        quote = ch;
        i += 1;
        continue;
      }

      if (ch === "(") depth += 1;
      else if (ch === ")") depth -= 1;
      i += 1;
    }

    if (depth === 0) {
      results.push({
        line,
        body: source.slice(startIdx, i - 1).trim(),
      });
    }
  }
  return results;
}

function returnsPrimitiveMeasure(expr: string): boolean {
  const trimmed = expr
    .trim()
    .replace(/,+$/, "")
    .trim()
    .replace(/^\(([\s\S]*)\)$/, "$1")
    .trim();
  return (
    /\.length$/.test(trimmed) ||
    /\.some\([^)]*\)$/.test(trimmed) ||
    /\.every\([^)]*\)$/.test(trimmed) ||
    /^Boolean\([\s\S]*\)$/.test(trimmed)
  );
}

function findUnstableReturnIssues(selectorArg: string): string[] {
  const arrowMatch = selectorArg.match(/^[\s\S]*?=>\s*([\s\S]*)$/);
  if (!arrowMatch) return [];
  const body = arrowMatch[1].trim().replace(/,+$/, "").trim();
  const issues: string[] = [];

  if (!body.startsWith("{")) {
    if (!returnsPrimitiveMeasure(body)) {
      if (/(?:\?\?|\|\||:)\s*(?:\[\s*\]|\{\s*\})/.test(body)) {
        issues.push(`expression selector allocates fresh fallback literal: ${body}`);
      } else if (/^\(\s*\{/.test(body) || /^\[/.test(body)) {
        issues.push(`expression selector returns fresh object/array literal: ${body}`);
      } else if (/\.(?:filter|map|slice)\(/.test(body)) {
        issues.push(`expression selector allocates fresh derived array: ${body}`);
      }
    }
    return issues;
  }

  const returnRe = /\breturn\s+([\s\S]*?);/g;
  let retMatch: RegExpExecArray | null;
  while ((retMatch = returnRe.exec(body)) !== null) {
    const retExpr = retMatch[1].trim();
    if (returnsPrimitiveMeasure(retExpr)) continue;
    if (/^(?:\[\s*\]|\{\s*\})$/.test(retExpr)) {
      issues.push(`block selector directly returns fresh literal: return ${retExpr};`);
    } else if (/(?:\?\?|\|\||:)\s*(?:\[\s*\]|\{\s*\})/.test(retExpr)) {
      issues.push(`block selector returns fresh fallback literal: return ${retExpr};`);
    } else if (/^\(\s*\{/.test(retExpr) || /^\[/.test(retExpr)) {
      issues.push(`block selector returns fresh object/array literal: return ${retExpr};`);
    } else if (/\.(?:filter|map|slice)\(/.test(retExpr)) {
      issues.push(`block selector returns fresh derived array: return ${retExpr};`);
    }
  }
  return issues;
}

describe("createCachedStateSnapshot", () => {
  it("returns the exact same reference across repeated calls for unchanged state even when selector allocates", () => {
    const state = { items: null as string[] | null, meta: { tag: "a" } };
    const getSnapshot = createCachedStateSnapshot(
      () => state,
      (s) => s.items ?? [],
    );

    const first = getSnapshot();
    const second = getSnapshot();
    const third = getSnapshot();

    assert.deepEqual(first, []);
    assert.strictEqual(first, second);
    assert.strictEqual(second, third);

    const objectSnapshot = createCachedStateSnapshot(
      () => state,
      (s) => ({ tag: s.meta.tag }),
    );
    assert.strictEqual(objectSnapshot(), objectSnapshot());
  });

  it("recomputes when state reference changes and stabilizes on the new state", () => {
    let state = { items: ["alpha"] };
    const getSnapshot = createCachedStateSnapshot(
      () => state,
      (s) => [...s.items],
    );

    const snap1 = getSnapshot();
    assert.strictEqual(getSnapshot(), snap1);
    assert.deepEqual(snap1, ["alpha"]);

    state = { items: ["alpha", "beta"] };
    const snap2 = getSnapshot();
    assert.notStrictEqual(snap2, snap1);
    assert.deepEqual(snap2, ["alpha", "beta"]);
    assert.strictEqual(getSnapshot(), snap2);
  });

  it("re-evaluates with updated closure props when wrapper is recreated on re-render", () => {
    const state = {
      byId: {
        a: ["one"],
        b: ["two"],
      } as Record<string, string[]>,
    };

    let activeId = "a";
    const snapA = createCachedStateSnapshot(
      () => state,
      (s) => s.byId[activeId] ?? [],
    );
    assert.deepEqual(snapA(), ["one"]);

    activeId = "b";
    const snapB = createCachedStateSnapshot(
      () => state,
      (s) => s.byId[activeId] ?? [],
    );
    assert.deepEqual(snapB(), ["two"]);
  });

  it("invokes onUnstableSelector only when selector produces non-Object.is-equal values for identical state", () => {
    const stableState = { items: [] as string[] };
    let unstableWarnings = 0;

    const stableSnapshot = createCachedStateSnapshot(
      () => stableState,
      (s) => s.items,
      () => {
        unstableWarnings += 1;
      },
    );
    assert.strictEqual(stableSnapshot(), stableState.items);
    assert.strictEqual(unstableWarnings, 0);

    const missingState = { items: null as string[] | null };
    const unstableSnapshot = createCachedStateSnapshot(
      () => missingState,
      (s) => s.items ?? [],
      () => {
        unstableWarnings += 1;
      },
    );
    const first = unstableSnapshot();
    assert.deepEqual(first, []);
    assert.strictEqual(unstableWarnings, 1);

    // Subsequent calls with identical state reuse the cached snapshot without re-invoking the selector.
    assert.strictEqual(unstableSnapshot(), first);
    assert.strictEqual(unstableWarnings, 1);
  });

  it("keeps session-selectors empty fallbacks frozen and avoids inline empty arrays in subagent crew dock", () => {
    const selectorSource = readFileSync(
      new URL("../state/selectors/session-selectors.ts", import.meta.url),
      "utf8",
    );
    assert.match(selectorSource, /const\s+EMPTY_INSIGHT_SUBAGENTS\b[^=]*=\s*Object\.freeze\(\[\]\)/);
    assert.match(selectorSource, /const\s+EMPTY_SUBAGENT_LIST\b[^=]*=\s*Object\.freeze\(\[\]\)/);

    const dockSource = readFileSync(
      new URL("../components/stage/subagent-crew-dock.tsx", import.meta.url),
      "utf8",
    );
    for (const call of extractSelectorCalls(dockSource)) {
      assert.deepEqual(findUnstableReturnIssues(call.body), []);
    }
  });

  it("audits every useOrbitSelector call across src/ against returning fresh object/array allocations", () => {
    const srcDir = path.resolve(fileURLToPath(new URL("..", import.meta.url)));
    const storeFile = path.join(srcDir, "state", "orbit-store.ts");
    const files = collectSourceFiles(srcDir).filter((file) => file !== storeFile);
    assert.ok(files.length > 10, "Expected desktop source files to audit");

    const violations: string[] = [];
    for (const file of files) {
      const content = readFileSync(file, "utf8");
      for (const call of extractSelectorCalls(content)) {
        const issues = findUnstableReturnIssues(call.body);
        for (const issue of issues) {
          violations.push(`${path.relative(srcDir, file)}:${call.line}: ${issue}`);
        }
      }
    }

    assert.deepEqual(violations, []);
  });
});
