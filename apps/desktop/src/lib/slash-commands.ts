import type { MessageKey } from "./i18n/index.ts";
import type { SlashCommandView, SlashDirective } from "./orbit-ipc.ts";

export type ComposerInlineDirective = SlashDirective | "btw" | "goal";

const inlineDirectives: ReadonlySet<string> = new Set<ComposerInlineDirective>([
  "btw", "goal", "plan", "review", "compact", "orbit",
  "orbit-handoff", "orbit-committee", "orbit-advisor",
]);

export function isComposerInlineDirective(value: unknown): value is ComposerInlineDirective {
  return typeof value === "string" && inlineDirectives.has(value);
}

/** Strips any leading slash(es) and outer whitespace so `/plan` and `plan` render identically. */
export function normalizeDirectiveLabel(raw: string): string {
  return raw.trim().replace(/^\/+/, "");
}

export function resolveComposerSubmission(directive: ComposerInlineDirective | null, draft: string) {
  if (directive === "btw" || directive === "goal") {
    return { kind: directive, text: draft.trim() } as const;
  }
  const match = !directive && /^\s*\/(btw|goal)(?=\s|$)/i.exec(draft);
  if (match) {
    const kind = match[1].toLowerCase() === "goal" ? "goal" : "btw";
    return { kind, text: draft.slice(match[0].length).trim() } as const;
  }
  return { kind: "prompt", directive } as const;
}

export type SlashCommandGroupId =
  | "workflows"
  | "session"
  | "tools"
  | "orchestration"
  | "skills";

export const SLASH_GROUP_ORDER: readonly SlashCommandGroupId[] = [
  "workflows",
  "session",
  "tools",
  "orchestration",
  "skills",
];

const SESSION_REQUIRED_IDS = new Set(["fork", "goal", "compact", "export", "copy"]);

export const SLASH_DESC_KEY_BY_ID: Readonly<Record<string, MessageKey>> = {
  fork: "composer.slashDescFork",
  btw: "composer.slashDescBtw",
  plan: "composer.slashDescPlan",
  goal: "composer.slashDescGoal",
  review: "composer.slashDescReview",
  diff: "composer.slashDescDiff",
  files: "composer.slashDescFiles",
  browser: "composer.slashDescBrowser",
  compact: "composer.slashDescCompact",
  export: "composer.slashDescExport",
  copy: "composer.slashDescCopy",
  fast: "composer.slashDescFast",
  orbit: "composer.slashDescOrbit",
  "orbit-handoff": "composer.slashDescOrbitHandoff",
  "orbit-committee": "composer.slashDescOrbitCommittee",
  "orbit-advisor": "composer.slashDescOrbitAdvisor",
  supervised: "composer.slashDescSupervised",
  auto: "composer.slashDescAuto",
  edits: "composer.slashDescEdits",
  full: "composer.slashDescFull",
  terminal: "composer.slashDescTerminal",
  new: "composer.slashDescNew",
  settings: "composer.slashDescSettings",
};

export const SLASH_GROUP_LABEL_KEY: Readonly<Record<SlashCommandGroupId, MessageKey>> = {
  workflows: "composer.slashGroupWorkflows",
  session: "composer.slashGroupSession",
  tools: "composer.slashGroupTools",
  orchestration: "composer.slashGroupOrchestration",
  skills: "composer.slashGroupSkills",
};

export function resolveSlashCommandGroup(cmd: SlashCommandView): SlashCommandGroupId {
  if (cmd.icon_key === "skill" || cmd.id.startsWith("skill:")) return "skills";
  switch (cmd.id) {
    case "plan":
    case "review":
    case "compact":
    case "goal":
    case "btw":
    case "fork":
      return "workflows";
    case "supervised":
    case "auto":
    case "edits":
    case "full":
    case "fast":
    case "new":
      return "session";
    case "orbit":
    case "orbit-handoff":
    case "orbit-committee":
    case "orbit-advisor":
      return "orchestration";
    case "diff":
    case "files":
    case "terminal":
    case "browser":
    case "export":
    case "copy":
    case "settings":
    default:
      return "tools";
  }
}

export function isSlashCommandSessionRequired(cmd: SlashCommandView): boolean {
  return SESSION_REQUIRED_IDS.has(cmd.id);
}

export function resolveSlashDescription(
  cmd: SlashCommandView,
  translate: (key: MessageKey) => string,
): string {
  const key = SLASH_DESC_KEY_BY_ID[cmd.id];
  return key ? translate(key) : cmd.description;
}

function scoreSlashMatch(
  cmd: SlashCommandView,
  q: string,
  localizedDescLower: string,
): number {
  const slug = cmd.command.replace(/^\//, "").toLowerCase();
  const id = cmd.id.toLowerCase();
  const label = cmd.label.toLowerCase();
  if (slug === q || id === q) return 120;
  if (slug.startsWith(q)) return 105;
  if (id.startsWith(q)) return 95;
  if (cmd.keywords.some((k) => k.toLowerCase() === q)) return 88;
  if (cmd.keywords.some((k) => k.toLowerCase().startsWith(q))) return 78;
  if (slug.includes(q) || id.includes(q)) return 72;
  if (label.includes(q)) return 62;
  if (cmd.keywords.some((k) => k.toLowerCase().includes(q))) return 56;
  if (localizedDescLower.includes(q) || cmd.description.toLowerCase().includes(q)) return 46;
  return -1;
}

/**
 * Filters, ranks, and orders slash commands in canonical display order so keyboard
 * navigation (`ArrowUp`/`ArrowDown`) matches the exact visual order across groups.
 */
export function filterSlashCommands(
  commands: readonly SlashCommandView[],
  query: string,
  translate?: (key: MessageKey) => string,
): SlashCommandView[] {
  const q = query.trim().toLowerCase();
  if (!q) {
    const grouped = new Map<SlashCommandGroupId, SlashCommandView[]>();
    for (const g of SLASH_GROUP_ORDER) grouped.set(g, []);
    for (const cmd of commands) {
      grouped.get(resolveSlashCommandGroup(cmd))?.push(cmd);
    }
    return SLASH_GROUP_ORDER.flatMap((g) => grouped.get(g) ?? []);
  }

  const scored: Array<{ cmd: SlashCommandView; score: number; origIdx: number }> = [];
  for (let i = 0; i < commands.length; i += 1) {
    const cmd = commands[i];
    const desc = translate ? resolveSlashDescription(cmd, translate).toLowerCase() : "";
    const score = scoreSlashMatch(cmd, q, desc);
    if (score >= 0) {
      scored.push({ cmd, score, origIdx: i });
    }
  }
  scored.sort((a, b) => b.score - a.score || a.origIdx - b.origIdx);
  return scored.map((entry) => entry.cmd);
}
