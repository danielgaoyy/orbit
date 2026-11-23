import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  computeNegativePhaseDelayMs,
  isSyncedAmbientAnimation,
  type SyncableAnimationLike,
} from "./animation-sync.ts";

describe("animation-sync", () => {
  it("computes consistent negative phase delays for shared clock synchronization", () => {
    assert.equal(computeNegativePhaseDelayMs(0, 850), 0);
    assert.equal(computeNegativePhaseDelayMs(850, 850), 0);
    assert.equal(computeNegativePhaseDelayMs(1000, 850), -150);
    assert.equal(computeNegativePhaseDelayMs(1850, 850), -150);
    assert.equal(computeNegativePhaseDelayMs(250, 1800), -250);
    assert.equal(computeNegativePhaseDelayMs(NaN, 850), 0);
    assert.equal(computeNegativePhaseDelayMs(500, 0), 0);
  });

  it("matches only running infinite ambient animations and ignores one-shot or paused animations", () => {
    const makeAnim = (overrides: Partial<SyncableAnimationLike>): SyncableAnimationLike => ({
      animationName: "spin",
      playState: "running",
      startTime: 1234,
      effect: { getTiming: () => ({ iterations: Infinity }) },
      ...overrides,
    });

    assert.equal(isSyncedAmbientAnimation(makeAnim({ animationName: "spin" })), true);
    assert.equal(isSyncedAmbientAnimation(makeAnim({ animationName: "pulse" })), true);
    assert.equal(
      isSyncedAmbientAnimation(makeAnim({ animationName: "orbit-status-pulse" })),
      true,
    );

    // Must not touch one-shot transitions or non-allowlisted keyframes
    assert.equal(
      isSyncedAmbientAnimation(makeAnim({ animationName: "orbit-floating-in" })),
      false,
    );
    assert.equal(
      isSyncedAmbientAnimation(
        makeAnim({
          animationName: "spin",
          effect: { getTiming: () => ({ iterations: 1 }) },
        }),
      ),
      false,
    );

    // Must not touch paused animations (would unpause hidden-window animations per WAAPI spec)
    assert.equal(
      isSyncedAmbientAnimation(makeAnim({ animationName: "spin", playState: "paused" })),
      false,
    );
  });
});
