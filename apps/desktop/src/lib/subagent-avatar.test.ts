import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  isAvatarDefinition,
  parseAvatarDefinition,
  resolveAvatarAnimationFrame,
} from "@oneworks/avatar";
import {
  SUBAGENT_AWAITING_CLIP,
  SUBAGENT_AVATAR_ROSTER,
  SUBAGENT_WORKING_CLIP,
  assignSubagentCharacters,
  buildSubagentAvatarDefinition,
  resolveSubagentCharacter,
} from "./subagent-avatar.ts";

describe("subagent-avatar", () => {
  it("builds valid OneWorks AvatarDefinitions for all 24 characters in light and dark themes", () => {
    assert.equal(SUBAGENT_AVATAR_ROSTER.length, 24);
    for (const character of SUBAGENT_AVATAR_ROSTER) {
      for (const theme of ["light", "dark"] as const) {
        const def = buildSubagentAvatarDefinition(character, theme);
        const parsed = parseAvatarDefinition(def);
        assert.equal(isAvatarDefinition(parsed), true, `Expected valid definition for ${character.id} (${theme})`);
        assert.equal(parsed.scene.entity.preset, character.preset);
        assert.equal(parsed.scene.appearance.paletteId, character.paletteId);
        assert.equal(parsed.scene.camera.frame, "circle");
      }
    }
  });

  it("assigns 24 concurrent subagents without any duplicate character", () => {
    const items = Array.from({ length: 24 }, (_, idx) => ({
      key: `managed:local:subagent-${idx}`,
    }));
    const assigned = assignSubagentCharacters(items);
    assert.equal(assigned.size, 24);
    const uniqueIds = new Set(Array.from(assigned.values()).map((c) => c.id));
    assert.equal(uniqueIds.size, 24);
  });

  it("keeps character assignment stable even when input list order changes", () => {
    const itemsA = [
      { key: "managed:local:alpha" },
      { key: "managed:local:beta" },
      { key: "managed:local:gamma" },
    ];
    const itemsB = [...itemsA].reverse();
    const mapA = assignSubagentCharacters(itemsA);
    const mapB = assignSubagentCharacters(itemsB);
    for (const item of itemsA) {
      assert.equal(mapA.get(item.key)?.id, mapB.get(item.key)?.id);
    }
  });

  it("resolves working and awaiting animation frames cleanly", () => {
    const def = buildSubagentAvatarDefinition(resolveSubagentCharacter("codebase-researcher"), "dark");
    const workingFrame = resolveAvatarAnimationFrame(def, SUBAGENT_WORKING_CLIP, 280);
    assert.ok(workingFrame.scene.view.positionY < def.scene.view.positionY);
    const awaitingFrame = resolveAvatarAnimationFrame(def, SUBAGENT_AWAITING_CLIP, 520);
    assert.ok(awaitingFrame.scene.view.yaw > def.scene.view.yaw);
  });
});
