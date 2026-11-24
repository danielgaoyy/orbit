import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  ARTIFACT_SANDBOX_CSP,
  JSON_RPC_ERR_INVALID_PARAMS,
  JSON_RPC_ERR_METHOD_NOT_FOUND,
  JSON_RPC_ERR_USER_DENIED,
  SANDBOX_MAX_HEIGHT_PX,
  SANDBOX_MIN_HEIGHT_PX,
  createMcpAppHostBridge,
  injectSandboxCsp,
  validateUiResourceUri,
  type JsonRpcResponse,
  type PendingReadResourceRequest,
  type PendingToolCallRequest,
  type PostMessageTargetLike,
} from "./mcp-app-bridge.ts";

class FakeIframeWindow implements PostMessageTargetLike {
  readonly posted: Array<{ message: unknown; targetOrigin: string }> = [];

  postMessage(message: unknown, targetOrigin: string): void {
    this.posted.push({ message, targetOrigin });
  }
}

describe("artifact-sandbox-bridge (SEP-1865 bidirectional postMessage bridge)", () => {
  it("validates strict ui:// URIs and rejects non-ui schemes, control chars, whitespace, and empty paths", () => {
    assert.equal(validateUiResourceUri("ui://metrics/dashboard?v=2"), "ui://metrics/dashboard?v=2");
    for (const bad of [
      "https://example.com/app",
      "http://127.0.0.1:6868/mcp",
      "file:///etc/passwd",
      "javascript:alert(1)",
      "ui://",
      "ui:///leading-slash",
      "ui://metrics/bad path",
      "ui://metrics/bad\nline",
      "ui://metrics/bad\u0000nul",
      "",
      null,
      42,
    ]) {
      assert.equal(validateUiResourceUri(bad), null, `expected rejection for ${String(bad)}`);
    }
  });

  it("handles ui/size-change clamping and coalescing without consuming RPC rate-limit quota", () => {
    const iframe = new FakeIframeWindow();
    const heights: number[] = [];
    let nowMs = 1000;

    const bridge = createMcpAppHostBridge({
      getExpectedSource: () => iframe,
      getTheme: () => "dark",
      now: () => nowMs,
      onSizeChanged: (h) => heights.push(h),
    });

    assert.equal(
      bridge.handleMessageEvent({
        source: iframe,
        origin: "null",
        data: { jsonrpc: "2.0", method: "ui/size-change", params: { height: 50 } },
      }),
      true,
    );
    assert.equal(
      bridge.handleMessageEvent({
        source: iframe,
        origin: "null",
        data: { jsonrpc: "2.0", method: "ui/size-change", params: { height: 50 } },
      }),
      true,
    );
    nowMs += 150;
    assert.equal(
      bridge.handleMessageEvent({
        source: iframe,
        origin: "null",
        data: { jsonrpc: "2.0", method: "ui/size-change", params: { height: 99999 } },
      }),
      true,
    );

    assert.deepEqual(heights, [SANDBOX_MIN_HEIGHT_PX, SANDBOX_MAX_HEIGHT_PX]);
  });

  it("advertises serverResources & serverTools on ui/initialize and binds resources/read strictly to owning server", () => {
    const iframe = new FakeIframeWindow();
    const attacker = new FakeIframeWindow();
    const readReqs: PendingReadResourceRequest[] = [];

    const bridge = createMcpAppHostBridge({
      getExpectedSource: () => iframe,
      getTheme: () => "light",
      getOwningMcpServerName: () => "analytics-mcp",
      onReadResourceRequest: (req) => readReqs.push(req),
    });

    // Reject spoofed source or non-null origin
    assert.equal(
      bridge.handleMessageEvent({
        source: attacker,
        origin: "null",
        data: { jsonrpc: "2.0", id: 1, method: "resources/read", params: { uri: "ui://analytics/v1" } },
      }),
      false,
    );
    assert.equal(
      bridge.handleMessageEvent({
        source: iframe,
        origin: "https://evil.example",
        data: { jsonrpc: "2.0", id: 1, method: "resources/read", params: { uri: "ui://analytics/v1" } },
      }),
      false,
    );

    // Initialize advertises serverResources & serverTools
    bridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: { jsonrpc: "2.0", id: "init-1", method: "ui/initialize" },
    });
    const initResp = iframe.posted.at(-1)?.message as JsonRpcResponse;
    const caps = (initResp.result as { hostCapabilities: Record<string, unknown> }).hostCapabilities;
    assert.ok("serverTools" in caps);
    assert.ok("serverResources" in caps);

    // Non-ui:// scheme rejected with -32602
    bridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "r-bad-scheme",
        method: "resources/read",
        params: { uri: "file:///etc/hosts" },
      },
    });
    assert.equal((iframe.posted.at(-1)?.message as JsonRpcResponse).error?.code, JSON_RPC_ERR_INVALID_PARAMS);

    // Cross-server override rejected with -32001
    bridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "r-cross",
        method: "resources/read",
        params: { uri: "ui://other/secret", serverName: "other-mcp" },
      },
    });
    assert.equal((iframe.posted.at(-1)?.message as JsonRpcResponse).error?.code, JSON_RPC_ERR_USER_DENIED);
    assert.equal(readReqs.length, 0);

    // Valid ui:// read forwarded with owning server name and resolved back to iframe
    bridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "r-ok",
        method: "resources/read",
        params: { uri: "ui://analytics/chart" },
      },
    });
    assert.equal(readReqs.length, 1);
    assert.equal(readReqs[0].serverName, "analytics-mcp");
    assert.equal(readReqs[0].uri, "ui://analytics/chart");

    bridge.resolveResourceRead("r-ok", {
      uri: "ui://analytics/chart",
      mimeType: "text/html;profile=mcp-app",
      text: "<section>Chart HTML</section>",
    });
    const readResp = iframe.posted.at(-1)?.message as JsonRpcResponse;
    assert.equal(readResp.id, "r-ok");
    assert.deepEqual(readResp.result, {
      contents: [
        {
          uri: "ui://analytics/chart",
          mimeType: "text/html;profile=mcp-app",
          text: "<section>Chart HTML</section>",
        },
      ],
    });
  });

  it("denies resources/read when unbound and blocks cross-server tools/call while round-tripping real MCP results", () => {
    const iframe = new FakeIframeWindow();
    const unboundBridge = createMcpAppHostBridge({
      getExpectedSource: () => iframe,
      getTheme: () => "dark",
      getOwningMcpServerName: () => null,
      onReadResourceRequest: () => {
        throw new Error("must not invoke callback when unbound");
      },
    });
    unboundBridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "u-1",
        method: "resources/read",
        params: { uri: "ui://analytics/chart" },
      },
    });
    assert.equal((iframe.posted.at(-1)?.message as JsonRpcResponse).error?.code, JSON_RPC_ERR_USER_DENIED);

    // Bound bridge for tools/call
    const toolReqs: PendingToolCallRequest[] = [];
    const boundBridge = createMcpAppHostBridge({
      getExpectedSource: () => iframe,
      getTheme: () => "dark",
      getOwningMcpServerName: () => "analytics-mcp",
      onToolCallRequest: (req) => toolReqs.push(req),
    });

    // Reject cross-server tool invocation attempt
    boundBridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "t-cross",
        method: "tools/call",
        params: { name: "query", server: "shell-mcp", arguments: { sql: "SELECT 1" } },
      },
    });
    assert.equal((iframe.posted.at(-1)?.message as JsonRpcResponse).error?.code, JSON_RPC_ERR_USER_DENIED);
    assert.equal(toolReqs.length, 0);

    // Reject unknown non-allowlisted method
    boundBridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: { jsonrpc: "2.0", id: "bad-method", method: "fs/write_text_file", params: {} },
    });
    assert.equal(
      (iframe.posted.at(-1)?.message as JsonRpcResponse).error?.code,
      JSON_RPC_ERR_METHOD_NOT_FOUND,
    );

    // Allow valid tool call and return real MCP JSON-RPC tool result
    boundBridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "t-ok",
        method: "tools/call",
        params: { name: "refresh_metrics", arguments: { window: "1h" } },
      },
    });
    assert.equal(toolReqs.length, 1);
    assert.equal(toolReqs[0].name, "refresh_metrics");

    boundBridge.resolveToolCallResult("t-ok", {
      ok: true,
      isError: false,
      content: [{ type: "text", text: "rows=42" }],
      structuredContent: { rows: 42 },
    });
    const toolResp = iframe.posted.at(-1)?.message as JsonRpcResponse;
    assert.equal(toolResp.id, "t-ok");
    assert.deepEqual(toolResp.result, {
      content: [{ type: "text", text: "rows=42" }],
      isError: false,
      structuredContent: { rows: 42 },
    });
  });

  it("enforces frame-src 'none' in byte-0 CSP and invalidates in-flight replies on iframe navigation", () => {
    assert.ok(ARTIFACT_SANDBOX_CSP.includes("frame-src 'none'"));
    assert.ok(ARTIFACT_SANDBOX_CSP.includes("form-action 'none'"));
    assert.ok(ARTIFACT_SANDBOX_CSP.includes("base-uri 'none'"));
    const wrapped = injectSandboxCsp("<div>app</div>", "dark");
    assert.ok(wrapped.includes("frame-src 'none'"));

    const iframe = new FakeIframeWindow();
    const readReqs: PendingReadResourceRequest[] = [];
    const toolReqs: PendingToolCallRequest[] = [];

    const bridge = createMcpAppHostBridge({
      getExpectedSource: () => iframe,
      getTheme: () => "dark",
      getOwningMcpServerName: () => "analytics-mcp",
      onReadResourceRequest: (req) => readReqs.push(req),
      onToolCallRequest: (req) => toolReqs.push(req),
    });

    bridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "nav-r1",
        method: "resources/read",
        params: { uri: "ui://analytics/page1" },
      },
    });
    bridge.handleMessageEvent({
      source: iframe,
      origin: "null",
      data: {
        jsonrpc: "2.0",
        id: "nav-t1",
        method: "tools/call",
        params: { name: "refresh", arguments: {} },
      },
    });
    assert.equal(readReqs.length, 1);
    assert.equal(toolReqs.length, 1);
    const postedBeforeNav = iframe.posted.length;

    // Simulate iframe self-navigation / reload before async replies arrive
    const nextEpoch = bridge.invalidateNavigation();
    assert.ok(nextEpoch > readReqs[0].navigationEpoch);

    // Late replies for the pre-navigation epoch MUST be dropped (never posted to replacement doc!)
    assert.equal(
      bridge.resolveResourceRead(
        readReqs[0].id,
        { uri: readReqs[0].uri, text: "<div>secret</div>" },
        undefined,
        readReqs[0].navigationEpoch,
      ),
      false,
    );
    assert.equal(
      bridge.resolveToolCallResult(
        toolReqs[0].id,
        { ok: true, content: [{ type: "text", text: "secret-tool-output" }] },
        toolReqs[0].navigationEpoch,
      ),
      false,
    );
    assert.equal(iframe.posted.length, postedBeforeNav);
  });
});
