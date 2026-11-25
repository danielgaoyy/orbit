import type { MessageKey } from "./i18n/index.ts";
import type {
  ProviderKind,
  ProviderModelCatalog,
  ProviderModelSpec,
  ReasoningEffort,
  ThinkingOptionSpec,
} from "./orbit-ipc.ts";

export const PROVIDER_DISPLAY_LABELS: Readonly<Record<ProviderKind, string>> = {
  claude_code: "Claude Code",
  codex: "Codex",
  antigravity: "Antigravity",
  opencode: "OpenCode",
  gemini_cli: "Gemini CLI",
  cursor_agent: "Cursor Agent",
  copilot: "GitHub Copilot",
  goose: "Goose",
  pi: "Pi",
  kimi: "Kimi CLI",
  cline: "Cline",
  mistral_vibe: "Mistral Vibe",
  augment: "Augment",
  acp: "ACP Agent",
};

export function formatProviderDisplayName(provider: ProviderKind | string): string {
  if (Object.hasOwn(PROVIDER_DISPLAY_LABELS, provider)) {
    return PROVIDER_DISPLAY_LABELS[provider as ProviderKind];
  }
  return provider.replace(/_/g, " ");
}

export function stripAgyEffortSuffix(modelId: string): {
  baseId: string;
  inferredEffort: ReasoningEffort | null;
} {
  const m = /^(.*)-(xhigh|ultra|medium|high|low|max)$/i.exec(modelId.trim());
  if (!m || modelId.toLowerCase() === "gpt-oss-120b-medium") {
    return { baseId: modelId.trim(), inferredEffort: null };
  }
  return {
    baseId: m[1],
    inferredEffort: m[2].toLowerCase() as ReasoningEffort,
  };
}

export function prettifyModelSlug(rawId: string, provider?: ProviderKind): string {
  let cleaned = rawId
    .trim()
    .replace(/-\d{8}$/, "")
    .replace(/-\d{4}-\d{2}-\d{2}$/, "")
    .replace(/-(?:latest|preview(?:-[a-z0-9-]+)?|exp(?:-[a-z0-9-]+)?)$/i, "");

  if (provider === "antigravity") {
    cleaned = stripAgyEffortSuffix(cleaned).baseId;
  } else if (provider === "claude_code") {
    cleaned = cleaned.replace(/^claude-/i, "");
  }

  const gptMatch = /^gpt-(\d+(?:[.-]\d+)?)(?:-(.+))?$/i.exec(cleaned);
  if (gptMatch) {
    const ver = gptMatch[1].replace("-", ".");
    if (!gptMatch[2]) return `GPT ${ver}`;
    const rest = gptMatch[2]
      .split("-")
      .filter(Boolean)
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ");
    return `GPT ${ver} ${rest}`;
  }

  const oMatch = /^(o\d+)(?:-(.+))?$/i.exec(cleaned);
  if (oMatch) {
    const base = oMatch[1].toLowerCase();
    if (!oMatch[2]) return base;
    const rest = oMatch[2]
      .split("-")
      .filter(Boolean)
      .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
      .join(" ");
    return `${base} ${rest}`;
  }

  cleaned = cleaned.replace(/(\d+)-(\d+)(?=-|$)/g, "$1.$2");

  return cleaned
    .split("-")
    .filter(Boolean)
    .map((w) => (/^\d/.test(w) ? w : w.charAt(0).toUpperCase() + w.slice(1)))
    .join(" ");
}

