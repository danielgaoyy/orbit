import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  ARTIFACT_SANDBOX_CSP,
  JSON_RPC_ERR_CONFIRMATION_PENDING,
  JSON_RPC_ERR_INVALID_PARAMS,
  JSON_RPC_ERR_RATE_LIMITED,
  JSON_RPC_ERR_USER_DENIED,
  MAX_BRIDGE_ARGS_BYTES,
  MAX_BRIDGE_TEXT_BYTES,
  SANDBOX_MAX_HEIGHT_PX,
  SANDBOX_MIN_HEIGHT_PX,
  SEP_1865_PROTOCOL_VERSION,
  buildHostContextChangedNotification,
  clampSandboxHeight,
  createMcpAppHostBridge,
  extractInlineArtifactDirectives,
  formatArtifactInlineComment,
  injectSandboxCsp,
  safeFormatToolArgsPreview,
  validateExternalHttpsUrl,
  type PendingMessageRequest,
  type PendingModelContextRequest,
  type PendingOpenLinkRequest,
  type PendingToolCallRequest,
  type PostMessageTargetLike,
} from "./mcp-app-bridge.ts";

class FakeIframeWindow implements PostMessageTargetLike {
  readonly posted: Array<{ message: unknown; targetOrigin: string }> = [];

  postMessage(message: unknown, targetOrigin: string): void {
    this.posted.push({ message, targetOrigin });
  }
}

