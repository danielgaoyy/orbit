import assert from "node:assert/strict";
import { it } from "node:test";
import { createElement } from "react";
import { renderToStaticMarkup } from "react-dom/server";
import { createServer } from "vite";
import type { SessionPlanStepView, TimelineItemView } from "../../lib/orbit-ipc.ts";
import { summarizeWorkChunk } from "../../lib/turn-delegation-split.ts";
import {
  createPlanShelfCache,
  describePlan,
  planSourceTurn,
  reconcilePlanShelf,
  traceStatus,
} from "./composer-plan-shelf-model.ts";

function step(id: string, status: SessionPlanStepView["status"], seq = 2): SessionPlanStepView {
  return { id, title: `Step ${id}`, active_form: `Working on ${id}`, status, updated_seq: seq };
}

function update(
  steps: SessionPlanStepView[],
  running = false,
  previous: ReturnType<typeof reconcilePlanShelf> = null,
  turn: number | null = 1,
  latestTurnAnchorSeq: number | null = null,
) {
  const next = reconcilePlanShelf(
    previous,
    describePlan(steps, running, latestTurnAnchorSeq),
    turn,
  );
  assert.ok(next);
  return next;
}

it("removes the shelf and its preference on an empty projection", () => {
  const previous = update([step("a", "pending")]);
  assert.equal(describePlan([], true), null);
  assert.equal(reconcilePlanShelf(previous, describePlan([], false), 1), null);
});

it("renders no empty wrapper, hides stale settled plans on fresh mount, and renders refined ring header", async () => {
  const server = await createServer({
    server: { middlewareMode: true, hmr: false, watch: null },
    optimizeDeps: { noDiscovery: true, include: [] },
  });
  try {
    const { ComposerPlanShelf } = await server.ssrLoadModule(
      "/src/components/composer/composer-plan-shelf.tsx",
    );
    const render = (
      planSteps: SessionPlanStepView[],
      hasPendingDecisions: boolean,
      isForegroundRunning: boolean,
      agentId = "session",
      latestTurnAnchorSeq: number | null = 1,
    ) =>
      renderToStaticMarkup(
        createElement(ComposerPlanShelf, {
          hostId: "host",
          agentId,
          sourceTurn: 1,
          latestTurnAnchorSeq,
          planSteps,
          hasPendingDecisions,
          isForegroundRunning,
        }),
      );
    assert.equal(render([], true, false), "");
    const active = [step("a", "in_progress"), step("b", "completed")];
    const openHtml = render(active, false, true, "active-open");
    assert.match(openHtml, /<button\b[^>]*aria-expanded="true"/);
    assert.match(openHtml, /aria-valuenow="1"/);
    // Expanded header stays clean and does not duplicate the active step title above the list.
    assert.equal((openHtml.match(/Working on a/g) ?? []).length, 1);

    const yielding = render(active, true, true, "active-yielding");
    assert.match(yielding, /<button\b[^>]*aria-expanded="false"/);
    assert.match(yielding, /aria-disabled="true"/);
    assert.equal(yielding.match(/<button\b/g)?.length, 1);

    // Stopped or failed plan on the current turn can be dismissed cleanly instead of staying pinned.
    const stoppedCurrentTurn = render(active, false, false, "stopped-current", 1);
    assert.match(stoppedCurrentTurn, /aria-label="Dismiss finished plan"/);
    assert.match(stoppedCurrentTurn, /paused/i);
    const failedCurrentTurn = render([step("a", "failed", 2)], false, false, "failed-current", 1);
    assert.match(failedCurrentTurn, /aria-label="Dismiss finished plan"/);
    assert.match(failedCurrentTurn, /failed/i);

    // Already-settled historical plan on fresh mount stays hidden so old tasks never haunt the composer.
    const settled = [step("a", "skipped")];
    assert.equal(render(settled, false, false, "historical-settled"), "");
    // While still running in the foreground, a completed plan remains visible until the turn ends.
    const liveSettled = render(settled, false, true, "live-settled");
    assert.match(liveSettled, /aria-valuenow="1"/);
    assert.doesNotMatch(liveSettled, /aria-label="Dismiss finished plan"/);

    // Incomplete plan from an earlier turn stays visible without a false "(paused)" tag
    // while a continuation turn is actively running, and shows "(paused)" when stopped.
    const staleRunningContinuation = render(
      [step("a", "in_progress", 2)],
      false,
      true,
      "stale-prev-run",
      10,
    );
    assert.match(staleRunningContinuation, /aria-label="Dismiss finished plan"/);
    assert.doesNotMatch(staleRunningContinuation, /paused/i);

    const staleStopped = render([step("a", "in_progress", 2)], false, false, "stale-prev-stop", 10);
    assert.match(staleStopped, /aria-label="Dismiss finished plan"/);
    assert.match(staleStopped, /paused/i);
  } finally {
    await server.close();
  }
});

