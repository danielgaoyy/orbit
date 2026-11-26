import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { cn } from "./utils.ts";

describe("cn (tailwind-merge v4 typed arbitrary syntax)", () => {
  it("preserves text color when combined with text-(length:--orbit-read-body)", () => {
    const merged = cn("text-ink-primary", "text-(length:--orbit-read-body)");
    assert.ok(merged.includes("text-ink-primary"), `expected text-ink-primary in "${merged}"`);
    assert.ok(
      merged.includes("text-(length:--orbit-read-body)"),
      `expected text-(length:--orbit-read-body) in "${merged}"`,
    );
  });

  it("overrides an earlier pixel font-size with text-(length:--orbit-read-body)", () => {
    assert.equal(
      cn("text-[15px]", "text-(length:--orbit-read-body)"),
      "text-(length:--orbit-read-body)",
    );
  });

  it("overrides an earlier line-height with leading-(--orbit-read-leading)", () => {
    assert.equal(
      cn("leading-[1.65]", "leading-(--orbit-read-leading)"),
      "leading-(--orbit-read-leading)",
    );
  });

  it("preserves text color when combined with font-(family-name:--orbit-font-content)", () => {
    const merged = cn("text-ink-secondary", "font-(family-name:--orbit-font-content)");
    assert.ok(merged.includes("text-ink-secondary"), `expected text-ink-secondary in "${merged}"`);
    assert.ok(
      merged.includes("font-(family-name:--orbit-font-content)"),
      `expected font-(family-name:--orbit-font-content) in "${merged}"`,
    );
  });
});
