import assert from "node:assert/strict";
import { describe, it } from "node:test";
import {
  extractCleanAgentMessageBody,
  extractDirectAgentMessagePresentation,
  formatCleanAgentMessagePreview,
  parseOrbitSystemEnvelope,
  stripLeadingOrbitProtocolBlocks,
} from "./agent-message-clean.ts";

describe("extractCleanAgentMessageBody", () => {
  it("strips Agent finished boilerplate and <agent-response> wrapper tags", () => {
    const raw = [
      "Agent ag_5e9187b1234 (Codebase Researcher) finished.",
      "",
      "Host: local",
      "",
      "<agent-response>",
      "Found 3 alignment issues in omni-composer.tsx.",
      "</agent-response>",
    ].join("\n");

    assert.equal(
      extractCleanAgentMessageBody(raw),
      "Found 3 alignment issues in omni-composer.tsx.",
    );
  });

  it("unwraps <orbit-system> envelopes and unescapes XML entities", () => {
    const raw = [
      '<orbit-system version="1" kind="agent-notification" source-agent-id="ag_5e9187b" event="finished">',
      "Agent ag_5e9187b (Reviewer) finished.",
      "",
      "&lt;agent-response&gt;",
      "Checked `a &lt; b` &amp; verified tests.",
      "&lt;/agent-response&gt;",
      "</orbit-system>",
    ].join("\n");

    assert.equal(
      extractCleanAgentMessageBody(raw),
      "Checked `a < b` & verified tests.",
    );
  });

  it("handles inline Agent finished prefix with <agent-response> on one line", () => {
    const raw =
      "Agent ag_5e9187b finished. <agent-response>All checks passed.</agent-response>";
    assert.equal(extractCleanAgentMessageBody(raw), "All checks passed.");
  });

  it("handles unclosed <agent-response> and strips internal truncation tool hints", () => {
    const raw = [
      "Agent ag_1234 (Worker) finished.",
      "",
      "<agent-response>",
      "Completed refactoring the stage pipeline",
      "[truncated 640 chars; use get_agent_activity for the full response]",
      "</agent-response>",
    ].join("\n");

    assert.equal(
      extractCleanAgentMessageBody(raw),
      "Completed refactoring the stage pipeline…",
    );

    const unclosed =
      "Agent ag_1234 (Worker) finished.\n\n<agent-response>\nPartial CJK 输出内容";
    assert.equal(extractCleanAgentMessageBody(unclosed), "Partial CJK 输出内容");
  });

  it("preserves Error lines alongside or without <agent-response>", () => {
    const errorOnly = [
      "Agent ag_err (Tester) failed.",
      "",
      "Error: exit status 1",
    ].join("\n");
    assert.equal(extractCleanAgentMessageBody(errorOnly), "Error: exit status 1");

    const errorWithBody = [
      "Agent ag_err (Tester) failed.",
      "",
      "Error: timeout after 60s",
      "",
      "<agent-response>",
      "Last test running: stage_cache",
      "</agent-response>",
    ].join("\n");
    assert.equal(
      extractCleanAgentMessageBody(errorWithBody),
      "Error: timeout after 60s\n\nLast test running: stage_cache",
    );
  });

  it("returns empty string when only finish or permission boilerplate is present", () => {
    assert.equal(
      extractCleanAgentMessageBody("Agent ag_5e9187b (Codebase Researcher) finished."),
      "",
    );

    const permRaw = [
      "Agent ag_5e9187b (Builder) needs permission.",
      "",
      "Host: devbox",
      "",
      "Respond with `respond_to_permission` using the `agent_id` and `request_id` below.",
      "",
      "<permission-request>",
      '{"child_agent_id":"ag_5e9187b","title":"Run cargo test"}',
      "</permission-request>",
    ].join("\n");
    assert.equal(extractCleanAgentMessageBody(permRaw), "");
  });

  it("preserves plain direct messages untouched", () => {
    const msg = "Please check `apps/desktop/src/index.css` & verify CJK line height.";
    assert.equal(extractCleanAgentMessageBody(msg), msg);
  });

  it("formats a clean single-line summary preview", () => {
    const clean = "## Summary\n- Fixed **scrollbar** gap in `subagents-track.tsx`";
    assert.equal(formatCleanAgentMessagePreview(clean), "Summary");

    const bullet = "- Fixed **scrollbar** gap in `subagents-track.tsx`";
    assert.equal(
      formatCleanAgentMessagePreview(bullet),
      "Fixed scrollbar gap in subagents-track.tsx",
    );
  });
});

