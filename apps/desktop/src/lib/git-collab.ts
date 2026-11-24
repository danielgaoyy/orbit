import type {
  DiffFileView,
  GitConflictBlock,
  GitConflictFile,
  GitPrReviewComment,
  GitPrStatus,
  GitRebaseStatus,
} from "./orbit-ipc.ts";

export interface ReviewCommentDraft {
  id: string;
  path: string;
  side: "old" | "new";
  /** Anchor line number (always matches `endLine ?? startLine` for backward compatibility). */
  lineNumber: number | null;
  /** Inclusive start line of a single-line or multi-line review comment. */
  startLine?: number | null;
  /** Inclusive end line of a single-line or multi-line review comment. */
  endLine?: number | null;
  code: string;
  comment: string;
}

export interface LineRangeBounds {
  startLine: number;
  endLine: number;
}

export function normalizeLineRange(a: number, b: number): LineRangeBounds {
  return a <= b ? { startLine: a, endLine: b } : { startLine: b, endLine: a };
}

export function resolveDraftLineBounds(draft: {
  lineNumber: number | null;
  startLine?: number | null;
  endLine?: number | null;
}): { startLine: number | null; endLine: number | null } {
  const s = draft.startLine ?? draft.lineNumber ?? null;
  const e = draft.endLine ?? draft.lineNumber ?? s;
  if (s == null && e == null) {
    return { startLine: null, endLine: null };
  }
  if (s != null && e != null) {
    return normalizeLineRange(s, e);
  }
  const single = s ?? e;
  return { startLine: single, endLine: single };
}

/**
 * Format a compact line or line-range tag: `L10`, `L10-L24`, or fallback `side`.
 */
export function formatLineRangeTag(
  startLine: number | null | undefined,
  endLine: number | null | undefined,
  side?: "old" | "new",
): string {
  const s = startLine ?? endLine ?? null;
  const e = endLine ?? startLine ?? null;
  if (s == null || e == null) {
    return side ?? "file";
  }
  const { startLine: lo, endLine: hi } = normalizeLineRange(s, e);
  return lo === hi ? `L${lo}` : `L${lo}-L${hi}`;
}

/**
 * Format a canonical file + line-range reference such as `src/app.ts:L10-L24` or `src/app.ts:L10`.
 */
export function formatPathLineRange(
  path: string,
  startLine: number | null | undefined,
  endLine: number | null | undefined,
  side?: "old" | "new",
): string {
  const s = startLine ?? endLine ?? null;
  const e = endLine ?? startLine ?? null;
  if (s == null || e == null) {
    return side ? `${path} (${side})` : path;
  }
  return `${path}:${formatLineRangeTag(s, e, side)}`;
}

/**
 * Test whether a rendered diff line falls inside an active or saved line selection range.
 */
export function isLineWithinSelection(
  lineNumber: number | null | undefined,
  side: "old" | "new",
  selection:
    | {
        side: "old" | "new";
        startLine: number | null | undefined;
        endLine: number | null | undefined;
      }
    | null
    | undefined,
): boolean {
  if (!selection || lineNumber == null || selection.side !== side) {
    return false;
  }
  const s = selection.startLine ?? selection.endLine ?? null;
  const e = selection.endLine ?? selection.startLine ?? null;
  if (s == null || e == null) {
    return false;
  }
  const { startLine: lo, endLine: hi } = normalizeLineRange(s, e);
  return lineNumber >= lo && lineNumber <= hi;
}

/**
 * Extract the code lines covered by `[startLine, endLine]` on a given side of a `DiffFileView`.
 */
export function extractRangeSnippetFromDiffFile(
  file: DiffFileView,
  side: "old" | "new",
  startLine: number,
  endLine: number,
  maxLines = 16,
): string {
  const { startLine: lo, endLine: hi } = normalizeLineRange(startLine, endLine);
  const collected: string[] = [];

  for (const line of file.lines) {
    if (line.kind === "meta" || line.kind === "hunk_header") {
      continue;
    }
    const ln =
      side === "old"
        ? line.old_line
        : line.kind === "removed"
          ? null
          : (line.new_line ?? line.old_line);
    if (ln != null && ln >= lo && ln <= hi) {
      collected.push(line.content);
      if (collected.length >= maxLines) {
        break;
      }
    }
  }

  return collected.join("\n");
}

/**
 * Format draft inline (single-line & multi-line) review comments into a structured Composer prompt.
 */
