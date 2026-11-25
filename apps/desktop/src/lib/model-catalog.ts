import {
  PROVIDER_LIST,
  type ProviderKind,
  type ProviderModelCatalog,
  type ProviderModelSpec,
  type ReasoningEffort,
} from "@/lib/orbit-ipc";

export {
  defaultEffortDescription,
  effortDescriptionKey,
  effortLabelKey,
  estimateContextWindowLabel,
  formatCatalogModelLabel,
  formatEffortLabel,
  formatProviderDisplayName,
  groupAntigravityOrchestratorModels,
  prettifyModelSlug,
  PROVIDER_DISPLAY_LABELS,
  resolveActiveModelSpec,
  stripAgyEffortSuffix,
  type BrowsedModelEntry,
  type BrowsedModelLeaf,
} from "./model-metadata";

function emptyCatalog(provider: ProviderKind): ProviderModelCatalog {
  return {
    provider,
    configured_default_model: null,
    configured_default_effort: null,
    models: [],
  };
}

const FALLBACK_CATALOG_CACHE: Record<ProviderKind, ProviderModelCatalog> = Object.fromEntries(
  [...PROVIDER_LIST.map((p) => p.id), "acp" as const].map((id) => [id, emptyCatalog(id)]),
) as Record<ProviderKind, ProviderModelCatalog>;

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
  return FALLBACK_CATALOG_CACHE[provider] ?? emptyCatalog(provider);
}

/**
 * Determines whether a model should expose Fast mode in the Desktop UI.
 * Explicitly disables Fast mode for `antigravity` and `acp` so Desktop never displays a
 * non-functional Fast toggle even when connected to older pre-v0.9.1 daemons.
 */
export function modelSupportsFastMode(
  provider: ProviderKind,
  spec: Pick<ProviderModelSpec, "supports_fast_mode"> | null | undefined,
): boolean {
  if (provider === "antigravity" || provider === "acp") {
    return false;
  }
  return Boolean(spec?.supports_fast_mode);
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
