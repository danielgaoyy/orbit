import { memo, useMemo } from "react";
import { Zap } from "lucide-react";
import { PROVIDERS } from "@/components/composer/composer-constants";
import { getCatalogForProvider, resolveActiveModelSpec } from "@/lib/model-catalog";
import type { ProviderKind, ProviderModelCatalog } from "@/lib/orbit-ipc";
import { SimpleTooltip } from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_PROVIDERS: readonly ProviderKind[] = [];
const EMPTY_CATALOGS: readonly ProviderModelCatalog[] = [];

/**
 * Subtle compact Fast Output Mode pill rendered beside Model / Effort ONLY when
 * the active provider/model advertises `supports_fast_mode === true`.
 */
export const ComposerFastToggle = memo(function ComposerFastToggle({
  iconOnly = false,
}: {
  iconOnly?: boolean;
}) {
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);
  const draftModels = useOrbitSelector((s) => s.draftModels);
  const draftFastMode = useOrbitSelector((s) => s.draftFastMode);

  const availableProviders = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.hostDetails[s.selectedHostId]?.host.available_providers ?? EMPTY_PROVIDERS)
      : EMPTY_PROVIDERS,
  );
  const providerCatalogs = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.hostDetails[s.selectedHostId]?.provider_catalogs ?? EMPTY_CATALOGS)
      : EMPTY_CATALOGS,
  );
  const activeAgentProvider = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return null;
    return (
      s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
        ?.provider ?? null
    );
  });
  const activeAgentModel = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return null;
    return (
      s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
        ?.model ?? null
    );
  });
  const activeAgentFastMode = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return false;
    return Boolean(
      s.hostDetails[s.selectedHostId]?.agents.find((a) => a.agent_id === s.selectedAgentId)
        ?.fast_mode,
    );
  });

  const detectedProviders = useMemo(
    () => PROVIDERS.filter((p) => availableProviders.includes(p.id)),
    [availableProviders],
  );
  const resolvedDraftProvider: ProviderKind =
    detectedProviders.length > 0 && !detectedProviders.some((p) => p.id === draftProvider)
      ? detectedProviders[0].id
      : draftProvider;

  const isDraftMode = isDraft || activeAgentProvider === null;
  const activeProvider: ProviderKind =
    !isDraftMode && activeAgentProvider ? activeAgentProvider : resolvedDraftProvider;
  const rawActiveModel: string | null = !isDraftMode
    ? activeAgentModel
    : (draftModels[activeProvider] ?? null);

  const activeCatalog = useMemo(
    () => getCatalogForProvider(providerCatalogs, activeProvider),
    [providerCatalogs, activeProvider],
  );
  const activeModelSpec = useMemo(
    () => resolveActiveModelSpec(activeCatalog, rawActiveModel),
    [activeCatalog, rawActiveModel],
  );

  if (!activeModelSpec?.supports_fast_mode) {
    return null;
  }

  const isFastActive = !isDraftMode ? activeAgentFastMode : draftFastMode;

  return (
    <SimpleTooltip label="Fast mode — lower latency, higher quota usage">
      <button
        type="button"
        aria-label="Fast mode"
        aria-pressed={isFastActive}
        onClick={() => orbitActions.toggleActiveFastMode()}
        className={cn(
          "inline-flex h-8.5 shrink-0 cursor-pointer items-center justify-center rounded-full border text-[13px] transition-colors",
          iconOnly ? "w-8.5 px-0" : "gap-1 px-2.5",
          isFastActive
            ? "border-amber-500/35 bg-amber-500/10 text-amber-700 hover:bg-amber-500/15 dark:border-amber-400/35 dark:bg-amber-400/12 dark:text-amber-300 dark:hover:bg-amber-400/18"
            : "text-foreground/62 hover:text-foreground border-transparent hover:bg-black/[0.045] dark:hover:bg-white/[0.06]",
        )}
      >
        <Zap
          className={cn(
            "size-3.5 shrink-0 stroke-[1.9]",
            isFastActive && "fill-current",
          )}
        />
        {!iconOnly && <span className="font-normal">Fast</span>}
      </button>
    </SimpleTooltip>
  );
});
