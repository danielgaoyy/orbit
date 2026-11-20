import type {
  ProviderKind,
  ProviderModelCatalog,
  ProviderModelSpec,
  ReasoningEffort,
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

const FALLBACK_CATALOG_CACHE: Record<ProviderKind, ProviderModelCatalog> = {
  codex: {
    provider: "codex",
    configured_default_model: null,
    configured_default_effort: null,
    models: [],
  },
  claude_code: {
    provider: "claude_code",
    configured_default_model: null,
    configured_default_effort: null,
    models: [],
  },
  antigravity: {
    provider: "antigravity",
    configured_default_model: null,
    configured_default_effort: null,
    models: [],
  },
};

export function seedFallbackProviderCatalogs(
  catalogs: readonly ProviderModelCatalog[],
): void {
  for (const cat of catalogs) {
    FALLBACK_CATALOG_CACHE[cat.provider] = cat;
  }
}

export function getCatalogForProvider(
  hostCatalogs: readonly ProviderModelCatalog[] | undefined,
  provider: ProviderKind,
): ProviderModelCatalog {
  const live = hostCatalogs?.find((c) => c.provider === provider);
  if (live && live.models.length > 0) {
    return live;
  }
  return FALLBACK_CATALOG_CACHE[provider];
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
