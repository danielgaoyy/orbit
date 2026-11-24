import assert from "node:assert/strict";
import { it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import type {
  AgentStageView,
  PendingPlanApprovalView,
  TimelineItemView,
} from "../../lib/orbit-ipc.ts";
import { selectPendingDecisions } from "../../state/selectors/session-selectors.ts";
import { createStageSessionSelector } from "./stage-helpers.ts";
import {
  buildPlanApprovalSubmitPayload,
  canSubmitPlanChanges,
  createInitialPlanDraft,
  createPlanReviewDraftCache,
  isPlanDraftModified,
  planDraftScopeKey,
  resetPlanDraftMarkdown,
  resolvePlanReviewHeaderTitle,
  shouldTriggerPlanModEnter,
} from "./plan-review-card-model.ts";

function samplePlan(overrides?: Partial<PendingPlanApprovalView>): PendingPlanApprovalView {
  return {
    seq: 42,
    request_id: "clp_plan_1",
    title: "Review implementation plan",
    plan_markdown: "# Auth Migration\n\n1. Add refresh token rotation\n2. Add regression tests",
    plan_title: "Auth Migration",
    plan_file_path: "/repo/.claude/plans/auth-migration.md",
    allowed_prompts: ["Bash: cargo test"],
    source: "exit_plan_mode",
    blocks: [
      {
        kind: "heading",
        level: 1,
        spans: [
          {
            text: "Auth Migration",
            bold: false,
            italic: false,
            strikethrough: false,
            inline_code: false,
            link_url: null,
          },
        ],
      },
      {
        kind: "paragraph",
        spans: [
          {
            text: "Add refresh token rotation and regression tests.",
            bold: false,
            italic: false,
            strikethrough: false,
            inline_code: false,
            link_url: null,
          },
        ],
      },
    ],
    timestamp_ms: 1_700_000_000_000,
    ...overrides,
  };
}

it("tracks draft modifications with CRLF normalization and resets cleanly", () => {
  const original = "# Plan\n- Step 1\n- Step 2";
  assert.equal(isPlanDraftModified(original, "# Plan\r\n- Step 1\r\n- Step 2\n"), false);
  assert.equal(isPlanDraftModified(original, "# Plan\n- Step 1\n- Step 2 (updated)"), true);

  const initial = createInitialPlanDraft(original);
  const edited = { ...initial, markdown: "# Plan\n- Edited", tabMode: "edit" as const };
  assert.equal(isPlanDraftModified(original, edited.markdown), true);
  const reset = resetPlanDraftMarkdown(edited, original);
  assert.equal(reset.markdown, original);
  assert.equal(reset.tabMode, "edit");
  assert.equal(isPlanDraftModified(original, reset.markdown), false);
});

it("builds accurate submission payloads for Approve & Build, Request Changes, and Dismiss", () => {
  const original = "# Plan\n- Step 1";
  const unmodified = createInitialPlanDraft(original);

  assert.deepEqual(buildPlanApprovalSubmitPayload("approve_and_build", original, unmodified), {
    action: "approve_and_build",
    editedPlanMarkdown: null,
    feedback: null,
    targetMode: "default",
  });

  const edited = {
    ...unmodified,
    markdown: "# Plan\n- Step 1\n- Step 2",
    feedback: "  Prioritize migration safety  ",
  };
  assert.deepEqual(buildPlanApprovalSubmitPayload("approve_and_build", original, edited), {
    action: "approve_and_build",
    editedPlanMarkdown: "# Plan\n- Step 1\n- Step 2",
    feedback: "Prioritize migration safety",
    targetMode: "default",
  });

  // Request Changes keeps targetMode null so session stays in SessionMode::Plan
  assert.equal(canSubmitPlanChanges(original, unmodified), false);
  assert.equal(canSubmitPlanChanges(original, { ...unmodified, feedback: "Split step 1" }), true);
  assert.equal(
    canSubmitPlanChanges(original, { ...unmodified, markdown: "# Plan\n- Revised" }),
    true,
  );
  assert.deepEqual(buildPlanApprovalSubmitPayload("request_changes", original, edited), {
    action: "request_changes",
    editedPlanMarkdown: "# Plan\n- Step 1\n- Step 2",
    feedback: "Prioritize migration safety",
    targetMode: null,
  });

  // Dismiss never sends edits or switches mode
  assert.deepEqual(buildPlanApprovalSubmitPayload("dismiss", original, edited), {
    action: "dismiss",
    editedPlanMarkdown: null,
    feedback: null,
    targetMode: null,
  });
});

it("resolves header titles cleanly, filters generic tool labels, and guards Cmd+Enter shortcuts", () => {
  assert.equal(
    resolvePlanReviewHeaderTitle(
      { plan_title: "Auth Migration", title: "Claude wants to use ExitPlanMode" },
      "Implementation plan",
    ),
    "Auth Migration",
  );
  assert.equal(
    resolvePlanReviewHeaderTitle(
      { plan_title: null, title: "Claude wants to use ExitPlanMode" },
      "Implementation plan",
    ),
    "Implementation plan",
  );
  assert.equal(
    resolvePlanReviewHeaderTitle(
      { plan_title: null, title: "Refactor Session Cache" },
      "Implementation plan",
    ),
    "Refactor Session Cache",
  );

  assert.equal(
    shouldTriggerPlanModEnter({
      key: "Enter",
      metaKey: true,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
    }),
    true,
  );
  assert.equal(
    shouldTriggerPlanModEnter({
      key: "Enter",
      metaKey: true,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
      isComposing: true,
    }),
    false,
  );
  assert.equal(
    shouldTriggerPlanModEnter({
      key: "Enter",
      metaKey: false,
      ctrlKey: false,
      shiftKey: false,
      altKey: false,
    }),
    false,
  );
});

it("preserves drafts across re-renders and evicts LRU entries cleanly", () => {
  const cache = createPlanReviewDraftCache(2);
  const k1 = planDraftScopeKey("h1", "ag1", "req1");
  const k2 = planDraftScopeKey("h1", "ag1", "req2");
  const k3 = planDraftScopeKey("h1", "ag1", "req3");

  const d1 = { ...createInitialPlanDraft("a"), markdown: "edited a" };
  cache.save(k1, d1);
  cache.save(k2, createInitialPlanDraft("b"));
  assert.equal(cache.get(k1)?.markdown, "edited a"); // touches k1 as MRU
  cache.save(k3, createInitialPlanDraft("c")); // evicts k2
  assert.equal(cache.size(), 2);
  assert.equal(cache.get(k1)?.markdown, "edited a");
  assert.equal(cache.get(k2), undefined);
});

it("deduplicates ExitPlanMode from DecisionDock permissions and keeps stage selector stable", () => {
  const plan = samplePlan();
  const exitPlanPerm: TimelineItemView = {
    item_type: "permission_request",
    seq: 42,
    request_id: plan.request_id,
    title: plan.title,
    detail: {
      kind: "generic",
      tool_name: "ExitPlanMode",
      arguments_json: JSON.stringify({ plan: plan.plan_markdown }),
    },
    allowed_decisions: ["allow", "deny"],
    resolved_decision: null,
    plan_approval: {
      plan_markdown: plan.plan_markdown,
      plan_title: plan.plan_title,
      plan_file_path: plan.plan_file_path,
      allowed_prompts: plan.allowed_prompts,
      source: plan.source,
    },
  };
  const regularShellPerm: TimelineItemView = {
    item_type: "permission_request",
    seq: 43,
    request_id: "clp_shell_2",
    title: "Run cargo test",
    detail: {
      kind: "shell",
      command: "cargo test",
      cwd: "/repo",
      exit_code: null,
      output_preview: null,
    },
    allowed_decisions: ["allow", "deny"],
    resolved_decision: null,
  };

  const stage: AgentStageView = {
    revision: 3,
    host_id: "h1",
    agent: null,
    turns: [],
    pending_permissions: [exitPlanPerm, regularShellPerm],
    pending_questions: [],
    pending_plan_approval: plan,
    session_turn_diffs: [],
    insights: {
      touched_files: [],
      background_tasks: [],
      subagents: [],
      detected_local_urls: [],
    },
    latest_seq: 43,
  };

  const state = {
    stages: { "h1::ag1": stage },
    hostDetails: {},
    fleet: { revision: 1, hosts: [], total_pending_approvals: 2 },
    archivedSubagents: {},
  };

  const decisions = selectPendingDecisions(state, "h1", "ag1");
  assert.equal(decisions.pendingPlanApproval?.request_id, "clp_plan_1");
  assert.deepEqual(
    decisions.parentPermissions.map((p) => p.request_id),
    ["clp_shell_2"],
    "ExitPlanMode permission must NOT duplicate inside parentPermissions",
  );
  assert.equal(decisions.totalCount, 2);

  // React 19 useSyncExternalStore stability check
  const selector = createStageSessionSelector("h1", "ag1");
  const snap1 = selector(state);
  const snap2 = selector({ ...state });
  assert.equal(snap1, snap2, "selector must return referentially stable snapshot");

  // Clearing pending_plan_approval invalidates cleanly
  const clearedState = {
    ...state,
    stages: {
      "h1::ag1": {
        ...stage,
        pending_permissions: [regularShellPerm],
        pending_plan_approval: null,
      },
    },
  };
  const snap3 = selector(clearedState);
  assert.notEqual(snap3, snap2);
  assert.equal(snap3.decisions.pendingPlanApproval, null);
  assert.equal(snap3.decisions.totalCount, 1);
});

it("renders PlanReviewCard with Preview/Edit tabs and Approve & Build CTA while DecisionDock skips plan_approval", async () => {
  const server = await createServer({
    server: { middlewareMode: true, hmr: false, ws: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
  try {
    const { PlanReviewCard } = await server.ssrLoadModule(
      "/src/components/stage/plan-review-card.tsx",
    );
    const { DecisionDock } = await server.ssrLoadModule(
      "/src/components/stage/stage-approval-dock.tsx",
    );

    const plan = samplePlan();
    const html = renderToStaticMarkup(
      createElement(PlanReviewCard, {
        hostId: "h1",
        agentId: "ag1",
        cwd: "/repo",
        plan,
      }),
    );

    assert.match(html, /data-slot="plan-review-card"/);
    assert.match(html, /Auth Migration/);
    assert.match(html, /role="tablist"/);
    assert.match(html, /aria-selected="true"[^>]*>Preview</);
    assert.match(html, /aria-selected="false"[^>]*>Edit</);
    assert.match(html, /Approve &amp; Build/);
    assert.match(html, /Request Changes/);
    assert.match(html, /Dismiss/);
    assert.match(html, /\.claude\/plans\/auth-migration\.md/);
    assert.match(html, /Bash: cargo test/);

    // Defense-in-depth: even if passed directly to DecisionDock, a plan_approval permission renders nothing
    const duplicateDockHtml = renderToStaticMarkup(
      createElement(DecisionDock, {
        hostId: "h1",
        agentId: "ag1",
        cwd: "/repo",
        permissions: [
          {
            item_type: "permission_request",
            seq: 42,
            request_id: plan.request_id,
            title: plan.title,
            detail: {
              kind: "generic",
              tool_name: "ExitPlanMode",
              arguments_json: "{}",
            },
            allowed_decisions: ["allow", "deny"],
            resolved_decision: null,
            plan_approval: {
              plan_markdown: plan.plan_markdown,
              plan_title: plan.plan_title,
              plan_file_path: plan.plan_file_path,
              allowed_prompts: plan.allowed_prompts,
              source: plan.source,
            },
          },
        ],
        questions: [],
      }),
    );
    assert.equal(duplicateDockHtml, "");
  } finally {
    await server.close();
  }
});
