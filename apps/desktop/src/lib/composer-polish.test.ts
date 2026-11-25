import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, it } from "node:test";
import { canSkipCappedMeasurement, detectActiveTriggerKind } from "./composer-input.ts";
import { MESSAGES, translate } from "./i18n/index.ts";
import {
  defaultEffortDescription,
  estimateContextWindowLabel,
  formatEffortLabel,
  groupAntigravityOrchestratorModels,
  type BrowsedModelLeaf,
} from "./model-metadata.ts";
import type { ReasoningEffort, SlashCommandView } from "./orbit-ipc.ts";
import {
  filterSlashCommands,
  isSlashCommandSessionRequired,
  normalizeDirectiveLabel,
  resolveSlashCommandGroup,
  resolveSlashDescription,
  resolveComposerSubmission,
} from "./slash-commands.ts";

const SAMPLE_COMMANDS: SlashCommandView[] = [
  {
    id: "terminal",
    command: "/terminal",
    label: "Terminal",
    description: "Toggle integrated workspace terminal",
    shortcut_key: "J",
    icon_key: "terminal",
    keywords: ["pty", "shell"],
    desktop_only: true,
  },
  {
    id: "auto",
    command: "/auto",
    label: "Auto",
    description: "Switch to Auto policy mode",
    shortcut_key: null,
    icon_key: "shield_check",
    keywords: ["guardian", "policy"],
    desktop_only: false,
  },
  {
    id: "edits",
    command: "/edits",
    label: "Accept Edits",
    description: "Auto-approve file edits and confirm commands",
    shortcut_key: null,
    icon_key: "file_pen",
    keywords: ["mode", "edit"],
    desktop_only: false,
  },
  {
    id: "plan",
    command: "/plan",
    label: "Plan",
    description: "Plan before editing",
    shortcut_key: null,
    icon_key: "plan",
    keywords: ["design", "outline"],
    desktop_only: false,
  },
  {
    id: "orbit-committee",
    command: "/orbit-committee",
    label: "Committee",
    description: "Spawn independent review perspectives",
    shortcut_key: null,
    icon_key: "users",
    keywords: ["team", "review"],
    desktop_only: false,
  },
  {
    id: "export",
    command: "/export",
    label: "Export",
    description: "Export conversation",
    shortcut_key: null,
    icon_key: "download",
    keywords: ["markdown", "save"],
    desktop_only: false,
  },
  {
    id: "skill:interface-design",
    command: "/interface-design",
    label: "Interface Design",
    description: "Craft-first UI design audit",
    shortcut_key: null,
    icon_key: "skill",
    keywords: ["ui", "design"],
    desktop_only: false,
  },
];

describe("composer slash command grouping, ranking, and localization", () => {
  it("groups unfiltered slash commands in canonical visual order so keyboard nav matches display", () => {
    const ordered = filterSlashCommands(SAMPLE_COMMANDS, "");
    assert.deepEqual(
      ordered.map((c) => c.id),
      ["plan", "auto", "edits", "terminal", "export", "orbit-committee", "skill:interface-design"],
    );
    assert.deepEqual(
      ordered.map((c) => resolveSlashCommandGroup(c)),
      ["workflows", "session", "session", "tools", "tools", "orchestration", "skills"],
    );
  });

  it("ranks exact slug > prefix > keyword > description and supports localized zh-CN and ja queries", () => {
    const bySlug = filterSlashCommands(SAMPLE_COMMANDS, "auto");
    assert.equal(bySlug[0]?.id, "auto");

    const tZh = (key: Parameters<typeof translate>[1]) => translate("zh-CN", key);
    const byChineseDesc = filterSlashCommands(SAMPLE_COMMANDS, "终端", tZh);
    assert.equal(byChineseDesc[0]?.id, "terminal");

    const byChineseAuto = filterSlashCommands(SAMPLE_COMMANDS, "智能审批", tZh);
    assert.equal(byChineseAuto[0]?.id, "auto");

    const tJa = (key: Parameters<typeof translate>[1]) => translate("ja", key);
    const byJapaneseExport = filterSlashCommands(SAMPLE_COMMANDS, "エクスポート", tJa);
    assert.equal(byJapaneseExport[0]?.id, "export");

    assert.equal(
      resolveSlashDescription(SAMPLE_COMMANDS[1], tZh),
      MESSAGES["zh-CN"]["composer.slashDescAuto"],
    );
    assert.equal(
      resolveSlashDescription(SAMPLE_COMMANDS[6], tZh),
      "Craft-first UI design audit",
    );
  });

  it("identifies commands that require an active session", () => {
    assert.equal(isSlashCommandSessionRequired(SAMPLE_COMMANDS[5]), true);
    assert.equal(isSlashCommandSessionRequired(SAMPLE_COMMANDS[3]), false);
    assert.equal(isSlashCommandSessionRequired(SAMPLE_COMMANDS[1]), false);
  });
});

