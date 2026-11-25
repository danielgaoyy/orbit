import type { ExternalIdeMigrationItem, MigrationConflict } from "@/lib/orbit-ipc";

/**
 * Identity of the inputs a dry-run preview was computed under.
 *
 * The daemon re-scans the filesystem and re-derives every `decision` on execute
 * (`crates/orbit-daemon/src/ide_migrate.rs`), so a preview is only a meaningful
 * confirmation of what will happen while the source, workspace scope, and
 * conflict strategy still match the ones it was produced with. Comparing this
 * key invalidates the preview declaratively — including when `activeCwd`
 * changes from outside this panel (the user switching workspace in the main
 * window), which per-control `onChange` resets cannot observe.
 */
export function migrationPreviewKey(
  source: string,
  cwd: string | null,
  conflict: MigrationConflict,
): string {
  return JSON.stringify([source, cwd, conflict]);
}

/** Destinations the daemon reported as `overwrite`. */
export function overwriteDestinations(items: readonly ExternalIdeMigrationItem[]): string[] {
  return items.filter((item) => item.decision === "overwrite").map((item) => item.destination);
}

export function countOverwrites(items: readonly ExternalIdeMigrationItem[]): number {
  return items.reduce((n, item) => (item.decision === "overwrite" ? n + 1 : n), 0);
}

/**
 * Destinations overwritten by an execute run that the preview did not show as
 * an overwrite — i.e. files clobbered without the user ever confirming it.
 *
 * Happens when another session or process creates a destination between the
 * dry run and the execute: the preview said `create`, the re-scan says
 * `overwrite`, and the write lands on content the user never saw flagged.
 */
export function unexpectedOverwrites(
  preview: readonly ExternalIdeMigrationItem[],
  result: readonly ExternalIdeMigrationItem[],
): string[] {
  const confirmed = new Set(overwriteDestinations(preview));
  const seen = new Set<string>();
  const unexpected: string[] = [];
  for (const destination of overwriteDestinations(result)) {
    if (confirmed.has(destination) || seen.has(destination)) continue;
    seen.add(destination);
    unexpected.push(destination);
  }
  return unexpected;
}
