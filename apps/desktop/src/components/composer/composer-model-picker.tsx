import {
  memo,
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { Check, ChevronDown, ChevronRight, Sparkles, Zap } from "lucide-react";
import {
  DEFAULT_CUSTOM_THINKING_OPTIONS,
  PROVIDERS,
  modeLabel,
} from "@/components/composer/composer-constants";
import {
  formatCatalogModelLabel,
  formatEffortLabel,
  getCatalogForProvider,
  groupAntigravityOrchestratorModels,
  modelSupportsFastMode,
  prettifyModelSlug,
  resolveActiveModelSpec,
  resolveSupportedEffortForModel,
  stripAgyEffortSuffix,
  type BrowsedModelEntry,
  type BrowsedModelLeaf,
} from "@/lib/model-catalog";
import {
  type AcpAgentAvailability,
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
import { useI18n } from "@/lib/i18n";
import { cn } from "@/lib/utils";

const EMPTY_PROVIDERS: readonly ProviderKind[] = [];
const EMPTY_CATALOGS: readonly ProviderModelCatalog[] = [];
const EMPTY_PROFILES: readonly AgentProfile[] = [];
const EMPTY_ACP_AGENTS: readonly AcpAgentAvailability[] = [];

const HOVER_INTENT_MS = 90;

/**
 * Isolated Agent + Model + Thinking Mode Selector (`[ProviderIcon Model · Effort ⚡ ▾]`).
 * - Never subscribes to `composerDrafts`, and packs active-agent config into 1 primitive selector
 *   so keystrokes and unrelated background heartbeats trigger zero re-renders.
 * - Auto-focuses active model row on open, supports hover-intent (`90ms`), full keyboard navigation
 *   (`ArrowUp`/`ArrowDown`/`ArrowRight`/`ArrowLeft`/`Esc`), localized effort descriptions,
 *   context window hints, and manual model catalog refresh.
 */
export const ComposerModelPicker = memo(function ComposerModelPicker({
  disabled = false,
  disabledTitle,
}: {
  disabled?: boolean;
  disabledTitle?: string;
} = {}) {
  const { t } = useI18n();
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedAgentId = useOrbitSelector((s) => s.selectedAgentId);
  const isDraft = useOrbitSelector((s) => s.isDraftNewSession);
  const draftProvider = useOrbitSelector((s) => s.draftProvider);
  const draftAcpAgentId = useOrbitSelector((s) => s.draftAcpAgentId);
  const draftModels = useOrbitSelector((s) => s.draftModels);
  const draftEffort = useOrbitSelector((s) => s.draftEffort);
  const draftFastMode = useOrbitSelector((s) => s.draftFastMode);
  const draftProfileId = useOrbitSelector((s) => s.draftProfileId);

  const profiles = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.hostDetails[s.selectedHostId]?.profiles ?? EMPTY_PROFILES)
      : EMPTY_PROFILES,
  );
  const hostAcpAgents = useOrbitSelector((s) =>
    s.selectedHostId
      ? (s.hostDetails[s.selectedHostId]?.acp_agents ?? EMPTY_ACP_AGENTS)
      : EMPTY_ACP_AGENTS,
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

  // Single primitive-serialized selector replaces repeated `.agents.find(...)` scans
  const activeAgentConfigKey = useOrbitSelector((s): string => {
    if (s.isDraftNewSession || !s.selectedHostId || !s.selectedAgentId) return "";
    const ag = s.hostDetails[s.selectedHostId]?.agents.find(
      (a) => a.agent_id === s.selectedAgentId,
    );
    if (!ag) return "";
    return `${ag.provider}\0${ag.acp_agent_id ?? ""}\0${ag.model ?? ""}\0${ag.effort ?? ""}\0${ag.fast_mode ? "1" : "0"}`;
  });

  const {
    activeAgentProvider,
    activeAgentAcpId,
    activeAgentModel,
    activeAgentEffort,
    activeAgentFastMode,
  } = useMemo(() => {
    if (!activeAgentConfigKey) {
      return {
        activeAgentProvider: null as ProviderKind | null,
        activeAgentAcpId: null as string | null,
        activeAgentModel: null as string | null,
        activeAgentEffort: null as ReasoningEffort | null,
        activeAgentFastMode: false,
      };
    }
    const [prov, acpId, model, effort, fast] = activeAgentConfigKey.split("\0");
    return {
      activeAgentProvider: (prov || null) as ProviderKind | null,
      activeAgentAcpId: acpId || null,
      activeAgentModel: model || null,
      activeAgentEffort: (effort || null) as ReasoningEffort | null,
      activeAgentFastMode: fast === "1",
    };
  }, [activeAgentConfigKey]);

  const modelPopoverRef = useRef<HTMLDivElement | null>(null);
  const modelListRef = useRef<HTMLDivElement | null>(null);
  const rowRefs = useRef<Map<string, HTMLButtonElement | null>>(new Map());
  const hoveredRowElRef = useRef<HTMLButtonElement | null>(null);
  const thinkingSubmenuRef = useRef<HTMLDivElement | null>(null);
  const subRowRefs = useRef<Map<string, HTMLButtonElement | null>>(new Map());
  const hoveredSubElRef = useRef<HTMLButtonElement | null>(null);
  const level3SubmenuRef = useRef<HTMLDivElement | null>(null);
  const hoverIntentTimerRef = useRef<number | null>(null);
  const subHoverIntentTimerRef = useRef<number | null>(null);

  const [modelPopoverOpen, setModelPopoverOpen] = useState(false);
  const [browsedProvider, setBrowsedProvider] = useState<ProviderKind>("codex");
  const [hoveredModelKey, setHoveredModelKey] = useState<string | null>(null);
  const [hoveredSubKey, setHoveredSubKey] = useState<string | null>(null);
  const [submenuPos, setSubmenuPos] = useState<{ top: number; flipLeft: boolean }>({
    top: -1,
    flipLeft: false,
  });
  const [level3Pos, setLevel3Pos] = useState<{ top: number; flipLeft: boolean }>({
    top: 0,
    flipLeft: false,
  });

  const clearHoverIntent = useCallback(() => {
    if (hoverIntentTimerRef.current !== null) {
      window.clearTimeout(hoverIntentTimerRef.current);
      hoverIntentTimerRef.current = null;
    }
    if (subHoverIntentTimerRef.current !== null) {
      window.clearTimeout(subHoverIntentTimerRef.current);
      subHoverIntentTimerRef.current = null;
    }
  }, []);

  useEffect(() => clearHoverIntent, [clearHoverIntent]);

  const scheduleFlyoutRow = useCallback(
    (rowKey: string | null, rowEl: HTMLButtonElement | null, immediate = false) => {
      clearHoverIntent();
      if (immediate) {
        hoveredRowElRef.current = rowEl;
        setHoveredModelKey(rowKey);
        setHoveredSubKey(null);
        hoveredSubElRef.current = null;
        return;
      }
      hoverIntentTimerRef.current = window.setTimeout(() => {
        hoverIntentTimerRef.current = null;
        hoveredRowElRef.current = rowEl;
        setHoveredModelKey(rowKey);
        setHoveredSubKey(null);
        hoveredSubElRef.current = null;
      }, HOVER_INTENT_MS);
    },
    [clearHoverIntent],
  );

  const scheduleLevel3Row = useCallback(
    (subKey: string | null, subEl: HTMLButtonElement | null, immediate = false) => {
      if (subHoverIntentTimerRef.current !== null) {
        window.clearTimeout(subHoverIntentTimerRef.current);
        subHoverIntentTimerRef.current = null;
      }
      if (immediate) {
        hoveredSubElRef.current = subEl;
        setHoveredSubKey(subKey);
        return;
      }
      subHoverIntentTimerRef.current = window.setTimeout(() => {
        subHoverIntentTimerRef.current = null;
        hoveredSubElRef.current = subEl;
        setHoveredSubKey(subKey);
      }, HOVER_INTENT_MS);
    },
    [],
  );

  const detectedProviders = useMemo(
    () => PROVIDERS.filter((p) => availableProviders.includes(p.id)),
    [availableProviders],
  );
  const installedAcpAgents = useMemo(
    () => hostAcpAgents.filter((a) => a.installed && a.spec.enabled),
    [hostAcpAgents],
  );
  const hasInstalledAcp = installedAcpAgents.length > 0;

  const resolvedDraftProvider: ProviderKind =
    draftProvider === "acp" && hasInstalledAcp
      ? "acp"
      : detectedProviders.length > 0 && !detectedProviders.some((p) => p.id === draftProvider)
        ? detectedProviders[0].id
        : detectedProviders.length === 0 && hasInstalledAcp
          ? "acp"
          : draftProvider;

  const canSwitchAgentEngine = isDraft || activeAgentProvider === null;
  const activeProvider: ProviderKind =
    !canSwitchAgentEngine && activeAgentProvider ? activeAgentProvider : resolvedDraftProvider;
  const effectiveAcpAgentId: string | null =
    activeProvider === "acp"
      ? !canSwitchAgentEngine
        ? activeAgentAcpId
        : (draftAcpAgentId && installedAcpAgents.some((a) => a.spec.id === draftAcpAgentId)
            ? draftAcpAgentId
            : (installedAcpAgents[0]?.spec.id ?? draftAcpAgentId))
      : null;
  const activeAcpSpec = useMemo(
    () =>
      effectiveAcpAgentId
        ? (hostAcpAgents.find((a) => a.spec.id === effectiveAcpAgentId)?.spec ?? null)
        : null,
    [hostAcpAgents, effectiveAcpAgentId],
  );

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
  const effectiveActiveEffort: ReasoningEffort | null =
    activeProvider === "acp"
      ? null
      : resolveSupportedEffortForModel(
          activeModelSpec,
          rawDesiredEffort,
          activeCatalog.configured_default_effort,
        );

  const handleSelectModelAndEffort = useCallback(
    (
      provider: ProviderKind,
      model: string | null,
      effort: ReasoningEffort | null,
      fastMode?: boolean,
    ) => {
      if (canSwitchAgentEngine) {
        orbitActions.setDraftConfig({
          provider,
          model,
          ...(effort ? { effort } : {}),
          ...(fastMode !== undefined ? { fastMode } : {}),
        });
        return;
      }
      if (selectedAgentId && selectedHostId && activeAgentProvider) {
        orbitActions.setDraftConfig({
          provider: activeAgentProvider,
          model,
          ...(effort ? { effort } : {}),
          ...(fastMode !== undefined ? { fastMode } : {}),
        });
        void orbitIpc
          .setAgentConfig(selectedHostId, selectedAgentId, model, effort)
          .catch((err) => {
            orbitActions.showToast(t("composer.modelConfigFailed", { error: String(err) }));
          });
        if (fastMode === true && !activeAgentFastMode) {
          void orbitIpc
            .updateAgent({
              hostId: selectedHostId,
              agentId: selectedAgentId,
              fastMode: true,
            })
            .catch((err) => {
              orbitActions.showToast(t("composer.modelConfigFailed", { error: String(err) }));
            });
        } else if (fastMode === false && activeAgentFastMode) {
          void orbitIpc
            .updateAgent({
              hostId: selectedHostId,
              agentId: selectedAgentId,
              fastMode: false,
            })
            .catch((err) => {
              orbitActions.showToast(t("composer.modelConfigFailed", { error: String(err) }));
            });
        }
      }
    },
    [
      canSwitchAgentEngine,
      selectedAgentId,
      selectedHostId,
      activeAgentProvider,
      activeAgentFastMode,
      t,
    ],
  );

  const rawBrowsedProvider: ProviderKind = canSwitchAgentEngine
    ? modelPopoverOpen
      ? browsedProvider
      : activeProvider
    : activeProvider;
  const effectiveBrowsedProvider: ProviderKind =
    canSwitchAgentEngine && rawBrowsedProvider === "acp" && hasInstalledAcp
      ? "acp"
      : canSwitchAgentEngine &&
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

  const browsedRows: BrowsedModelEntry[] = useMemo(() => {
    if (!modelPopoverOpen) return [];
    const baseModels =
      browsedCatalog.models.length > 0
        ? browsedCatalog.models
        : [
            {
              id: "default",
              label: t("common.default"),
              description: t("composer.providerDefaultModel"),
              is_default: true,
              default_effort: null,
              thinking_options: [],
              supports_fast_mode: false,
            },
          ];
    const leaves: BrowsedModelLeaf[] = [
      ...(isCustomModelActiveOnBrowsed && rawActiveModel
        ? [
            {
              key: `custom:${rawActiveModel}`,
              id: rawActiveModel,
              label: prettifyModelSlug(rawActiveModel, effectiveBrowsedProvider),
              desc: rawActiveModel,
              defaultEffort: "medium" as ReasoningEffort,
              thinkingOptions: DEFAULT_CUSTOM_THINKING_OPTIONS,
              supportsFastMode: false,
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
        supportsFastMode: modelSupportsFastMode(effectiveBrowsedProvider, m),
      })),
    ];
    return groupAntigravityOrchestratorModels(effectiveBrowsedProvider, leaves);
  }, [
    modelPopoverOpen,
    isCustomModelActiveOnBrowsed,
    rawActiveModel,
    effectiveBrowsedProvider,
    browsedCatalog,
    t,
  ]);

  const hoveredRow = useMemo(
    () =>
      hoveredModelKey ? (browsedRows.find((r) => r.key === hoveredModelKey) ?? null) : null,
    [hoveredModelKey, browsedRows],
  );

  const hoveredSubModel = useMemo(
    () =>
      hoveredRow?.kind === "group" && hoveredSubKey
        ? (hoveredRow.children.find((c) => c.key === hoveredSubKey) ?? null)
        : null,
    [hoveredRow, hoveredSubKey],
  );

  // Auto-focus active model row (or its parent group, or first row) when opening Popover so arrow keys work immediately
  useEffect(() => {
    if (!modelPopoverOpen || effectiveBrowsedProvider === "acp" || browsedRows.length === 0) {
      return;
    }
    const raf = window.requestAnimationFrame(() => {
      const loweredActive = resolvedSelectedModelIdOnActive?.toLowerCase();
      const activeRow =
        effectiveBrowsedProvider === activeProvider && loweredActive
          ? browsedRows.find((r) =>
              r.kind === "group"
                ? r.children.some((c) => c.id.toLowerCase() === loweredActive)
                : r.id.toLowerCase() === loweredActive,
            )
          : null;
      const targetKey = activeRow?.key ?? browsedRows[0]?.key;
      if (targetKey) {
        rowRefs.current.get(targetKey)?.focus({ preventScroll: true });
      }
    });
    return () => window.cancelAnimationFrame(raf);
    // Focus only when opening or switching browsed provider tab
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [modelPopoverOpen, effectiveBrowsedProvider]);

  useLayoutEffect(() => {
    if (
      !modelPopoverOpen ||
      !hoveredRow ||
      (hoveredRow.kind === "model" &&
        hoveredRow.thinkingOptions.length === 0 &&
        !hoveredRow.supportsFastMode)
    ) {
      return;
    }
    const popEl = modelPopoverRef.current;
    const rowEl = hoveredRowElRef.current ?? rowRefs.current.get(hoveredRow.key) ?? null;
    const subEl = thinkingSubmenuRef.current;
    if (!popEl || !rowEl || !subEl) return;

    const popRect = popEl.getBoundingClientRect();
    const rowRect = rowEl.getBoundingClientRect();
    const originY = popRect.top + popEl.clientTop;
    const subHeight = subEl.offsetHeight;
    const subWidth = subEl.offsetWidth;
    const edgeMargin = 12;

    const desiredTop = rowRect.top - originY - 5;
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

  useLayoutEffect(() => {
    if (
      !modelPopoverOpen ||
      !hoveredSubModel ||
      (hoveredSubModel.thinkingOptions.length === 0 && !hoveredSubModel.supportsFastMode)
    ) {
      return;
    }
    const subContainer = thinkingSubmenuRef.current;
    const subRowEl = hoveredSubElRef.current ?? subRowRefs.current.get(hoveredSubModel.key) ?? null;
    const l3El = level3SubmenuRef.current;
    if (!subContainer || !subRowEl || !l3El) return;

    const subRect = subContainer.getBoundingClientRect();
    const rowRect = subRowEl.getBoundingClientRect();
    const originY = subRect.top;
    const l3Height = l3El.offsetHeight;
    const l3Width = l3El.offsetWidth;
    const edgeMargin = 12;

    const desiredTop = rowRect.top - originY - 5;
    const maxTopRelative = window.innerHeight - edgeMargin - originY - l3Height;
    const minTopRelative = edgeMargin - originY;
    const clampedTop = Math.round(
      Math.max(minTopRelative, Math.min(desiredTop, maxTopRelative)),
    );

    const flipLeft =
      submenuPos.flipLeft ||
      (subRect.right + l3Width + 8 > window.innerWidth - edgeMargin &&
        subRect.left - l3Width - 8 >= edgeMargin);

    setLevel3Pos((prev) =>
      prev.top === clampedTop && prev.flipLeft === flipLeft
        ? prev
        : { top: clampedTop, flipLeft },
    );
  }, [modelPopoverOpen, hoveredSubModel, submenuPos.flipLeft]);

  const activeDraftProfile = useMemo(
    () =>
      canSwitchAgentEngine && draftProfileId
        ? (profiles.find((p) => p.profile_id === draftProfileId) ?? null)
        : null,
    [canSwitchAgentEngine, draftProfileId, profiles],
  );

  const railProviderCount = detectedProviders.length + (hasInstalledAcp ? 1 : 0);
  const showAgentRail = canSwitchAgentEngine && railProviderCount > 1;
  const compactAgentRail = railProviderCount > 5;
  const rawFormattedModelLabel = formatCatalogModelLabel(activeCatalog, rawActiveModel);
  const activeTriggerLabel = activeDraftProfile
    ? activeDraftProfile.name
    : activeProvider === "acp"
      ? (activeAcpSpec?.name ?? effectiveAcpAgentId ?? t("composer.acpAgentFallback"))
      : rawFormattedModelLabel === "Default"
        ? t("common.default")
        : rawFormattedModelLabel;

  const isFastActive = Boolean(
    modelSupportsFastMode(activeProvider, activeModelSpec) &&
      (!canSwitchAgentEngine ? activeAgentFastMode : draftFastMode),
  );
  const activeEffortLabel =
    !activeDraftProfile && effectiveActiveEffort
      ? formatEffortLabel(effectiveActiveEffort, t)
      : null;

  const triggerTooltipLabel = isFastActive
    ? t("composer.fastModeTooltip", {
        label: `${activeTriggerLabel}${activeEffortLabel ? ` · ${activeEffortLabel}` : ""}`,
      })
    : activeEffortLabel
      ? `${activeTriggerLabel} · ${activeEffortLabel}`
      : activeTriggerLabel;

  const focusFlyoutFirstItem = useCallback(() => {
    window.requestAnimationFrame(() => {
      const firstBtn = thinkingSubmenuRef.current?.querySelector<HTMLButtonElement>("button");
      firstBtn?.focus();
    });
  }, []);

  const focusLevel3FirstItem = useCallback(() => {
    window.requestAnimationFrame(() => {
      const firstBtn = level3SubmenuRef.current?.querySelector<HTMLButtonElement>("button");
      firstBtn?.focus();
    });
  }, []);

  if (disabled) {
    return (
      <SimpleTooltip label={disabledTitle ?? triggerTooltipLabel} side="top">
        <span className="inline-flex max-w-76 shrink-0">
          <button
            type="button"
            disabled
            className="inline-flex h-8.5 max-w-76 cursor-not-allowed items-center gap-1.5 rounded-full px-2.5 text-[13px] opacity-65"
          >
            <ProviderIcon provider={activeProvider} className="size-4 shrink-0" />
            <span className="text-ink-primary truncate font-normal opacity-90">
              {activeTriggerLabel}
            </span>
            {activeEffortLabel && (
              <>
                <span aria-hidden="true" className="text-ink-faint text-[11px]">
                  ·
                </span>
                <span className="text-ink-muted shrink-0 text-[13px] font-normal">
                  {activeEffortLabel}
                </span>
              </>
            )}
            {isFastActive && (
              <Zap className="text-signal-info size-3 shrink-0 stroke-[2.1]" aria-hidden="true" />
            )}
            <ChevronDown className="text-ink-faint size-3.5 shrink-0 stroke-[1.75]" />
          </button>
        </span>
      </SimpleTooltip>
    );
  }

  return (
    <Popover
      open={modelPopoverOpen}
      onOpenChange={(open) => {
        clearHoverIntent();
        setModelPopoverOpen(open);
        if (open) {
          setBrowsedProvider(activeProvider);
          setHoveredModelKey(null);
          setHoveredSubKey(null);
          if (selectedHostId && providerCatalogs.length === 0) {
            void orbitIpc.refreshProviderModels(selectedHostId, false);
          }
        }
      }}
    >
      <SimpleTooltip label={triggerTooltipLabel} side="top">
        <PopoverTrigger asChild>
          <button
            type="button"
            className="hover:text-ink-primary hover:bg-accent inline-flex h-8.5 max-w-76 cursor-pointer items-center gap-1.5 rounded-full px-2.5 text-[13px] transition-colors"
          >
            <ProviderIcon provider={activeProvider} className="size-4 shrink-0" />
            <span className="text-ink-primary truncate font-normal opacity-90">
              {activeTriggerLabel}
            </span>
            {activeEffortLabel && (
              <>
                <span aria-hidden="true" className="text-ink-faint text-[11px]">
                  ·
                </span>
                <span className="text-ink-muted shrink-0 text-[13px] font-normal">
                  {activeEffortLabel}
                </span>
              </>
            )}
            {isFastActive && (
              <Zap className="text-signal-info size-3 shrink-0 stroke-[2.1]" aria-hidden="true" />
            )}
            <ChevronDown className="text-ink-faint size-3.5 shrink-0 stroke-[1.75]" />
          </button>
        </PopoverTrigger>
      </SimpleTooltip>

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
              onMouseEnter={() => scheduleFlyoutRow(null, null, true)}
              className={cn(
                "orbit-native-scroll text-popover-foreground floating-surface border-border-default flex max-h-[min(420px,calc(var(--available-height,540px)-8px))] flex-col items-center gap-0.5 overflow-y-auto overscroll-contain rounded-[11px] border p-1",
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
                        scheduleFlyoutRow(null, null, true);
                        orbitActions.setDraftConfig({
                          provider: p.id,
                          ...(p.id === "antigravity" ? { fastMode: false } : {}),
                        });
                      }}
                      className={cn(
                        "flex shrink-0 cursor-pointer items-center justify-center rounded-[7px] transition-colors duration-150",
                        compactAgentRail ? "size-[26px]" : "size-[28px]",
                        isRailSelected
                          ? "bg-accent"
                          : "hover:bg-accent/70 opacity-85 hover:opacity-100",
                      )}
                    >
                      <ProviderIcon
                        provider={p.id}
                        className={compactAgentRail ? "size-[14px]" : "size-[15px]"}
                      />
                    </button>
                  </SimpleTooltip>
                );
              })}
              {hasInstalledAcp && (
                <>
                  {detectedProviders.length > 0 && (
                    <div className="bg-border-subtle my-0.5 h-px w-4 shrink-0" />
                  )}
                  <SimpleTooltip label={t("composer.acpAgents")} side="left">
                    <button
                      type="button"
                      aria-label={t("composer.acpAgents")}
                      onClick={() => {
                        setBrowsedProvider("acp");
                        scheduleFlyoutRow(null, null, true);
                        const fallbackId =
                          draftAcpAgentId &&
                          installedAcpAgents.some((a) => a.spec.id === draftAcpAgentId)
                            ? draftAcpAgentId
                            : (installedAcpAgents[0]?.spec.id ?? null);
                        orbitActions.setDraftConfig({
                          provider: "acp",
                          acpAgentId: fallbackId,
                          fastMode: false,
                        });
                      }}
                      className={cn(
                        "flex shrink-0 cursor-pointer items-center justify-center rounded-[7px] transition-colors duration-150",
                        compactAgentRail ? "size-[26px]" : "size-[28px]",
                        effectiveBrowsedProvider === "acp"
                          ? "bg-accent"
                          : "hover:bg-accent/70 opacity-85 hover:opacity-100",
                      )}
                    >
                      <ProviderIcon provider="acp" className="size-[15px]" />
                    </button>
                  </SimpleTooltip>
                </>
              )}
            </div>
          )}

          {/* Compact Model Menu Card — shadcn-scale density, Level-2/3 cascading flyouts anchored flush */}
          <div
            ref={modelPopoverRef}
            className="text-popover-foreground floating-surface border-border-default relative w-[226px] rounded-[11px] border p-1"
          >
            <div
              onMouseEnter={() => scheduleFlyoutRow(null, null, true)}
              className="text-ink-muted flex h-[24px] items-center gap-1.5 px-2 text-[11px] font-medium tracking-[0.01em]"
            >
              {!showAgentRail && (
                <ProviderIcon provider={effectiveBrowsedProvider} className="size-3.5 shrink-0" />
              )}
              <span className="truncate">
                {effectiveBrowsedProvider === "acp"
                  ? t("composer.acpAgents")
                  : (PROVIDERS.find((p) => p.id === effectiveBrowsedProvider)?.label ??
                    effectiveBrowsedProvider)}
              </span>
            </div>

            <ScrollArea
              className="-mx-1 -mb-1"
              viewportRef={modelListRef}
              onViewportScroll={() => scheduleFlyoutRow(null, null, true)}
              viewportClassName="max-h-[min(400px,calc(var(--available-height,540px)-40px))] px-1 pb-1"
            >
              {canSwitchAgentEngine && profiles.length > 0 && effectiveBrowsedProvider !== "acp" && (
                <div className="border-border-subtle mb-1 flex flex-col gap-0.5 border-b pb-1">
                  <div className="text-ink-muted flex items-center gap-1 px-2 py-0.5 text-[11px] font-medium uppercase">
                    <Sparkles className="size-3" />
                    <span>{t("composer.profiles")}</span>
                  </div>
                  {profiles.map((prof) => {
                    const isActiveProf = draftProfileId === prof.profile_id;
                    const profMeta = [
                      prof.model ? prettifyModelSlug(prof.model, prof.provider) : null,
                      modeLabel(prof.mode, t),
                    ]
                      .filter(Boolean)
                      .join(" · ");
                    return (
                      <button
                        key={prof.profile_id}
                        type="button"
                        onMouseEnter={() => scheduleFlyoutRow(null, null, true)}
                        onClick={() => {
                          orbitActions.setDraftConfig({
                            provider: prof.provider,
                            acpAgentId: prof.acp_agent_id ?? null,
                            model: prof.model,
                            ...(prof.effort ? { effort: prof.effort } : {}),
                            fastMode: Boolean(prof.fast_mode),
                            mode: prof.mode,
                            profileId: prof.profile_id,
                          });
                          setModelPopoverOpen(false);
                        }}
                        className={cn(
                          "flex w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2 py-1 text-left transition-colors",
                          isActiveProf
                            ? "bg-accent text-ink-primary"
                            : "text-ink-secondary hover:bg-accent/70 hover:text-ink-primary",
                        )}
                      >
                        <span className="flex min-w-0 items-center gap-2">
                          <ProviderIcon provider={prof.provider} className="size-3.5 shrink-0" />
                          <span className="flex min-w-0 flex-col">
                            <span
                              className={cn(
                                "truncate text-[13px] leading-tight",
                                isActiveProf ? "font-medium" : "font-normal",
                              )}
                            >
                              {prof.name}
                            </span>
                            {profMeta && (
                              <span className="text-ink-faint truncate text-[11px] leading-tight">
                                {profMeta}
                              </span>
                            )}
                          </span>
                        </span>
                        {isActiveProf && <Check className="size-3.5 shrink-0 opacity-75" />}
                      </button>
                    );
                  })}
                </div>
              )}

              {effectiveBrowsedProvider === "acp" ? (
                <div className="flex flex-col gap-0.5">
                  {canSwitchAgentEngine ? (
                    <>
                      {installedAcpAgents.map((item) => {
                        const isSelectedAcp =
                          activeProvider === "acp" && effectiveAcpAgentId === item.spec.id;
                        return (
                          <button
                            key={item.spec.id}
                            type="button"
                            onMouseEnter={() => scheduleFlyoutRow(null, null, true)}
                            onClick={() => {
                              orbitActions.setDraftConfig({
                                provider: "acp",
                                acpAgentId: item.spec.id,
                                model: null,
                                fastMode: false,
                              });
                              setModelPopoverOpen(false);
                            }}
                            className={cn(
                              "flex h-[28px] w-full cursor-pointer items-center justify-between gap-2 rounded-[7px] px-2 text-left transition-colors",
                              isSelectedAcp
                                ? "bg-accent text-ink-primary"
                                : "hover:bg-accent/65",
                            )}
                          >
                            <div className="flex min-w-0 items-center gap-1.5">
                              <span
                                className={cn(
                                  "truncate text-[13px]",
                                  isSelectedAcp
                                    ? "text-ink-primary font-medium"
                                    : "text-ink-secondary font-normal",
                                )}
                              >
                                {item.spec.name}
                              </span>
                              {item.spec.source === "custom" && (
                                <span className="text-ink-muted shrink-0 text-[11px]">
                                  {t("common.custom")}
                                </span>
                              )}
                            </div>
                            {isSelectedAcp && (
                              <Check className="text-ink-primary size-3.5 shrink-0 stroke-[1.9]" />
                            )}
                          </button>
                        );
                      })}
                      {selectedHostId && (
                        <div className="border-border-subtle mt-1 border-t pt-1">
                          <button
                            type="button"
                            onClick={() => {
                              setModelPopoverOpen(false);
                              orbitActions.openSettings(`host:${selectedHostId}:acp`);
                            }}
                            className="text-ink-muted hover:text-ink-primary hover:bg-accent/65 flex h-[26px] w-full cursor-pointer items-center rounded-[7px] px-2 text-left text-[13px] transition-colors"
                          >
                            {t("composer.manageAcpAgents")}
                          </button>
                        </div>
                      )}
                    </>
                  ) : (
                    <>
                      <div className="text-ink-primary flex items-center justify-between gap-2 px-2 py-1 text-[13px] font-medium">
                        <span className="truncate">
                          {activeAcpSpec?.name ??
                            effectiveAcpAgentId ??
                            t("composer.acpAgentFallback")}
                        </span>
                        {rawActiveModel && (
                          <span className="text-ink-muted shrink-0 font-mono text-[11px] font-normal">
                            {rawActiveModel}
                          </span>
                        )}
                      </div>
                      {selectedHostId && (
                        <div className="border-border-subtle mt-1 border-t pt-1">
                          <button
                            type="button"
                            onClick={() => {
                              setModelPopoverOpen(false);
                              orbitActions.openSettings(`host:${selectedHostId}:acp`);
                            }}
                            className="text-ink-muted hover:text-ink-primary hover:bg-accent/65 flex h-[26px] w-full cursor-pointer items-center rounded-[7px] px-2 text-left text-[13px] transition-colors"
                          >
                            {t("composer.manageAcpAgents")}
                          </button>
                        </div>
                      )}
                    </>
                  )}
                </div>
              ) : (
                <div className="flex flex-col gap-0.5">
                  {browsedRows.map((row, rowIdx) => {
                    const loweredActive = resolvedSelectedModelIdOnActive?.toLowerCase();
                    const activeGroupChild =
                      row.kind === "group" && effectiveBrowsedProvider === activeProvider && loweredActive
                        ? (row.children.find((c) => c.id.toLowerCase() === loweredActive) ?? null)
                        : null;
                    const isModelActive =
                      row.kind === "group"
                        ? activeGroupChild !== null
                        : effectiveBrowsedProvider === activeProvider &&
                          loweredActive === row.id.toLowerCase();
                    const isSubOpen = hoveredModelKey === row.key;
                    const hasFlyout =
                      row.kind === "group"
                        ? row.children.length > 0
                        : row.thinkingOptions.length > 0 || row.supportsFastMode;

                    const primaryTargetLeaf: BrowsedModelLeaf | null =
                      row.kind === "group" ? (activeGroupChild ?? row.children[0] ?? null) : row;
                    const rowEffort: ReasoningEffort | null =
                      primaryTargetLeaf && primaryTargetLeaf.thinkingOptions.length > 0
                        ? isModelActive
                          ? effectiveActiveEffort
                          : (primaryTargetLeaf.defaultEffort ??
                            primaryTargetLeaf.thinkingOptions[0]?.effort ??
                            null)
                        : null;
                    const rowSecondary =
                      row.kind === "group"
                        ? [
                            primaryTargetLeaf?.label ?? null,
                            rowEffort ? formatEffortLabel(rowEffort, t) : null,
                          ]
                            .filter(Boolean)
                            .join(" · ")
                        : [
                            rowEffort ? formatEffortLabel(rowEffort, t) : null,
                            isModelActive && isFastActive ? t("composer.fast") : null,
                          ]
                            .filter(Boolean)
                            .join(" · ");

                    return (
                      <div
                        key={row.key}
                        className={cn(
                          "group/model flex h-[28px] w-full items-center rounded-[7px] transition-colors",
                          isSubOpen
                            ? "bg-accent"
                            : isModelActive
                              ? "bg-accent/65 hover:bg-accent"
                              : "hover:bg-accent/65",
                        )}
                      >
                        <button
                          ref={(el) => {
                            rowRefs.current.set(row.key, el);
                          }}
                          type="button"
                          title={row.kind === "model" ? row.desc || undefined : undefined}
                          onMouseEnter={(e) => {
                            if (hasFlyout) {
                              scheduleFlyoutRow(row.key, e.currentTarget, hoveredModelKey === null);
                            } else {
                              scheduleFlyoutRow(null, null, false);
                            }
                          }}
                          onKeyDown={(e) => {
                            if (e.key === "ArrowDown") {
                              e.preventDefault();
                              const nextRow = browsedRows[(rowIdx + 1) % browsedRows.length];
                              if (nextRow) rowRefs.current.get(nextRow.key)?.focus();
                            } else if (e.key === "ArrowUp") {
                              e.preventDefault();
                              const prevRow =
                                browsedRows[(rowIdx - 1 + browsedRows.length) % browsedRows.length];
                              if (prevRow) rowRefs.current.get(prevRow.key)?.focus();
                            } else if (e.key === "ArrowRight" && hasFlyout) {
                              e.preventDefault();
                              scheduleFlyoutRow(row.key, e.currentTarget, true);
                              focusFlyoutFirstItem();
                            }
                          }}
                          onClick={() => {
                            if (!primaryTargetLeaf) return;
                            handleSelectModelAndEffort(
                              effectiveBrowsedProvider,
                              primaryTargetLeaf.id,
                              rowEffort,
                              primaryTargetLeaf.supportsFastMode ? undefined : false,
                            );
                            setModelPopoverOpen(false);
                          }}
                          className="focus-visible:ring-ring/40 flex h-full min-w-0 flex-1 cursor-pointer items-center justify-between gap-1.5 rounded-[7px] pr-0.5 pl-2 text-left outline-none focus-visible:ring-1"
                        >
                          <div className="flex min-w-0 items-baseline gap-1.5">
                            <span
                              className={cn(
                                "shrink-0 text-[13px] leading-normal",
                                row.kind === "model" && "truncate",
                                isModelActive
                                  ? "text-ink-primary font-medium"
                                  : "text-ink-secondary group-hover/model:text-ink-primary font-normal",
                              )}
                            >
                              {row.label}
                            </span>
                            {rowSecondary && (
                              <span className="text-ink-muted min-w-0 truncate text-[11px] leading-normal font-normal">
                                {rowSecondary}
                              </span>
                            )}
                          </div>
                        </button>

                        {hasFlyout ? (
                          <button
                            type="button"
                            aria-label={t("composer.thinkingOptionsAria", { model: row.label })}
                            aria-expanded={isSubOpen}
                            onMouseEnter={() => {
                              const rowEl = rowRefs.current.get(row.key) ?? null;
                              scheduleFlyoutRow(row.key, rowEl, true);
                            }}
                            onClick={(e) => {
                              e.stopPropagation();
                              const rowEl = rowRefs.current.get(row.key) ?? null;
                              if (isSubOpen) {
                                scheduleFlyoutRow(null, null, true);
                              } else {
                                scheduleFlyoutRow(row.key, rowEl, true);
                                focusFlyoutFirstItem();
                              }
                            }}
                            className="text-ink-muted group-hover/model:text-ink-primary mr-0.5 inline-flex size-[22px] shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                          >
                            {isModelActive ? (
                              <span className="relative inline-flex size-3.5 items-center justify-center">
                                <Check
                                  className={cn(
                                    "text-ink-primary size-3.5 stroke-[1.9] transition-opacity duration-120",
                                    isSubOpen
                                      ? "opacity-0"
                                      : "opacity-100 group-hover/model:opacity-0",
                                  )}
                                />
                                <ChevronRight
                                  className={cn(
                                    "absolute inset-0 size-3.5 stroke-[1.75] transition-opacity duration-120",
                                    isSubOpen
                                      ? "opacity-100"
                                      : "opacity-0 group-hover/model:opacity-100",
                                  )}
                                />
                              </span>
                            ) : (
                              <ChevronRight className="size-3.5 shrink-0 stroke-[1.75]" />
                            )}
                          </button>
                        ) : isModelActive ? (
                          <span className="mr-0.5 inline-flex size-[22px] shrink-0 items-center justify-center">
                            <Check className="text-ink-primary size-3.5 shrink-0 stroke-[1.9]" />
                          </span>
                        ) : null}
                      </div>
                    );
                  })}
                </div>
              )}
            </ScrollArea>

            {/* Zero-Footprint Overlay Anchor for Compact Level-2 (Sub-Models or Thinking/Fast) & Level-3 Flyouts */}
            <div className="pointer-events-none absolute inset-0 overflow-visible">
              {hoveredRow &&
                (hoveredRow.kind === "group"
                  ? hoveredRow.children.length > 0
                  : hoveredRow.thinkingOptions.length > 0 || hoveredRow.supportsFastMode) && (
                  <div
                    ref={thinkingSubmenuRef}
                    style={{ top: `${submenuPos.top}px` }}
                    onMouseEnter={() => {
                      if (hoverIntentTimerRef.current !== null) {
                        window.clearTimeout(hoverIntentTimerRef.current);
                        hoverIntentTimerRef.current = null;
                      }
                    }}
                    onKeyDown={(e) => {
                      if (e.key === "ArrowLeft" || e.key === "Escape") {
                        e.preventDefault();
                        e.stopPropagation();
                        const keyToFocus = hoveredRow.key;
                        scheduleFlyoutRow(null, null, true);
                        rowRefs.current.get(keyToFocus)?.focus();
                        return;
                      }
                      if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                        const buttons = Array.from(
                          thinkingSubmenuRef.current?.querySelectorAll<HTMLButtonElement>(
                            "[data-l2-item='true']:not(:disabled)",
                          ) ?? [],
                        );
                        if (buttons.length === 0) return;
                        e.preventDefault();
                        e.stopPropagation();
                        const currentIdx = buttons.indexOf(
                          document.activeElement as HTMLButtonElement,
                        );
                        const delta = e.key === "ArrowDown" ? 1 : -1;
                        const nextIdx =
                          currentIdx < 0
                            ? 0
                            : (currentIdx + delta + buttons.length) % buttons.length;
                        buttons[nextIdx]?.focus();
                      }
                    }}
                    className={cn(
                      "pointer-events-auto absolute z-50",
                      submenuPos.flipLeft
                        ? showAgentRail
                          ? compactAgentRail
                            ? "right-[calc(100%+40px)] pr-1.5"
                            : "right-[calc(100%+44px)] pr-1.5"
                          : "right-full pr-1.5"
                        : "left-full pl-1.5",
                    )}
                  >
                    {hoveredRow.kind === "group" ? (
                      <div
                        data-side={submenuPos.flipLeft ? "left" : "right"}
                        className="text-popover-foreground floating-surface orbit-inline-flyout-side border-border-default relative flex w-[188px] flex-col gap-0.5 rounded-[11px] border p-1"
                      >
                        {hoveredRow.children.map((child) => {
                          const isChildActive =
                            effectiveBrowsedProvider === activeProvider &&
                            resolvedSelectedModelIdOnActive?.toLowerCase() ===
                              child.id.toLowerCase();
                          const isL3Open = hoveredSubKey === child.key;
                          const childHasFlyout =
                            child.thinkingOptions.length > 0 || child.supportsFastMode;
                          const childEffort: ReasoningEffort | null =
                            child.thinkingOptions.length > 0
                              ? isChildActive
                                ? effectiveActiveEffort
                                : (child.defaultEffort ?? child.thinkingOptions[0]?.effort ?? null)
                              : null;
                          const childEffortLabel = childEffort
                            ? formatEffortLabel(childEffort, t)
                            : null;

                          return (
                            <div
                              key={child.key}
                              className={cn(
                                "group/submodel flex h-[28px] w-full items-center rounded-[7px] transition-colors",
                                isL3Open
                                  ? "bg-accent"
                                  : isChildActive
                                    ? "bg-accent/65 hover:bg-accent"
                                    : "hover:bg-accent/65",
                              )}
                            >
                              <button
                                ref={(el) => {
                                  subRowRefs.current.set(child.key, el);
                                }}
                                data-l2-item="true"
                                type="button"
                                onMouseEnter={(e) => {
                                  if (childHasFlyout) {
                                    scheduleLevel3Row(
                                      child.key,
                                      e.currentTarget,
                                      hoveredSubKey === null,
                                    );
                                  } else {
                                    scheduleLevel3Row(null, null, false);
                                  }
                                }}
                                onKeyDown={(e) => {
                                  if (e.key === "ArrowRight" && childHasFlyout) {
                                    e.preventDefault();
                                    e.stopPropagation();
                                    scheduleLevel3Row(child.key, e.currentTarget, true);
                                    focusLevel3FirstItem();
                                  }
                                }}
                                onClick={(e) => {
                                  e.stopPropagation();
                                  handleSelectModelAndEffort(
                                    effectiveBrowsedProvider,
                                    child.id,
                                    childEffort,
                                    child.supportsFastMode ? undefined : false,
                                  );
                                  setModelPopoverOpen(false);
                                }}
                                className="focus-visible:ring-ring/40 flex h-full min-w-0 flex-1 cursor-pointer items-center justify-between gap-1.5 rounded-[7px] pr-0.5 pl-2 text-left outline-none focus-visible:ring-1"
                              >
                                <div className="flex min-w-0 items-baseline gap-1.5">
                                  <span
                                    className={cn(
                                      "truncate text-[13px] leading-normal",
                                      isChildActive
                                        ? "text-ink-primary font-medium"
                                        : "text-ink-secondary group-hover/submodel:text-ink-primary font-normal",
                                    )}
                                  >
                                    {child.label}
                                  </span>
                                  {childEffortLabel && (
                                    <span className="text-ink-muted shrink-0 text-[11px] leading-normal font-normal">
                                      {childEffortLabel}
                                    </span>
                                  )}
                                </div>
                              </button>

                              {childHasFlyout ? (
                                <button
                                  type="button"
                                  aria-label={t("composer.thinkingOptionsAria", {
                                    model: child.label,
                                  })}
                                  aria-expanded={isL3Open}
                                  onMouseEnter={() => {
                                    const el = subRowRefs.current.get(child.key) ?? null;
                                    scheduleLevel3Row(child.key, el, true);
                                  }}
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    const el = subRowRefs.current.get(child.key) ?? null;
                                    if (isL3Open) {
                                      scheduleLevel3Row(null, null, true);
                                    } else {
                                      scheduleLevel3Row(child.key, el, true);
                                      focusLevel3FirstItem();
                                    }
                                  }}
                                  className="text-ink-muted group-hover/submodel:text-ink-primary mr-0.5 inline-flex size-[22px] shrink-0 cursor-pointer items-center justify-center rounded-[5px] transition-colors"
                                >
                                  {isChildActive ? (
                                    <span className="relative inline-flex size-3.5 items-center justify-center">
                                      <Check
                                        className={cn(
                                          "text-ink-primary size-3.5 stroke-[1.9] transition-opacity duration-120",
                                          isL3Open
                                            ? "opacity-0"
                                            : "opacity-100 group-hover/submodel:opacity-0",
                                        )}
                                      />
                                      <ChevronRight
                                        className={cn(
                                          "absolute inset-0 size-3.5 stroke-[1.75] transition-opacity duration-120",
                                          isL3Open
                                            ? "opacity-100"
                                            : "opacity-0 group-hover/submodel:opacity-100",
                                        )}
                                      />
                                    </span>
                                  ) : (
                                    <ChevronRight className="size-3.5 shrink-0 stroke-[1.75]" />
                                  )}
                                </button>
                              ) : isChildActive ? (
                                <span className="mr-0.5 inline-flex size-[22px] shrink-0 items-center justify-center">
                                  <Check className="text-ink-primary size-3.5 shrink-0 stroke-[1.9]" />
                                </span>
                              ) : null}
                            </div>
                          );
                        })}

                        {/* Level-3 Thinking Flyout for Orchestrator sub-models when advertised */}
                        {hoveredSubModel &&
                          (hoveredSubModel.thinkingOptions.length > 0 ||
                            hoveredSubModel.supportsFastMode) && (
                            <div
                              ref={level3SubmenuRef}
                              style={{ top: `${level3Pos.top}px` }}
                              onMouseEnter={() => {
                                if (subHoverIntentTimerRef.current !== null) {
                                  window.clearTimeout(subHoverIntentTimerRef.current);
                                  subHoverIntentTimerRef.current = null;
                                }
                              }}
                              onKeyDown={(e) => {
                                if (e.key === "ArrowLeft" || e.key === "Escape") {
                                  e.preventDefault();
                                  e.stopPropagation();
                                  const keyToFocus = hoveredSubModel.key;
                                  scheduleLevel3Row(null, null, true);
                                  subRowRefs.current.get(keyToFocus)?.focus();
                                  return;
                                }
                                if (e.key === "ArrowDown" || e.key === "ArrowUp") {
                                  const buttons = Array.from(
                                    level3SubmenuRef.current?.querySelectorAll<HTMLButtonElement>(
                                      "button:not(:disabled)",
                                    ) ?? [],
                                  );
                                  if (buttons.length === 0) return;
                                  e.preventDefault();
                                  e.stopPropagation();
                                  const currentIdx = buttons.indexOf(
                                    document.activeElement as HTMLButtonElement,
                                  );
                                  const delta = e.key === "ArrowDown" ? 1 : -1;
                                  const nextIdx =
                                    currentIdx < 0
                                      ? 0
                                      : (currentIdx + delta + buttons.length) % buttons.length;
                                  buttons[nextIdx]?.focus();
                                }
                              }}
                              className={cn(
                                "pointer-events-auto absolute z-50",
                                level3Pos.flipLeft ? "right-full pr-1.5" : "left-full pl-1.5",
                              )}
                            >
                              <div
                                data-side={level3Pos.flipLeft ? "left" : "right"}
                                className="text-popover-foreground floating-surface orbit-inline-flyout-side border-border-default flex w-max min-w-[132px] max-w-[184px] flex-col gap-0.5 rounded-[11px] border p-1"
                              >
                                {hoveredSubModel.thinkingOptions.map((opt) => {
                                  const isHoveredSubActive =
                                    effectiveBrowsedProvider === activeProvider &&
                                    resolvedSelectedModelIdOnActive?.toLowerCase() ===
                                      hoveredSubModel.id.toLowerCase();
                                  const isEffortSelected = isHoveredSubActive
                                    ? opt.effort === effectiveActiveEffort
                                    : opt.effort === hoveredSubModel.defaultEffort;
                                  const isSelected = isEffortSelected && isHoveredSubActive;
                                  return (
                                    <button
                                      key={opt.effort}
                                      type="button"
                                      onClick={(e) => {
                                        e.stopPropagation();
                                        handleSelectModelAndEffort(
                                          effectiveBrowsedProvider,
                                          hoveredSubModel.id,
                                          opt.effort,
                                          hoveredSubModel.supportsFastMode ? undefined : false,
                                        );
                                        setModelPopoverOpen(false);
                                      }}
                                      className={cn(
                                        "focus-visible:ring-ring/40 flex h-[28px] w-full cursor-pointer items-center justify-between gap-3 rounded-[7px] px-2 text-left outline-none transition-colors focus-visible:ring-1",
                                        isSelected
                                          ? "bg-accent text-ink-primary"
                                          : "hover:bg-accent/65",
                                      )}
                                    >
                                      <span
                                        className={cn(
                                          "truncate text-[13px] leading-normal",
                                          isSelected
                                            ? "text-ink-primary font-medium"
                                            : "text-ink-secondary font-normal",
                                        )}
                                      >
                                        {formatEffortLabel(opt.effort, t)}
                                      </span>
                                      {isSelected && (
                                        <Check className="text-ink-primary size-3.5 shrink-0 stroke-[1.9]" />
                                      )}
                                    </button>
                                  );
                                })}
                              </div>
                            </div>
                          )}
                      </div>
                    ) : (
                      <div
                        data-side={submenuPos.flipLeft ? "left" : "right"}
                        className="text-popover-foreground floating-surface orbit-inline-flyout-side border-border-default flex w-max min-w-[132px] max-w-[184px] flex-col gap-0.5 rounded-[11px] border p-1"
                      >
                        {hoveredRow.thinkingOptions.map((opt) => {
                          const isHoveredModelActive =
                            effectiveBrowsedProvider === activeProvider &&
                            resolvedSelectedModelIdOnActive?.toLowerCase() ===
                              hoveredRow.id.toLowerCase();
                          const isEffortSelected = isHoveredModelActive
                            ? opt.effort === effectiveActiveEffort
                            : opt.effort === hoveredRow.defaultEffort;
                          const isSelected = isEffortSelected && isHoveredModelActive;
                          const effortLabel = formatEffortLabel(opt.effort, t);
                          return (
                            <button
                              key={opt.effort}
                              data-l2-item="true"
                              type="button"
                              onClick={(e) => {
                                e.stopPropagation();
                                handleSelectModelAndEffort(
                                  effectiveBrowsedProvider,
                                  hoveredRow.id,
                                  opt.effort,
                                  hoveredRow.supportsFastMode ? undefined : false,
                                );
                                setModelPopoverOpen(false);
                              }}
                              className={cn(
                                "focus-visible:ring-ring/40 flex h-[28px] w-full cursor-pointer items-center justify-between gap-3 rounded-[7px] px-2 text-left outline-none transition-colors focus-visible:ring-1",
                                isSelected
                                  ? "bg-accent text-ink-primary"
                                  : "hover:bg-accent/65",
                              )}
                            >
                              <span
                                className={cn(
                                  "truncate text-[13px] leading-normal",
                                  isSelected
                                    ? "text-ink-primary font-medium"
                                    : "text-ink-secondary font-normal",
                                )}
                              >
                                {effortLabel}
                              </span>
                              {isSelected && (
                                <Check className="text-ink-primary size-3.5 shrink-0 stroke-[1.9]" />
                              )}
                            </button>
                          );
                        })}

                        {hoveredRow.supportsFastMode && (() => {
                          const isHoveredModelActive =
                            effectiveBrowsedProvider === activeProvider &&
                            resolvedSelectedModelIdOnActive?.toLowerCase() ===
                              hoveredRow.id.toLowerCase();
                          const fastChecked = isHoveredModelActive && isFastActive;
                          const targetEffort: ReasoningEffort | null =
                            hoveredRow.thinkingOptions.length > 0
                              ? isHoveredModelActive
                                ? effectiveActiveEffort
                                : (hoveredRow.defaultEffort ??
                                  hoveredRow.thinkingOptions[0]?.effort ??
                                  null)
                              : null;
                          return (
                            <>
                              {hoveredRow.thinkingOptions.length > 0 && (
                                <div className="bg-border-subtle my-0.5 h-px" />
                              )}
                              <SimpleTooltip label={t("composer.fastModeDesc")} side="right">
                                <button
                                  data-l2-item="true"
                                  type="button"
                                  onClick={(e) => {
                                    e.stopPropagation();
                                    const nextFast = !fastChecked;
                                    if (isHoveredModelActive) {
                                      orbitActions.toggleActiveFastMode(nextFast);
                                    } else {
                                      handleSelectModelAndEffort(
                                        effectiveBrowsedProvider,
                                        hoveredRow.id,
                                        targetEffort,
                                        nextFast,
                                      );
                                    }
                                    setModelPopoverOpen(false);
                                  }}
                                  className={cn(
                                    "focus-visible:ring-ring/40 flex h-[28px] w-full cursor-pointer items-center justify-between gap-3 rounded-[7px] px-2 text-left outline-none transition-colors focus-visible:ring-1",
                                    fastChecked
                                      ? "bg-accent text-ink-primary"
                                      : "hover:bg-accent/65",
                                  )}
                                >
                                  <span
                                    className={cn(
                                      "inline-flex items-center gap-1.5 truncate text-[13px] leading-normal",
                                      fastChecked
                                        ? "text-ink-primary font-medium"
                                        : "text-ink-secondary font-normal",
                                    )}
                                  >
                                    <Zap
                                      className={cn(
                                        "size-3 shrink-0 stroke-[2]",
                                        fastChecked ? "text-signal-info" : "text-ink-muted",
                                      )}
                                    />
                                    <span>{t("composer.fast")}</span>
                                  </span>
                                  <span
                                    aria-hidden="true"
                                    className={cn(
                                      "inline-flex h-[14px] w-[24px] shrink-0 items-center rounded-full p-[2px] transition-colors",
                                      fastChecked
                                        ? "bg-action-primary-bg"
                                        : "bg-border-strong",
                                    )}
                                  >
                                    <span
                                      className={cn(
                                        "bg-surface-base size-[10px] rounded-full shadow-2xs transition-transform duration-150",
                                        fastChecked ? "translate-x-[10px]" : "translate-x-0",
                                      )}
                                    />
                                  </span>
                                </button>
                              </SimpleTooltip>
                            </>
                          );
                        })()}
                      </div>
                    )}
                  </div>
                )}
            </div>
          </div>
        </div>
      </PopoverContent>
    </Popover>
  );
});