describe("model metadata context window & effort localization", () => {
  it("estimates compact context window labels across providers and 1M variants", () => {
    assert.equal(estimateContextWindowLabel("claude_code", "claude-opus-4-7"), "200K");
    assert.equal(estimateContextWindowLabel("claude_code", "claude-sonnet-4-6[1m]"), "1M");
    assert.equal(estimateContextWindowLabel("claude_code", "claude-haiku-4-5"), "200K");
    assert.equal(estimateContextWindowLabel("codex", "gpt-5.4"), "272K");
    assert.equal(estimateContextWindowLabel("codex", "gpt-5-mini"), "400K");
    assert.equal(estimateContextWindowLabel("codex", "gpt-4.1"), "1M");
    assert.equal(estimateContextWindowLabel("gemini_cli", null), "1M");
    assert.equal(estimateContextWindowLabel("antigravity", "gemini-2.5-pro"), "1M");
    assert.equal(estimateContextWindowLabel("kimi", "kimi-k2"), "256K");
    assert.equal(estimateContextWindowLabel("opencode", "deepseek-r1"), "128K");
    assert.equal(estimateContextWindowLabel("acp", "anything"), null);
  });

  it("localizes every ReasoningEffort label and description across en, zh-CN, and ja", () => {
    const levels: ReasoningEffort[] = ["off", "low", "medium", "high", "xhigh", "max", "ultra"];
    for (const locale of ["en", "zh-CN", "ja"] as const) {
      const tr = (key: Parameters<typeof translate>[1]) => translate(locale, key);
      for (const effort of levels) {
        const label = formatEffortLabel(effort, tr);
        const desc = defaultEffortDescription(effort, tr);
        assert.ok(label.trim().length > 0, `${locale} missing label for ${effort}`);
        assert.ok(desc.trim().length > 0, `${locale} missing description for ${effort}`);
      }
    }
  });

  it("collapses Antigravity Orchestrator models into one cascading group with resolved sub-model names", () => {
    const leaves: BrowsedModelLeaf[] = [
      {
        key: "gemini-4-neon",
        id: "gemini-4-neon",
        label: "Gemini 4 Neon",
        desc: "",
        defaultEffort: "high",
        thinkingOptions: [],
        supportsFastMode: false,
      },
      {
        key: "gemini-4-carbon",
        id: "gemini-4-carbon",
        label: "Gemini 4 Carbon",
        desc: "",
        defaultEffort: "medium",
        thinkingOptions: [],
        supportsFastMode: false,
      },
      {
        key: "gemini-4-argon",
        id: "gemini-4-argon",
        label: "Gemini 4 Argon",
        desc: "",
        defaultEffort: "medium",
        thinkingOptions: [],
        supportsFastMode: false,
      },
      {
        key: "gemini-pro-orchestrator-neon",
        id: "gemini-pro-orchestrator-neon",
        label: "Orchestrator (Neon)",
        desc: "",
        defaultEffort: "xhigh",
        thinkingOptions: [{ effort: "xhigh", label: "Extra High", description: null, is_default: true }],
        supportsFastMode: false,
      },
      {
        key: "gemini-pro-orchestrator",
        id: "gemini-pro-orchestrator",
        label: "Orchestrator (Carbon)",
        desc: "",
        defaultEffort: null,
        thinkingOptions: [],
        supportsFastMode: false,
      },
      {
        key: "gemini-pro-orchestrator-argon",
        id: "gemini-pro-orchestrator-argon",
        label: "Orchestrator (Argon)",
        desc: "",
        defaultEffort: null,
        thinkingOptions: [],
        supportsFastMode: false,
      },
      {
        key: "claude-opus-5-5",
        id: "claude-opus-5-5",
        label: "Claude Opus 5.5",
        desc: "",
        defaultEffort: "medium",
        thinkingOptions: [],
        supportsFastMode: false,
      },
    ];

    const grouped = groupAntigravityOrchestratorModels("antigravity", leaves);
    assert.equal(grouped.length, 5);
    assert.equal(grouped[3]?.kind, "group");
    if (grouped[3]?.kind === "group") {
      assert.equal(grouped[3].label, "Orchestrator");
      assert.deepEqual(
        grouped[3].children.map((c) => c.label),
        ["Gemini 4 Neon", "Gemini 4 Carbon", "Gemini 4 Argon"],
      );
      assert.equal(grouped[3].children[0]?.thinkingOptions.length, 1);
    }

    // Non-Antigravity catalogs stay completely flat
    const untouched = groupAntigravityOrchestratorModels("claude_code", leaves);
    assert.equal(untouched.length, leaves.length);
    assert.ok(untouched.every((entry) => entry.kind === "model"));
  });
});