export function resolveActiveModelSpec(
  catalog: ProviderModelCatalog,
  rawModelId: string | null,
): ProviderModelSpec | null {
  if (catalog.models.length === 0) return null;
  if (!rawModelId) {
    return catalog.models.find((m) => m.is_default) ?? catalog.models[0];
  }
  const lowered = rawModelId.toLowerCase();
  const exact = catalog.models.find((m) => m.id.toLowerCase() === lowered);
  if (exact) return exact;

  if (catalog.provider === "antigravity") {
    const stripped = stripAgyEffortSuffix(rawModelId).baseId.toLowerCase();
    const byBase = catalog.models.find((m) => m.id.toLowerCase() === stripped);
    if (byBase) return byBase;
  }

  // Claude short alias compatibility (`opus` -> `claude-opus-...`, `sonnet` -> `claude-sonnet-...`)
  if (catalog.provider === "claude_code") {
    const byFamily = catalog.models.find((m) => m.id.toLowerCase().includes(`-${lowered}-`));
    if (byFamily) return byFamily;
  }

  return null;
}

export function formatCatalogModelLabel(
  catalog: ProviderModelCatalog,
  rawModelId: string | null,
): string {
  const matched = resolveActiveModelSpec(catalog, rawModelId);
  if (matched) {
    return matched.label;
  }
  if (!rawModelId) {
    return "Default";
  }
  return prettifyModelSlug(rawModelId, catalog.provider);
}

export interface BrowsedModelLeaf {
  key: string;
  id: string;
  label: string;
  desc: string;
  defaultEffort: ReasoningEffort | null;
  thinkingOptions: readonly ThinkingOptionSpec[];
  supportsFastMode: boolean;
}

export type BrowsedModelEntry =
  | ({ kind: "model" } & BrowsedModelLeaf)
  | {
      kind: "group";
      key: string;
      label: string;
      children: BrowsedModelLeaf[];
    };

function isAntigravityOrchestratorLeaf(leaf: BrowsedModelLeaf): boolean {
  const idLower = leaf.id.trim().toLowerCase();
  const labelLower = leaf.label.trim().toLowerCase();
  return idLower.includes("orchestrator") || /^orchestrator\s*\(.+\)$/.test(labelLower);
}

function resolveOrchestratorSubModelLabel(
  leaf: BrowsedModelLeaf,
  siblingModels: readonly BrowsedModelLeaf[],
): string {
  const parenMatch = /^orchestrator\s*\(([^)]+)\)\s*$/i.exec(leaf.label.trim());
  let token = parenMatch?.[1]?.trim() ?? "";
  if (!token) {
    const idMatch = /orchestrator[-_](.+)$/i.exec(leaf.id.trim());
    token = idMatch?.[1]?.replace(/[-_]+/g, " ").trim() ?? "";
  }
  if (!token) {
    return leaf.label;
  }
  const tokenLower = token.toLowerCase();
  const matchedSibling = siblingModels.find((m) => {
    const ml = m.label.trim().toLowerCase();
    return (
      ml === tokenLower ||
      ml.endsWith(` ${tokenLower}`) ||
      m.id.trim().toLowerCase().endsWith(`-${tokenLower}`)
    );
  });
  return matchedSibling ? matchedSibling.label : token;
}

/**
 * Collapses Antigravity's flat `Orchestrator (Neon)` / `Orchestrator (Carbon)` / `Orchestrator (Argon)`
 * entries into a single `Orchestrator` cascading row (matching Antigravity's native model picker),
 * resolving each child to its full sibling model display name (e.g., `Gemini 4 Neon`) and preserving
 * any thinking options advertised by the CLI for Level-3 selection.
 */
export function groupAntigravityOrchestratorModels(
  provider: ProviderKind,
  leaves: readonly BrowsedModelLeaf[],
): BrowsedModelEntry[] {
  if (provider !== "antigravity") {
    return leaves.map((leaf) => ({ kind: "model", ...leaf }));
  }

  const orchestrators = leaves.filter(isAntigravityOrchestratorLeaf);
  if (orchestrators.length < 2) {
    return leaves.map((leaf) => ({ kind: "model", ...leaf }));
  }

  const siblings = leaves.filter((leaf) => !isAntigravityOrchestratorLeaf(leaf));
  const groupedChildren: BrowsedModelLeaf[] = orchestrators.map((leaf) => ({
    ...leaf,
    label: resolveOrchestratorSubModelLabel(leaf, siblings),
  }));

  const result: BrowsedModelEntry[] = [];
  let insertedGroup = false;
  for (const leaf of leaves) {
    if (isAntigravityOrchestratorLeaf(leaf)) {
      if (!insertedGroup) {
        insertedGroup = true;
        result.push({
          kind: "group",
          key: "group:orchestrator",
          label: "Orchestrator",
          children: groupedChildren,
        });
      }
      continue;
    }
    result.push({ kind: "model", ...leaf });
  }
  return result;
}

