import { useEffect, useMemo, useState } from "react";
import { Loader2, Plus, RefreshCw, ShieldCheck, X } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { type MessageKey, useI18n } from "@/lib/i18n";
import type {
  GuardianAuditEntry,
  GuardianDecision,
  GuardianPolicyConfig,
  GuardianRiskLevel,
  ProviderKind,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_AUDITS: readonly GuardianAuditEntry[] = [];
const DEFAULT_LLM_TIMEOUT_MS = 8_000;
const PRESET_LLM_TIMEOUTS_MS = [4_000, 8_000, 12_000, 15_000, 20_000] as const;

type LlmProviderOptionValue = "auto" | "claude_code" | "codex";

function formatRiskBadge(
  level: GuardianRiskLevel,
  t: (key: MessageKey) => string,
): { label: string; className: string } {
  switch (level) {
    case "low":
      return {
        label: t("guardian.riskLow"),
        className: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
      };
    case "medium":
      return {
        label: t("guardian.riskMedium"),
        className: "bg-amber-500/10 text-amber-800 dark:text-amber-300",
      };
    case "high":
      return {
        label: t("guardian.riskHigh"),
        className: "bg-rose-500/10 text-rose-700 dark:text-rose-300",
      };
    case "critical":
      return {
        label: t("guardian.riskCritical"),
        className: "bg-rose-500/16 text-rose-800 dark:text-rose-200 font-semibold",
      };
  }
}

function formatDecisionLabel(
  decision: GuardianDecision,
  t: (key: MessageKey) => string,
): string {
  switch (decision) {
    case "auto_allow":
      return t("guardian.decisionAllowed");
    case "require_human":
      return t("guardian.decisionEscalated");
    case "auto_deny":
      return t("guardian.decisionDenied");
  }
}

function formatClassifierTierBadge(
  tier: string,
  t: (key: MessageKey) => string,
): { label: string; className: string } {
  switch (tier) {
    case "llm_tier2":
      return {
        label: t("guardian.tierLlm"),
        className: "bg-sky-500/12 text-sky-700 dark:text-sky-300",
      };
    case "heuristic_fallback":
    case "ai_rubric":
      return {
        label: t("guardian.tierHeuristic"),
        className: "bg-muted text-muted-foreground",
      };
    case "fail_closed":
      return {
        label: t("guardian.tierFailClosed"),
        className: "bg-amber-500/12 text-amber-800 dark:text-amber-300",
      };
    case "deterministic_tier1":
    case "deterministic":
    case "tier1":
    default:
      return {
        label: t("guardian.tierDeterministic"),
        className: "border-border bg-background text-muted-foreground border",
      };
  }
}

function formatAuditTime(ms: number): string {
  if (!ms) return "";
  try {
    return new Date(ms).toLocaleTimeString([], {
      hour: "2-digit",
      minute: "2-digit",
      second: "2-digit",
    });
  } catch {
    return "";
  }
}

function PatternListEditor({
  title,
  description,
  placeholder,
  addLabel,
  removeAriaLabel,
  items,
  disabled,
  onChange,
}: {
  title: string;
  description: string;
  placeholder: string;
  addLabel: string;
  removeAriaLabel: (item: string) => string;
  items: string[];
  disabled: boolean;
  onChange: (next: string[]) => void;
}) {
  const [draft, setDraft] = useState("");

  const handleAdd = () => {
    const trimmed = draft.trim();
    if (!trimmed) return;
    if (items.some((x) => x.toLowerCase() === trimmed.toLowerCase())) {
      setDraft("");
      return;
    }
    onChange([...items, trimmed]);
    setDraft("");
  };

  return (
    <div className="flex flex-col gap-2.5 px-4 py-3">
      <div>
        <div className="text-foreground text-[13px] font-medium">{title}</div>
        <div className="text-muted-foreground mt-0.5 text-[11px] leading-snug">{description}</div>
      </div>

      <div className="flex items-center gap-2">
        <Input
          type="text"
          value={draft}
          disabled={disabled}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === "Enter") {
              e.preventDefault();
              handleAdd();
            }
          }}
          placeholder={placeholder}
          className="h-[30px] flex-1 font-mono text-[12px]"
        />
        <Button
          type="button"
          variant="outline"
          size="sm"
          disabled={disabled || !draft.trim()}
          onClick={handleAdd}
          className="gap-1 px-2.5"
        >
          <Plus className="h-[12.5px] w-[12.5px]" />
          {addLabel}
        </Button>
      </div>

      {items.length > 0 && (
        <div className="flex flex-wrap gap-1.5 pt-0.5">
          {items.map((item) => (
            <span
              key={item}
              className="border-border bg-muted/60 text-foreground inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[11px]"
            >
              <span className="max-w-[260px] truncate">{item}</span>
              <button
                type="button"
                disabled={disabled}
                aria-label={removeAriaLabel(item)}
                onClick={() => onChange(items.filter((x) => x !== item))}
                className="text-muted-foreground hover:text-foreground cursor-pointer disabled:opacity-45"
              >
                <X className="h-[11px] w-[11px]" />
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

export function HostGuardianSection({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const hostCard = useOrbitSelector(
    (s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null,
  );
  const entry = useOrbitSelector((s) => s.guardianByHost[hostId] ?? null);

  const isOnline =
    hostCard?.connection_state === "online_direct" ||
    hostCard?.connection_state === "online_relay";

  useEffect(() => {
    if (isOnline && hostId) {
      void orbitActions.loadGuardianConfig(hostId);
    }
  }, [isOnline, hostId]);

  const config = entry?.config ?? null;
  const audits = entry?.recentAudits ?? EMPTY_AUDITS;
  const isLoading = entry?.status === "loading" && !config;
  const isSaving = entry?.saving ?? false;

  const persistedModel = config?.llm_model ?? "";
  const [modelDraft, setModelDraft] = useState(persistedModel);

  useEffect(() => {
    setModelDraft(persistedModel);
  }, [hostId, persistedModel]);

  const maxRiskOptions: SettingSelectOption<"low" | "medium">[] = useMemo(
    () => [
      {
        value: "low",
        label: t("guardian.thresholdLowLabel"),
        description: t("guardian.thresholdLowDesc"),
      },
      {
        value: "medium",
        label: t("guardian.thresholdMediumLabel"),
        description: t("guardian.thresholdMediumDesc"),
      },
    ],
    [t],
  );

  const llmProviderOptions: SettingSelectOption<LlmProviderOptionValue>[] = useMemo(
    () => [
      {
        value: "auto",
        label: t("guardian.llmProviderAuto"),
        description: t("guardian.llmProviderAutoDesc"),
      },
      { value: "claude_code", label: "Claude Code" },
      { value: "codex", label: "Codex" },
    ],
    [t],
  );

  const llmTier2Enabled = config?.llm_tier2_enabled ?? true;
  const llmTimeoutMs = config?.llm_timeout_ms ?? DEFAULT_LLM_TIMEOUT_MS;

  const llmTimeoutOptions: SettingSelectOption<string>[] = useMemo(() => {
    const values: number[] = [...PRESET_LLM_TIMEOUTS_MS];
    if (!values.includes(llmTimeoutMs)) {
      values.push(llmTimeoutMs);
      values.sort((a, b) => a - b);
    }
    return values.map((ms) => ({
      value: String(ms),
      label: ms % 1000 === 0 ? `${ms / 1000}s (${ms}ms)` : `${ms}ms`,
    }));
  }, [llmTimeoutMs]);

  const effectiveLlmProvider: LlmProviderOptionValue =
    config?.llm_provider === "claude_code" || config?.llm_provider === "codex"
      ? config.llm_provider
      : "auto";

  const commitConfig = (patch: Partial<GuardianPolicyConfig>) => {
    if (!config) return;
    void orbitActions.updateGuardianConfig(hostId, {
      ...config,
      llm_tier2_enabled: config.llm_tier2_enabled ?? true,
      llm_timeout_ms: config.llm_timeout_ms ?? DEFAULT_LLM_TIMEOUT_MS,
      ...patch,
    });
  };

  const commitModelDraft = () => {
    const clean = modelDraft.trim();
    const current = (config?.llm_model ?? "").trim();
    if (clean === current) return;
    commitConfig({ llm_model: clean || null });
  };

  const effectiveMaxRisk: "low" | "medium" =
    config?.auto_approve_max_risk === "medium" ? "medium" : "low";

  return (
    <div className="flex flex-col gap-5">
      <SettingSectionHeader
        title={t("guardian.title")}
        description={t("guardian.description")}
        action={
          isOnline ? (
            <button
              type="button"
              disabled={entry?.status === "loading" || isSaving}
              onClick={() => void orbitActions.loadGuardianConfig(hostId)}
              className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:opacity-50"
            >
              <RefreshCw
                className={cn(
                  "h-[12.5px] w-[12.5px]",
                  entry?.status === "loading" && "animate-spin",
                )}
              />
              {t("guardian.refresh")}
            </button>
          ) : undefined
        }
      />

      {!isOnline ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-6 text-center text-[13px]">
            {t("guardian.offline")}
          </div>
        </SettingCard>
      ) : isLoading ? (
        <SettingCard>
          <div className="text-muted-foreground flex items-center justify-center gap-2 px-4 py-8 text-[12.5px]">
            <Loader2 className="h-[14px] w-[14px] animate-spin" />
            <span>{t("guardian.loading")}</span>
          </div>
        </SettingCard>
      ) : !config ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-6 text-center text-[13px]">
            {entry?.error || t("guardian.loadError")}
          </div>
        </SettingCard>
      ) : (
        <>
          {entry?.error && (
            <div className="rounded-lg border border-rose-500/25 bg-rose-500/[0.07] px-3 py-2 text-[12px] text-rose-700 dark:text-rose-300">
              {entry.error}
            </div>
          )}

          <div className="flex flex-col gap-2">
            <SettingGroupLabel>{t("guardian.groupPolicy")}</SettingGroupLabel>
            <SettingCard>
              <div className="divide-border divide-y">
                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      {t("guardian.enableTitle")}
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11px]">
                      {t("guardian.enableDesc")}
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.enabled}
                    disabled={isSaving}
                    ariaLabel={t("guardian.enableTitle")}
                    onChange={(enabled) => commitConfig({ enabled })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      {t("guardian.thresholdTitle")}
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11px]">
                      {t("guardian.thresholdDesc")}
                    </div>
                  </div>
                  <SettingSelect
                    value={effectiveMaxRisk}
                    options={maxRiskOptions}
                    disabled={isSaving || !config.enabled}
                    onChange={(val) => commitConfig({ auto_approve_max_risk: val })}
                    triggerClassName="w-[176px]"
                    menuClassName="w-[260px]"
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      {t("guardian.readOnlyTitle")}
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11px]">
                      {t("guardian.readOnlyDesc")}
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.allow_read_only_tools}
                    disabled={isSaving || !config.enabled}
                    ariaLabel={t("guardian.readOnlyTitle")}
                    onChange={(allow_read_only_tools) => commitConfig({ allow_read_only_tools })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      {t("guardian.workspaceTestsTitle")}
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11px]">
                      {t("guardian.workspaceTestsDesc")}
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.allow_workspace_tests}
                    disabled={isSaving || !config.enabled}
                    ariaLabel={t("guardian.workspaceTestsTitle")}
                    onChange={(allow_workspace_tests) => commitConfig({ allow_workspace_tests })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      {t("guardian.tier2Title")}
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11px]">
                      {t("guardian.tier2Desc")}
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.use_ai_classifier_for_medium}
                    disabled={isSaving || !config.enabled}
                    ariaLabel={t("guardian.tier2Title")}
                    onChange={(use_ai_classifier_for_medium) =>
                      commitConfig({ use_ai_classifier_for_medium })
                    }
                  />
                </div>

                {config.use_ai_classifier_for_medium && (
                  <div className="flex flex-col gap-3 px-4 py-3">
                    <div className="flex items-center justify-between gap-4">
                      <div className="min-w-0">
                        <div className="text-foreground text-[13px] font-medium">
                          {t("guardian.llmTier2Title")}
                        </div>
                        <div className="text-muted-foreground mt-0.5 text-[11px]">
                          {t("guardian.llmTier2Desc", { timeoutMs: String(llmTimeoutMs) })}
                        </div>
                      </div>
                      <ToggleSwitch
                        checked={llmTier2Enabled}
                        disabled={isSaving || !config.enabled}
                        ariaLabel={t("guardian.llmTier2Title")}
                        onChange={(enabled) => commitConfig({ llm_tier2_enabled: enabled })}
                      />
                    </div>

                    {llmTier2Enabled && (
                      <div className="border-border/70 bg-muted/25 grid grid-cols-1 gap-2.5 rounded-lg border p-3 sm:grid-cols-3">
                        <div className="flex flex-col gap-1">
                          <span className="text-muted-foreground text-[11px] font-medium">
                            {t("guardian.llmProviderLabel")}
                          </span>
                          <SettingSelect
                            value={effectiveLlmProvider}
                            options={llmProviderOptions}
                            disabled={isSaving || !config.enabled}
                            onChange={(val) =>
                              commitConfig({
                                llm_provider: val === "auto" ? null : (val as ProviderKind),
                              })
                            }
                            triggerClassName="h-[30px] w-full text-[12px]"
                            menuClassName="w-[240px]"
                          />
                        </div>

                        <div className="flex flex-col gap-1">
                          <span className="text-muted-foreground text-[11px] font-medium">
                            {t("guardian.llmModelLabel")}
                          </span>
                          <Input
                            type="text"
                            value={modelDraft}
                            disabled={isSaving || !config.enabled}
                            onChange={(e) => setModelDraft(e.target.value)}
                            onBlur={commitModelDraft}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                commitModelDraft();
                              }
                            }}
                            placeholder={t("guardian.llmModelPlaceholder")}
                            className="h-[30px] font-mono text-[12px]"
                          />
                        </div>

                        <div className="flex flex-col gap-1">
                          <span className="text-muted-foreground text-[11px] font-medium">
                            {t("guardian.llmTimeoutLabel")}
                          </span>
                          <SettingSelect
                            value={String(llmTimeoutMs)}
                            options={llmTimeoutOptions}
                            disabled={isSaving || !config.enabled}
                            onChange={(val) => {
                              const parsed = Number.parseInt(val, 10);
                              if (Number.isFinite(parsed)) {
                                commitConfig({ llm_timeout_ms: parsed });
                              }
                            }}
                            triggerClassName="h-[30px] w-full font-mono text-[12px] tabular-nums"
                            menuClassName="w-[180px]"
                          />
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </SettingCard>
          </div>

          <div className="flex flex-col gap-2">
            <SettingGroupLabel>{t("guardian.groupCustomRules")}</SettingGroupLabel>
            <SettingCard>
              <div className="divide-border divide-y">
                <PatternListEditor
                  title={t("guardian.trustedPrefixesTitle")}
                  description={t("guardian.trustedPrefixesDesc")}
                  placeholder={t("guardian.trustedPrefixesPlaceholder")}
                  addLabel={t("guardian.addRule")}
                  removeAriaLabel={(item) => t("guardian.removeRule", { item })}
                  items={config.trusted_command_prefixes}
                  disabled={isSaving || !config.enabled}
                  onChange={(trusted_command_prefixes) =>
                    commitConfig({ trusted_command_prefixes })
                  }
                />
                <PatternListEditor
                  title={t("guardian.blockedPatternsTitle")}
                  description={t("guardian.blockedPatternsDesc")}
                  placeholder={t("guardian.blockedPatternsPlaceholder")}
                  addLabel={t("guardian.addRule")}
                  removeAriaLabel={(item) => t("guardian.removeRule", { item })}
                  items={config.blocked_command_patterns}
                  disabled={isSaving}
                  onChange={(blocked_command_patterns) =>
                    commitConfig({ blocked_command_patterns })
                  }
                />
              </div>
            </SettingCard>
          </div>

          <div className="flex flex-col gap-2">
            <SettingGroupLabel>{t("guardian.groupRecentDecisions")}</SettingGroupLabel>
            <SettingCard>
              {audits.length === 0 ? (
                <div className="text-muted-foreground flex items-center justify-center gap-2 px-4 py-7 text-[12.5px]">
                  <ShieldCheck className="h-[14px] w-[14px] opacity-65" />
                  <span>{t("guardian.noAudits")}</span>
                </div>
              ) : (
                <div className="orbit-native-scroll divide-border max-h-[280px] divide-y overflow-y-auto overscroll-contain">
                  {audits.slice(0, 40).map((item) => {
                    const risk = formatRiskBadge(item.assessment.risk_level, t);
                    const tier = formatClassifierTierBadge(item.assessment.classifier_tier, t);
                    return (
                      <div key={item.id} className="flex flex-col gap-1 px-4 py-2.5">
                        <div className="flex items-center justify-between gap-2">
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span
                              className={cn(
                                "shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium",
                                risk.className,
                              )}
                            >
                              {risk.label}
                            </span>
                            <span
                              className={cn(
                                "shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium",
                                tier.className,
                              )}
                            >
                              {tier.label}
                            </span>
                            <span className="text-foreground shrink-0 text-[11.5px] font-medium">
                              {formatDecisionLabel(item.assessment.decision, t)}
                            </span>
                            <span className="text-muted-foreground/50">·</span>
                            <span className="text-muted-foreground truncate font-mono text-[11px]">
                              {item.tool_name}
                            </span>
                          </div>
                          <span className="text-muted-foreground/75 shrink-0 font-mono text-[10.5px] tabular-nums">
                            {item.assessment.latency_ms}ms · {formatAuditTime(item.decided_at_ms)}
                          </span>
                        </div>
                        <div className="text-foreground/85 truncate font-mono text-[11.5px]">
                          {item.target_summary}
                        </div>
                        <div className="text-muted-foreground truncate text-[11px]">
                          {item.assessment.reason}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </SettingCard>
          </div>
        </>
      )}
    </div>
  );
}
