import { memo, type ReactNode, useEffect, useMemo, useState } from "react";
import { Check, ChevronRight, Loader2, Plus, RefreshCw, ShieldCheck, X } from "lucide-react";
import {
  SettingCard,
  SettingGroup,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { PermissionRulesPanel } from "@/components/settings/permission-rules-panel";
import { HostSandboxAndAuditPanel } from "@/components/settings/host-security-ops-panels";
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
        className: "bg-signal-success-bg text-signal-success",
      };
    case "medium":
      return {
        label: t("guardian.riskMedium"),
        className: "bg-signal-warning-bg text-signal-warning",
      };
    case "high":
      return {
        label: t("guardian.riskHigh"),
        className: "bg-signal-danger-bg text-signal-danger",
      };
    case "critical":
      return {
        label: t("guardian.riskCritical"),
        className: "bg-signal-danger-bg text-signal-danger font-semibold",
      };
  }
}

function formatDecisionLabel(decision: GuardianDecision, t: (key: MessageKey) => string): string {
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
        className: "bg-signal-info-bg text-signal-info",
      };
    case "heuristic_fallback":
    case "ai_rubric":
      return {
        label: t("guardian.tierHeuristic"),
        className: "bg-surface-recessed text-ink-muted",
      };
    case "fail_closed":
      return {
        label: t("guardian.tierFailClosed"),
        className: "bg-signal-warning-bg text-signal-warning",
      };
    case "deterministic_tier1":
    case "deterministic":
    case "tier1":
    default:
      return {
        label: t("guardian.tierDeterministic"),
        className: "border-border-default bg-surface-base text-ink-muted border",
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
        <div className="text-ink-primary text-[13px] font-medium">{title}</div>
        <div className="text-ink-muted mt-0.5 text-[11px] leading-snug">{description}</div>
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
          className="h-[30px] flex-1 font-mono text-[13px]"
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
              className="border-border-default bg-surface-recessed/60 text-ink-primary inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[11px]"
            >
              <span className="max-w-[260px] truncate">{item}</span>
              <button
                type="button"
                disabled={disabled}
                aria-label={removeAriaLabel(item)}
                onClick={() => onChange(items.filter((x) => x !== item))}
                className="text-ink-muted hover:text-ink-primary cursor-pointer disabled:opacity-45"
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

function SafetyDisclosure({ title, children }: { title: string; children: ReactNode }) {
  return (
    <details className="group/disclosure rounded-xl border border-border-subtle bg-surface-raised">
      <summary className="flex cursor-pointer list-none items-center justify-between px-4 py-3 text-[13px] font-medium text-ink-primary [&::-webkit-details-marker]:hidden">
        {title}
        <ChevronRight className="size-[14px] text-ink-muted transition-transform group-open/disclosure:rotate-90 motion-reduce:transition-none" />
      </summary>
      <div className="space-y-3 border-t border-border-subtle p-3">{children}</div>
    </details>
  );
}

const AUTONOMY_MODES = [
  ["default", "composer.modeAlwaysAsk", "composer.modeAlwaysAskShortDesc"],
  ["auto", "settings.agentAutoGuardian", "composer.modeAutoShortDesc"],
  ["accept_edits", "composer.modeAcceptEdits", "composer.modeAcceptEditsShortDesc"],
  ["full_access", "composer.modeFullAccess", "composer.modeFullAccessShortDesc"],
] as const;

export const HostGuardianSection = memo(function HostGuardianSection({
  hostId,
}: {
  hostId: string | null;
}) {
  const { t } = useI18n();
  const draftMode = useOrbitSelector((s) => s.draftMode);
  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("settings.nav4Safety")}
        description={t("settings.agentSafetyDesc")}
      />
      <SettingGroup settingId="settings.defaultPermissions" title={t("settings.defaultPermissions")}>
        <p className="text-[13px] text-ink-secondary">{t("settings.defaultPermissionsDesc")}</p>
        <div
          className="grid grid-cols-2 gap-2 lg:grid-cols-4"
          role="group"
          aria-label={t("settings.defaultPermissions")}
        >
          {AUTONOMY_MODES.map(([mode, label, description]) => {
            const checked = mode === draftMode;
            return (
              <button
                key={mode}
                type="button"
                aria-pressed={checked}
                onClick={() => orbitActions.setDraftConfig({ mode })}
                className={cn(
                  "cursor-pointer rounded-lg border p-3 text-left outline-none focus-visible:ring-1 focus-visible:ring-ring/45",
                  checked
                    ? "border-border-strong bg-surface-recessed"
                    : "border-border-subtle bg-surface-raised hover:border-border-default",
                )}
              >
                <span className="flex items-center justify-between gap-1 text-[13px] font-medium text-ink-primary">
                  {t(label)}
                  {checked && <Check className="size-[13px] shrink-0" />}
                </span>
                <span className="mt-1 block text-[11px] text-ink-secondary">{t(description)}</span>
              </button>
            );
          })}
        </div>
      </SettingGroup>
      {hostId ? (
        <GuardianPolicies key={hostId} hostId={hostId} />
      ) : (
        <p
          data-setting-id="guardian.groupPolicy"
          data-setting-fallback
          className="text-[13px] text-ink-secondary"
        >
          {t("guardian.offline")}
        </p>
      )}
    </div>
  );
});