describe("composer directive submission", () => {
  for (const kind of ["btw", "goal"] as const) {
    it(`routes bare, inline and pasted /${kind} locally`, () => {
      for (const text of ["", "some text"]) {
        assert.deepEqual(resolveComposerSubmission(kind, `  ${text}  `), { kind, text });
        assert.deepEqual(resolveComposerSubmission(null, `/${kind} ${text}`), { kind, text });
      }
      assert.deepEqual(resolveComposerSubmission(null, `/${kind}`), { kind, text: "" });
    });
  }

  it("preserves plan and only recognizes whole raw commands", () => {
    assert.deepEqual(resolveComposerSubmission("plan", "/btw example"), { kind: "prompt", directive: "plan" });
    assert.deepEqual(resolveComposerSubmission(null, "/goalkeeper"), { kind: "prompt", directive: null });
    assert.deepEqual(resolveComposerSubmission("btw", "/goal example"), { kind: "btw", text: "/goal example" });
  });

  it("normalizes slash directive labels and shares frameless InlineDirectiveToken across surfaces", () => {
    assert.equal(normalizeDirectiveLabel("/plan"), "plan");
    assert.equal(normalizeDirectiveLabel("  ///orbit-handoff  "), "orbit-handoff");
    assert.equal(normalizeDirectiveLabel("review"), "review");
    assert.equal(normalizeDirectiveLabel(" / "), "");

    const desktopSrc = resolve(import.meta.dirname, "..");
    const tokenSrc = readFileSync(
      resolve(desktopSrc, "components/composer/composer-directive-token.tsx"),
      "utf8",
    );
    const bubbleSrc = readFileSync(
      resolve(desktopSrc, "components/stage/stage-prompt-controls.tsx"),
      "utf8",
    );
    const queueSrc = readFileSync(
      resolve(desktopSrc, "components/composer/composer-queue-strip.tsx"),
      "utf8",
    );

    assert.match(tokenSrc, /export const InlineDirectiveToken/);
    assert.match(tokenSrc, /items-baseline align-baseline/);
    assert.match(tokenSrc, /self-center/);
    assert.doesNotMatch(tokenSrc, /bg-signal-info-bg|border-signal-info-border|font-mono/);

    assert.match(bubbleSrc, /<InlineDirectiveToken/);
    assert.doesNotMatch(bubbleSrc, /bg-signal-info-bg|border-signal-info-border/);

    assert.match(queueSrc, /<InlineDirectiveToken/);
    assert.doesNotMatch(queueSrc, /ring-border-strong|ring-1/);
  });
});

describe("composer caret triggers", () => {
  it("matches the slash and mention grammar at every caret boundary", () => {
    for (const text of [
      "/plan", "prefix /plan suffix", "see /src/main.rs then type here",
      "@file", "read @src/main.rs next", "mail@host a@b/c",
      "/x@y @x/y @a@b //", "x\t/@\n@file  /", "你好　/计划 接续　@文件",
      "前缀@无效 🪐 /计划", "",
    ]) {
      for (let pos = 0; pos <= text.length; pos++) {
        const prefix = text.slice(0, pos);
        const expected = /(?:^|\s)\/([^\s/]*)$/.test(prefix) ? "slash"
          : /(?:^|\s)@([^\s@]*)$/.test(prefix) ? "mention" : null;
        assert.equal(detectActiveTriggerKind(text, pos), expected, JSON.stringify(prefix));
      }
    }
  });
});

describe("capped textarea measurement", () => {
  const capped = { prevText: "long draft", width: 640, indent: 42, scopeKey: "agent-a" };
  it("skips only unchanged or appended text in the same capped layout", () => {
    for (const text of ["long draft", "long draft more", "long draft\n下一行"]) {
      assert.ok(canSkipCappedMeasurement(capped, text, 640, 42, "agent-a"));
    }
    for (const text of ["", "long", "LONG draft", "long middle draft"]) {
      assert.equal(canSkipCappedMeasurement(capped, text, 640, 42, "agent-a"), false);
    }
    assert.equal(canSkipCappedMeasurement(null, "long draft", 640, 42, "agent-a"), false);
    assert.equal(canSkipCappedMeasurement(capped, "long draft", 641, 42, "agent-a"), false);
    assert.equal(canSkipCappedMeasurement(capped, "long draft", 640, 0, "agent-a"), false);
    assert.equal(canSkipCappedMeasurement(capped, "long draft", 640, 42, "agent-b"), false);
  });
});