export function formatReviewCommentsPrompt(comments: readonly ReviewCommentDraft[]): string {
  if (comments.length === 0) return "";
  const grouped = new Map<string, ReviewCommentDraft[]>();
  for (const c of comments) {
    const list = grouped.get(c.path);
    if (list) list.push(c);
    else grouped.set(c.path, [c]);
  }

  const sections: string[] = [
    `Please address the following ${comments.length} code review ${comments.length === 1 ? "comment" : "comments"}:`,
  ];

  for (const [path, list] of grouped.entries()) {
    sections.push(`\n### \`${path}\``);
    for (const item of list) {
      const { startLine, endLine } = resolveDraftLineBounds(item);
      const loc = formatPathLineRange(path, startLine, endLine, item.side);
      const snippet = item.code
        .split(/\r?\n/)
        .map((l) => l.trim())
        .filter(Boolean)
        .slice(0, 3)
        .join(" / ")
        .slice(0, 140);
      if (snippet) {
        sections.push(`- **\`${loc}\`** (\`${snippet}\`):\n  ${item.comment}`);
      } else {
        sections.push(`- **\`${loc}\`**:\n  ${item.comment}`);
      }
    }
  }

  return sections.join("\n");
}

/**
 * Build a structured agent prompt for fixing failing CI checks on the active PR.
 */
export function buildFixFailingCiPrompt(pr: GitPrStatus): string {
  const failing = pr.checks.filter((c) => c.state === "failing");
  const header =
    pr.number != null
      ? `Please investigate and fix the ${failing.length} failing CI ${failing.length === 1 ? "check" : "checks"} on PR #${pr.number}${pr.title ? ` (${pr.title})` : ""}:`
      : `Please investigate and fix the ${failing.length} failing CI ${failing.length === 1 ? "check" : "checks"} on the current branch:`;

  const lines: string[] = [header];
  if (pr.head_ref) {
    lines.push(`- Branch: \`${pr.head_ref}\`${pr.base_ref ? ` → \`${pr.base_ref}\`` : ""}`);
  }
  if (pr.url) {
    lines.push(`- PR: ${pr.url}`);
  }
  lines.push("");
  for (const check of failing) {
    const workflowPart = check.workflow ? ` (${check.workflow})` : "";
    const conclusionPart = check.raw_conclusion ? ` — ${check.raw_conclusion}` : "";
    const urlPart = check.details_url ? ` — ${check.details_url}` : "";
    lines.push(`- **${check.name}**${workflowPart}${conclusionPart}${urlPart}`);
  }
  lines.push(
    "\nReproduce each failing check locally, apply a minimal clean fix, and re-run the failing checks to verify they pass.",
  );
  return lines.join("\n");
}

/**
 * Filter PR comments to actionable unresolved review feedback.
 */
export function getUnresolvedPrComments(pr: GitPrStatus): GitPrReviewComment[] {
  return pr.review_comments.filter(
    (c) =>
      !c.is_resolved &&
      (c.kind === "review_thread" || c.kind === "review_summary") &&
      c.body.trim().length > 0,
  );
}

/**
 * Build a structured agent prompt for addressing unresolved PR review comments.
 */
export function buildFixPrCommentsPrompt(pr: GitPrStatus): string {
  const unresolved = getUnresolvedPrComments(pr);
  const header =
    pr.number != null
      ? `Please address the ${unresolved.length} unresolved review ${unresolved.length === 1 ? "comment" : "comments"} on PR #${pr.number}${pr.title ? ` (${pr.title})` : ""}:`
      : `Please address the ${unresolved.length} unresolved PR review ${unresolved.length === 1 ? "comment" : "comments"}:`;

  const lines: string[] = [header];
  if (pr.url) {
    lines.push(`- PR: ${pr.url}`);
  }
  lines.push("");

  for (const item of unresolved) {
    const loc = item.path
      ? formatPathLineRange(item.path, item.start_line ?? item.line, item.line)
      : "General review";
    const outdatedTag = item.is_outdated ? " (outdated)" : "";
    const body = item.body.trim().replace(/\n/g, "\n  ");
    lines.push(`- **\`${loc}\`** by @${item.author}${outdatedTag}:\n  ${body}`);
  }

  lines.push(
    "\nMake the necessary code changes, add or update tests where appropriate, and summarize how each review comment was addressed.",
  );
  return lines.join("\n");
}

export function countTotalConflictBlocks(files: readonly GitConflictFile[]): number {
  let total = 0;
  for (const f of files) {
    total += f.blocks.length > 0 ? f.blocks.length : f.total_blocks;
  }
  return total;
}

export function hasActiveConflictOrRebase(
  rebase: GitRebaseStatus | null | undefined,
  conflicts: readonly GitConflictFile[],
): boolean {
  return Boolean(
    rebase?.in_progress ||
      rebase?.merge_in_progress ||
      rebase?.cherry_pick_in_progress ||
      (rebase?.conflicted_files?.length ?? 0) > 0 ||
      conflicts.length > 0,
  );
}

export function defaultManualResolutionText(block: GitConflictBlock): string {
  if (!block.ours) return block.theirs;
  if (!block.theirs) return block.ours;
  return `${block.ours}\n${block.theirs}`;
}