describe("extractDirectAgentMessagePresentation", () => {
  it("unwraps <orbit-system kind='agent-message'> and extracts readable body + source metadata", () => {
    const raw = [
      '<orbit-system version="1" kind="agent-message" source-agent-id="ag_router" source-host-id="host_mac" source-agent-title="Personal Router" message-id="am_1">',
      "Please fix &lt;UserPromptBubble&gt; &amp; verify tests.",
      "</orbit-system>",
    ].join("\n");

    const env = parseOrbitSystemEnvelope(raw);
    assert.equal(env?.kind, "agent-message");
    assert.equal(env?.sourceAgentId, "ag_router");
    assert.equal(env?.sourceHostId, "host_mac");
    assert.equal(env?.sourceTitle, "Personal Router");

    const res = extractDirectAgentMessagePresentation(raw);
    assert.equal(res.isSystemNoise, false);
    assert.equal(res.strippedEnvelope, true);
    assert.equal(res.cleanText, "Please fix <UserPromptBubble> & verify tests.");
    assert.equal(res.sourceAgentId, "ag_router");
    assert.equal(res.sourceHostId, "host_mac");
    assert.equal(res.sourceTitle, "Personal Router");
  });

  it("strips leading <orbit-fleet-state> and <orbit-fleet-delta> while preserving inline mentions", () => {
    const withFleet = [
      '<orbit-fleet-delta rev="4">',
      "- @worker1 running",
      "</orbit-fleet-delta>",
      "",
      "Check `<orbit-fleet-delta>` handling in personal_router.rs.",
    ].join("\n");
    const res = extractDirectAgentMessagePresentation(withFleet);
    assert.equal(res.isSystemNoise, false);
    assert.equal(res.strippedEnvelope, true);
    assert.equal(res.cleanText, "Check `<orbit-fleet-delta>` handling in personal_router.rs.");

    assert.deepEqual(
      stripLeadingOrbitProtocolBlocks("Mention <orbit-fleet-state> mid-sentence."),
      { text: "Mention <orbit-fleet-state> mid-sentence.", stripped: false },
    );
  });

  it("marks pure fleet/system control envelopes as system noise so they never render as bubbles", () => {
    for (const raw of [
      "",
      "   \n  ",
      '<orbit-fleet-state rev="1" scope="All">\n- worker1\n</orbit-fleet-state>',
      '<orbit-fleet-delta rev="2">\n- worker1 idle\n</orbit-fleet-delta>',
      '<orbit-system version="1" kind="agent-message" source-agent-id="ag_router">\n&lt;orbit-fleet-delta rev=&quot;3&quot;&gt;\n- worker1\n&lt;/orbit-fleet-delta&gt;\n</orbit-system>',
      '<orbit-system version="1" kind="agent-notification" source-agent-id="ag_w1" event="finished">\nAgent ag_w1 finished.\n</orbit-system>',
      '<orbit-system version="1" kind="heartbeat" message-id="hb_1">\nCheck timers\n</orbit-system>',
    ]) {
      const res = extractDirectAgentMessagePresentation(raw);
      assert.equal(res.isSystemNoise, true, `expected system noise for: ${raw}`);
      assert.equal(res.cleanText, "");
    }
  });
});
