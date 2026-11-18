import type {
  ProviderKind,
  ProviderModelCatalog,
  ProviderModelSpec,
  ReasoningEffort,
  ThinkingOptionSpec,
} from "@/lib/orbit-ipc";

export function formatEffortLabel(effort: ReasoningEffort): string {
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

export function defaultEffortDescription(effort: ReasoningEffort): string {
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
      return "Extra high reasoning depth";
    case "max":
      return "Maximum reasoning budget";
    case "ultra":
      return "Max reasoning + delegation";
  }
}

function makeThinkingOptions(
  efforts: ReasoningEffort[],
  defaultEffort: ReasoningEffort | null,
): ThinkingOptionSpec[] {
  return efforts.map((effort) => ({
    effort,
    label: formatEffortLabel(effort),
    description: defaultEffortDescription(effort),
    is_default: effort === defaultEffort,
  }));
}

const CODEX_FULL_EFFORTS: ReasoningEffort[] = [
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
  "ultra",
];
const CODEX_LUNA_EFFORTS: ReasoningEffort[] = ["low", "medium", "high", "xhigh", "max"];
const CLAUDE_XHIGH_EFFORTS: ReasoningEffort[] = ["low", "medium", "high", "xhigh", "max"];
const CLAUDE_XHIGH_OFF_EFFORTS: ReasoningEffort[] = [
  "off",
  "low",
  "medium",
  "high",
  "xhigh",
  "max",
];
const CLAUDE_STD_OFF_EFFORTS: ReasoningEffort[] = ["off", "low", "medium", "high", "max"];
const AGY_STD_EFFORTS: ReasoningEffort[] = ["low", "medium", "high"];
const AGY_XHIGH_EFFORTS: ReasoningEffort[] = ["low", "medium", "high", "xhigh"];

