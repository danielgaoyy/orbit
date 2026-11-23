import {
  type AvatarAnimationClip,
  type AvatarDefinition,
  type AvatarEntityPreset,
  createDefaultAvatarDefinition,
  parseAvatarAnimationClip,
} from "@oneworks/avatar";

export interface SubagentCharacterSpec {
  readonly id: string;
  readonly name: string;
  readonly preset: AvatarEntityPreset;
  readonly paletteId: string;
  readonly lightBg: string;
  readonly darkBg: string;
}

/**
 * Curated 24-character OneWorks 3D animal roster for Orbit Subagents.
 * Soft, harmonious circular backdrops complement Orbit's monochrome precision shell
 * while making every parallel specialist immediately recognizable at a glance.
 */
export const SUBAGENT_AVATAR_ROSTER: readonly SubagentCharacterSpec[] = [
  { id: "red-fox", name: "Red Fox", preset: "fox", paletteId: "red-fox", lightBg: "#F5ECE3", darkBg: "#2B2522" },
  { id: "shiba-inu", name: "Shiba Inu", preset: "dog", paletteId: "shiba-inu", lightBg: "#F6EFE2", darkBg: "#2B2721" },
  { id: "orange-tabby", name: "Orange Tabby", preset: "cat", paletteId: "orange-tabby", lightBg: "#F6EDE4", darkBg: "#2C2521" },
  { id: "giant-panda", name: "Giant Panda", preset: "bear", paletteId: "giant-panda", lightBg: "#EBF2EC", darkBg: "#222926" },
  { id: "sea-otter", name: "Sea Otter", preset: "otter", paletteId: "sea-otter", lightBg: "#E8F1F4", darkBg: "#21292E" },
  { id: "barn-owl", name: "Barn Owl", preset: "owl", paletteId: "barn-owl", lightBg: "#F3EFE6", darkBg: "#292623" },
  { id: "emperor-penguin", name: "Emperor Penguin", preset: "penguin", paletteId: "emperor-penguin", lightBg: "#EAF0F7", darkBg: "#212730" },
  { id: "capybara", name: "Capybara", preset: "capybara", paletteId: "capybara", lightBg: "#F2ECE3", darkBg: "#292522" },
  { id: "red-panda", name: "Red Panda", preset: "bear", paletteId: "red-panda", lightBg: "#F5EAE6", darkBg: "#2C2423" },
  { id: "holland-lop", name: "Holland Lop", preset: "rabbit", paletteId: "holland-lop", lightBg: "#F4EEF1", darkBg: "#2A2528" },
  { id: "corgi", name: "Corgi", preset: "dog", paletteId: "corgi", lightBg: "#F6F0E3", darkBg: "#2B2722" },
  { id: "sika-deer", name: "Sika Deer", preset: "deer", paletteId: "sika-deer", lightBg: "#EDF2EA", darkBg: "#232924" },
  { id: "koala", name: "Koala", preset: "bear", paletteId: "koala", lightBg: "#ECF2EE", darkBg: "#232927" },
  { id: "raccoon", name: "Raccoon", preset: "bear", paletteId: "raccoon", lightBg: "#ECEEF3", darkBg: "#24262C" },
  { id: "arctic-fox", name: "Arctic Fox", preset: "fox", paletteId: "arctic-fox", lightBg: "#EBF1F6", darkBg: "#222830" },
  { id: "russian-blue", name: "Russian Blue", preset: "cat", paletteId: "russian-blue", lightBg: "#ECEFF5", darkBg: "#24272E" },
  { id: "husky", name: "Husky", preset: "dog", paletteId: "husky", lightBg: "#EAF0F6", darkBg: "#22272F" },
  { id: "cream-alpaca", name: "Cream Alpaca", preset: "alpaca", paletteId: "cream-alpaca", lightBg: "#F4F0E8", darkBg: "#2A2724" },
  { id: "european-hedgehog", name: "Hedgehog", preset: "hedgehog", paletteId: "european-hedgehog", lightBg: "#F2EEE7", darkBg: "#292623" },
  { id: "red-squirrel", name: "Red Squirrel", preset: "squirrel", paletteId: "red-squirrel", lightBg: "#F5ECE5", darkBg: "#2B2522" },
  { id: "harbor-seal", name: "Harbor Seal", preset: "seal", paletteId: "harbor-seal", lightBg: "#E9F1F4", darkBg: "#22292D" },
  { id: "snowy-owl", name: "Snowy Owl", preset: "owl", paletteId: "snowy-owl", lightBg: "#EEF1F6", darkBg: "#242830" },
  { id: "mallard-duck", name: "Mallard Duck", preset: "duck", paletteId: "mallard-duck", lightBg: "#EAF3EE", darkBg: "#212A27" },
  { id: "gray-chinchilla", name: "Chinchilla", preset: "chinchilla", paletteId: "gray-chinchilla", lightBg: "#EFEFF4", darkBg: "#26272D" },
] as const;

const ROSTER_BY_ID = new Map<string, SubagentCharacterSpec>(
  SUBAGENT_AVATAR_ROSTER.map((spec) => [spec.id, spec]),
);

/** FNV-1a 32-bit stable hash for deterministic avatar slot assignment */
export function hashSubagentAvatarSeed(seed: string): number {
  let hash = 0x811c9dc5;
  for (let i = 0; i < seed.length; i += 1) {
    hash ^= seed.charCodeAt(i);
    hash = Math.imul(hash, 0x01000193);
  }
  return hash >>> 0;
}

/**
 * Resolves a stable, deterministic character for any standalone identity key
 * (e.g. a timeline card or detached Subagent tab without full sibling list).
 */