describe("mcp-app-bridge (SEP-1865 & Hardened Sandbox CSP)", () => {
  it("unconditionally emits byte-0 CSP (with base-uri/form-action none) before any untrusted HTML bypass payloads", () => {
    assert.ok(ARTIFACT_SANDBOX_CSP.includes("base-uri 'none'"));
    assert.ok(ARTIFACT_SANDBOX_CSP.includes("form-action 'none'"));

    const bypassPayloads = [
      `<!-- <head> --><script src="https://evil.com/pwn.js"></script>`,
      `<script src="https://evil.com/early.js"></script><head></head>`,
      `<div title="<head>"></div><base href="https://evil.example/">`,
      `<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src *"></head><body>ok</body></html>`,
    ];

    for (const payload of bypassPayloads) {
      const wrapped = injectSandboxCsp(payload, "dark");
      assert.ok(
        wrapped.startsWith('<!doctype html>\n<html data-theme="dark">\n<head>\n'),
        `must start at byte 0 with Orbit document header for payload: ${payload}`,
      );
      const cspIdx = wrapped.indexOf(
        `<meta http-equiv="Content-Security-Policy" content="${ARTIFACT_SANDBOX_CSP}">`,
      );
      const payloadIdx = wrapped.indexOf(payload);
      assert.ok(cspIdx > 0, "CSP meta tag must exist");
      assert.ok(payloadIdx > cspIdx, "CSP meta tag MUST appear strictly before untrusted HTML bytes");
    }
  });

  it("strictly validates https:// URLs and rejects non-HTTPS, credentials, control chars, and whitespace", () => {
    assert.equal(
      validateExternalHttpsUrl("https://orbit.dev/docs?tab=1#top"),
      "https://orbit.dev/docs?tab=1#top",
    );
    for (const bad of [
      "http://example.com",
      "javascript:alert(1)",
      "file:///etc/passwd",
      "data:text/html,<script>alert(1)</script>",
      "https://user:pass@example.com/secret",
      "ftp://files.example.com",
      " https://example.com",
      "https://example.com ",
      "https://example.com/a b",
      "https://example.com/\nfoo",
      "https://example.com/\tbar",
      "https://example.com/\u0000baz",
      "",
      null,
      123,
    ]) {
      assert.equal(validateExternalHttpsUrl(bad), null, `expected rejection for ${String(bad)}`);
    }
  });

  it("rejects postMessage events from wrong sources, null sources, or non-null origins", () => {
    const iframeWin = new FakeIframeWindow();
    const attackerWin = new FakeIframeWindow();
    let initialized = false;

    const bridge = createMcpAppHostBridge({
      getExpectedSource: () => iframeWin,
      getTheme: () => "dark",
      onInitialized: () => {
        initialized = true;
      },
    });

    assert.equal(
      bridge.handleMessageEvent({
        source: attackerWin,
        origin: "null",
        data: { jsonrpc: "2.0", id: 1, method: "ui/initialize" },
      }),
      false,
    );
    assert.equal(
      bridge.handleMessageEvent({
        source: null,
        origin: "null",
        data: { jsonrpc: "2.0", method: "ui/notifications/initialized" },
      }),
      false,
    );
    assert.equal(
      bridge.handleMessageEvent({
        source: iframeWin,
        origin: "https://evil.example",
        data: { jsonrpc: "2.0", id: 2, method: "ui/initialize" },
      }),
      false,
    );
    assert.equal(iframeWin.posted.length, 0);
    assert.equal(initialized, false);
  });

  it("defers confirmations, rejects bait-and-switch overwrites (-32002), and handles all SEP-1865 methods", () => {
    const iframeWin = new FakeIframeWindow();
    let initialized = false;
    const heights: number[] = [];
    const linkReqs: PendingOpenLinkRequest[] = [];
    const msgReqs: PendingMessageRequest[] = [];
    const ctxReqs: PendingModelContextRequest[] = [];
    const toolReqs: PendingToolCallRequest[] = [];

    const bridge = createMcpAppHostBridge({
      getExpectedSource: () => iframeWin,
      getTheme: () => "dark",
      onInitialized: () => {
        initialized = true;
      },
      onSizeChanged: (h) => heights.push(h),
      onOpenLinkRequest: (r) => linkReqs.push(r),
      onMessageRequest: (r) => msgReqs.push(r),
      onUpdateModelContextRequest: (r) => ctxReqs.push(r),
      onToolCallRequest: (r) => toolReqs.push(r),
    });

    // 1. ui/initialize
    assert.equal(
      bridge.handleMessageEvent({
        source: iframeWin,
        origin: "null",
        data: { jsonrpc: "2.0", id: "init-1", method: "ui/initialize", params: {} },
      }),
      true,
    );
    assert.equal(iframeWin.posted.length, 1);
    const initResp = iframeWin.posted[0].message as {
      id: string;
      result: { protocolVersion: string; hostInfo: { name: string }; hostContext: { theme: string } };
    };
    assert.equal(initResp.id, "init-1");
    assert.equal(initResp.result.protocolVersion, SEP_1865_PROTOCOL_VERSION);
    assert.equal(initResp.result.hostInfo.name, "Orbit");
    assert.equal(initResp.result.hostContext.theme, "dark");

    // 2. ui/notifications/initialized
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", method: "ui/notifications/initialized" },
    });
    assert.equal(initialized, true);

    // 3. ui/notifications/size-changed (with clamping)
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", method: "ui/notifications/size-changed", params: { height: 40 } },
    });
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", method: "ui/notifications/size-changed", params: { height: 99999 } },
    });
    assert.deepEqual(heights, [SANDBOX_MIN_HEIGHT_PX, SANDBOX_MAX_HEIGHT_PX]);
    assert.equal(clampSandboxHeight("bad"), null);

    // 4. ui/open-link: reject http:// immediately, defer https:// until user resolves
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", id: 2, method: "ui/open-link", params: { url: "http://insecure.test" } },
    });
    assert.equal(linkReqs.length, 0);
    assert.equal(
      (iframeWin.posted[1].message as { error?: { code: number } }).error?.code,
      JSON_RPC_ERR_INVALID_PARAMS,
    );

    const beforeLinkCount = iframeWin.posted.length;
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", id: 3, method: "ui/open-link", params: { url: "https://docs.example.com/a" } },
    });
    // Response MUST be deferred until user clicks Open or Cancel
    assert.equal(iframeWin.posted.length, beforeLinkCount);
    assert.equal(linkReqs.length, 1);

    // Bait-and-switch attempt while open-link confirmation is open MUST fail with -32002
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", id: 33, method: "tools/call", params: { name: "evil_tool" } },
    });
    assert.equal(toolReqs.length, 0);
    assert.equal(
      (iframeWin.posted[iframeWin.posted.length - 1].message as { error?: { code: number } }).error
        ?.code,
      JSON_RPC_ERR_CONFIRMATION_PENDING,
    );

    // Resolve link (approved) -> unlocks confirmation slot
    bridge.resolveOpenLink(3, true);
    assert.deepEqual(iframeWin.posted[iframeWin.posted.length - 1].message, {
      jsonrpc: "2.0",
      id: 3,
      result: { ok: true },
    });

    // 5. ui/message (deferred)
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: 4,
        method: "ui/message",
        params: { content: [{ type: "text", text: "Filter by latency > 200ms" }] },
      },
    });
    assert.equal(msgReqs.length, 1);
    assert.equal(msgReqs[0].text, "Filter by latency > 200ms");
    bridge.resolveMessage(4, true);

    // 6. ui/update-model-context (staged, never auto-attached silently)
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: 5,
        method: "ui/update-model-context",
        params: { content: "Selected cluster: prod-us-east" },
      },
    });
    assert.equal(ctxReqs.length, 1);
    assert.equal(ctxReqs[0].note, "Selected cluster: prod-us-east");
    bridge.resolveModelContext(5, false);
    assert.equal(
      (iframeWin.posted[iframeWin.posted.length - 1].message as { error?: { code: number } }).error
        ?.code,
      JSON_RPC_ERR_USER_DENIED,
    );

    // 7. tools/call with bounded argsPreview + user deny
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "tc-9",
        method: "tools/call",
        params: { name: "run_query", arguments: { sql: "SELECT 1" } },
      },
    });
    assert.equal(toolReqs.length, 1);
    assert.equal(toolReqs[0].name, "run_query");
    assert.ok(toolReqs[0].argsPreview.includes('"sql": "SELECT 1"'));
    bridge.resolveToolCall("tc-9", false);
    assert.equal(
      (iframeWin.posted[iframeWin.posted.length - 1].message as { error?: { code: number } }).error
        ?.code,
      JSON_RPC_ERR_USER_DENIED,
    );

    // 8. Theme notification push
    bridge.pushThemeChange("light");
    assert.deepEqual(
      iframeWin.posted[iframeWin.posted.length - 1].message,
      buildHostContextChangedNotification("light"),
    );
  });

  it("enforces payload caps, circular/BigInt args preview safety, and sliding-window rate limiting while coalescing size-changed", () => {
    const iframeWin = new FakeIframeWindow();
    let clock = 10_000;
    const heights: number[] = [];

    const bridge = createMcpAppHostBridge({
      getExpectedSource: () => iframeWin,
      getTheme: () => "light",
      now: () => clock,
      maxRequestsPerSecond: 3,
      onSizeChanged: (h) => heights.push(h),
    });

    // Oversized ui/message (> 16 KiB) rejected
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "big-msg",
        method: "ui/message",
        params: { content: "x".repeat(MAX_BRIDGE_TEXT_BYTES + 10) },
      },
    });
    assert.equal(
      (iframeWin.posted[0].message as { error?: { code: number } }).error?.code,
      JSON_RPC_ERR_INVALID_PARAMS,
    );

    // Oversized tools/call arguments (> 32 KiB) rejected
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "big-args",
        method: "tools/call",
        params: { name: "heavy", arguments: { blob: "y".repeat(MAX_BRIDGE_ARGS_BYTES + 10) } },
      },
    });
    assert.equal(
      (iframeWin.posted[1].message as { error?: { code: number } }).error?.code,
      JSON_RPC_ERR_INVALID_PARAMS,
    );

    // 3rd request within window succeeds; 4th is rate-limited (-32029)
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", id: "ok-3", method: "ui/initialize" },
    });
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", id: "rl-4", method: "ui/initialize" },
    });
    assert.equal(
      (iframeWin.posted[3].message as { error?: { code: number } }).error?.code,
      JSON_RPC_ERR_RATE_LIMITED,
    );

    // Burst of identical size-changed notifications coalesces & never starves RPC quota
    for (let i = 0; i < 50; i += 1) {
      bridge.handleMessageEvent({
        source: iframeWin,
        origin: "null",
        data: { jsonrpc: "2.0", method: "ui/notifications/size-changed", params: { height: 420 } },
      });
    }
    assert.deepEqual(heights, [420]);

    // After sliding window advances 1s, requests succeed again
    clock += 1001;
    bridge.handleMessageEvent({
      source: iframeWin,
      origin: "null",
      data: { jsonrpc: "2.0", id: "ok-after-window", method: "ui/initialize" },
    });
    assert.ok((iframeWin.posted[4].message as { result?: unknown }).result);

    // Circular + BigInt safe preview formatting
    const circular: Record<string, unknown> = { count: 42n };
    circular.self = circular;
    const preview = safeFormatToolArgsPreview(circular, 600);
    assert.ok(preview.includes('"42"'));
    assert.ok(preview.includes("[Circular]"));
  });

  it("extracts inline artifact directives while skipping fences/inline code and handling '>' inside quoted attrs", () => {
    const msg = [
      'Inline code `<orbit-artifact title="IgnoredInline">` is skipped.',
      "```html",
      '<orbit-artifact title="IgnoredFence"><div>no</div></orbit-artifact>',
      "```",
      '<ORBIT-ARTIFACT id="art_cmp" title="Latency > 200ms" kind="html"><div>ok</div></ORBIT-ARTIFACT>',
      "<agent-embed title='Quota >= 90%' uri='ui://orbit/quota'><div>ok</div></agent-embed>",
    ].join("\n");

    const refs = extractInlineArtifactDirectives(msg);
    assert.equal(refs.length, 2);
    assert.equal(refs[0].id, "art_cmp");
    assert.equal(refs[0].title, "Latency > 200ms");
    assert.equal(refs[1].title, "Quota >= 90%");
    assert.equal(refs[1].resourceUri, "ui://orbit/quota");
  });

  it("formats concise inline artifact comments with version, line ranges, and sections", () => {
    assert.equal(
      formatArtifactInlineComment({
        title: "Metrics Dashboard",
        version: 3,
        startLine: 10,
        endLine: 18,
        comment: "Clamp Y-axis to p99",
      }),
      'Artifact "Metrics Dashboard" (v3, L10-L18): Clamp Y-axis to p99',
    );
    assert.equal(
      formatArtifactInlineComment({
        title: "API Spec",
        version: 1,
        startLine: 7,
        endLine: 7,
        comment: "Use snake_case",
      }),
      'Artifact "API Spec" (v1, L7): Use snake_case',
    );
    assert.equal(
      formatArtifactInlineComment({
        title: "Diagram",
        version: 2,
        section: "Preview",
        comment: "Split ingress node",
      }),
      'Artifact "Diagram" (v2, Preview): Split ingress node',
    );
  });
});
