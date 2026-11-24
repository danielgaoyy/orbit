import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it } from "node:test";
import {
  FILE_LOADING_GRACE_MS,
  HOVER_PREFETCH_DELAY_MS,
  MAX_CONCURRENT_FILE_PREFETCHES,
  cancelHoverPrefetch,
  isHeavyHoverPrefetchPath,
  scheduleHoverPrefetch,
  shouldPrefetchOnPointerDown,
} from "./file-prefetch.ts";

describe("file preview intent prefetch guards", () => {
  it("skips heavy media, image, archive, and binary paths during passive hover", () => {
    assert.equal(isHeavyHoverPrefetchPath("docs/spec.pdf"), true);
    assert.equal(isHeavyHoverPrefetchPath("assets/demo.MP4?v=1"), true);
    assert.equal(isHeavyHoverPrefetchPath("screenshots/ui.png"), true);
    assert.equal(isHeavyHoverPrefetchPath("dist/bundle.wasm"), true);
    assert.equal(isHeavyHoverPrefetchPath("src/app.tsx"), false);
    assert.equal(isHeavyHoverPrefetchPath("crates/orbit-client-core/src/render.rs"), false);
    assert.equal(isHeavyHoverPrefetchPath("README.md#L10-L20"), false);
    assert.equal(isHeavyHoverPrefetchPath("logs/build.log"), false);
  });

  it("only prefetches on unmodified primary pointerdown", () => {
    assert.equal(shouldPrefetchOnPointerDown({ button: 0 }), true);
    assert.equal(shouldPrefetchOnPointerDown({ button: 1 }), false);
    assert.equal(shouldPrefetchOnPointerDown({ button: 2 }), false);
    assert.equal(shouldPrefetchOnPointerDown({ button: 0, metaKey: true }), false);
    assert.equal(shouldPrefetchOnPointerDown({ button: 0, ctrlKey: true }), false);
    assert.equal(shouldPrefetchOnPointerDown({ button: 0, shiftKey: true }), false);
    assert.equal(shouldPrefetchOnPointerDown({ button: 0, altKey: true }), false);
  });

  it("debounces hover dwell and cancels cleanly on rapid sweeps", async () => {
    const calls: string[] = [];
    scheduleHoverPrefetch(() => {
      calls.push("a");
    }, 20);
    scheduleHoverPrefetch(() => {
      calls.push("b");
    }, 20);
    cancelHoverPrefetch();
    await new Promise((r) => setTimeout(r, 35));
    assert.deepEqual(calls, []);

    const nextCalls: string[] = [];
    scheduleHoverPrefetch(() => {
      nextCalls.push("first");
    }, 20);
    scheduleHoverPrefetch(() => {
      nextCalls.push("second");
    }, 20);
    await new Promise((r) => setTimeout(r, 40));
    assert.deepEqual(nextCalls, ["second"]);
  });

  it("enforces bounded prefetch concurrency, promise dedup, SWR cache, and loading grace delay", () => {
    assert.ok(HOVER_PREFETCH_DELAY_MS >= 50 && HOVER_PREFETCH_DELAY_MS <= 120);
    assert.ok(FILE_LOADING_GRACE_MS >= 80 && FILE_LOADING_GRACE_MS <= 180);
    assert.equal(MAX_CONCURRENT_FILE_PREFETCHES, 2);

    const desktopSrc = resolve(import.meta.dirname, "..");
    const sliceSrc = readFileSync(resolve(desktopSrc, "state/slices/workspace-slice.ts"), "utf8");
    const viewerSrc = readFileSync(
      resolve(desktopSrc, "components/side-panel/file-viewer-tab.tsx"),
      "utf8",
    );
    const mediaSrc = readFileSync(
      resolve(desktopSrc, "components/viewers/media-preview-modal.tsx"),
      "utf8",
    );

    assert.match(sliceSrc, /inFlightFileReads\s*=\s*new Map/);
    assert.match(sliceSrc, /startOrJoinFileRead/);
    assert.match(sliceSrc, /isUnchangedWorkspaceFileView/);
    assert.match(sliceSrc, /stale:\s*true/);
    assert.match(sliceSrc, /MAX_CONCURRENT_FILE_PREFETCHES/);

    assert.match(viewerSrc, /useDelayedLoading\(isInitialLoading\)/);
    assert.match(viewerSrc, /rawCodeLines/);
    assert.doesNotMatch(viewerSrc, /chunkEnd <= 600/);

    assert.match(mediaSrc, /useDelayedLoading\(isInitialLoading\)/);
  });
});
