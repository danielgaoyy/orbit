import assert from "node:assert/strict";
import { describe, it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import type {
  GuardianAssessment,
  PermissionRuleRecord,
  TimelineItemView,
} from "../../lib/orbit-ipc.ts";
import {
  permitsAllowDecision,
  selectSafeBatchPermissions,
  type SafeBatchDecisionCandidate,
} from "../stage/stage-helpers.ts";
import {
  basenameFromPath,
  countAlwaysAllowRules,
  createHostPermissionRulesSelector,
  defaultPatternKindForTool,
  filterPermissionRules,
} from "./permission-rules-model.ts";

function makeAssessment(
  risk_level: GuardianAssessment["risk_level"],
): GuardianAssessment {
  return {
    risk_level,
    decision: "require_human",
    confidence: 0.95,
    classifier_tier: "deterministic",
    reason: `${risk_level} risk`,
    signals: [],
    latency_ms: 1,
  };
}

function makeRule(overrides: Partial<PermissionRuleRecord>): PermissionRuleRecord {
  return {
    id: overrides.id ?? "pr_1",
    scope_kind: overrides.scope_kind ?? "project",
    project_cwd: overrides.project_cwd ?? "/workspace/orbit",
    provider: overrides.provider ?? null,
    tool_kind: overrides.tool_kind ?? "shell",
    pattern_kind: overrides.pattern_kind ?? "command_prefix",
    pattern: overrides.pattern ?? "cargo test",
    decision: overrides.decision ?? "allow",
    source: overrides.source ?? "manual",
    note: overrides.note ?? null,
    created_at: overrides.created_at ?? "2026-11-24T00:00:00Z",
    last_matched_at: overrides.last_matched_at ?? null,
    match_count: overrides.match_count ?? 0,
  };
}

describe("B3-02: selectSafeBatchPermissions & permitsAllowDecision", () => {
  it("excludes questions, plan approvals, High/Critical risks, and deny-only prompts", () => {
    const queue: SafeBatchDecisionCandidate[] = [
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_low",
        allowedDecisions: ["allow", "allow_always", "deny"],
        guardianAssessment: makeAssessment("low"),
      },
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_med",
        allowedDecisions: ["allow_once", "deny"],
        guardianAssessment: makeAssessment("medium"),
      },
      {
        kind: "permission",
        targetHostId: "h2",
        targetAgentId: "a2",
        requestId: "p_unassessed",
        allowedDecisions: ["accept", "decline"],
        guardianAssessment: null,
      },
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_high",
        allowedDecisions: ["allow", "deny"],
        guardianAssessment: makeAssessment("high"),
      },
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_critical",
        allowedDecisions: ["allow", "deny"],
        guardianAssessment: makeAssessment("critical"),
      },
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_plan_flag",
        allowedDecisions: ["allow", "deny"],
        guardianAssessment: makeAssessment("low"),
        isPlanApproval: true,
      },
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_plan_payload",
        allowedDecisions: ["allow", "deny"],
        guardianAssessment: makeAssessment("low"),
        planApproval: { plan_markdown: "# Plan" },
      },
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_deny_only",
        allowedDecisions: ["deny"],
        guardianAssessment: makeAssessment("low"),
      },
      {
        kind: "permission",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "p_empty_decisions",
        allowedDecisions: [],
        guardianAssessment: makeAssessment("low"),
      },
      {
        kind: "question",
        targetHostId: "h1",
        targetAgentId: "a1",
        requestId: "q_1",
      },
    ];

    const eligible = selectSafeBatchPermissions(queue);
    assert.deepEqual(
      eligible.map((item) => item.requestId),
      ["p_low", "p_med", "p_unassessed"],
    );
    assert.equal(permitsAllowDecision(["allow", "deny"]), true);
    assert.equal(permitsAllowDecision(["allow_once"]), true);
    assert.equal(permitsAllowDecision(["ACCEPT"]), true);
    assert.equal(permitsAllowDecision(["allow_always", "deny"]), false);
    assert.equal(permitsAllowDecision(["deny"]), false);
    assert.equal(permitsAllowDecision([]), false);
  });
});

