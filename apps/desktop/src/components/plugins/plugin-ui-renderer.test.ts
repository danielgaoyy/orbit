import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { isSafePluginUrl, tokenizeSafePluginMarkdownLine } from "./plugin-ui-safety.ts";

describe("plugin-ui-renderer safety helpers", () => {
  it("allows only http and https URLs", () => {
    assert.equal(isSafePluginUrl("https://orbit.dev/docs"), true);
    assert.equal(isSafePluginUrl("http://localhost:8080/health"), true);
    assert.equal(isSafePluginUrl("javascript:alert(1)"), false);
    assert.equal(isSafePluginUrl("data:text/html,<script>alert(1)</script>"), false);
    assert.equal(isSafePluginUrl("file:///etc/passwd"), false);
    assert.equal(isSafePluginUrl("vbscript:msgbox(1)"), false);
    assert.equal(isSafePluginUrl(""), false);
  });

  it("tokenizes inline markdown safely and neutralizes unsafe link schemes", () => {
    const safe = tokenizeSafePluginMarkdownLine(
      "Status **ok** in `cargo.toml` via [Docs](https://example.com/a)",
    );
    assert.deepEqual(safe, [
      { kind: "text", text: "Status " },
      { kind: "strong", text: "ok" },
      { kind: "text", text: " in " },
      { kind: "code", text: "cargo.toml" },
      { kind: "text", text: " via " },
      { kind: "link", text: "Docs", href: "https://example.com/a" },
    ]);

    const unsafe = tokenizeSafePluginMarkdownLine("[Click](javascript:alert(1))");
    assert.deepEqual(unsafe, [{ kind: "text", text: "Click" }]);
  });
});