it("reconciles all five wire statuses and counts completed plus skipped", () => {
  const steps = [
    step("a", "pending"),
    step("b", "in_progress"),
    step("c", "completed"),
    step("d", "skipped"),
    step("e", "failed"),
  ];
  const plan = describePlan(steps, true);
  assert.ok(plan);
  assert.deepEqual(
    plan.rows.map((row) => [row.title, traceStatus[row.state]]),
    [
      ["Step a", "pending"],
      ["Working on b", "running"],
      ["Step c", undefined],
      ["Step d", undefined],
      ["Step e", "error"],
    ],
  );
  assert.equal(plan.headline?.id, "b");
  assert.equal(plan.done, 2);
  assert.equal(plan.skipped, 1);
  assert.equal(plan.hasFailure, true);
  assert.equal(plan.canDismiss, false);
  assert.equal(describePlan([steps[4]], false)?.allSettled, false);
});

it("keeps stale progress static when idle or when belonging to a previous turn", () => {
  const rows = [step("next", "pending", 4), step("current", "in_progress", 4)];
  const paused = describePlan(rows, false)?.headline;
  assert.equal(paused?.state, "paused");
  assert.equal(paused.title, "Step current");
  assert.equal(traceStatus[paused.state], "pending");
  // When a newer turn (anchor_seq 10) is actively running, turn 1's carried-over in_progress
  // steps neither spin as live `running` nor display misleading `paused`; they present as
  // neutral `pending` while remaining the headline and dismissible until the new turn updates the plan.
  const prevTurnRunning = describePlan(rows, true, 10);
  assert.equal(prevTurnRunning?.isFromPreviousTurn, true);
  assert.equal(prevTurnRunning?.headline?.id, "current");
  assert.equal(prevTurnRunning?.headline?.state, "pending");
  assert.equal(prevTurnRunning?.canDismiss, true);
  assert.equal(describePlan([...rows, step("error", "failed")], false)?.headline?.state, "failed");
  assert.equal(
    describePlan([{ ...rows[1], active_form: "" }], true)?.headline?.title,
    "Step current",
  );
  assert.equal(rows[1].status, "in_progress");
});

it("preserves manual collapse across same-turn progress, idle transitions and snapshot replays", () => {
  const rows = [step("a", "pending"), step("b", "pending")];
  const initial = update(rows, true);
  assert.equal(initial.open, true);
  const collapsed = { ...initial, open: false };
  const progress = [step("a", "completed", 4), step("b", "in_progress", 4)];
  const active = update(progress, true, collapsed);
  assert.equal(active.open, false);
  assert.strictEqual(
    update(
      progress.map((row) => ({ ...row })),
      false,
      active,
    ),
    active,
  );
  assert.equal(update([step("x", "pending", 7)], false, active).open, true);
});

it("resets disclosure for a new turn's snapshot even if explicit IDs and titles repeat", () => {
  const previous = { ...update([step("a", "in_progress")]), open: false };
  assert.equal(update([step("a", "pending", 11)], true, previous, 10).open, true);
});

it("finds the plan's source turn across new prompts, history gaps and incremental TaskUpdate", () => {
  const updateItem: TimelineItemView = {
    item_type: "tool_call",
    seq: 12,
    call_id: "u",
    status: "completed",
    summary_title: "Updated plan",
    detail: { kind: "generic", tool_name: "mcp__tasks__TaskUpdate", arguments_json: "{}" },
    diff_files: [],
  };
  const turns = [
    { anchor_seq: 3, live_segments: [] },
    {
      anchor_seq: 10,
      live_segments: [
        {
          kind: "step_group" as const,
          key: "u",
          first_seq: 12,
          items: [updateItem],
          summary: summarizeWorkChunk([updateItem]),
          has_following_content: false,
          has_running_step: false,
        },
      ],
    },
  ];
  assert.equal(planSourceTurn(turns, 2), null);
  assert.equal(planSourceTurn(turns, 7), 3);
  assert.equal(planSourceTurn(turns, 10), 10);
  const collapsed = { ...update([step("a", "pending", 7)], true, null, 3), open: false };
  const source = planSourceTurn(turns, 12);
  assert.equal(source, null);
  assert.equal(update([step("a", "in_progress", 12)], true, collapsed, source).open, false);
});

