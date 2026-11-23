import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createXtermWriteBatcher } from "./xterm-write-batcher.ts";

describe("createXtermWriteBatcher", () => {
  it("coalesces bursts into one requestAnimationFrame write when visible", () => {
    let hidden = false;
    let nextId = 1;
    const rafCbs = new Map<number, () => void>();
    const timerCbs = new Map<number, () => void>();
    const written: Uint8Array[] = [];

    const batcher = createXtermWriteBatcher((bytes) => written.push(bytes), {
      isDocumentHidden: () => hidden,
      requestAnimationFrame: (cb) => {
        const id = nextId++;
        rafCbs.set(id, cb);
        return id;
      },
      cancelAnimationFrame: (id) => {
        rafCbs.delete(id);
      },
      setTimeout: (cb) => {
        const id = nextId++;
        timerCbs.set(id, cb);
        return id;
      },
      clearTimeout: (id) => {
        timerCbs.delete(id);
      },
      maxPendingBytes: 1024,
    });

    batcher.enqueue(new Uint8Array([1, 2]));
    batcher.enqueue(new Uint8Array([3, 4, 5]));
    assert.equal(written.length, 0);
    assert.equal(rafCbs.size, 1);
    assert.equal(timerCbs.size, 0);
    assert.equal(batcher.pendingByteCount(), 5);

    for (const [id, cb] of [...rafCbs.entries()]) {
      rafCbs.delete(id);
      cb();
    }
    assert.equal(written.length, 1);
    assert.deepEqual([...written[0]], [1, 2, 3, 4, 5]);
    assert.equal(batcher.pendingByteCount(), 0);

    // Transition to hidden while an rAF is queued switches to setTimeout drain
    batcher.enqueue(new Uint8Array([9]));
    assert.equal(rafCbs.size, 1);
    hidden = true;
    batcher.onVisibilityChange();
    assert.equal(rafCbs.size, 0);
    assert.equal(timerCbs.size, 1);
    for (const [id, cb] of [...timerCbs.entries()]) {
      timerCbs.delete(id);
      cb();
    }
    assert.equal(written.length, 2);
    assert.deepEqual([...written[1]], [9]);
  });

  it("drains via setTimeout when hidden and flushes immediately when byte cap is hit", () => {
    let hidden = true;
    let nextId = 1;
    const rafCbs = new Map<number, () => void>();
    const timerCbs = new Map<number, () => void>();
    const written: Uint8Array[] = [];

    const batcher = createXtermWriteBatcher((bytes) => written.push(bytes), {
      isDocumentHidden: () => hidden,
      requestAnimationFrame: (cb) => {
        const id = nextId++;
        rafCbs.set(id, cb);
        return id;
      },
      cancelAnimationFrame: (id) => {
        rafCbs.delete(id);
      },
      setTimeout: (cb) => {
        const id = nextId++;
        timerCbs.set(id, cb);
        return id;
      },
      clearTimeout: (id) => {
        timerCbs.delete(id);
      },
      maxPendingBytes: 16,
    });

    batcher.enqueue(new Uint8Array(10));
    assert.equal(rafCbs.size, 0);
    assert.equal(timerCbs.size, 1);
    assert.equal(written.length, 0);

    // Pushing past 16-byte cap flushes synchronously and cancels timer so memory never grows unbounded
    batcher.enqueue(new Uint8Array(8));
    assert.equal(written.length, 1);
    assert.equal(written[0].byteLength, 18);
    assert.equal(timerCbs.size, 0);
    assert.equal(batcher.pendingByteCount(), 0);

    // Dispose cancels any pending drain and drops unread chunks
    hidden = false;
    batcher.enqueue(new Uint8Array(4));
    assert.equal(rafCbs.size, 1);
    batcher.dispose();
    assert.equal(rafCbs.size, 0);
    assert.equal(batcher.pendingByteCount(), 0);
  });
});
