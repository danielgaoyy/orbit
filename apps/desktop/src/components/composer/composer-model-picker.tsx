import { memo, useCallback, useLayoutEffect, useMemo, useRef, useState } from "react";
import { Check, ChevronDown, ChevronRight, Sparkles } from "lucide-react";
import {
  DEFAULT_CUSTOM_THINKING_OPTIONS,
  PROVIDERS,
} from "@/components/composer/composer-constants";
import {
  formatCatalogModelLabel,
  formatEffortLabel,
  getCatalogForProvider,
  prettifyModelSlug,
  resolveActiveModelSpec,
  resolveSupportedEffortForModel,
  stripAgyEffortSuffix,
} from "@/lib/model-catalog";
import {
  type AgentProfile,
  type ProviderKind,
  type ProviderModelCatalog,
  type ReasoningEffort,
  orbitIpc,
} from "@/lib/orbit-ipc";
import { ProviderIcon } from "@/components/ui/provider-icons";
import { ScrollArea } from "@/components/ui/scroll-area";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
  SimpleTooltip,
} from "@/components/ui/overlays";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_PROVIDERS: readonly ProviderKind[] = [];
const EMPTY_CATALOGS: readonly ProviderModelCatalog[] = [];
const EMPTY_PROFILES: readonly AgentProfile[] = [];

/**
 * Isolated Agent + Model + Thinking Mode Selector (`[ProviderIcon Model Effort ▾]`).
 * Never subscribes to `composerDrafts`, and subscribes narrowly to provider/catalog slices
 * so keystrokes and unrelated background ticks trigger zero re-renders.
 */
