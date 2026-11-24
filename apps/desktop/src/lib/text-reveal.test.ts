import assert from "node:assert/strict";
import { afterEach, describe, it } from "node:test";
import React from "react";
import { renderToStaticMarkup } from "react-dom/server";
import type { RichBlockView } from "./orbit-ipc.ts";
import {
  countRichBlocksChars,
  isDocumentHidden,
  prefersReducedMotion,
  shouldPaceTextReveal,
  sliceRichBlocks,
  usePacedRichBlocks,
} from "./text-reveal.ts";

const originalWindow = globalThis.window;
const originalDocument = globalThis.document;

function restoreGlobals() {
  if (originalWindow === undefined) {
    delete (globalThis as { window?: Window }).window;
  } else {
    globalThis.window = originalWindow;
  }
  if (originalDocument === undefined) {
    delete (globalThis as { document?: Document }).document;
  } else {
    globalThis.document = originalDocument;
  }
}

function installBrowserStub(opts: { reducedMotion: boolean; hidden: boolean }) {
  let rafCalls = 0;
  const stubWindow = {
    matchMedia: (query: string) =>
      ({
        matches: opts.reducedMotion && query === "(prefers-reduced-motion: reduce)",
        media: query,
        onchange: null,
        addEventListener() {},
        removeEventListener() {},
        addListener() {},
        removeListener() {},
        dispatchEvent() {
          return false;
        },
      }) as MediaQueryList,
    requestAnimationFrame: () => {
      rafCalls += 1;
      return rafCalls;
    },
    cancelAnimationFrame: () => {},
  };
  globalThis.window = stubWindow as unknown as Window & typeof globalThis;
  globalThis.document = { hidden: opts.hidden } as unknown as Document;
  return {
    getRafCalls: () => rafCalls,
    setHidden: (hidden: boolean) => {
      globalThis.document = { hidden } as unknown as Document;
    },
  };
}

function makeParagraph(text: string): RichBlockView {
  return {
    kind: "paragraph",
    spans: [
      {
        text,
        bold: false,
        italic: false,
        strikethrough: false,
        inline_code: false,
        link_url: null,
      },
    ],
  };
}

describe("text-reveal", () => {
  afterEach(() => {
    restoreGlobals();
  });

  it("counts and slices rich blocks cleanly", () => {
    const blocks: RichBlockView[] = [makeParagraph("Hello Orbit"), makeParagraph("World")];
    assert.equal(countRichBlocksChars(blocks), 16);
    assert.equal(sliceRichBlocks(blocks, 16), blocks);
    assert.deepEqual(sliceRichBlocks(blocks, 0), []);
    assert.deepEqual(sliceRichBlocks(blocks, 5), [makeParagraph("Hello")]);
  });

  it("short-circuits pacing when prefers-reduced-motion is enabled", () => {
    const stub = installBrowserStub({ reducedMotion: true, hidden: false });

    assert.equal(prefersReducedMotion(), true);
    assert.equal(isDocumentHidden(), false);
    assert.equal(shouldPaceTextReveal(true), false);

    const blocks: RichBlockView[] = [makeParagraph("Immediate reduced-motion text")];
    let observed: RichBlockView[] | null = null;
    function Probe() {
      observed = usePacedRichBlocks(blocks, true, "turn-1");
      return React.createElement("div", null, observed.length);
    }

    renderToStaticMarkup(React.createElement(Probe));
    assert.equal(observed, blocks);
    assert.equal(stub.getRafCalls(), 0);
  });

  it("short-circuits pacing when document is hidden and enables it when visible with motion allowed", () => {
    const stub = installBrowserStub({ reducedMotion: false, hidden: true });

    assert.equal(prefersReducedMotion(), false);
    assert.equal(isDocumentHidden(), true);
    assert.equal(shouldPaceTextReveal(true), false);
    assert.equal(shouldPaceTextReveal(false), false);

    stub.setHidden(false);
    assert.equal(isDocumentHidden(), false);
    assert.equal(shouldPaceTextReveal(true), true);
    assert.equal(shouldPaceTextReveal(false), false);
  });
});