describe("B3-02: Permission rules selector stability & filtering", () => {
  it("keeps React 19 useOrbitSelector snapshot referentially stable across unrelated updates", () => {
    const selector = createHostPermissionRulesSelector("host_1");
    const host1Entry = {
      status: "ready" as const,
      rules: [makeRule({ id: "pr_1" })],
      saving: false,
      error: null,
    };
    const stateA = {
      permissionRulesByHost: {
        host_1: host1Entry,
        host_2: { status: "idle" as const, rules: [], saving: false, error: null },
      },
    };
    const stateB = {
      permissionRulesByHost: {
        host_1: host1Entry,
        host_2: {
          status: "ready" as const,
          rules: [makeRule({ id: "pr_other" })],
          saving: false,
          error: null,
        },
      },
    };

    const snapA1 = selector(stateA);
    const snapA2 = selector(stateA);
    const snapB = selector(stateB);
    assert.strictEqual(snapA1, snapA2, "identical state must return identical snapshot reference");
    assert.strictEqual(
      snapA1,
      snapB,
      "unrelated host update must NOT allocate a new snapshot reference",
    );

    const stateC = {
      permissionRulesByHost: {
        host_1: {
          ...host1Entry,
          rules: [makeRule({ id: "pr_1" }), makeRule({ id: "pr_2" })],
        },
      },
    };
    const snapC = selector(stateC);
    assert.notStrictEqual(snapC, snapA1);
    assert.equal(snapC.rules.length, 2);

    const missingSelector = createHostPermissionRulesSelector("host_missing");
    const empty1 = missingSelector(stateA);
    const empty2 = missingSelector(stateB);
    assert.strictEqual(empty1, empty2, "missing host must return cached idle snapshot");
  });

  it("filters rules cleanly by tab and search query", () => {
    const rules: PermissionRuleRecord[] = [
      makeRule({
        id: "r1",
        pattern: "cargo test",
        source: "always_allow_grant",
        decision: "allow",
        project_cwd: "/workspace/orbit",
        provider: null,
      }),
      makeRule({
        id: "r2",
        pattern: "rm -rf",
        source: "manual",
        decision: "deny",
        scope_kind: "host",
        project_cwd: null,
        note: "Block destructive delete",
      }),
      makeRule({
        id: "r3",
        pattern: "mcp__github__*",
        tool_kind: "mcp_tool",
        pattern_kind: "tool_name",
        source: "manual",
        decision: "allow",
        provider: "claude_code",
      }),
    ];

    assert.equal(countAlwaysAllowRules(rules), 1);
    assert.deepEqual(
      filterPermissionRules(rules, "", "always_allow").map((r) => r.id),
      ["r1"],
    );
    assert.deepEqual(
      filterPermissionRules(rules, "", "manual").map((r) => r.id),
      ["r2", "r3"],
    );
    assert.deepEqual(
      filterPermissionRules(rules, "", "deny").map((r) => r.id),
      ["r2"],
    );
    assert.deepEqual(
      filterPermissionRules(rules, "destructive", "all").map((r) => r.id),
      ["r2"],
    );
    assert.deepEqual(
      filterPermissionRules(rules, "claude", "all").map((r) => r.id),
      ["r3"],
    );
    assert.equal(basenameFromPath("/Users/dev/code/orbit/"), "orbit");
    assert.equal(defaultPatternKindForTool("shell"), "command_prefix");
    assert.equal(defaultPatternKindForTool("file_write"), "path_glob");
    assert.equal(defaultPatternKindForTool("mcp_tool"), "tool_name");
  });
});

describe("B3-02: DecisionDock safe batch CTA rendering", () => {
  it("renders 'Approve safe (N)' strictly when >= 2 safe permissions exist and hides when < 2", async () => {
    const server = await createServer({
      root: process.cwd(),
      appType: "custom",
      logLevel: "silent",
      server: { middlewareMode: true, hmr: false },
    });
    try {
      const { DecisionDock } = await server.ssrLoadModule(
        "/src/components/stage/stage-approval-dock.tsx",
      );

      const safePerm1: Extract<TimelineItemView, { item_type: "permission_request" }> = {
        item_type: "permission_request",
        seq: 10,
        request_id: "perm_safe_1",
        title: "Run cargo test",
        detail: {
          kind: "shell",
          command: "cargo test",
          cwd: null,
          exit_code: null,
          output_preview: null,
        },
        allowed_decisions: ["allow", "allow_always", "deny"],
        resolved_decision: null,
        guardian_assessment: makeAssessment("low"),
        plan_approval: null,
      };

      const safePerm2: Extract<TimelineItemView, { item_type: "permission_request" }> = {
        item_type: "permission_request",
        seq: 11,
        request_id: "perm_safe_2",
        title: "Edit src/lib.rs",
        detail: {
          kind: "file_edit",
          path: "src/lib.rs",
          unified_diff: null,
        },
        allowed_decisions: ["allow", "deny"],
        resolved_decision: null,
        guardian_assessment: makeAssessment("medium"),
        plan_approval: null,
      };

      const highRiskPerm: Extract<TimelineItemView, { item_type: "permission_request" }> = {
        item_type: "permission_request",
        seq: 12,
        request_id: "perm_high_1",
        title: "Delete /etc",
        detail: {
          kind: "shell",
          command: "rm -rf /",
          cwd: null,
          exit_code: null,
          output_preview: null,
        },
        allowed_decisions: ["allow", "deny"],
        resolved_decision: null,
        guardian_assessment: makeAssessment("high"),
        plan_approval: null,
      };

      // Case 1: 1 safe + 1 high risk => safeBatch.length === 1 => NO batch button
      const htmlOneSafe = renderToStaticMarkup(
        createElement(DecisionDock, {
          hostId: "host_1",
          agentId: "ag_1",
          cwd: "/workspace/orbit",
          permissions: [safePerm1, highRiskPerm],
          questions: [],
        }),
      );
      assert.equal(
        htmlOneSafe.includes("Approve safe"),
        false,
        "must NOT render batch button when fewer than 2 safe permissions exist",
      );

      // Case 2: 2 safe + 1 high risk => safeBatch.length === 2 => renders 'Approve safe (2)'
      const htmlTwoSafe = renderToStaticMarkup(
        createElement(DecisionDock, {
          hostId: "host_1",
          agentId: "ag_1",
          cwd: "/workspace/orbit",
          permissions: [safePerm1, safePerm2, highRiskPerm],
          questions: [],
        }),
      );
      assert.equal(
        htmlTwoSafe.includes("Approve safe (2)"),
        true,
        "must render 'Approve safe (2)' excluding the High-risk item",
      );
    } finally {
      await server.close();
    }
  });
});
