/**
 * Shared helpers for create-only name conflicts in settings ("new skill", "copy to…",
 * "import skill", "add MCP server").
 *
 * Each of those entries used to submit a blind upsert, so saving under a name that already
 * existed silently replaced the stored resource. The daemon now rejects create-only writes with
 * a typed error; these helpers let the UI pre-empt the round trip and recognize the error when a
 * conflict appears between the local snapshot and the write.
 */

/** Daemon error codes/messages that mean "create-only write hit an existing resource". */
export function isAlreadyExistsError(err: unknown): boolean {
  const msg = (err instanceof Error ? err.message : String(err)).toLowerCase();
  return msg.includes("skill_already_exists") || msg.includes("already exists");
}

/**
 * Suggest the first free `name-N` variant (`review` → `review-2` → `review-3`), so the conflict
 * prompt can offer a one-click rename instead of making the user invent a name.
 *
 * Any trailing `-N` on the input is treated as a counter and advanced rather than stacked, so
 * repeated conflicts never produce `review-2-2`. Falls back to the base name if the whole range
 * is somehow taken.
 */
export function suggestAvailableName(
  base: string,
  taken: Iterable<string>,
  maxAttempts = 999,
): string {
  const used = new Set<string>();
  for (const item of taken) {
    used.add(item.toLowerCase());
  }
  const match = /^(.*?)-(\d+)$/.exec(base);
  const stem = match ? match[1] : base;
  const start = match ? Number(match[2]) + 1 : 2;
  for (let n = start; n < start + maxAttempts; n += 1) {
    const candidate = `${stem}-${n}`;
    if (!used.has(candidate.toLowerCase())) return candidate;
  }
  return base;
}