const GuardianPolicies = memo(function GuardianPolicies({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const isOnline = useHostIsOnline(hostId);
  const entry = useOrbitSelector((s) => s.guardianByHost[hostId] ?? null);

  useEffect(() => {
    if (isOnline && hostId) {
      void orbitActions.loadGuardianConfig(hostId);
    }
  }, [isOnline, hostId]);

  const config = entry?.config ?? null;
  const audits = entry?.recentAudits ?? EMPTY_AUDITS;
  const isLoading = entry?.status === "loading" && !config;

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
        subpage
        title={t("guardian.title")}
        description={t("guardian.description")}
        action={
          isOnline ? (
            <button
              type="button"
              disabled={entry?.status === "loading"}
              onClick={() => {
                void orbitActions.loadGuardianConfig(hostId);
                void orbitActions.loadPermissionRules(hostId);
              }}
              className="border-border-default bg-surface-base hover:bg-surface-recessed text-ink-primary inline-flex h-[28px] cursor-pointer items-center gap-1.5 rounded-lg border px-2.5 text-[13px] font-medium transition-colors disabled:opacity-50"
            >
              <RefreshCw
                className={cn("h-[12.5px] w-[12.5px]", entry?.status === "loading" && "animate-spin")}
              />
              {t("guardian.refresh")}
            </button>
          ) : undefined
        }
      />

      {!isOnline ? (
        <SettingCard>
          <div
            data-setting-id="guardian.groupPolicy"
            data-setting-fallback
            className="text-ink-muted px-4 py-6 text-center text-[13px]"
          >
            {t("guardian.offline")}
          </div>
        </SettingCard>
      ) : isLoading ? (
        <SettingCard>
          <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-8 text-[13px]">
            <Loader2 className="h-[14px] w-[14px] animate-spin" />
            <span>{t("guardian.loading")}</span>
          </div>
        </SettingCard>
      ) : !config ? (
        <SettingCard>
          <div
            data-setting-id="guardian.groupPolicy"
            data-setting-fallback
            className="text-ink-muted px-4 py-6 text-center text-[13px]"
          >
            {entry?.error || t("guardian.loadError")}
          </div>
        </SettingCard>
      ) : (
        <>
          {entry?.error && (
            <div className="rounded-lg border border-signal-danger-border bg-signal-danger-bg px-3 py-2 text-[13px] text-signal-danger">
              {entry.error}
            </div>
          )}

          <SafetyDisclosure title={t("guardian.groupPolicy")}>
            <SettingCard settingId="guardian.groupPolicy">
              <div className="divide-border-subtle divide-y">
                <SettingRow
                  settingId="guardian.enableTitle"
                  title={t("guardian.enableTitle")}
                  description={t("guardian.enableDesc")}
                >
                  <ToggleSwitch
                    checked={config.enabled}
                    ariaLabel={t("guardian.enableTitle")}
                    onChange={(enabled) => commitConfig({ enabled })}
                  />
                </SettingRow>

                <SettingRow
                  settingId="guardian.thresholdTitle"
                  title={t("guardian.thresholdTitle")}
                  description={t("guardian.thresholdDesc")}
                >
                  <SettingSelect
                    value={effectiveMaxRisk}
                    options={maxRiskOptions}
                    disabled={!config.enabled}
                    onChange={(val) => commitConfig({ auto_approve_max_risk: val })}
                    triggerClassName="w-[176px]"
                    menuClassName="w-[260px]"
                  />
                </SettingRow>

                <SettingRow
                  settingId="guardian.readOnlyTitle"
                  title={t("guardian.readOnlyTitle")}
                  description={t("guardian.readOnlyDesc")}
                >
                  <ToggleSwitch
                    checked={config.allow_read_only_tools}
                    disabled={!config.enabled}
                    ariaLabel={t("guardian.readOnlyTitle")}
                    onChange={(allow_read_only_tools) => commitConfig({ allow_read_only_tools })}
                  />
                </SettingRow>

                <SettingRow
                  settingId="guardian.workspaceTestsTitle"
                  title={t("guardian.workspaceTestsTitle")}
                  description={t("guardian.workspaceTestsDesc")}
                >
                  <ToggleSwitch
                    checked={config.allow_workspace_tests}
                    disabled={!config.enabled}
                    ariaLabel={t("guardian.workspaceTestsTitle")}
                    onChange={(allow_workspace_tests) => commitConfig({ allow_workspace_tests })}
                  />
                </SettingRow>

                <SettingRow
                  settingId="guardian.tier2Title"
                  title={t("guardian.tier2Title")}
                  description={t("guardian.tier2Desc")}
                >
                  <ToggleSwitch
                    checked={config.use_ai_classifier_for_medium}
                    disabled={!config.enabled}
                    ariaLabel={t("guardian.tier2Title")}
                    onChange={(use_ai_classifier_for_medium) =>
                      commitConfig({ use_ai_classifier_for_medium })
                    }
                  />
                </SettingRow>

                {config.use_ai_classifier_for_medium && (
                  <div className="flex flex-col gap-3 px-4 py-3">
                    <div className="flex items-center justify-between gap-4">
                      <div className="min-w-0">
                        <div className="text-ink-primary text-[13px] font-medium">
                          {t("guardian.llmTier2Title")}
                        </div>
                        <div className="text-ink-muted mt-0.5 text-[11px]">
                          {t("guardian.llmTier2Desc", { timeoutMs: String(llmTimeoutMs) })}
                        </div>
                      </div>
                      <ToggleSwitch
                        checked={llmTier2Enabled}
                        disabled={!config.enabled}
                        ariaLabel={t("guardian.llmTier2Title")}
                        onChange={(enabled) => commitConfig({ llm_tier2_enabled: enabled })}
                      />
                    </div>

                    {llmTier2Enabled && (
                      <div className="border-border-default/70 bg-surface-recessed/25 grid grid-cols-1 gap-2.5 rounded-lg border p-3 sm:grid-cols-3">
                        <div className="flex flex-col gap-1">
                          <span className="text-ink-muted text-[11px] font-medium">
                            {t("guardian.llmProviderLabel")}
                          </span>
                          <SettingSelect
                            value={effectiveLlmProvider}
                            options={llmProviderOptions}
                            disabled={!config.enabled}
                            onChange={(val) =>
                              commitConfig({
                                llm_provider: val === "auto" ? null : (val as ProviderKind),
                              })
                            }
                            triggerClassName="h-[30px] w-full text-[13px]"
                            menuClassName="w-[240px]"
                          />
                        </div>

                        <div className="flex flex-col gap-1">
                          <span className="text-ink-muted text-[11px] font-medium">
                            {t("guardian.llmModelLabel")}
                          </span>
                          <Input
                            type="text"
                            value={modelDraft}
                            disabled={!config.enabled}
                            onChange={(e) => setModelDraft(e.target.value)}
                            onBlur={commitModelDraft}
                            onKeyDown={(e) => {
                              if (e.key === "Enter") {
                                e.preventDefault();
                                commitModelDraft();
                              }
                            }}
                            placeholder={t("guardian.llmModelPlaceholder")}
                            className="h-[30px] font-mono text-[13px]"
                          />
                        </div>

                        <div className="flex flex-col gap-1">
                          <span className="text-ink-muted text-[11px] font-medium">
                            {t("guardian.llmTimeoutLabel")}
                          </span>
                          <SettingSelect
                            value={String(llmTimeoutMs)}
                            options={llmTimeoutOptions}
                            disabled={!config.enabled}
                            onChange={(val) => {
                              const parsed = Number.parseInt(val, 10);
                              if (Number.isFinite(parsed)) {
                                commitConfig({ llm_timeout_ms: parsed });
                              }
                            }}
                            triggerClassName="h-[30px] w-full font-mono text-[13px] tabular-nums"
                            menuClassName="w-[180px]"
                          />
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </div>
            </SettingCard>
          </SafetyDisclosure>

          <SafetyDisclosure title={t("guardian.groupCustomRules")}>
            <SettingCard settingId="guardian.groupCustomRules">
              <div className="divide-border-subtle divide-y">
                <PatternListEditor
                  title={t("guardian.trustedPrefixesTitle")}
                  description={t("guardian.trustedPrefixesDesc")}
                  placeholder={t("guardian.trustedPrefixesPlaceholder")}
                  addLabel={t("guardian.addRule")}
                  removeAriaLabel={(item) => t("guardian.removeRule", { item })}
                  items={config.trusted_command_prefixes}
                  disabled={!config.enabled}
                  onChange={(trusted_command_prefixes) => commitConfig({ trusted_command_prefixes })}
                />
                <PatternListEditor
                  title={t("guardian.blockedPatternsTitle")}
                  description={t("guardian.blockedPatternsDesc")}
                  placeholder={t("guardian.blockedPatternsPlaceholder")}
                  addLabel={t("guardian.addRule")}
                  removeAriaLabel={(item) => t("guardian.removeRule", { item })}
                  items={config.blocked_command_patterns}
                  disabled={false}
                  onChange={(blocked_command_patterns) => commitConfig({ blocked_command_patterns })}
                />
              </div>
            </SettingCard>
          </SafetyDisclosure>

          <SafetyDisclosure title={t("guardian.groupRecentDecisions")}>
            <SettingCard settingId="guardian.groupRecentDecisions">
              {audits.length === 0 ? (
                <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-7 text-[13px]">
                  <ShieldCheck className="h-[14px] w-[14px] opacity-65" />
                  <span>{t("guardian.noAudits")}</span>
                </div>
              ) : (
                <div className="orbit-native-scroll divide-border-subtle max-h-[280px] divide-y overflow-y-auto overscroll-contain">
                  {audits.slice(0, 40).map((item) => {
                    const risk = formatRiskBadge(item.assessment.risk_level, t);
                    const tier = formatClassifierTierBadge(item.assessment.classifier_tier, t);
                    return (
                      <div key={item.id} className="flex flex-col gap-1 px-4 py-2.5">
                        <div className="flex items-center justify-between gap-2">
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span
                              className={cn(
                                "shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium",
                                risk.className,
                              )}
                            >
                              {risk.label}
                            </span>
                            <span
                              className={cn(
                                "shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium",
                                tier.className,
                              )}
                            >
                              {tier.label}
                            </span>
                            <span className="text-ink-primary shrink-0 text-[11px] font-medium">
                              {formatDecisionLabel(item.assessment.decision, t)}
                            </span>
                            <span className="text-ink-muted/50">·</span>
                            <span className="text-ink-muted truncate font-mono text-[11px]">
                              {item.tool_name}
                            </span>
                          </div>
                          <span className="text-ink-muted/75 shrink-0 font-mono text-[11px] tabular-nums">
                            {item.assessment.latency_ms}ms · {formatAuditTime(item.decided_at_ms)}
                          </span>
                        </div>
                        <div className="text-ink-primary/85 truncate font-mono text-[11px]">
                          {item.target_summary}
                        </div>
                        <div className="text-ink-muted truncate text-[11px]">
                          {item.assessment.reason}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </SettingCard>
          </SafetyDisclosure>
        </>
      )}
      <SafetyDisclosure title={t("permissionRules.sectionTitle")}>
        <div data-setting-id="permissionRules.sectionTitle">
          <PermissionRulesPanel hostId={hostId} isOnline={isOnline} />
        </div>
      </SafetyDisclosure>
      <SafetyDisclosure title={t("settings.agentSandboxAudit")}>
        <HostSandboxAndAuditPanel hostId={hostId} isOnline={isOnline} />
      </SafetyDisclosure>
    </div>
  );
});
