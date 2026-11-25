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

  it("renders model and thinking effort in the bottom trigger without a dot separator across both interactive and disabled states", () => {
    const source = readFileSync(new URL("./composer-model-picker.tsx", import.meta.url), "utf8");
    const triggerMatch = source.match(/const triggerContent = \(([\s\S]*?)\n  \);/);
    assert.ok(triggerMatch, "expected shared triggerContent in composer-model-picker.tsx");
    assert.match(triggerMatch[1], /\{activeTriggerLabel\}/);
    assert.match(triggerMatch[1], /\{activeEffortLabel\}/);
    assert.doesNotMatch(triggerMatch[1], /·/);
    const triggerUsages = source.match(/\{triggerContent\}/g) ?? [];
    assert.equal(
      triggerUsages.length,
      2,
      `expected both disabled and interactive triggers to share triggerContent, got ${triggerUsages.length}`,
    );
  });

  it("truncates long model IDs across primary rows, group rows, sub-models, and fixed ACP rows without overflowing", () => {
    const source = readFileSync(new URL("./composer-model-picker.tsx", import.meta.url), "utf8");
    // Fixed ACP row must bound rawActiveModel instead of using unbounded shrink-0.
    const acpFixedMatch = source.match(/\{rawActiveModel && \(([\s\S]*?)\)\}/);
    assert.ok(acpFixedMatch, "expected fixed ACP rawActiveModel row");
    assert.match(acpFixedMatch[1], /\bmax-w-\[55%\]/);
    assert.match(acpFixedMatch[1], /\btruncate\b/);
    assert.match(acpFixedMatch[1], /\bmin-w-0\b/);

    // Primary model row & group row labels must never use unbounded shrink-0 + truncate.
    assert.match(
      source,
      /row\.kind === "model"\s*\?\s*"min-w-0 truncate"\s*:\s*"min-w-0 max-w-\[60%\] shrink-0 truncate"/,
    );
    // Level-2 sub-model label truncates while keeping effort badge shrink-0.
    assert.match(source, /"min-w-0 truncate text-\[13px\] leading-normal"/);
    // Profile, ACP agent, and group rows expose native tooltips for truncated labels.
    assert.match(source, /title=\{\[prof\.name, profMeta\]\.filter\(Boolean\)\.join\(" — "\)\}/);
    assert.match(source, /title=\{item\.spec\.name\}/);
    assert.match(source, /:\s*\[row\.label, rowSecondary\]\.filter\(Boolean\)\.join\(" — "\)/);
  });
});
