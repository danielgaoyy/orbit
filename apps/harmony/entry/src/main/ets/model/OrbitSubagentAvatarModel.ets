import {
  ORBIT_SUBAGENT_AVATAR_IDS,
  ORBIT_SUBAGENT_AVATAR_ROSTER,
  OrbitSubagentAvatarAsset,
  getOrbitSubagentAvatarAssetById
} from '../components/common/OrbitAvatarRoster';

export {
  ORBIT_SUBAGENT_AVATAR_IDS,
  ORBIT_SUBAGENT_AVATAR_ROSTER,
  OrbitSubagentAvatarAsset
} from '../components/common/OrbitAvatarRoster';

export interface OrbitAvatarSeedKeyed {
  readonly key: string;
}

interface CanonicalAvatarSeedEntry {
  readonly key: string;
  readonly seed: string;
}

/**
 * FNV-1a 32-bit stable hash — exact verbatim port of Desktop `hashSubagentAvatarSeed`.
 */
export function hashSubagentAvatarSeed(seed: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/**
 * Resolves a deterministic animal portrait for any standalone identity key.
 * Empty/whitespace seeds fall back to `"subagent"`.
 */
export function resolveSubagentCharacter(seedKey: string): OrbitSubagentAvatarAsset {
  const normalized = seedKey.trim() || 'subagent';
  const index = hashSubagentAvatarSeed(normalized) % ORBIT_SUBAGENT_AVATAR_ROSTER.length;
  return ORBIT_SUBAGENT_AVATAR_ROSTER[index];
}

export function getSubagentCharacterById(
  id: string | null | undefined
): OrbitSubagentAvatarAsset | null {
  return getOrbitSubagentAvatarAssetById(id);
}

/**
 * Resolves an avatar asset by explicit ID, falling back to deterministic seed hashing.
 * Never falls back to letter/initial avatars.
 */
export function resolveOrbitSubagentAvatar(
  characterId: string | null | undefined,
  seedKey: string
): OrbitSubagentAvatarAsset {
  const byId = getOrbitSubagentAvatarAssetById(characterId);
  if (byId !== null) {
    return byId;
  }
  return resolveSubagentCharacter(seedKey);
}

/**
 * Assigns collision-free characters across a sibling list:
 * - Canonically sorts entries before linear probing so status reordering never swaps faces.
 * - Guarantees zero duplicate portraits for the first 24 concurrent items.
 *
 * Cross-client parity maintenance contract:
 * Strict ArkTS-typed equivalent port of `apps/desktop/src/lib/subagent-avatar.ts:assignSubagentCharacters`
 * (using explicit typed loops instead of untyped object/spread helpers). Behavioral equivalence is
 * enforced by the shared 49-vector truth table (25 colliding seeds + 24 specialist keys) verified in
 * `apps/harmony/entry/src/ohosTest/ets/test/OrbitPr2.test.ets` against Desktop's `subagent-avatar.test.ts`.
 */
export function assignSubagentCharacters<T extends OrbitAvatarSeedKeyed>(
  items: readonly T[],
  getSeed?: (item: T) => string
): Map<string, OrbitSubagentAvatarAsset> {
  const out = new Map<string, OrbitSubagentAvatarAsset>();
  const rosterSize = ORBIT_SUBAGENT_AVATAR_ROSTER.length;

  const canonical: CanonicalAvatarSeedEntry[] = [];
  for (let i = 0; i < items.length; i += 1) {
    const item = items[i];
    const rawSeed = getSeed !== undefined ? getSeed(item) : item.key;
    const trimmed = rawSeed.trim();
    canonical.push({
      key: item.key,
      seed: trimmed.length > 0 ? trimmed : item.key
    });
  }

  canonical.sort((a: CanonicalAvatarSeedEntry, b: CanonicalAvatarSeedEntry): number => {
    const seedCmp = a.seed.localeCompare(b.seed);
    if (seedCmp !== 0) {
      return seedCmp;
    }
    if (a.key < b.key) {
      return -1;
    }
    if (a.key > b.key) {
      return 1;
    }
    return 0;
  });

  const usedIndices = new Set<number>();
  for (let i = 0; i < canonical.length; i += 1) {
    const entry = canonical[i];
    let chosen = hashSubagentAvatarSeed(entry.seed) % rosterSize;
    if (usedIndices.size < rosterSize) {
      while (usedIndices.has(chosen)) {
        chosen = (chosen + 1) % rosterSize;
      }
      usedIndices.add(chosen);
    }
    out.set(entry.key, ORBIT_SUBAGENT_AVATAR_ROSTER[chosen]);
  }

  return out;
}
