import assert from "node:assert/strict";
import { after, before, describe, it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer, type ViteDevServer } from "vite";
import {
  buildAgentListItemTooltip,
  filterDistinctWorkerStepPreview,
  formatAgentListItemStatusLabel,
  isAgentListItemActiveStatus,
  mapAgentCardToItemStatus,
  mapBriefingWorkerStateToStatus,
  resolveAgentItemRuntime,
} from "./agent-list-item.ts";
import { en } from "./i18n/locales/en.ts";
import { zhCN } from "./i18n/locales/zh-CN.ts";
import type { AgentCardView, HostDetailView } from "./orbit-ipc.ts";

function makeAgent(overrides: Partial<AgentCardView> = {}): AgentCardView {
  return {
    agent_id: "ag_worker_1",
    workspace_id: "ws_1",
    title: "Worker 1",
    provider: "antigravity",
    provider_label: "Antigravity",
    model: null,
    effort: null,
    mode: "default",
    parent_agent_id: null,
    cwd: "/repo",
    status_label: "Idle",
    is_running: false,
    is_awaiting_permission: false,
    is_pinned: false,
    seen_seq: 10,
    is_unread: false,
    latest_seq: 10,
    updated_at_rfc3339: "2026-11-26T00:00:00Z",
    ...overrides,
  };
}

function assertIndependentControls(html: string) {
  for (const [, , content] of html.matchAll(/<(button|a)\b[^>]*>([\s\S]*?)<\/\1>/g)) {
    assert.doesNotMatch(content, /<(?:button|a)\b/);
  }
}

describe("agent-list-item pure helpers", () => {
  it("maps briefing worker states and preserves canceled state when live card is closed", () => {
    assert.equal(mapBriefingWorkerStateToStatus("running"), "running");
    assert.equal(mapBriefingWorkerStateToStatus("awaiting"), "awaiting_permission");
    assert.equal(mapBriefingWorkerStateToStatus("error"), "failed");
    assert.equal(mapBriefingWorkerStateToStatus("done"), "completed");
    assert.equal(
      mapBriefingWorkerStateToStatus("done", makeAgent({ status_label: "Stopped" })),
      "canceled",
    );
    assert.equal(mapBriefingWorkerStateToStatus("idle"), "idle");
    assert.equal(isAgentListItemActiveStatus("running"), true);
    assert.equal(isAgentListItemActiveStatus("awaiting_permission"), true);
    assert.equal(isAgentListItemActiveStatus("idle"), false);
    assert.equal(isAgentListItemActiveStatus("completed"), false);
  });

  it("maps AgentCardView lifecycle states cleanly for group peers", () => {
    assert.equal(
      mapAgentCardToItemStatus(makeAgent({ is_awaiting_permission: true, is_running: true })),
      "awaiting_permission",
    );
    assert.equal(
      mapAgentCardToItemStatus(makeAgent({ is_running: true, has_background_tasks: false })),
      "running",
    );
    assert.equal(
      mapAgentCardToItemStatus(makeAgent({ is_running: true, has_background_tasks: true })),
      "idle",
    );
    assert.equal(
      mapAgentCardToItemStatus(makeAgent({ status_label: "Failed: timeout" })),
      "failed",
    );
    assert.equal(
      mapAgentCardToItemStatus(makeAgent({ status_label: "Stopped" })),
      "canceled",
    );
    assert.equal(mapAgentCardToItemStatus(makeAgent()), "idle");
  });

  it("formats localized status labels in English and Chinese", () => {
    const tEn = (key: keyof typeof en) => en[key];
    const tZh = (key: keyof typeof zhCN) => zhCN[key];
    assert.equal(formatAgentListItemStatusLabel("running", tEn), "Working…");
    assert.equal(formatAgentListItemStatusLabel("awaiting_permission", tEn), "Approval required");
    assert.equal(formatAgentListItemStatusLabel("idle", tEn), "Idle");
    assert.equal(formatAgentListItemStatusLabel("canceled", tZh), "已停止");
    assert.equal(formatAgentListItemStatusLabel("idle", tZh), "空闲");
    assert.equal(formatAgentListItemStatusLabel("completed", tZh), "已完成");
  });

  it("resolves provider, catalog model label, and Antigravity effort suffix", () => {
    const tEn = (key: keyof typeof en) => en[key];
    const hostDetail: Pick<HostDetailView, "provider_catalogs" | "acp_agents"> = {
      provider_catalogs: [
        {
          provider: "antigravity",
          models: [
            {
              id: "gemini-4-neon",
              label: "Gemini 4 Neon",
              description: "",
              is_default: true,
              default_effort: null,
              thinking_options: [],
              supports_fast_mode: false,
            },
          ],
          configured_default_model: "gemini-4-neon",
          configured_default_effort: null,
        },
      ],
      acp_agents: [],
    };
    const runtime = resolveAgentItemRuntime({
      agent: makeAgent({ provider: "antigravity", model: "gemini-4-neon-xhigh" }),
      hostDetail,
      t: tEn,
    });
    assert.equal(runtime.provider, "antigravity");
    assert.equal(runtime.agentLabel, "Antigravity");
    assert.equal(runtime.modelLabel, "Gemini 4 Neon");
    assert.equal(runtime.effortLabel, "Extra High");
  });

  it("filters redundant status echoes and duplicate titles from worker step previews", () => {
    assert.equal(filterDistinctWorkerStepPreview("Running", "Worker"), null);
    assert.equal(filterDistinctWorkerStepPreview("Idle", "Worker"), null);
    assert.equal(filterDistinctWorkerStepPreview("运行中…", "Worker"), null);
    assert.equal(filterDistinctWorkerStepPreview("PA 重设计", "PA 重设计"), null);
    assert.equal(
      filterDistinctWorkerStepPreview("Editing agent-list-item.tsx", "PA 重设计"),
      "Editing agent-list-item.tsx",
    );
  });

  it("builds consistent multi-line hover tooltips", () => {
    const tip = buildAgentListItemTooltip({
      title: "PA 重设计 · Desktop UI",
      agentLabel: "Antigravity",
      rawModel: "gemini-4-neon",
      modelLabel: "Gemini 4 Neon",
      effortLabel: "Extra High",
      statusLabel: "Stopped",
      hostName: "mac-studio",
      worktreeBranch: "feat/agent-row",
      detail: "Inspect shared agent row",
    });
    assert.equal(
      tip,
      "PA 重设计 · Desktop UI\nAntigravity · gemini-4-neon Extra High · Stopped · mac-studio · feat/agent-row\nInspect shared agent row",
    );
  });
});

