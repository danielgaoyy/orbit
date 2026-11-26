import assert from "node:assert/strict";
import { readdirSync, readFileSync, statSync } from "node:fs";
import { join, relative, resolve } from "node:path";
import { describe, it } from "node:test";

const COMPONENTS_ROOT = resolve(import.meta.dirname, "../components");

function collectTsxFiles(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry);
    const stat = statSync(full);
    if (stat.isDirectory()) {
      collectTsxFiles(full, out);
    } else if (full.endsWith(".tsx")) {
      out.push(full);
    }
  }
  return out;
}

describe("desktop flexbox truncation & overflow guards", () => {
  it("never combines shrink-0 with truncate without a width bound (max-w-* or w-*) across desktop components", () => {
    const violations: string[] = [];
    const classStringRe = /["'`]([^"'`\n]*\bshrink-0\b[^"'`\n]*\btruncate\b[^"'`\n]*|[^"'`\n]*\btruncate\b[^"'`\n]*\bshrink-0\b[^"'`\n]*)["'`]/g;

    for (const file of collectTsxFiles(COMPONENTS_ROOT)) {
      const lines = readFileSync(file, "utf8").split("\n");
      lines.forEach((line, idx) => {
        classStringRe.lastIndex = 0;
        let match: RegExpExecArray | null;
        while ((match = classStringRe.exec(line)) !== null) {
          const classes = match[1];
          const hasWidthBound = /\b(?:max-w-|w-)/.test(classes);
          if (!hasWidthBound) {
            violations.push(`${relative(COMPONENTS_ROOT, file)}:${idx + 1}: ${classes}`);
          }
        }
      });
    }

    assert.deepEqual(
      violations,
      [],
      `Found unbounded shrink-0 + truncate conflicts:\n${violations.join("\n")}`,
    );
  });

  it("protects short mention prefixes and bounds dynamic titles across MentionMenu and SlashCommandMenu", () => {
    const mentionSrc = readFileSync(join(COMPONENTS_ROOT, "composer/mention-menu.tsx"), "utf8");
    const slashSrc = readFileSync(join(COMPONENTS_ROOT, "composer/slash-menu.tsx"), "utf8");

    // MentionMenu rows must clip overflow and never leave dynamic titles as unbounded shrink-0.
    assert.doesNotMatch(
      mentionSrc,
      /className="text-ink-primary shrink-0 font-mono text-\[13px\] font-medium"/,
    );
    assert.doesNotMatch(
      mentionSrc,
      /className="text-ink-muted shrink-0 font-mono text-\[11px\]"/,
    );
    // Short @folder / @web hint keywords stay shrink-0 so long descriptions cannot squeeze them.
    assert.match(mentionSrc, /item\.kind === "folder_hint"\s*\?\s*"shrink-0"/);
    assert.match(mentionSrc, /item\.kind === "web_hint"\s*\?\s*"shrink-0"/);
    // Symbol locLabel is bounded so long symbols/paths never crush each other.
    assert.match(mentionSrc, /max-w-\[42%\] shrink-0 truncate font-mono text-\[11px\]/);

    // SlashCommandMenu must bound long custom command slugs & argument hints with overflow-hidden + title.
    assert.match(slashSrc, /overflow-hidden rounded-\[9px\]/);
    assert.match(slashSrc, /max-w-\[55%\] shrink-0/);
    assert.match(slashSrc, /max-w-\[30%\] shrink-0 truncate/);
  });

  it("keeps basename/title/label truncation balanced across QuickOpen, ContextTray, Editor, PR checks, Usage, and Stage headers", () => {
    const quickOpenSrc = readFileSync(
      join(COMPONENTS_ROOT, "side-panel/quick-open-file-dialog.tsx"),
      "utf8",
    );
    assert.match(quickOpenSrc, /dir \? "max-w-\[65%\] shrink-0" : "flex-1"/);
    assert.match(quickOpenSrc, /title=\{filePath\}/);

    const contextTraySrc = readFileSync(join(COMPONENTS_ROOT, "composer/context-tray.tsx"), "utf8");
    assert.equal(
      (contextTraySrc.match(/dirHint && "max-w-\[68%\] shrink-0"/g) ?? []).length,
      2,
      "expected both attachment and inline mention file pills to protect basename against long dirHint",
    );

    const editorSrc = readFileSync(
      join(COMPONENTS_ROOT, "side-panel/workspace-code-editor.tsx"),
      "utf8",
    );
    assert.match(editorSrc, /refLoc\.preview_line \? "max-w-\[55%\] shrink-0" : "flex-1"/);

    const fileViewerSrc = readFileSync(
      join(COMPONENTS_ROOT, "side-panel/file-viewer-tab.tsx"),
      "utf8",
    );
    assert.match(fileViewerSrc, /shrink-0 whitespace-nowrap font-medium">\s*Unsaved changes/);

    const prBarSrc = readFileSync(
      join(COMPONENTS_ROOT, "side-panel/git-pr-status-bar.tsx"),
      "utf8",
    );
    assert.match(prBarSrc, /check\.workflow \? "max-w-\[65%\] shrink-0" : "flex-1"/);

    const usageSrc = readFileSync(
      join(COMPONENTS_ROOT, "settings/host-usage-section.tsx"),
      "utf8",
    );
    assert.match(usageSrc, /text-ink-primary shrink-0 text-\[13px\] font-semibold/);

    const terminalWinSrc = readFileSync(
      join(COMPONENTS_ROOT, "detached-terminal-window.tsx"),
      "utf8",
    );
    assert.match(terminalWinSrc, /cwd && "max-w-\[55%\] shrink-0"/);

    const diffViewerSrc = readFileSync(join(COMPONENTS_ROOT, "diff-viewer.tsx"), "utf8");
    assert.match(diffViewerSrc, /!isMultiLine && target\.lineNumber != null && "max-w-\[65%\] shrink-0"/);

    const agentPaneSrc = readFileSync(
      join(COMPONENTS_ROOT, "stage/agent-stage-pane.tsx"),
      "utf8",
    );
    assert.match(agentPaneSrc, /max-w-\[110px\] shrink-0 truncate rounded/);

    const stepGroupSrc = readFileSync(
      join(COMPONENTS_ROOT, "stage/stage-step-group.tsx"),
      "utf8",
    );
    assert.match(
      stepGroupSrc,
      /inline-flex min-w-0 max-w-full items-center gap-1\.5 text-\[11\.5px\]/,
    );

    // Shared AgentListItem renders ProviderIcon + Model + Effort (no dot between Model & Effort) + Status,
    // and is composed by SubagentCrewList, PersonalAgentBriefingRail, and WorkspaceBoardTab.
    const agentListItemSrc = readFileSync(
      join(COMPONENTS_ROOT, "ui/agent-list-item.tsx"),
      "utf8",
    );
    assert.match(agentListItemSrc, /<ProviderIcon[\s\S]*?provider=\{provider\}/);
    assert.match(agentListItemSrc, /<span className="min-w-0 truncate">\{modelLabel\}<\/span>/);
    assert.match(agentListItemSrc, /<span className="text-ink-muted shrink-0">\{effortLabel\}<\/span>/);
    assert.doesNotMatch(agentListItemSrc, /modelLabel\}[\s\S]{0,80}·[\s\S]{0,80}\{effortLabel\}/);
    assert.doesNotMatch(agentListItemSrc, /\bdetail\?:/);

    const crewDockSrc = readFileSync(
      join(COMPONENTS_ROOT, "stage/subagent-crew-dock.tsx"),
      "utf8",
    );
    assert.match(crewDockSrc, /const showModel = Boolean\(sa\.modelLabel\) && depth < 4;/);
    assert.match(crewDockSrc, /<AgentListItem\b/);

    const briefingRailSrc = readFileSync(
      join(COMPONENTS_ROOT, "personal-agent/personal-agent-briefing-rail.tsx"),
      "utf8",
    );
    assert.match(briefingRailSrc, /<AgentListItem\b/);
    assert.match(briefingRailSrc, /orbitActions\.openSubagentTab\(/);
    assert.doesNotMatch(briefingRailSrc, /orbitActions\.openPeerSplitTab\(/);
    assert.doesNotMatch(briefingRailSrc, /<DiffStat\b/);
    // Briefing Rail shares the exact StageRail primitives and Overview section/goal/timer components with StageOverviewPanel
    assert.match(briefingRailSrc, /<StageRailShell\b/);
    assert.match(briefingRailSrc, /<StageRailHeader\b/);
    assert.match(briefingRailSrc, /<StageRailScrollBody\b/);
    assert.match(briefingRailSrc, /<OverviewSection\b/);
    assert.match(briefingRailSrc, /<StageRailExpandFooter\b/);
    assert.match(briefingRailSrc, /<OverviewGoalRow\b/);
    assert.match(briefingRailSrc, /<ArmedTimerRow\b/);
    assert.doesNotMatch(briefingRailSrc, /\bconst BriefingSection\b/);
    assert.doesNotMatch(briefingRailSrc, /\bconst MissionGroupCard\b/);
    assert.doesNotMatch(briefingRailSrc, /\bPersonalAgentTopBarCapsule\b/);

    // TopBar aligns Personal Agent header identically to standard sessions (SubagentCrewMiniDock + standard toggle, no special capsule)
    const topBarSrc = readFileSync(
      join(COMPONENTS_ROOT, "layout/top-bar.tsx"),
      "utf8",
    );
    assert.doesNotMatch(topBarSrc, /PersonalAgentTopBarCapsule/);
    assert.match(topBarSrc, /const needsTopBarActivity = hasActiveSession && !overviewVisible;/);
    assert.doesNotMatch(topBarSrc, /showTopBarCrewRelay\s*=\s*[\s\S]{0,120}!isPersonalAgentSession/);
    assert.match(topBarSrc, /isDefaultPersonalAgentTitle\(/);
    assert.match(topBarSrc, /<SubagentCrewMiniDock\b/);
    assert.match(topBarSrc, /data-overview-trigger/);

    const stageOverviewSrc = readFileSync(
      join(COMPONENTS_ROOT, "stage/stage-overview-panel.tsx"),
      "utf8",
    );
    assert.match(stageOverviewSrc, /<StageRailShell\b/);
    assert.match(stageOverviewSrc, /<StageRailHeader\b/);
    assert.match(stageOverviewSrc, /<StageRailScrollBody\b/);
    assert.match(stageOverviewSrc, /<OverviewSection\b/);

    const boardTabSrc = readFileSync(
      join(COMPONENTS_ROOT, "side-panel/workspace-board-tab.tsx"),
      "utf8",
    );
    assert.match(boardTabSrc, /<AgentListItem\b/);

    // Side Panel SubagentTab renders the unified 32px semantic context header (Provider + Agent + Model + Effort + Status, crumbs only for depth >= 2)
    const subagentTabSrc = readFileSync(
      join(COMPONENTS_ROOT, "side-panel/subagent-tab.tsx"),
      "utf8",
    );
    assert.match(subagentTabSrc, /data-slot="subagent-context-header"/);
    assert.match(
      subagentTabSrc,
      /const ancestorCrumbs = ancestorLineage\.length > 2 \? ancestorLineage\.slice\(0, -1\) : \[\];/,
    );
    assert.match(subagentTabSrc, /<ProviderIcon/);
    assert.match(subagentTabSrc, /runtimeDisplay\.agentLabel/);
    assert.match(subagentTabSrc, /runtimeDisplay\.modelLabel/);
    assert.match(subagentTabSrc, /\{effortLabelText\}/);
    assert.doesNotMatch(
      subagentTabSrc,
      /runtimeDisplay\.modelLabel\}[\s\S]{0,80}·[\s\S]{0,80}\{effortLabelText\}/,
    );
  });
});
