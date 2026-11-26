import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it } from "node:test";

const F8_SURFACES = [
  "src/components/markdown-blocks.tsx",
  "src/components/composer/omni-composer.tsx",
  "src/components/composer/composer-directive-token.tsx",
  "src/components/conversation/conversation-composer.tsx",
  "src/components/stage/stage-prompt-controls.tsx",
  "src/components/side-panel/side-question-tab.tsx",
  "src/components/conversation/conversation-viewport.tsx",
  "src/components/side-panel/subagent-tab.tsx",
  "src/components/stage/subagents-track.tsx",
  "src/components/side-panel/file-viewer-tab.tsx",
  "src/components/diff-viewer.tsx",
] as const;

const FORBIDDEN_PATTERNS = [
  { label: "text-[15px] leading-[1.65]", regex: /text-\[15px\]\s+leading-\[1\.65\]/ },
  { label: "text-[15px] leading-[22px]", regex: /text-\[15px\]\s+leading-\[22px\]/ },
] as const;

const CODE_SURFACE_FILES = [
  "src/components/markdown-blocks.tsx",
  "src/components/diff-viewer.tsx",
  "src/components/side-panel/file-viewer-tab.tsx",
] as const;

const CODE_PANE_FILES = [
  "src/components/diff-viewer.tsx",
  "src/components/side-panel/file-viewer-tab.tsx",
] as const;

describe("F8 typography surface migration gate", () => {
  for (const relPath of F8_SURFACES) {
    it(`${relPath} has no legacy hardcoded reading/composer text-size pairs`, () => {
      const absPath = resolve(import.meta.dirname, "..", "..", relPath);
      const source = readFileSync(absPath, "utf8");
      for (const { label, regex } of FORBIDDEN_PATTERNS) {
        assert.doesNotMatch(
          source,
          regex,
          `${relPath} still contains hardcoded "${label}" instead of --orbit-* typography tokens`,
        );
      }
    });
  }

  for (const relPath of CODE_SURFACE_FILES) {
    it(`${relPath} preserves default Tailwind font-mono via --orbit-font-code-surface`, () => {
      const absPath = resolve(import.meta.dirname, "..", "..", relPath);
      const source = readFileSync(absPath, "utf8");
      assert.doesNotMatch(
        source,
        /--orbit-font-code\)/,
        `${relPath} must use --orbit-font-code-surface instead of --orbit-font-code to avoid changing default font-mono stack`,
      );
      assert.match(source, /font-\(family-name:--orbit-font-code-surface\)/);
    });
  }

  for (const relPath of CODE_PANE_FILES) {
    it(`${relPath} preserves default 12px code pane size via --orbit-code-pane-size`, () => {
      const absPath = resolve(import.meta.dirname, "..", "..", relPath);
      const source = readFileSync(absPath, "utf8");
      assert.doesNotMatch(
        source,
        /text-\(length:--orbit-code-size\)/,
        `${relPath} must use --orbit-code-pane-size (default 12px) to preserve v0.11.7 default size`,
      );
      assert.match(source, /text-\(length:--orbit-code-pane-size\)/);
    });
  }
});
