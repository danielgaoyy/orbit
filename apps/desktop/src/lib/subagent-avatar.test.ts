import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  SUBAGENT_AVATAR_ROSTER as roster,
  assignSubagentCharacters,
  getSubagentCharacterById,
  hashSubagentAvatarSeed,
  resolveSubagentCharacter,
} from "./subagent-avatar.ts";

const ids = [
  "red-fox", "shiba-inu", "orange-tabby", "giant-panda", "sea-otter", "barn-owl",
  "emperor-penguin", "capybara", "red-panda", "holland-lop", "corgi", "sika-deer",
  "koala", "raccoon", "arctic-fox", "russian-blue", "husky", "cream-alpaca",
  "european-hedgehog", "red-squirrel", "harbor-seal", "snowy-owl", "mallard-duck", "gray-chinchilla",
];

// These drawings use absolute SVG commands. Check each segment as well as its endpoints.
const arity = { M: 2, L: 2, H: 1, V: 1, C: 6, S: 4, Q: 4, T: 2, Z: 0 };

function assertCanvasPath(d: string, id: string) {
  assert.match(d, /^M[\d .MLHVCSQTZ-]+Z$/, id);
  for (const [, command, argumentsText] of d.matchAll(/([A-Z])([^A-Z]*)/g)) {
    const count = arity[command as keyof typeof arity];
    const coords = argumentsText.trim() ? argumentsText.trim().split(/\s+/).map(Number) : [];
    assert.equal(coords.length, count, `${id}: ${command}${argumentsText}`);
    assert.ok(coords.every(n => n >= 0 && n <= 64), id);
  }
}

describe("subagent animal portraits", () => {
  it("preserves the published roster order and exact-ID lookup", () => {
    assert.deepEqual(roster.map(({ id }) => id), ids);
    for (const character of roster) assert.equal(getSubagentCharacterById(character.id), character);
    for (const key of ["", "fox", null, undefined]) assert.equal(getSubagentCharacterById(key), null);
  });

  it("draws each animal with solid-color paths, a translucent highlight and paired catchlights", () => {
    const silhouettes = new Set<string>();
    for (const { id, name, lightBg, darkBg, back, head, face, eyes } of roster) {
      assert.ok(name.trim(), id);
      for (const bg of [lightBg, darkBg]) assert.match(bg, /^#[\da-f]{6}$/i, id);
      assert.notEqual(lightBg, darkBg, id);
      for (const layer of [back, head, face, eyes]) {
        assert.ok(layer.length, id);
        for (const { d, fill } of layer) {
          assertCanvasPath(d, id);
          assert.match(fill, /^#[\da-f]{6}([\da-f]{2})?$/i, id);
        }
      }
      assert.ok(head.some(({ fill }) => fill.length === 9 && !fill.endsWith("FF")), id);
      const glints = eyes.at(-1)!;
      assert.equal(glints.fill, "#FFFFFF", id);
      assert.equal(glints.d.split("M").length - 1, 2, id);
      // Ignore metadata, backdrop and coat colors: the drawn geometry must distinguish every face.
      const signature = [back, head, face, eyes].map(layer => layer.map(({ d }) => d));
      silhouettes.add(JSON.stringify(signature));
    }
    assert.equal(silhouettes.size, ids.length);
  });

  it("keeps standalone seed assignments compatible, including whitespace and the fallback", () => {
    assert.equal(hashSubagentAvatarSeed("hello"), 0x4f9f2cab);
    assert.equal(resolveSubagentCharacter("hello").id, "giant-panda");
    assert.equal(resolveSubagentCharacter("  codebase-researcher  ").id, "orange-tabby");
    assert.equal(resolveSubagentCharacter("  "), getSubagentCharacterById("emperor-penguin"));
  });

  it("resolves colliding seeds through every slot before reusing a character", () => {
    const items = Array.from({ length: 25 }, (_, i) => ({ key: String(i).padStart(2, "0") }));
    const allocate = (list: typeof items) => assignSubagentCharacters(list, () => " hello ");
    const assigned = allocate(items);
    assert.equal(assigned.size, 25);
    assert.equal(new Set(items.slice(0, 24).map(({ key }) => assigned.get(key))).size, 24);
    assert.equal(assigned.get("00"), resolveSubagentCharacter("hello"));
    assert.equal(assigned.get("24"), assigned.get("00"));
    assert.deepEqual(allocate([...items].reverse()), assigned);
  });

  it("keeps each specialist's character when status ordering changes", () => {
    const items = Array.from({ length: 24 }, (_, i) => ({ key: `managed:local:subagent-${i}` }));
    const assigned = assignSubagentCharacters(items);
    assert.equal(new Set(assigned.values()).size, 24);
    assert.deepEqual(assignSubagentCharacters([...items].reverse()), assigned);
    assert.deepEqual(assignSubagentCharacters(items, () => " "), assigned);
    assert.deepEqual(assignSubagentCharacters([]), new Map());
  });
});