export function effortLabelKey(effort: ReasoningEffort): MessageKey {
  switch (effort) {
    case "off":
      return "composer.effortOff";
    case "low":
      return "composer.effortLow";
    case "medium":
      return "composer.effortMedium";
    case "high":
      return "composer.effortHigh";
    case "xhigh":
      return "composer.effortXhigh";
    case "max":
      return "composer.effortMax";
    case "ultra":
      return "composer.effortUltra";
  }
}

export function effortDescriptionKey(effort: ReasoningEffort): MessageKey {
  switch (effort) {
    case "off":
      return "composer.effortOffDesc";
    case "low":
      return "composer.effortLowDesc";
    case "medium":
      return "composer.effortMediumDesc";
    case "high":
      return "composer.effortHighDesc";
    case "xhigh":
      return "composer.effortXhighDesc";
    case "max":
      return "composer.effortMaxDesc";
    case "ultra":
      return "composer.effortUltraDesc";
  }
}

export function formatEffortLabel(
  effort: ReasoningEffort,
  translate?: (key: MessageKey) => string,
): string {
  if (translate) {
    return translate(effortLabelKey(effort));
  }
  switch (effort) {
    case "off":
      return "Off";
    case "low":
      return "Low";
    case "medium":
      return "Medium";
    case "high":
      return "High";
    case "xhigh":
      return "Extra High";
    case "max":
      return "Max";
    case "ultra":
      return "Ultra";
  }
}

export function defaultEffortDescription(
  effort: ReasoningEffort,
  translate?: (key: MessageKey) => string,
): string {
  if (translate) {
    return translate(effortDescriptionKey(effort));
  }
  switch (effort) {
    case "off":
      return "Thinking disabled";
    case "low":
      return "Fast & concise";
    case "medium":
      return "Balanced speed & depth";
    case "high":
      return "Greater reasoning depth";
    case "xhigh":
      return "Extra deep reasoning";
    case "max":
      return "Maximum reasoning budget";
    case "ultra":
      return "Max reasoning + delegation";
  }
}

/**
 * Compact context window label matching `orbit-daemon::usage::pricing_for_model`
 * so users can distinguish 200K vs 1M / 272K / 400K models directly inside the model picker.
 */
export function estimateContextWindowLabel(
  provider: ProviderKind,
  modelId: string | null | undefined,
): string | null {
  if (provider === "acp") return null;
  const m = (modelId ?? "").trim().toLowerCase();
  const is1m = m.includes("1m") || m.includes("[1m]");

  if (m.includes("opus") || m.includes("sonnet")) {
    return is1m ? "1M" : "200K";
  }
  if (m.includes("haiku")) return "200K";
  if (m.includes("gpt-5") && (m.includes("mini") || m.includes("nano"))) return "400K";
  if (m.includes("gpt-5") || m.includes("codex")) return "272K";
  if (m.includes("o4-mini") || m.includes("o3")) return "200K";
  if (m.includes("gpt-4.1")) return "1M";
  if (m.includes("gpt-4o")) return "128K";
  if (m.includes("gemini")) return "1M";
  if (m.includes("deepseek")) return "128K";
  if (m.includes("kimi") || m.includes("moonshot") || m.includes("qwen")) return "256K";

  switch (provider) {
    case "claude_code":
      return "200K";
    case "codex":
      return "272K";
    case "antigravity":
    case "gemini_cli":
      return "1M";
    case "kimi":
      return "256K";
    default:
      return null;
  }
}
