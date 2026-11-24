import assert from "node:assert/strict";
import { tmpdir } from "node:os";
import { resolve } from "node:path";
import { after, before, describe, it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import {
  CODE_BLOCK_COLLAPSE_THRESHOLD,
  buildNestedListForest,
  classifyRunnableShellSnippet,
  groupMarkdownBlocks,
  type ListItemBlock,
} from "./markdown-helpers.ts";
import type { RichBlockView } from "./orbit-ipc.ts";

function makeListItem(
  text: string,
  opts: { depth?: number; ordered?: boolean; index?: number; checked?: boolean | null } = {},
): ListItemBlock {
  return {
    kind: "list_item",
    ordered: opts.ordered ?? false,
    index: opts.index ?? 0,
    checked: opts.checked ?? null,
    depth: opts.depth ?? 0,
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

let server: ViteDevServer;

before(async () => {
  server = await createServer({
    cacheDir: resolve(tmpdir(), `orbit-vite-test-markdown-blocks-${process.pid}`),
    server: { middlewareMode: true, hmr: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
    plugins: [
      {
        name: "test-markdown-blocks-mocks",
        enforce: "pre",
        load(id) {
          if (id.endsWith("/state/orbit-store.ts")) {
            return `
              export const orbitActions = {
                showToast: () => {},
                openFileInSidePanel: () => {},
                runCommandInTerminal: () => {},
              };
              export const useOrbitSelector = () => null;
            `;
          }
          if (id.endsWith("/lib/orbit-ipc.ts")) {
            return `
              export const isMacDesktop = () => true;
              export const detectDiagramKind = () => null;
              export const orbitIpc = {
                openExternalUrl: async () => {},
              };
            `;
          }
        },
      },
    ],
  });
});

after(async () => {
  await server?.close();
});

describe("markdown-helpers & MarkdownBlocksView (B3-08)", () => {
  it("groups and builds hierarchical 3-level nested list trees including mixed ordered/unordered kinds", () => {
    const blocks: RichBlockView[] = [
      makeListItem("Top 1", { ordered: true, index: 1, depth: 0 }),
      makeListItem("Child A", { ordered: false, depth: 1 }),
      makeListItem("Grandchild i", { ordered: false, depth: 2 }),
      makeListItem("Grandchild ii", { ordered: false, depth: 2 }),
      makeListItem("Child B", { ordered: false, depth: 1 }),
      makeListItem("Top 2", { ordered: true, index: 2, depth: 0 }),
    ];

    const grouped = groupMarkdownBlocks(blocks);
    assert.equal(grouped.length, 1);
    assert.equal(grouped[0].kind, "list_group");
    if (grouped[0].kind !== "list_group") return;

    const forest = buildNestedListForest(grouped[0].items);
    assert.equal(forest.length, 1);
    assert.equal(forest[0].listKind, "ordered");
    assert.equal(forest[0].depth, 0);
    assert.equal(forest[0].items.length, 2);

    const top1Children = forest[0].items[0].children;
    assert.equal(top1Children.length, 1);
    assert.equal(top1Children[0].listKind, "unordered");
    assert.equal(top1Children[0].depth, 1);
    assert.equal(top1Children[0].items.length, 2);

    const childAChildren = top1Children[0].items[0].children;
    assert.equal(childAChildren.length, 1);
    assert.equal(childAChildren[0].depth, 2);
    assert.equal(childAChildren[0].items.length, 2);
  });

  it("classifies runnable shell snippets safely (auto-run safe single-line, paste multi-line or destructive, ignore non-shell)", () => {
    const safe = classifyRunnableShellSnippet("bash", "$ cargo test -p orbit-client-core");
    assert.equal(safe.runnable, true);
    assert.equal(safe.cleanedCommand, "cargo test -p orbit-client-core");
    assert.equal(safe.safeToAutoExecute, true);
    assert.equal(safe.isMultiline, false);
    assert.equal(safe.isPotentiallyDestructive, false);

    const multi = classifyRunnableShellSnippet("sh", "cargo fmt\ncargo test");
    assert.equal(multi.runnable, true);
    assert.equal(multi.isMultiline, true);
    assert.equal(multi.safeToAutoExecute, false);

    const destructive = classifyRunnableShellSnippet("zsh", "rm -rf ./target");
    assert.equal(destructive.runnable, true);
    assert.equal(destructive.isPotentiallyDestructive, true);
    assert.equal(destructive.safeToAutoExecute, false);

    const resetHard = classifyRunnableShellSnippet("shell", "git reset --hard HEAD~1");
    assert.equal(resetHard.runnable, true);
    assert.equal(resetHard.isPotentiallyDestructive, true);
    assert.equal(resetHard.safeToAutoExecute, false);

    const nonShell = classifyRunnableShellSnippet("rust", "fn main() {}");
    assert.equal(nonShell.runnable, false);

    const commentOnly = classifyRunnableShellSnippet("bash", "# just a comment");
    assert.equal(commentOnly.runnable, false);
  });

  it("renders indented nested lists, >12-line code collapse toggle, and Run-in-Terminal button in MarkdownBlocksView", async () => {
    const mod = await server.ssrLoadModule("/src/components/markdown-blocks.tsx");
    const longRustCode = Array.from(
      { length: CODE_BLOCK_COLLAPSE_THRESHOLD + 4 },
      (_, idx) => `let line_${idx + 1} = ${idx + 1};`,
    ).join("\n");

    const blocks: RichBlockView[] = [
      makeListItem("Root item", { ordered: false, depth: 0 }),
      makeListItem("Nested item", { ordered: false, depth: 1 }),
      makeListItem("Deep item", { ordered: true, index: 1, depth: 2 }),
      {
        kind: "code_block",
        language: "bash",
        raw_code: "cargo test --workspace",
        lines: [
          {
            line_number: 1,
            tokens: [
              { text: "cargo test --workspace", fg_hex: "#d4d4d4", bold: false, italic: false },
            ],
          },
        ],
      },
      {
        kind: "code_block",
        language: "rust",
        raw_code: longRustCode,
        lines: longRustCode.split("\n").map((line, idx) => ({
          line_number: idx + 1,
          tokens: [{ text: line, fg_hex: "#d4d4d4", bold: false, italic: false }],
        })),
      },
    ];

    const html = renderToStaticMarkup(createElement(mod.MarkdownBlocksView, { blocks }));

    // Nested list depths render hierarchical <ul>/<ol> with pl-[20px] indentation
    assert.match(html, /data-list-depth="0"/);
    assert.match(html, /data-list-depth="1"[^>]*pl-\[20px\]|pl-\[20px\][^>]*data-list-depth="1"/);
    assert.match(html, /<ol[^>]*data-list-depth="2"/);

    // Bash snippet gets Run-in-Terminal button; Rust snippet does NOT
    assert.match(html, /Run in Terminal/);
    // Long (>12 line) Rust snippet gets collapse/expand toggle with aria-expanded="true"
    assert.match(html, /aria-expanded="true"/);
  });
});
