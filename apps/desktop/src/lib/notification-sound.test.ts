import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  playSoftNotificationChime,
  resetNotificationAudioContextForTests,
} from "./notification-sound.ts";

describe("notification-sound", () => {
  it("gracefully no-ops in headless / Node environments without AudioContext", () => {
    resetNotificationAudioContextForTests();
    assert.equal(playSoftNotificationChime("default"), false);
    assert.equal(playSoftNotificationChime("approval", { audioContextCtor: null }), false);
  });

  it("schedules gentle two-note chime with a mock AudioContext", () => {
    const frequencies: number[] = [];
    let started = 0;
    let stopped = 0;
    let resumed = false;

    class MockAudioContext {
      state = "suspended";
      currentTime = 1.0;
      destination = {};
      resume() {
        resumed = true;
        this.state = "running";
        return Promise.resolve();
      }
      createOscillator() {
        return {
          type: "sine",
          frequency: {
            setValueAtTime(val: number) {
              frequencies.push(val);
            },
          },
          connect() {},
          start() {
            started += 1;
          },
          stop() {
            stopped += 1;
          },
        };
      }
      createGain() {
        return {
          gain: {
            setValueAtTime() {},
            linearRampToValueAtTime() {},
            exponentialRampToValueAtTime() {},
          },
          connect() {},
        };
      }
    }

    const ok = playSoftNotificationChime("default", {
      audioContextCtor: MockAudioContext,
    });
    assert.equal(ok, true);
    assert.equal(resumed, true);
    assert.equal(frequencies.length, 2);
    assert.equal(started, 2);
    assert.equal(stopped, 2);
  });
});