it("marks live completion for auto-dismiss once foreground stops, hides immediately on next turn, and hides stale settled plans on fresh mount", () => {
  // Fresh mount on an already-completed historical plan starts hidden immediately.
  const staleMount = update([step("a", "completed", 5)], false, null, 1, 1);
  assert.equal(staleMount.dismissed, true);

  // Live transition: active -> completed while running -> idle marks settledLive for timed auto-dismiss.
  const active = update([step("a", "in_progress"), step("b", "pending")], true, null, 1, 1);
  const done = [step("a", "completed", 5), step("b", "skipped", 5)];
  assert.equal(describePlan(done, true, 1)?.canDismiss, false);
  const stillRunning = update(done, true, active, 1, 1);
  assert.equal(stillRunning.open, true);
  assert.equal(stillRunning.settledLive, false);
  const idle = update(done, false, stillRunning, 1, 1);
  assert.equal(idle.open, false);
  assert.equal(idle.dismissed, false);
  assert.equal(idle.settledLive, true);
  assert.equal(describePlan(done, false, 1)?.canDismiss, true);

  // Manual inspect keeps it open and stops live auto-dismiss.
  const reopened = { ...idle, open: true, settledLive: false };
  assert.strictEqual(update(done, false, reopened, 1, 1), reopened);

  // Starting a brand-new turn (anchor_seq 10) retires the completed plan immediately.
  const nextTurn = update(done, true, idle, 1, 10);
  assert.equal(nextTurn.dismissed, true);
});

it("reappears cleanly when a new todo list arrives after dismissal or completion", () => {
  const active = update([step("a", "in_progress", 2)], true, null, 1, 1);
  const done = [step("a", "completed", 5)];
  const settled = update(done, false, active, 1, 1);
  const dismissed = { ...settled, dismissed: true, settledLive: false };

  // Replaying identical settled plan stays hidden.
  assert.equal(update(done, false, dismissed, 1, 1).dismissed, true);

  // New plan in a later turn reappears even if step IDs/titles repeat.
  const nextTurnSameTitles = update([step("a", "in_progress", 12)], true, dismissed, 10, 10);
  assert.equal(nextTurnSameTitles.dismissed, false);
  assert.equal(nextTurnSameTitles.open, true);

  // New plan with new steps reappears and expands immediately.
  const brandNew = update([step("x", "pending", 8), step("y", "in_progress", 8)], true, dismissed, 1, 1);
  assert.equal(brandNew.dismissed, false);
  assert.equal(brandNew.open, true);

  // Dismissing a stopped or failed plan on the current turn persists until resumed or updated.
  const stoppedCurrent = update([step("a", "in_progress", 2), step("b", "failed", 2)], false, null, 1, 1);
  const dismissedStopped = { ...stoppedCurrent, dismissed: true };
  assert.equal(
    update([step("a", "in_progress", 2), step("b", "failed", 2)], false, dismissedStopped, 1, 1).dismissed,
    true,
  );
  // Resuming foreground execution on the same turn makes the active plan reappear.
  assert.equal(
    update([step("a", "in_progress", 2), step("b", "failed", 2)], true, dismissedStopped, 1, 1).dismissed,
    false,
  );

  // Dismissing an abandoned previous-turn incomplete plan stays dismissed until updated.
  const abandoned = update([step("a", "in_progress", 2)], false, null, 1, 10);
  const dismissedAbandoned = { ...abandoned, dismissed: true };
  assert.equal(update([step("a", "in_progress", 2)], false, dismissedAbandoned, 1, 10).dismissed, true);
  assert.equal(update([step("a", "in_progress", 14)], true, dismissedAbandoned, 10, 10).dismissed, false);
});

it("keeps host and thread preferences separate when revisiting and bounds the session cache", () => {
  const cache = createPlanShelfCache(2);
  const a = JSON.stringify(["host1", "agent"]);
  const b = JSON.stringify(["host2", "agent"]);
  const c = JSON.stringify(["host1", "other"]);
  const dismissed = { ...update([step("a", "skipped")]), dismissed: true };
  const visible = update([step("a", "pending")]);
  cache.save(a, dismissed);
  cache.save(b, visible);
  assert.strictEqual(cache.get(a), dismissed);
  assert.strictEqual(cache.get(b), visible);
  cache.save(a, dismissed);
  cache.save(c, visible);
  assert.equal(cache.get(b), null);
  assert.strictEqual(cache.get(a), dismissed);
  cache.save(a, null);
  assert.equal(cache.get(a), null);
});
