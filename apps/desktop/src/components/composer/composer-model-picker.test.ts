import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { after, before, describe, it } from "node:test";
import { Children, isValidElement, type ReactNode } from "react";
import { createServer, type ViteDevServer } from "vite";

let server: ViteDevServer;
before(async () => {
  server = await createServer({
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
});
after(async () => {
  await server?.close();
});

function findElements(
  node: ReactNode,
  predicate: (props: Record<string, unknown>) => boolean,
  out: Array<Record<string, unknown>> = [],
): Array<Record<string, unknown>> {
  Children.forEach(node, (child) => {
    if (!isValidElement<Record<string, unknown>>(child)) return;
    if (predicate(child.props)) out.push(child.props);
    if (child.props.children) {
      findElements(child.props.children as ReactNode, predicate, out);
    }
  });
  return out;
}

describe("PopoverContent frameless surface & composer model picker", () => {
  it("omits floating-surface and uses transparent click-through framing when surface='none'", async () => {
    const { PopoverContent } = await server.ssrLoadModule("/src/components/ui/overlays.tsx");

    const bareTree = PopoverContent({ surface: "none", className: "w-auto", children: "inner" });
    const barePopups = findElements(bareTree, (p) => p["data-slot"] === "popover-content");
    assert.equal(barePopups.length, 1);
    const bareClass = String(barePopups[0].className ?? "");
    assert.equal(barePopups[0]["data-surface"], "none");
    assert.match(bareClass, /\bbg-transparent\b/);
    assert.match(bareClass, /\bpointer-events-none\b/);
    assert.match(bareClass, /\bshadow-none\b/);
    assert.doesNotMatch(bareClass, /\bfloating-surface\b/);

    const barePositioners = findElements(bareTree, (p) => p.side === "bottom" && p.align === "start");
    assert.equal(barePositioners.length, 1);
    assert.match(String(barePositioners[0].className ?? ""), /\bpointer-events-none\b/);

    const defaultTree = PopoverContent({ children: "inner" });
    const defaultPopups = findElements(defaultTree, (p) => p["data-slot"] === "popover-content");
    assert.equal(defaultPopups.length, 1);
    assert.equal(defaultPopups[0]["data-surface"], "floating");
    assert.match(String(defaultPopups[0].className ?? ""), /\bfloating-surface\b/);
  });

  it("mounts the detached provider + model picker inside a frameless PopoverContent with interactive inner capsules", () => {
    const source = readFileSync(new URL("./composer-model-picker.tsx", import.meta.url), "utf8");
    const popoverMatch = source.match(/<PopoverContent\b[\s\S]*?>/);
    assert.ok(popoverMatch, "expected PopoverContent in composer-model-picker.tsx");
    assert.match(popoverMatch[0], /\bsurface="none"/);
    assert.doesNotMatch(popoverMatch[0], /!shadow-none|bg-transparent/);
    // Both detached capsules must opt back into pointer events inside the click-through shell.
    const autoMatches = source.match(/floating-surface[^\n]*pointer-events-auto/g) ?? [];
    assert.ok(
      autoMatches.length >= 2,
      `expected both detached capsules to restore pointer-events-auto, got ${autoMatches.length}`,
    );
  });
});
