/**
 * Bounded session-scoped persistence for `TurnActivityDisclosure` manual expand/collapse overrides.
 *
 * Why this exists:
 * Keeping `userExpanded` solely in component-local `useState` makes user clicks fragile during
 * active streaming whenever:
 * 1. An optimistic prompt (`anchor_seq === 0`, `opt_*`) is replaced by the daemon-confirmed turn.
 * 2. A brand-new session migrates from `opt_<req>` to its real `ag_<id>`.
 * 3. `items[0]` of a `step_group` changes (e.g. a resolved `PermissionRequest` inserts before the
 *    first `ToolCall`), causing `seg.key` to change and remounting `TurnActivityDisclosure`.
 * 4. Virtualization recycles an offscreen turn and later remounts it.
 */

export interface DisclosureStateEntry {
  expanded: boolean;
  anchorSeq: number;
}

const MAX_DISCLOSURE_ENTRIES = 400;

export class DisclosureStateStore {
  private readonly entries = new Map<string, DisclosureStateEntry>();

  private buildKey(agentId: string, turnIndex: number, workOrdinal: number): string {
    return `${agentId}::t${turnIndex}::w${workOrdinal}`;
  }

  /**
   * Looks up a saved manual override for `(agentId, turnIndex, workOrdinal)`.
   * - Matches when `entry.anchorSeq === anchorSeq`.
   * - Also matches (and promotes) an optimistic entry (`entry.anchorSeq === 0`) once the
   *   daemon assigns a real positive `anchorSeq`.
   * - Ignores stale entries from a different turn after rewind (`entry.anchorSeq > 0 && !== anchorSeq`).
   */
  get(agentId: string, turnIndex: number, workOrdinal: number, anchorSeq: number): boolean | null {
    if (!agentId) return null;
    const key = this.buildKey(agentId, turnIndex, workOrdinal);
    const entry = this.entries.get(key);
    if (!entry) return null;

    if (entry.anchorSeq === anchorSeq) {
      // Refresh LRU recency
      this.entries.delete(key);
      this.entries.set(key, entry);
      return entry.expanded;
    }

    if (entry.anchorSeq === 0 && anchorSeq > 0) {
      const promoted: DisclosureStateEntry = { expanded: entry.expanded, anchorSeq };
      this.entries.delete(key);
      this.entries.set(key, promoted);
      return promoted.expanded;
    }

    return null;
  }

  set(
    agentId: string,
    turnIndex: number,
    workOrdinal: number,
    anchorSeq: number,
    expanded: boolean,
  ): void {
    if (!agentId) return;
    const key = this.buildKey(agentId, turnIndex, workOrdinal);
    this.entries.delete(key);
    this.entries.set(key, { expanded, anchorSeq });

    while (this.entries.size > MAX_DISCLOSURE_ENTRIES) {
      const oldestKey = this.entries.keys().next().value;
      if (oldestKey === undefined) break;
      this.entries.delete(oldestKey);
    }
  }

  /**
   * Migrates saved overrides when a newly created conversation transitions from its optimistic
   * `opt_<req>` ID to the daemon-assigned `ag_<id>`.
   */
  migrateAgent(fromAgentId: string, toAgentId: string): void {
    if (!fromAgentId || !toAgentId || fromAgentId === toAgentId) return;
    const prefix = `${fromAgentId}::`;
    const toMove: Array<[string, DisclosureStateEntry]> = [];
    for (const [key, entry] of this.entries) {
      if (key.startsWith(prefix)) {
        toMove.push([key, entry]);
      }
    }
    for (const [oldKey, entry] of toMove) {
      this.entries.delete(oldKey);
      const nextKey = `${toAgentId}::${oldKey.slice(prefix.length)}`;
      if (!this.entries.has(nextKey)) {
        this.entries.set(nextKey, entry);
      }
    }
  }

  clear(): void {
    this.entries.clear();
  }

  get size(): number {
    return this.entries.size;
  }
}

export const sharedDisclosureStateStore = new DisclosureStateStore();
