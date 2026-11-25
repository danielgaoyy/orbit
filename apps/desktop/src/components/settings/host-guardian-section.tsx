import { memo, useEffect, useMemo, useState } from "react";
import {
  Check,
  FilePenLine,
  Plus,
  RefreshCw,
  Shield,
  ShieldCheck,
  X,
  Zap,
} from "lucide-react";
import {
  SettingCard,
  SettingDisclosureCard,
  SettingGroup,
  SettingRow,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { PermissionRulesPanel } from "@/components/settings/permission-rules-panel";
import { createHostPermissionRulesSelector } from "@/components/settings/permission-rules-model";
import {
  HostAuditChainSection,
  HostSandboxSection,
} from "@/components/settings/host-security-ops-panels";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
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
): { label: string; variant: "success" | "warning" | "destructive" } {
  switch (level) {
    case "low":
      return { label: t("guardian.riskLow"), variant: "success" };
    case "medium":
      return { label: t("guardian.riskMedium"), variant: "warning" };
    case "high":
    case "critical":
      return {
        label: level === "critical" ? t("guardian.riskCritical") : t("guardian.riskHigh"),
        variant: "destructive",
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
): { label: string; variant: "default" | "secondary" | "warning" | "outline" } {
  switch (tier) {
    case "llm_tier2":
      return { label: t("guardian.tierLlm"), variant: "default" };
    case "heuristic_fallback":
    case "ai_rubric":
      return { label: t("guardian.tierHeuristic"), variant: "secondary" };
    case "fail_closed":
      return { label: t("guardian.tierFailClosed"), variant: "warning" };
    case "deterministic_tier1":
    case "deterministic":
    case "tier1":
    default:
      return { label: t("guardian.tierDeterministic"), variant: "outline" };
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
    <div className="flex flex-col gap-2.5 px-4 py-3.5">
      <div className={cn("transition-opacity", disabled && "opacity-55")}>
        <div className="text-ink-primary text-[13px] font-medium">{title}</div>
        <div className="text-ink-secondary mt-0.5 text-[13px] leading-[1.45]">{description}</div>
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
          className="text-[13px]"
        >
          <Plus className="size-[13px]" />
          <span>{addLabel}</span>
        </Button>
      </div>

      {items.length > 0 && (
        <div className="flex flex-wrap gap-1.5 pt-0.5">
          {items.map((item) => (
            <span
              key={item}
              className="border-border-subtle bg-surface-recessed text-ink-primary inline-flex items-center gap-1.5 rounded-[6px] border px-2 py-0.5 font-mono text-[11px]"
            >
              <span className="max-w-[280px] truncate">{item}</span>
              <button
                type="button"
                disabled={disabled}
                aria-label={removeAriaLabel(item)}
                onClick={() => onChange(items.filter((x) => x !== item))}
                className="text-ink-muted hover:text-ink-primary cursor-pointer transition-colors disabled:opacity-45"
              >
                <X className="size-[11px]" />
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  );
}

const AUTONOMY_MODES = [
  ["default", "composer.modeAlwaysAsk", "composer.modeAlwaysAskShortDesc", Shield],
  ["auto", "settings.agentAutoGuardian", "composer.modeAutoShortDesc", ShieldCheck],
  ["accept_edits", "composer.modeAcceptEdits", "composer.modeAcceptEditsShortDesc", FilePenLine],
  ["full_access", "composer.modeFullAccess", "composer.modeFullAccessShortDesc", Zap],
] as const;

export const HostGuardianSection = memo(function HostGuardianSection({
  hostId,
}: {
  hostId: string | null;
}) {
  const { t } = useI18n();
  const draftMode = useOrbitSelector((s) => s.draftMode);
  const isOnline = useHostIsOnline(hostId);
  const loadingGuardian = useOrbitSelector(
    (s) => (hostId ? s.guardianByHost[hostId]?.status === "loading" : false),
  );

  return (
    <div className="flex flex-col gap-6">
      <SettingSectionHeader
        title={t("settings.nav4Safety")}
        description={t("settings.agentSafetyDesc")}
        action={
          hostId && isOnline ? (
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={loadingGuardian}
              onClick={() => {
                void orbitActions.loadGuardianConfig(hostId);
                void orbitActions.loadPermissionRules(hostId);
              }}
              className="text-[13px]"
            >
              {loadingGuardian ? (
                <OrbitSpinner size={13} />
              ) : (
                <RefreshCw className="size-[13px]" />
              )}
              <span>{t("guardian.refresh")}</span>
            </Button>
          ) : undefined
        }
      />

      <SettingGroup settingId="settings.defaultPermissions" title={t("settings.defaultPermissions")}>
        <SettingCard>
          <div className="flex flex-col gap-3 px-4 py-3.5">
            <p className="text-ink-secondary text-[13px] leading-[1.45]">
              {t("settings.defaultPermissionsDesc")}
            </p>
            <div
              className="grid grid-cols-2 gap-2.5 sm:grid-cols-4"
              role="group"
              aria-label={t("settings.defaultPermissions")}
            >
              {AUTONOMY_MODES.map(([mode, label, description, Icon]) => {
                const checked = mode === draftMode;
                return (
                  <button
                    key={mode}
                    type="button"
                    aria-pressed={checked}
                    onClick={() => orbitActions.setDraftConfig({ mode })}
                    className={cn(
                      "flex cursor-pointer flex-col gap-1.5 rounded-[10px] border p-3 text-left transition-colors outline-none focus-visible:ring-1 focus-visible:ring-ring/45",
                      checked
                        ? "border-border-strong bg-surface-recessed"
                        : "border-border-subtle bg-surface-base/60 hover:border-border-default hover:bg-surface-recessed/40",
                    )}
                  >
                    <span className="flex items-center justify-between gap-1.5">
                      <span className="inline-flex min-w-0 items-center gap-1.5">
                        <Icon
                          aria-hidden
                          className={cn(
                            "size-[14px] shrink-0 stroke-[1.75]",
                            checked ? "text-ink-primary" : "text-ink-muted",
                          )}
                        />
                        <span className="text-ink-primary truncate text-[13px] font-medium">
                          {t(label)}
                        </span>
                      </span>
                      {checked && <Check className="text-ink-primary size-[13px] shrink-0" />}
                    </span>
                    <span className="text-ink-secondary block text-[11px] leading-[1.45]">
                      {t(description)}
                    </span>
                  </button>
                );
              })}
            </div>
          </div>
        </SettingCard>
      </SettingGroup>

      {hostId ? (
        <GuardianPolicies key={hostId} hostId={hostId} />
      ) : (
        <SettingCard>
          <div
            data-setting-id="guardian.groupPolicy"
            data-setting-fallback
            className="text-ink-muted px-4 py-6 text-center text-[13px]"
          >
            {t("guardian.offline")}
          </div>
        </SettingCard>
      )}
    </div>
  );
});

const GuardianPolicies = memo(function GuardianPolicies({ hostId }: { hostId: string }) {
  const { t } = useI18n();
  const isOnline = useHostIsOnline(hostId);
  const entry = useOrbitSelector((s) => s.guardianByHost[hostId] ?? null);
  const rulesSelector = useMemo(() => createHostPermissionRulesSelector(hostId), [hostId]);
  const permRulesCount = useOrbitSelector((s) => rulesSelector(s).rules.length);

  useEffect(() => {
    if (isOnline && hostId) {
      void orbitActions.loadGuardianConfig(hostId);
      void orbitActions.loadPermissionRules(hostId);
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

  const customRuleCount = config
    ? config.trusted_command_prefixes.length + config.blocked_command_patterns.length
    : 0;

  return (
    <div className="flex flex-col gap-6">
      {/* OS Sandbox Policy — First-class visible card */}
      <HostSandboxSection hostId={hostId} isOnline={isOnline} />

      {/* Guardian Auto-Approval Policy */}
      <SettingGroup
        settingId="guardian.groupPolicy"
        title={t("guardian.title")}
        action={
          config ? (
            <Badge variant={config.enabled ? "success" : "secondary"}>
              {config.enabled
                ? effectiveMaxRisk === "medium"
                  ? t("guardian.thresholdMediumLabel")
                  : t("guardian.thresholdLowLabel")
                : t("securityOps.sandboxOff")}
            </Badge>
          ) : undefined
        }
      >
        {!isOnline ? (
          <SettingCard>
            <div data-setting-fallback className="text-ink-muted px-4 py-6 text-center text-[13px]">
              {t("guardian.offline")}
            </div>
          </SettingCard>
        ) : isLoading ? (
          <SettingCard>
            <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-8 text-[13px]">
              <OrbitSpinner size={14} />
              <span>{t("guardian.loading")}</span>
            </div>
          </SettingCard>
        ) : !config ? (
          <SettingCard>
            <div data-setting-fallback className="text-ink-muted px-4 py-6 text-center text-[13px]">
              {entry?.error || t("guardian.loadError")}
            </div>
          </SettingCard>
        ) : (
          <div className="flex flex-col gap-3">
            {entry?.error && (
              <div
                role="alert"
                className="border-signal-danger-border bg-signal-danger-bg text-signal-danger rounded-[10px] border px-3.5 py-2.5 text-[13px]"
              >
                {entry.error}
              </div>
            )}

            <SettingCard>
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
                disabled={!config.enabled}
              >
                <SettingSelect
                  value={effectiveMaxRisk}
                  options={maxRiskOptions}
                  disabled={!config.enabled}
                  onChange={(val) => commitConfig({ auto_approve_max_risk: val })}
                  triggerClassName="w-[180px]"
                  menuClassName="w-[260px]"
                />
              </SettingRow>

              <SettingRow
                settingId="guardian.readOnlyTitle"
                title={t("guardian.readOnlyTitle")}
                description={t("guardian.readOnlyDesc")}
                disabled={!config.enabled}
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
                disabled={!config.enabled}
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
                disabled={!config.enabled}
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
                <div className="bg-surface-recessed/45 flex flex-col gap-3 px-4 py-3.5">
                  <div className="flex items-center justify-between gap-4">
                    <div
                      className={cn("min-w-0 transition-opacity", !config.enabled && "opacity-55")}
                    >
                      <div className="text-ink-primary text-[13px] font-medium">
                        {t("guardian.llmTier2Title")}
                      </div>
                      <div className="text-ink-secondary mt-0.5 text-[13px] leading-[1.45]">
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
                    <div className="grid grid-cols-1 gap-3 pt-0.5 sm:grid-cols-3">
                      <div className="flex flex-col gap-1.5">
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

                      <div className="flex flex-col gap-1.5">
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

                      <div className="flex flex-col gap-1.5">
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
            </SettingCard>

            <SettingDisclosureCard
              settingId="guardian.groupCustomRules"
              title={t("guardian.groupCustomRules")}
              meta={
                <Badge variant="secondary" className="font-mono tabular-nums">
                  {customRuleCount}
                </Badge>
              }
              contentClassName="divide-border-subtle divide-y"
            >
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
            </SettingDisclosureCard>

            <SettingDisclosureCard
              settingId="guardian.groupRecentDecisions"
              title={t("guardian.groupRecentDecisions")}
              meta={
                <Badge variant="secondary" className="font-mono tabular-nums">
                  {audits.length}
                </Badge>
              }
            >
              {audits.length === 0 ? (
                <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-7 text-[13px]">
                  <ShieldCheck className="size-[14px] opacity-65" />
                  <span>{t("guardian.noAudits")}</span>
                </div>
              ) : (
                <div className="orbit-native-scroll divide-border-subtle max-h-[280px] divide-y overflow-y-auto overscroll-contain">
                  {audits.slice(0, 40).map((item) => {
                    const risk = formatRiskBadge(item.assessment.risk_level, t);
                    const tier = formatClassifierTierBadge(item.assessment.classifier_tier, t);
                    return (
                      <div key={item.id} className="flex flex-col gap-1 px-4 py-3">
                        <div className="flex items-center justify-between gap-2">
                          <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                            <Badge variant={risk.variant}>{risk.label}</Badge>
                            <Badge variant={tier.variant}>{tier.label}</Badge>
                            <span className="text-ink-primary shrink-0 text-[11px] font-medium">
                              {formatDecisionLabel(item.assessment.decision, t)}
                            </span>
                            <span aria-hidden className="text-ink-faint">
                              ·
                            </span>
                            <span className="text-ink-muted truncate font-mono text-[11px]">
                              {item.tool_name}
                            </span>
                          </div>
                          <span className="text-ink-muted shrink-0 font-mono text-[11px] tabular-nums">
                            {item.assessment.latency_ms}ms · {formatAuditTime(item.decided_at_ms)}
                          </span>
                        </div>
                        <div className="text-ink-primary truncate font-mono text-[11px]">
                          {item.target_summary}
                        </div>
                        <div className="text-ink-secondary truncate text-[11px]">
                          {item.assessment.reason}
                        </div>
                      </div>
                    );
                  })}
                </div>
              )}
            </SettingDisclosureCard>
          </div>
        )}
      </SettingGroup>

      {/* Saved Permission Rules — Single-border disclosure with embedded toolbar */}
      <SettingDisclosureCard
        settingId="permissionRules.sectionTitle"
        title={t("permissionRules.sectionTitle")}
        meta={
          <Badge variant="secondary" className="font-mono tabular-nums">
            {permRulesCount}
          </Badge>
        }
      >
        <PermissionRulesPanel hostId={hostId} isOnline={isOnline} embedded />
      </SettingDisclosureCard>

      {/* Tamper-Evident Audit Chain — Compact visible status card */}
      <HostAuditChainSection hostId={hostId} isOnline={isOnline} />
    </div>
  );
});