export function resolveSubagentCharacter(seedKey: string): SubagentCharacterSpec {
  const normalized = seedKey.trim() || "subagent";
  const index = hashSubagentAvatarSeed(normalized) % SUBAGENT_AVATAR_ROSTER.length;
  return SUBAGENT_AVATAR_ROSTER[index]!;
}

export function getSubagentCharacterById(id: string | null | undefined): SubagentCharacterSpec | null {
  if (!id) return null;
  return ROSTER_BY_ID.get(id) ?? null;
}

/**
 * Assigns collision-free characters to a session's subagent list:
 * - Deterministic per `stableSeed` across re-renders & status updates
 * - Sorts keys canonically before linear-probing so changing status order (e.g. running -> completed)
 *   NEVER swaps two subagents' faces mid-session
 * - Guarantees zero duplicate characters for the first 24 concurrent subagents in a session
 */
export function assignSubagentCharacters<T extends { readonly key: string }>(
  items: readonly T[],
  getSeed?: (item: T) => string,
): Map<string, SubagentCharacterSpec> {
  const out = new Map<string, SubagentCharacterSpec>();
  if (items.length === 0) return out;

  const rosterSize = SUBAGENT_AVATAR_ROSTER.length;
  const canonical = items
    .map((item) => ({
      key: item.key,
      seed: (getSeed ? getSeed(item) : item.key).trim() || item.key,
    }))
    .sort((a, b) => a.seed.localeCompare(b.seed));

  const usedIndices = new Set<number>();
  for (const entry of canonical) {
    const preferred = hashSubagentAvatarSeed(entry.seed) % rosterSize;
    let chosen = preferred;
    if (usedIndices.size < rosterSize) {
      for (let offset = 0; offset < rosterSize; offset += 1) {
        const candidate = (preferred + offset) % rosterSize;
        if (!usedIndices.has(candidate)) {
          chosen = candidate;
          usedIndices.add(candidate);
          break;
        }
      }
    }
    out.set(entry.key, SUBAGENT_AVATAR_ROSTER[chosen]!);
  }

  return out;
}

/**
 * Lively, joyful working loop for active subagents:
 * gentle double-hop bounce + curious left/right head turn + crisp blink,
 * seamlessly looping back to frame 0 over the trailing 280ms segment.
 */
export const SUBAGENT_WORKING_CLIP: AvatarAnimationClip = parseAvatarAnimationClip({
  label: "orbit-subagent-working",
  anchor: "relative",
  playback: "loop",
  durationMs: 1160,
  keyframes: [
    {
      atMs: 0,
      easing: "ease-in-out",
      patch: {
        view: { positionY: 0, yaw: 0, pitch: 0 },
      },
    },
    {
      atMs: 280,
      easing: "ease-out",
      patch: {
        view: { positionY: -24, yaw: 0.17, pitch: -0.06 },
      },
    },
    {
      atMs: 580,
      easing: "ease-in-out",
      patch: {
        view: { positionY: 5, yaw: -0.04, pitch: 0.04 },
      },
    },
    {
      atMs: 880,
      easing: "ease-out",
      patch: {
        view: { positionY: -20, yaw: -0.17, pitch: -0.05 },
      },
    },
  ],
});

/**
 * Attentive waiting-for-approval loop:
 * slower, inquisitive head tilt inviting the user to review a pending decision.
 */
export const SUBAGENT_AWAITING_CLIP: AvatarAnimationClip = parseAvatarAnimationClip({
  label: "orbit-subagent-awaiting",
  anchor: "relative",
  playback: "loop",
  durationMs: 1560,
  keyframes: [
    {
      atMs: 0,
      easing: "ease-in-out",
      patch: {
        view: { positionY: 0, yaw: -0.06, pitch: 0.02 },
      },
    },
    {
      atMs: 520,
      easing: "ease-in-out",
      patch: {
        view: { positionY: -10, yaw: 0.22, pitch: -0.09 },
      },
    },
    {
      atMs: 1040,
      easing: "ease-in-out",
      patch: {
        view: { positionY: -5, yaw: 0.12, pitch: -0.04 },
      },
    },
  ],
});

const DEFINITION_CACHE = new Map<string, AvatarDefinition>();

/**
 * Builds and caches a lightweight circular OneWorks `AvatarDefinition` for the given character
 * and resolved UI theme. `entity.parts: []` delegates full 3D animal anatomy generation directly
 * to OneWorks's internal preset builder.
 */
export function buildSubagentAvatarDefinition(
  character: SubagentCharacterSpec,
  theme: "light" | "dark",
): AvatarDefinition {
  const cacheKey = `${character.id}:${theme}`;
  const cached = DEFINITION_CACHE.get(cacheKey);
  if (cached) return cached;

  const base = createDefaultAvatarDefinition();
  const definition: AvatarDefinition = {
    ...base,
    metadata: {
      id: `orbit-subagent-${character.id}`,
      name: character.name,
    },
    scene: {
      ...base.scene,
      appearance: {
        ...base.scene.appearance,
        backgroundStyle: "solid",
        paletteId: character.paletteId,
      },
      camera: {
        ...base.scene.camera,
        background: theme === "dark" ? character.darkBg : character.lightBg,
        frame: "circle",
        showFrameShadow: false,
        size: 128,
      },
      effects: {
        ...base.scene.effects,
        showAvatarShadow: false,
        showFaceShadow: false,
      },
      entity: {
        preset: character.preset,
        parts: [],
      },
      view: {
        ...base.scene.view,
        pitch: -0.03,
        positionX: 0,
        positionY: 16,
        roll: 0,
        scale: 1.46,
        yaw: 0.06,
      },
    },
  };

  DEFINITION_CACHE.set(cacheKey, definition);
  return definition;
}
