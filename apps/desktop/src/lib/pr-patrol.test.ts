import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  buildPatrolBatchFixPrompt,
  buildPatrolFindingFixPrompt,
  computePatrolVerdict,
  countFindingsBySeverity,
  findMatchingDiffLine,
  formatPatrolDuration,
  formatPatrolLocation,
  indexPatrolFindings,
  normalizePatrolPath,
  sortPatrolFindings,
} from "./pr-patrol.ts";
import type { DiffFileView, PrPatrolFinding, PrPatrolReport } from "./orbit-ipc.ts";

function sampleFinding(overrides: Partial<PrPatrolFinding> = {}): PrPatrolFinding {
  return {
    id: "f_1",
    severity: "P1",
    category: "security",
    file_path: "crates/orbit-daemon/src/auth.rs",
    start_line: 42,
    end_line: 42,
    title: "Potential SQL interpolation",
    summary: "User input is formatted directly into a SQL query string.",
    evidence: 'db.execute(format!("SELECT * FROM users WHERE name = \'{name}\'"))',
    suggested_fix: "Use bound query parameters instead of format! interpolation.",
    verified_by_stage2: true,
    confidence: 0.98,
    ...overrides,
  };
}

function sampleReport(findings: PrPatrolFinding[]): PrPatrolReport {
  return {
    repo_root: "/repo/orbit",
    base_ref: "abcdef1234567890",
    head_ref: "9876543210fedcba",
    pr_number: 72,
    files_reviewed: 4,
    candidates_found: findings.length + 2,
    findings,
    dropped_false_positives: 2,
    summary: "Two-stage review completed.",
    duration_ms: 1840,
  };
}

describe("Orbit Patrol UI helpers", () => {
  it("computes verdict and severity counts accurately", () => {
    assert.equal(computePatrolVerdict(sampleReport([])), "clean");
    assert.equal(computePatrolVerdict(sampleReport([sampleFinding({ severity: "P2" })])), "needs_review");
    assert.equal(computePatrolVerdict(sampleReport([sampleFinding({ severity: "P1" })])), "needs_review");
    assert.equal(
      computePatrolVerdict(
        sampleReport([
          sampleFinding({ id: "a", severity: "P2" }),
          sampleFinding({ id: "b", severity: "P0" }),
        ]),
      ),
      "blocked",
    );

    const counts = countFindingsBySeverity([
      sampleFinding({ id: "1", severity: "P0" }),
      sampleFinding({ id: "2", severity: "P1" }),
      sampleFinding({ id: "3", severity: "P1" }),
      sampleFinding({ id: "4", severity: "P2" }),
    ]);
    assert.deepEqual(counts, { p0: 1, p1: 2, p2: 1, actionable: 3, total: 4 });
  });

  it("sorts findings strictly by severity P0 -> P1 -> P2, file path, and line", () => {
    const unsorted = [
      sampleFinding({ id: "c", severity: "P2", file_path: "a.rs", start_line: 10 }),
      sampleFinding({ id: "b", severity: "P1", file_path: "b.rs", start_line: 20 }),
      sampleFinding({ id: "a", severity: "P0", file_path: "z.rs", start_line: 5 }),
      sampleFinding({ id: "d", severity: "P1", file_path: "b.rs", start_line: 8 }),
    ];
    const sorted = sortPatrolFindings(unsorted);
    assert.deepEqual(
      sorted.map((f) => f.id),
      ["a", "d", "b", "c"],
    );
  });

  it("normalizes paths and indexes findings per file and line for diff badges", () => {
    assert.equal(normalizePatrolPath("./src/app.ts"), "src/app.ts");
    assert.equal(normalizePatrolPath("/repo/orbit/src/app.ts", "/repo/orbit"), "src/app.ts");
    assert.equal(formatPatrolDuration(420), "420ms");
    assert.equal(formatPatrolDuration(2350), "2.4s");
    assert.equal(formatPatrolLocation(sampleFinding({ start_line: 10, end_line: 14 })), "crates/orbit-daemon/src/auth.rs:10-14");

    const index = indexPatrolFindings([
      sampleFinding({ id: "1", severity: "P2", file_path: "src/app.ts", start_line: 12 }),
      sampleFinding({ id: "2", severity: "P0", file_path: "./src/app.ts", start_line: 12 }),
    ]);
    const fileEntry = index.byFile.get("src/app.ts");
    assert.ok(fileEntry);
    assert.equal(fileEntry.count, 2);
    assert.equal(fileEntry.highestSeverity, "P0");
    assert.equal(fileEntry.byLine.get(12)?.length, 2);
  });

  it("matches findings to diff lines when present and falls back cleanly when absent", () => {
    const diffFiles: DiffFileView[] = [
      {
        path: "src/auth.rs",
        additions: 2,
        deletions: 0,
        lines: [
          { kind: "hunk_header", old_line: null, new_line: null, content: "@@ -40,2 +40,4 @@" },
          { kind: "context", old_line: 40, new_line: 40, content: "fn run() {" },
          { kind: "added", old_line: null, new_line: 42, content: "    let x = val.unwrap();" },
        ],
      },
    ];

    assert.deepEqual(
      findMatchingDiffLine(
        diffFiles,
        sampleFinding({ file_path: "src/auth.rs", start_line: 42, end_line: 42 }),
      ),
      { filePath: "src/auth.rs", line: 42, exactLine: true },
    );

    assert.deepEqual(
      findMatchingDiffLine(
        diffFiles,
        sampleFinding({ file_path: "src/auth.rs", start_line: 99, end_line: 99 }),
      ),
      { filePath: "src/auth.rs", line: 99, exactLine: false },
    );

    assert.equal(
      findMatchingDiffLine(
        diffFiles,
        sampleFinding({ file_path: "src/other.rs", start_line: 1, end_line: 1 }),
      ),
      null,
    );
  });

  it("builds focused single-finding and batch Agent fix prompts with exact file:line citations", () => {
    const f1 = sampleFinding({ id: "1", severity: "P0", start_line: 42, end_line: 45 });
    const f2 = sampleFinding({
      id: "2",
      severity: "P1",
      category: "correctness",
      file_path: "src/main.rs",
      start_line: 12,
      end_line: 12,
      title: "Production unwrap",
      summary: "May panic on None.",
      suggested_fix: "Propagate error with ?.",
    });

    const single = buildPatrolFindingFixPrompt(f1);
    assert.ok(single.includes("`crates/orbit-daemon/src/auth.rs:L42-L45`"));
    assert.ok(single.includes("[P0 · Security]"));
    assert.ok(single.includes(f1.suggested_fix));

    const batch = buildPatrolBatchFixPrompt([f2, f1], sampleReport([f2, f1]));
    assert.ok(batch.includes("PR #72"));
    assert.ok(batch.indexOf("[P0 · Security]") < batch.indexOf("[P1 · Correctness]"));
    assert.ok(batch.includes("`src/main.rs:L12`"));
  });
});