describe("AgentListItem SSR rendering", () => {
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

  it("renders unified portrait, badge rules, metadata line, and independent controls across all states", async () => {
    const { AgentListItem } = await server.ssrLoadModule("/src/components/ui/agent-list-item.tsx");

    // 1. Canceled row matching the reference screenshot: avatar + muted X badge + truncated title + Provider + Model Effort · Stopped
    const canceledHtml = renderToStaticMarkup(
      createElement(AgentListItem, {
        title: "PA 重设计 · Desktop UI",
        status: "canceled",
        avatarSeedKey: "ag_pa_ui",
        provider: "antigravity",
        modelLabel: "Gemini 4 Neon",
        effortLabel: "Extra High",
        agentLabel: "Antigravity",
        onClick: () => {},
      }),
    );
    assert.match(canceledHtml, /data-slot="agent-list-item"/);
    assert.match(canceledHtml, /data-slot="agent-list-item-lines"/);
    assert.match(canceledHtml, /data-slot="subagent-avatar"/);
    assert.match(canceledHtml, /data-status="canceled"/);
    assert.match(canceledHtml, /bg-ink-muted/); // gray X corner badge
    assert.match(canceledHtml, /PA 重设计 · Desktop UI/);
    assert.match(canceledHtml, /Gemini 4 Neon<\/span><span class="text-ink-muted shrink-0">Extra High<\/span><span[^>]*>·<\/span><span[^>]*>Stopped<\/span>/);
    // Strict 2-line structure: exactly two child spans inside agent-list-item-lines (title + meta)
    const linesMatch = canceledHtml.match(/data-slot="agent-list-item-lines"[^>]*>([\s\S]*?)<\/span><\/button>/);
    assert.ok(linesMatch, "expected agent-list-item-lines container");
    assert.doesNotMatch(canceledHtml, /Inspect shared agent row/);
    assertIndependentControls(canceledHtml);

    // 2. Idle row renders calm upright portrait with NO corner badge
    const idleHtml = renderToStaticMarkup(
      createElement(AgentListItem, {
        title: "Reviewer",
        status: "idle",
        avatarSeedKey: "ag_idle",
        provider: "claude_code",
        agentLabel: "Claude Code",
        onClick: () => {},
      }),
    );
    assert.match(idleHtml, /data-status="idle"/);
    assert.match(idleHtml, />Idle</);
    assert.doesNotMatch(idleHtml, /orbit-pill-enter/);

    // 3. Awaiting permission row renders warning tone & attention ring without terminal badge
    const awaitingHtml = renderToStaticMarkup(
      createElement(AgentListItem, {
        title: "Deploy worker",
        status: "awaiting_permission",
        avatarSeedKey: "ag_wait",
        provider: "codex",
        modelLabel: "GPT 5.4",
        onClick: () => {},
      }),
    );
    assert.match(awaitingHtml, /data-attention="true"/);
    assert.match(awaitingHtml, /bg-signal-warning-bg\/75/);
    assert.match(awaitingHtml, /text-signal-warning/);
    assert.doesNotMatch(awaitingHtml, /orbit-pill-enter/);

    // 4. Row with trailing control keeps buttons independent (no nested <button>)
    const trailingHtml = renderToStaticMarkup(
      createElement(AgentListItem, {
        title: "Peer worker",
        status: "running",
        avatarSeedKey: "ag_peer",
        provider: "claude_code",
        modelLabel: "Sonnet 4.6",
        selected: true,
        onClick: () => {},
        trailing: createElement("button", { type: "button" }, "Split"),
      }),
    );
    assert.match(trailingHtml, /data-selected="true"/);
    assert.equal(trailingHtml.match(/<button\b/g)?.length, 2);
    assertIndependentControls(trailingHtml);

    // 5. Card variant with footer keeps header click and footer buttons independent
    const cardHtml = renderToStaticMarkup(
      createElement(AgentListItem, {
        variant: "card",
        title: "DB migration worker",
        status: "awaiting_permission",
        avatarSeedKey: "ag_db",
        provider: "claude_code",
        modelLabel: "Opus 4.6",
        statusLabel: "Plan review",
        secondaryMeta: "prod-box",
        onClick: () => {},
        footer: createElement(
          "div",
          null,
          createElement("p", null, "Apply schema change"),
          createElement("button", { type: "button" }, "Review"),
        ),
      }),
    );
    assert.match(cardHtml, /data-variant="card"/);
    assert.match(cardHtml, /Plan review/);
    assert.match(cardHtml, /prod-box/);
    assert.match(cardHtml, /Apply schema change/);
    assertIndependentControls(cardHtml);

    // 6. Localized Chinese status labels resolve via i18n handle and render in AgentListItem
    // (Note: renderToStaticMarkup invokes React's server snapshot, so we verify the zh-CN
    // i18n handle directly and pass its formatted label to AgentListItem.)
    const { getI18nHandle } = await server.ssrLoadModule("/src/lib/i18n/index.ts");
    const zhT = getI18nHandle("zh-CN").t;
    for (const [status, expectedLabel] of [
      ["running", "运行中…"],
      ["idle", "空闲"],
      ["canceled", "已停止"],
      ["completed", "已完成"],
      ["failed", "已失败"],
      ["awaiting_permission", "等待审批"],
    ] as const) {
      const resolvedLabel = formatAgentListItemStatusLabel(status, zhT);
      assert.equal(resolvedLabel, expectedLabel);
      const zhHtml = renderToStaticMarkup(
        createElement(AgentListItem, {
          title: "统一全局代理卡片组件",
          status,
          statusLabel: resolvedLabel,
          avatarSeedKey: `ag_zh_${status}`,
          provider: "antigravity",
          modelLabel: "Gemini 4 Neon",
          effortLabel: "High",
          onClick: () => {},
        }),
      );
      assert.ok(
        zhHtml.includes(`>${expectedLabel}</span>`),
        `expected zh-CN status "${expectedLabel}" for status=${status}`,
      );
    }
  });
});
