import type {
  DiffFileView,
  PrPatrolCategory,
  PrPatrolFinding,
  PrPatrolReport,
  PrPatrolSeverity,
} from "./orbit-ipc.ts";

export type PatrolVerdict = "clean" | "needs_review" | "blocked";

export interface PatrolSeverityCounts {
  p0: number;
  p1: number;
  p2: number;
  actionable: number;
  total: number;
}

export interface FilePatrolFindingsIndex {
  count: number;
  highestSeverity: PrPatrolSeverity;
  byLine: Map<number, PrPatrolFinding[]>;
  findings: PrPatrolFinding[];
}

export interface PatrolFindingsIndex {
  byFile: Map<string, FilePatrolFindingsIndex>;
  total: number;
}

const SEVERITY_RANK: Readonly<Record<PrPatrolSeverity, number>> = {
  P0: 0,
  P1: 1,
  P2: 2,
};

export function severityRank(severity: PrPatrolSeverity): number {
  return SEVERITY_RANK[severity] ?? 99;
}

export function computePatrolVerdict(report: PrPatrolReport): PatrolVerdict {
  if (report.findings.some((f) => f.severity === "P0")) {
    return "blocked";
  }
  if (report.findings.length > 0) {
    return "needs_review";
  }
  return "clean";
}

export function countFindingsBySeverity(
  findings: readonly PrPatrolFinding[],
): PatrolSeverityCounts {
  let p0 = 0;
  let p1 = 0;
  let p2 = 0;
  for (const f of findings) {
    if (f.severity === "P0") p0 += 1;
    else if (f.severity === "P1") p1 += 1;
    else p2 += 1;
  }
  return {
    p0,
    p1,
    p2,
    actionable: p0 + p1,
    total: findings.length,
  };
}

export function sortPatrolFindings(findings: readonly PrPatrolFinding[]): PrPatrolFinding[] {
  return [...findings].sort(
    (a, b) =>
      severityRank(a.severity) - severityRank(b.severity) ||
      a.file_path.localeCompare(b.file_path) ||
      a.start_line - b.start_line ||
      a.end_line - b.end_line ||
      a.title.localeCompare(b.title),
  );
}

export function formatPatrolDuration(durationMs: number): string {
  const safe = Math.max(0, Math.round(durationMs));
  if (safe < 1000) {
    return `${safe}ms`;
  }
  const secs = safe / 1000;
  if (secs < 60) {
    return `${secs.toFixed(1)}s`;
  }
  const mins = Math.floor(secs / 60);
  const remSecs = Math.round(secs % 60);
  return `${mins}m ${remSecs}s`;
}

export function formatPatrolLineRange(startLine: number, endLine: number): string {
  const s = Math.max(1, Math.floor(startLine));
  const e = Math.max(s, Math.floor(endLine));
  return s === e ? `L${s}` : `L${s}-L${e}`;
}

export function formatPatrolLocation(finding: Pick<PrPatrolFinding, "file_path" | "start_line" | "end_line">): string {
  const s = Math.max(1, Math.floor(finding.start_line));
  const e = Math.max(s, Math.floor(finding.end_line));
  return s === e ? `${finding.file_path}:${s}` : `${finding.file_path}:${s}-${e}`;
}