export const FALLBACK_PROVIDER_CATALOGS: Record<ProviderKind, ProviderModelCatalog> = {
  codex: {
    provider: "codex",
    configured_default_model: "gpt-6.2-sol",
    configured_default_effort: "low",
    models: [
      {
        id: "gpt-6.2-sol",
        label: "GPT 6.2 Sol",
        description: "Fast, go-to model for coding and everyday work.",
        is_default: true,
        default_effort: "low",
        thinking_options: makeThinkingOptions(CODEX_FULL_EFFORTS, "low"),
      },
      {
        id: "gpt-6.2-astra",
        label: "GPT 6.2 Astra",
        description: "Most capable model for complex coding and professional work.",
        is_default: false,
        default_effort: "low",
        thinking_options: makeThinkingOptions(CODEX_FULL_EFFORTS, "low"),
      },
      {
        id: "gpt-6.1-terra",
        label: "GPT 6.1 Terra",
        description: "Efficient model that excels at visual understanding and computer use.",
        is_default: false,
        default_effort: "low",
        thinking_options: makeThinkingOptions(CODEX_FULL_EFFORTS, "low"),
      },
      {
        id: "gpt-6.1-sol",
        label: "GPT 6.1 Sol",
        description: "Previous generation model for coding and everyday work.",
        is_default: false,
        default_effort: "low",
        thinking_options: makeThinkingOptions(CODEX_FULL_EFFORTS, "low"),
      },
      {
        id: "gpt-6-astra",
        label: "GPT 6 Astra",
        description: "Previous generation model for complex coding and professional work.",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(CODEX_FULL_EFFORTS, "medium"),
      },
      {
        id: "gpt-6-sol",
        label: "GPT 6 Sol",
        description: "Legacy model for coding and everyday work.",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(CODEX_FULL_EFFORTS, "medium"),
      },
      {
        id: "gpt-6-luna",
        label: "GPT 6 Luna",
        description: "Fast and affordable model for easier tasks.",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(CODEX_LUNA_EFFORTS, "medium"),
      },
    ],
  },
  claude_code: {
    provider: "claude_code",
    configured_default_model: "claude-opus-5-5",
    configured_default_effort: "medium",
    models: [
      {
        id: "claude-opus-5-5",
        label: "Opus 5.5",
        description: "Opus 5.5 · Latest release",
        is_default: true,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(CLAUDE_XHIGH_EFFORTS, "medium"),
      },
      {
        id: "claude-fable-5-2",
        label: "Fable 5.2",
        description: "Fable 5.2 · Most powerful model",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(CLAUDE_XHIGH_EFFORTS, "medium"),
      },
      {
        id: "claude-sonnet-5-5",
        label: "Sonnet 5.5",
        description: "Sonnet 5.5 · Best for everyday tasks",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(CLAUDE_XHIGH_EFFORTS, "medium"),
      },
      {
        id: "claude-haiku-5-5",
        label: "Haiku 5.5",
        description: "Haiku 5.5 · Fastest for quick answers",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(CLAUDE_XHIGH_OFF_EFFORTS, "medium"),
      },
      {
        id: "claude-opus-5",
        label: "Opus 5",
        description: "Opus 5 · Previous release",
        is_default: false,
        default_effort: "high",
        thinking_options: makeThinkingOptions(CLAUDE_XHIGH_OFF_EFFORTS, "high"),
      },
      {
        id: "claude-sonnet-5",
        label: "Sonnet 5",
        description: "Sonnet 5 · Previous release",
        is_default: false,
        default_effort: "high",
        thinking_options: makeThinkingOptions(CLAUDE_XHIGH_OFF_EFFORTS, "high"),
      },
      {
        id: "claude-opus-4-8",
        label: "Opus 4.8",
        description: "Opus 4.8 · Previous release",
        is_default: false,
        default_effort: "high",
        thinking_options: makeThinkingOptions(CLAUDE_XHIGH_OFF_EFFORTS, "high"),
      },
      {
        id: "claude-opus-4-6",
        label: "Opus 4.6",
        description: "Opus 4.6 · Most capable for complex work",
        is_default: false,
        default_effort: "high",
        thinking_options: makeThinkingOptions(CLAUDE_STD_OFF_EFFORTS, "high"),
      },
      {
        id: "claude-sonnet-4-6",
        label: "Sonnet 4.6",
        description: "Sonnet 4.6 · Best for everyday tasks",
        is_default: false,
        default_effort: "high",
        thinking_options: makeThinkingOptions(CLAUDE_STD_OFF_EFFORTS, "high"),
      },
      {
        id: "claude-haiku-4-5",
        label: "Haiku 4.5",
        description: "Haiku 4.5 · Previous release",
        is_default: false,
        default_effort: null,
        thinking_options: [],
      },
    ],
  },
  antigravity: {
    provider: "antigravity",
    configured_default_model: "gemini-4-carbon",
    configured_default_effort: "medium",
    models: [
      {
        id: "gemini-4-carbon",
        label: "Gemini 4 Carbon",
        description: "Flagship reasoning & multi-file software engineering",
        is_default: true,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(AGY_STD_EFFORTS, "medium"),
      },
      {
        id: "gemini-4-argon",
        label: "Gemini 4 Argon",
        description: "Balanced coding, architecture & tool orchestration",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(AGY_STD_EFFORTS, "medium"),
      },
      {
        id: "gemini-4-hydrogen",
        label: "Gemini 4 Hydrogen",
        description: "Extended deep reasoning with Extra High budget",
        is_default: false,
        default_effort: "high",
        thinking_options: makeThinkingOptions(AGY_XHIGH_EFFORTS, "high"),
      },
      {
        id: "gemini-pro-orchestrator",
        label: "Orchestrator (Carbon)",
        description: "Multi-agent coordinator powered by Carbon",
        is_default: false,
        default_effort: null,
        thinking_options: [],
      },
      {
        id: "gemini-pro-orchestrator-argon",
        label: "Orchestrator (Argon)",
        description: "Multi-agent coordinator powered by Argon",
        is_default: false,
        default_effort: null,
        thinking_options: [],
      },
      {
        id: "gemini-3.8-flash",
        label: "Gemini 3.8 Flash",
        description: "High-speed interactive coding & rapid iteration",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(AGY_STD_EFFORTS, "medium"),
      },
      {
        id: "claude-opus-5-5",
        label: "Claude Opus 5.5",
        description: "Anthropic flagship model on Antigravity",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(AGY_STD_EFFORTS, "medium"),
      },
      {
        id: "claude-sonnet-5-5",
        label: "Claude Sonnet 5.5",
        description: "Fast everyday coding & refactoring",
        is_default: false,
        default_effort: "medium",
        thinking_options: makeThinkingOptions(AGY_STD_EFFORTS, "medium"),
      },
    ],
  },
};

export function getCatalogForProvider(
  hostCatalogs: readonly ProviderModelCatalog[] | undefined,
  provider: ProviderKind,
): ProviderModelCatalog {
  const live = hostCatalogs?.find((c) => c.provider === provider);
  if (live && live.models.length > 0) {
    return live;
  }
  return FALLBACK_PROVIDER_CATALOGS[provider];
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

export function resolveSupportedEffortForModel(
  spec: ProviderModelSpec | null,
  desiredEffort: ReasoningEffort | null,
  catalogDefault: ReasoningEffort | null,
): ReasoningEffort | null {
  if (spec && spec.thinking_options.length === 0) {
    return null;
  }
  if (!spec) {
    return desiredEffort ?? catalogDefault ?? "medium";
  }
  if (desiredEffort && spec.thinking_options.some((o) => o.effort === desiredEffort)) {
    return desiredEffort;
  }
  if (spec.default_effort && spec.thinking_options.some((o) => o.effort === spec.default_effort)) {
    return spec.default_effort;
  }
  if (catalogDefault && spec.thinking_options.some((o) => o.effort === catalogDefault)) {
    return catalogDefault;
  }
  return spec.thinking_options[0]?.effort ?? null;
}