export const ComposerModelPicker = memo(function ComposerModelPicker({
  disabled = false,
  disabledTitle,
}: {
  disabled?: boolean;
  disabledTitle?: string;
} = {}) {
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);
  const draftModels = useOrbitSelector((s) => s.draftModels);
  const draftEffort = useOrbitSelector((s) => s.draftEffort);
  const draftProfileId = useOrbitSelector((s) => s.draftProfileId);

  const profiles = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.hostDetails[s.selectedHostId]?.profiles ?? EMPTY_PROFILES)
      : EMPTY_PROFILES,
  );

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
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    return ag?.provider ?? null;
  });
  const activeAgentModel = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return null;
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    return ag?.model ?? null;
  });
  const activeAgentEffort = useOrbitSelector((s) => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return null;
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    return ag?.effort ?? null;
  });

  const modelPopoverRef = useRef<HTMLDivElement | null>(null);
  const modelListRef = useRef<HTMLDivElement | null>(null);
  const hoveredRowElRef = useRef<HTMLButtonElement | null>(null);
  const thinkingSubmenuRef = useRef<HTMLDivElement | null>(null);

  const [modelPopoverOpen, setModelPopoverOpen] = useState(false);
  const [browsedProvider, setBrowsedProvider] = useState<ProviderKind>("codex");
  const [hoveredModelKey, setHoveredModelKey] = useState<string | null>(null);
  const [submenuPos, setSubmenuPos] = useState<{ top: number; flipLeft: boolean }>({
    top: -1,
    flipLeft: false,
  });

  const detectedProviders = useMemo(
    () => PROVIDERS.filter((p) => availableProviders.includes(p.id)),
    [availableProviders],
  );
  const resolvedDraftProvider: ProviderKind =
    detectedProviders.length > 0 && !detectedProviders.some((p) => p.id === draftProvider)
      ? detectedProviders[0].id
      : draftProvider;

  const canSwitchAgentEngine = isDraft || activeAgentProvider === null;
  const activeProvider: ProviderKind =
    !canSwitchAgentEngine && activeAgentProvider ? activeAgentProvider : resolvedDraftProvider;

  const catalogsArray = providerCatalogs as ProviderModelCatalog[];
  const activeCatalog = useMemo(
    () => getCatalogForProvider(catalogsArray, activeProvider),
    [catalogsArray, activeProvider],
  );
  const rawActiveModel: string | null = !canSwitchAgentEngine
    ? activeAgentModel
    : (draftModels[activeProvider] ?? null);
  const activeModelSpec = useMemo(
    () => resolveActiveModelSpec(activeCatalog, rawActiveModel),
    [activeCatalog, rawActiveModel],
  );

  const inferredAgyEffort =
    activeProvider === "antigravity" && rawActiveModel
      ? stripAgyEffortSuffix(rawActiveModel).inferredEffort
      : null;
  const rawDesiredEffort: ReasoningEffort | null = !canSwitchAgentEngine
    ? (activeAgentEffort ?? inferredAgyEffort ?? draftEffort)
    : draftEffort;
  const effectiveActiveEffort: ReasoningEffort | null = resolveSupportedEffortForModel(
    activeModelSpec,
    rawDesiredEffort,
    activeCatalog.configured_default_effort,
  );

  const handleSelectModelAndEffort = useCallback(
    (
      provider: ProviderKind,
      model: string | null,
      effort: ReasoningEffort | null,
    ) => {
      if (canSwitchAgentEngine) {
        orbitActions.setDraftConfig({
          provider,
          model,
          ...(effort ? { effort } : {}),
        });
        return;
      }
      if (selectedAgentId && selectedHostId && activeAgentProvider) {
        orbitActions.setDraftConfig({
          provider: activeAgentProvider,
          model,
          ...(effort ? { effort } : {}),
        });
        void orbitIpc.setAgentConfig(selectedHostId, selectedAgentId, model, effort);
      }
    },
    [canSwitchAgentEngine, selectedAgentId, selectedHostId, activeAgentProvider],
  );

  const rawBrowsedProvider: ProviderKind = canSwitchAgentEngine
    ? modelPopoverOpen
      ? browsedProvider
      : activeProvider
    : activeProvider;
  const effectiveBrowsedProvider: ProviderKind =
    canSwitchAgentEngine &&
    detectedProviders.length > 0 &&
    !detectedProviders.some((p) => p.id === rawBrowsedProvider)
      ? detectedProviders[0].id
      : rawBrowsedProvider;

  const browsedCatalog = useMemo(
    () => getCatalogForProvider(catalogsArray, effectiveBrowsedProvider),
    [catalogsArray, effectiveBrowsedProvider],
  );
  const resolvedSelectedModelIdOnActive = activeModelSpec?.id ?? rawActiveModel;
  const isCustomModelActiveOnBrowsed =
    effectiveBrowsedProvider === activeProvider &&
    rawActiveModel !== null &&
    activeModelSpec === null;

  const browsedRows = useMemo(() => {
    if (!modelPopoverOpen) return [];
    const baseModels =
      browsedCatalog.models.length > 0
        ? browsedCatalog.models
        : [
            {
              id: "default",
              label: "Default",
              description: "Provider default model",
              is_default: true,
              default_effort: null,
              thinking_options: [],
            },
          ];
    return [
      ...(isCustomModelActiveOnBrowsed && rawActiveModel
        ? [
            {
              key: `custom:${rawActiveModel}`,
              id: rawActiveModel,
              label: prettifyModelSlug(rawActiveModel, effectiveBrowsedProvider),
              desc: rawActiveModel,
              defaultEffort: "medium" as ReasoningEffort,
              thinkingOptions: DEFAULT_CUSTOM_THINKING_OPTIONS,
            },
          ]
        : []),
      ...baseModels.map((m) => ({
        key: m.id,
        id: m.id,
        label:
          effectiveBrowsedProvider === "claude_code"
            ? m.label.replace(/^Claude\s+/i, "")
            : m.label,
        desc: m.description,
        defaultEffort: m.default_effort,
        thinkingOptions: m.thinking_options,
      })),
    ];
  }, [
    modelPopoverOpen,
    isCustomModelActiveOnBrowsed,
    rawActiveModel,
    effectiveBrowsedProvider,
    browsedCatalog,
  ]);

  const hoveredRow = useMemo(
    () =>
      hoveredModelKey ? (browsedRows.find((r) => r.key === hoveredModelKey) ?? null) : null,
    [hoveredModelKey, browsedRows],
  );

  useLayoutEffect(() => {
    if (!modelPopoverOpen || !hoveredRow || hoveredRow.thinkingOptions.length === 0) return;
    const popEl = modelPopoverRef.current;
    const rowEl = hoveredRowElRef.current;
    const subEl = thinkingSubmenuRef.current;
    if (!popEl || !rowEl || !subEl) return;

    const popRect = popEl.getBoundingClientRect();
    const rowRect = rowEl.getBoundingClientRect();
    const originY = popRect.top + popEl.clientTop;
    const subHeight = subEl.offsetHeight;
    const subWidth = subEl.offsetWidth;
    const edgeMargin = 12;

    const desiredTop = rowRect.top - originY - 7;
    const maxTopRelative = window.innerHeight - edgeMargin - originY - subHeight;
    const minTopRelative = edgeMargin - originY;
    const clampedTop = Math.round(
      Math.max(minTopRelative, Math.min(desiredTop, maxTopRelative)),
    );

    const flipLeft =
      popRect.right + subWidth + 8 > window.innerWidth - edgeMargin &&
      popRect.left - subWidth - 8 >= edgeMargin;

    setSubmenuPos((prev) =>
      prev.top === clampedTop && prev.flipLeft === flipLeft
        ? prev
        : { top: clampedTop, flipLeft },
    );
  }, [modelPopoverOpen, hoveredRow]);

  const activeDraftProfile = useMemo(
    () =>
      canSwitchAgentEngine && draftProfileId
        ? (profiles.find((p) => p.profile_id === draftProfileId) ?? null)
        : null,
    [canSwitchAgentEngine, draftProfileId, profiles],
  );

  const showAgentRail = canSwitchAgentEngine && detectedProviders.length > 1;
  const compactAgentRail = detectedProviders.length > 5;

  if (disabled) {
    return (
      <button
        type="button"
        disabled
        title={disabledTitle}
        className="inline-flex h-8.5 max-w-76 cursor-not-allowed items-center gap-1.5 rounded-full px-2.5 text-[13.5px] opacity-65"
      >
        <ProviderIcon provider={activeProvider} className="size-4 shrink-0" />
        <span className="text-foreground/88 truncate font-normal">
          {activeDraftProfile
            ? activeDraftProfile.name
            : formatCatalogModelLabel(activeCatalog, rawActiveModel)}
        </span>
        {!activeDraftProfile && effectiveActiveEffort && (
          <span className="text-foreground/48 shrink-0 font-normal">
            {formatEffortLabel(effectiveActiveEffort)}
          </span>
        )}
        <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
      </button>
    );
  }

  return (
    <Popover
      open={modelPopoverOpen}
      onOpenChange={(open) => {
        setModelPopoverOpen(open);
        if (open) {
          setBrowsedProvider(activeProvider);
          setHoveredModelKey(null);
          if (selectedHostId && providerCatalogs.length === 0) {
            void orbitIpc.refreshProviderModels(selectedHostId, false);
          }
        }
      }}
    >
      <PopoverTrigger asChild>
        <button
          type="button"
          className="hover:text-foreground inline-flex h-8.5 max-w-76 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13.5px] transition-colors hover:bg-black/[0.045] dark:hover:bg-white/[0.06]"
        >
          <ProviderIcon provider={activeProvider} className="size-4" />
          <span className="text-foreground/88 truncate font-normal">
            {activeDraftProfile
              ? activeDraftProfile.name
              : formatCatalogModelLabel(activeCatalog, rawActiveModel)}
          </span>
          {!activeDraftProfile && effectiveActiveEffort && (
            <span className="text-foreground/48 shrink-0 font-normal">
              {formatEffortLabel(effectiveActiveEffort)}
            </span>
          )}
          <ChevronDown className="text-foreground/45 size-3.5 shrink-0 stroke-[1.75]" />
        </button>
      </PopoverTrigger>

      <PopoverContent
        side="top"
        align="start"
        collisionPadding={12}
        initialFocus={false}
        finalFocus={false}
        className="w-auto overflow-visible border-0 bg-transparent p-0 !shadow-none"
      >
        <div className="flex items-start gap-1.5">
          {/* Detached Top-Aligned Left Agent Icon Capsule — wraps installed providers tightly */}
          {showAgentRail && (
            <div
              onMouseEnter={() => setHoveredModelKey(null)}
              className={cn(
                "bg-popover text-popover-foreground floating-surface flex max-h-[min(420px,calc(var(--available-height,540px)-8px))] flex-col items-center overflow-y-auto rounded-[14px] border border-black/[0.09] dark:border-white/[0.10]",
                compactAgentRail ? "gap-0.5 p-1" : "gap-1 p-1.5",
              )}
            >
              {detectedProviders.map((p) => {
                const isRailSelected = p.id === effectiveBrowsedProvider;
                return (
                  <SimpleTooltip key={p.id} label={p.label} side="left">
                    <button
                      type="button"
                      aria-label={p.label}
                      onClick={() => {
                        setBrowsedProvider(p.id);
                        setHoveredModelKey(null);
                        orbitActions.setDraftConfig({ provider: p.id });
                      }}
                      className={cn(
                        "flex shrink-0 cursor-pointer items-center justify-center rounded-[9px] transition-colors duration-150",
                        compactAgentRail ? "size-[28px]" : "size-[32px]",
                        isRailSelected
                          ? "bg-black/[0.07] dark:bg-white/[0.11]"
                          : "opacity-85 hover:bg-black/[0.045] hover:opacity-100 dark:hover:bg-white/[0.06]",
                      )}
                    >
                      <ProviderIcon
                        provider={p.id}
                        className={compactAgentRail ? "size-[15px]" : "size-[16px]"}
                      />
                    </button>
                  </SimpleTooltip>
                );
              })}
            </div>
          )}

          {/* Pure Right Model Card — unified surface, crisp hierarchy, Level-2 Thinking Mode anchored flush */}
          <div
            ref={modelPopoverRef}
            className="bg-popover text-popover-foreground floating-surface relative w-[264px] rounded-[14px] border border-black/[0.09] p-1.5 dark:border-white/[0.10]"
          >
            <div
              onMouseEnter={() => setHoveredModelKey(null)}
              className="text-muted-foreground/75 flex h-[24px] items-center gap-1.5 px-2.5 pt-0.5 pb-1 text-[11px] font-medium tracking-[0.01em]"
            >
              {!showAgentRail && (
                <ProviderIcon provider={effectiveBrowsedProvider} className="size-3.5" />
              )}
              <span className="truncate">
                {PROVIDERS.find((p) => p.id === effectiveBrowsedProvider)?.label ??
                  effectiveBrowsedProvider}
              </span>
            </div>

            <ScrollArea
              className="-mx-1.5"
              viewportRef={modelListRef}
              onViewportScroll={() => setHoveredModelKey(null)}
              viewportClassName="max-h-[min(448px,calc(var(--available-height,540px)-42px))] px-1.5"
            >
              {canSwitchAgentEngine && profiles.length > 0 && (
                <div className="border-border mb-1.5 space-y-0.5 border-b pb-1.5">
                  <div className="text-muted-foreground/75 flex items-center gap-1 px-2.5 py-0.5 text-[10.5px] font-medium uppercase">
                    <Sparkles className="size-3" />
                    <span>Profiles</span>
                  </div>
                  {profiles.map((prof) => {
                    const isActiveProf = draftProfileId === prof.profile_id;
                    return (
                      <button
                        key={prof.profile_id}
                        type="button"
                        onMouseEnter={() => setHoveredModelKey(null)}
                        onClick={() => {
                          orbitActions.setDraftConfig({
                            provider: prof.provider,
                            model: prof.model,
                            ...(prof.effort ? { effort: prof.effort } : {}),
                            mode: prof.mode,
                            profileId: prof.profile_id,
                          });
                          setModelPopoverOpen(false);
                        }}
                        className={cn(
                          "flex w-full cursor-pointer items-center justify-between gap-2 rounded-[9px] px-2.5 py-1.5 text-left text-[13px] transition-colors",
                          isActiveProf
                            ? "bg-black/[0.06] font-medium dark:bg-white/[0.09]"
                            : "hover:bg-black/[0.04] dark:hover:bg-white/[0.06]",
                        )}
                      >
                        <span className="flex min-w-0 items-center gap-2">
                          <ProviderIcon provider={prof.provider} className="size-3.5 shrink-0" />
                          <span className="truncate">{prof.name}</span>
                        </span>
                        {isActiveProf && <Check className="size-3.5 shrink-0 opacity-75" />}
                      </button>
                    );
                  })}
                </div>
              )}

              <div className="space-y-0.5">
              {browsedRows.map((row) => {
                const isModelActive =
                  effectiveBrowsedProvider === activeProvider &&
                  resolvedSelectedModelIdOnActive?.toLowerCase() === row.id.toLowerCase();
                const isSubOpen = hoveredModelKey === row.key;
                const hasThinkingSubmenu = row.thinkingOptions.length > 0;
                const rowEffort: ReasoningEffort | null = hasThinkingSubmenu
                  ? isModelActive
                    ? effectiveActiveEffort
                    : (row.defaultEffort ?? row.thinkingOptions[0]?.effort ?? null)
                  : null;

                return (
                  <button
                    key={row.key}
                    type="button"
                    onMouseEnter={(e) => {
                      hoveredRowElRef.current = e.currentTarget;
                      setHoveredModelKey(row.key);
                    }}
                    onClick={() => {
                      handleSelectModelAndEffort(
                        effectiveBrowsedProvider,
                        row.id,
                        rowEffort,
                      );
                      setModelPopoverOpen(false);
                    }}
                    className={cn(
                      "group/model flex h-[29px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2.5 text-left transition-colors",
                      isSubOpen
                        ? "bg-accent/80"
                        : isModelActive
                          ? "bg-black/[0.04] hover:bg-accent/70 dark:bg-white/[0.06]"
                          : "hover:bg-accent/65",
                    )}
                  >
                    <div className="flex min-w-0 items-baseline gap-1.5">
                      <span
                        className={cn(
                          "truncate text-[13px] leading-normal",
                          isModelActive
                            ? "text-foreground font-medium"
                            : "text-foreground/84 group-hover/model:text-foreground font-normal",
                        )}
                      >
                        {row.label}
                      </span>
                      {rowEffort && (
                        <span className="text-muted-foreground/85 shrink-0 text-[11.5px] leading-normal font-normal">
                          {formatEffortLabel(rowEffort)}
                        </span>
                      )}
                    </div>

                    <div className="flex shrink-0 items-center">
                      {isModelActive ? (
                        <Check className="text-foreground size-3.5 shrink-0 stroke-[1.9]" />
                      ) : hasThinkingSubmenu ? (
                        <ChevronRight className="text-muted-foreground/70 size-3.5 shrink-0 stroke-[1.75]" />
                      ) : null}
                    </div>
                  </button>
                );
              })}
              </div>
            </ScrollArea>

            {/* Zero-Footprint Overlay Anchor for Level-2 Thinking Mode Flyout */}
            <div className="pointer-events-none absolute inset-0 overflow-visible">
              {hoveredRow && hoveredRow.thinkingOptions.length > 0 && (
                <div
                  ref={thinkingSubmenuRef}
                  style={{ top: `${submenuPos.top}px` }}
                  className={cn(
                    "pointer-events-auto absolute z-50",
                    submenuPos.flipLeft
                      ? showAgentRail
                        ? compactAgentRail
                          ? "right-[calc(100%+44px)] pr-1.5"
                          : "right-[calc(100%+52px)] pr-1.5"
                        : "right-full pr-1.5"
                      : "left-full pl-1.5",
                  )}
                >
                  <div className="bg-popover text-popover-foreground floating-surface orbit-inline-flyout-side w-[144px] space-y-0.5 rounded-[14px] border border-black/[0.09] p-1.5 dark:border-white/[0.10]">
                    {hoveredRow.thinkingOptions.map((opt) => {
                      const isHoveredModelActive =
                        effectiveBrowsedProvider === activeProvider &&
                        resolvedSelectedModelIdOnActive?.toLowerCase() ===
                          hoveredRow.id.toLowerCase();
                      const isEffortSelected = isHoveredModelActive
                        ? opt.effort === effectiveActiveEffort
                        : opt.effort === hoveredRow.defaultEffort;
                      const isSelected = isEffortSelected && isHoveredModelActive;
                      return (
                        <button
                          key={opt.effort}
                          type="button"
                          onClick={(e) => {
                            e.stopPropagation();
                            handleSelectModelAndEffort(
                              effectiveBrowsedProvider,
                              hoveredRow.id,
                              opt.effort,
                            );
                            setModelPopoverOpen(false);
                          }}
                          className={cn(
                            "flex h-[29px] w-full cursor-pointer items-center justify-between gap-2 rounded-[8px] px-2.5 text-left transition-colors",
                            isSelected
                              ? "bg-black/[0.04] hover:bg-accent/70 dark:bg-white/[0.06]"
                              : "hover:bg-accent/65",
                          )}
                        >
                          <span
                            className={cn(
                              "truncate text-[13px] leading-normal",
                              isSelected
                                ? "text-foreground font-medium"
                                : "text-foreground/84 font-normal",
                            )}
                          >
                            {opt.label}
                          </span>
                          {isSelected && (
                            <Check className="text-foreground size-3.5 shrink-0 stroke-[1.9]" />
                          )}
                        </button>
                      );
                    })}
                  </div>
                </div>
              )}
            </div>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
});