export function normalizePatrolPath(filePath: string, cwd?: string | null): string {
  let normalized = filePath.trim().replace(/\\/g, "/");
  if (cwd) {
    const cleanCwd = cwd.trim().replace(/\\/g, "/").replace(/\/+$/, "");
    if (cleanCwd && normalized.startsWith(`${cleanCwd}/`)) {
      normalized = normalized.slice(cleanCwd.length + 1);
    }
  }
  return normalized.replace(/^\.\//, "").replace(/^\/+/, "");
}

export function indexPatrolFindings(
  findings: readonly PrPatrolFinding[],
  cwd?: string | null,
): PatrolFindingsIndex {
  const byFile = new Map<string, FilePatrolFindingsIndex>();
  const sorted = sortPatrolFindings(findings);

  for (const f of sorted) {
    const key = normalizePatrolPath(f.file_path, cwd);
    let entry = byFile.get(key);
    if (!entry) {
      entry = {
        count: 0,
        highestSeverity: f.severity,
        byLine: new Map(),
        findings: [],
      };
      byFile.set(key, entry);
    }
    entry.count += 1;
    if (severityRank(f.severity) < severityRank(entry.highestSeverity)) {
      entry.highestSeverity = f.severity;
    }
    entry.findings.push(f);
    const anchorLine = Math.max(1, Math.floor(f.start_line));
    const list = entry.byLine.get(anchorLine);
    if (list) list.push(f);
    else entry.byLine.set(anchorLine, [f]);
  }

  return { byFile, total: sorted.length };
}

/**
 * Find whether a Patrol finding's file and line appear in the currently rendered diff files.
 */
export function findMatchingDiffLine(
  diffFiles: readonly DiffFileView[],
  finding: Pick<PrPatrolFinding, "file_path" | "start_line" | "end_line">,
  cwd?: string | null,
): { filePath: string; line: number; exactLine: boolean } | null {
  const targetNorm = normalizePatrolPath(finding.file_path, cwd);
  const file = diffFiles.find((f) => normalizePatrolPath(f.path, cwd) === targetNorm);
  if (!file) return null;

  const start = Math.max(1, Math.floor(finding.start_line));
  const end = Math.max(start, Math.floor(finding.end_line));

  let inRangeLine: number | null = null;
  for (const line of file.lines) {
    if (line.kind === "meta" || line.kind === "hunk_header" || line.kind === "removed") {
      continue;
    }
    const ln = line.new_line ?? line.old_line;
    if (ln == null) continue;
    if (ln === start) {
      return { filePath: file.path, line: ln, exactLine: true };
    }
    if (inRangeLine === null && ln >= start && ln <= end) {
      inRangeLine = ln;
    }
  }

  if (inRangeLine !== null) {
    return { filePath: file.path, line: inRangeLine, exactLine: true };
  }
  return { filePath: file.path, line: start, exactLine: false };
}

const CATEGORY_LABELS: Readonly<Record<PrPatrolCategory, string>> = {
  security: "Security",
  correctness: "Correctness",
  concurrency: "Concurrency",
  performance: "Performance",
  maintainability: "Maintainability",
};

export function formatPatrolCategoryLabel(category: PrPatrolCategory): string {
  return CATEGORY_LABELS[category] ?? category;
}

/**
 * Build a scoped, actionable Agent handoff prompt for a single Orbit Patrol finding.
 */
export function buildPatrolFindingFixPrompt(finding: PrPatrolFinding): string {
  const loc = `${finding.file_path}:${formatPatrolLineRange(finding.start_line, finding.end_line)}`;
  const lines: string[] = [
    `Please fix this Orbit Patrol [${finding.severity} · ${formatPatrolCategoryLabel(finding.category)}] finding at \`${loc}\`:`,
    "",
    `**${finding.title}**`,
    finding.summary.trim(),
  ];

  const evidence = finding.evidence.trim();
  if (evidence) {
    lines.push("", `Evidence (\`${loc}\`):`, "```", evidence, "```");
  }

  const fix = finding.suggested_fix.trim();
  if (fix) {
    lines.push("", `Suggested fix: ${fix}`);
  }

  lines.push(
    "",
    "Apply a minimal, clean fix directly at this location, preserve existing behavior outside the issue, and verify with relevant checks/tests.",
  );

  return lines.join("\n");
}

/**
 * Build a scoped batch Agent handoff prompt for multiple Orbit Patrol findings (e.g. all P0/P1).
 */
export function buildPatrolBatchFixPrompt(
  findings: readonly PrPatrolFinding[],
  report?: Pick<PrPatrolReport, "base_ref" | "head_ref" | "pr_number"> | null,
): string {
  const sorted = sortPatrolFindings(findings);
  if (sorted.length === 0) return "";
  if (sorted.length === 1) {
    return buildPatrolFindingFixPrompt(sorted[0]);
  }

  const refSuffix = report
    ? report.pr_number != null
      ? ` on PR #${report.pr_number}`
      : ` (${report.base_ref.slice(0, 8)}..${report.head_ref.slice(0, 8)})`
    : "";

  const lines: string[] = [
    `Please fix the following ${sorted.length} Orbit Patrol findings${refSuffix} in priority order:`,
    "",
  ];

  for (const f of sorted) {
    const loc = `${f.file_path}:${formatPatrolLineRange(f.start_line, f.end_line)}`;
    lines.push(
      `- **[${f.severity} · ${formatPatrolCategoryLabel(f.category)}] \`${loc}\` — ${f.title}**`,
    );
    if (f.summary.trim()) {
      lines.push(`  - Rationale: ${f.summary.trim()}`);
    }
    if (f.suggested_fix.trim()) {
      lines.push(`  - Suggested fix: ${f.suggested_fix.trim()}`);
    }
  }

  lines.push(
    "",
    "Apply minimal, well-scoped fixes for each item above and run the relevant checks/tests to confirm everything passes.",
  );

  return lines.join("\n");
}
