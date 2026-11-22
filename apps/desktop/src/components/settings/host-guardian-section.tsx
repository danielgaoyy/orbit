import { useEffect, useState } from "react";
import { Loader2, Plus, RefreshCw, ShieldCheck, X } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import type {
  GuardianAuditEntry,
  GuardianDecision,
  GuardianPolicyConfig,
  GuardianRiskLevel,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_AUDITS: readonly GuardianAuditEntry[] = [];

const MAX_RISK_OPTIONS: SettingSelectOption<"low" | "medium">[] = [
  {
    value: "low",
    label: "Low risk only",
    description: "Read-only inspection and workspace tests",
  },
  {
    value: "medium",
    label: "Up to Medium risk",
    description: "Also allow scoped in-workspace source edits",
  },
];

function formatRiskBadge(level: GuardianRiskLevel): { label: string; className: string } {
  switch (level) {
    case "low":
      return {
        label: "Low",
        className: "bg-emerald-500/10 text-emerald-700 dark:text-emerald-300",
      };
    case "medium":
      return {
        label: "Medium",
        className: "bg-amber-500/10 text-amber-800 dark:text-amber-300",
      };
    case "high":
      return {
        label: "High",
        className: "bg-rose-500/10 text-rose-700 dark:text-rose-300",
      };
    case "critical":
      return {
        label: "Critical",
        className: "bg-rose-500/16 text-rose-800 dark:text-rose-200 font-semibold",
      };
  }
}

function formatDecisionLabel(decision: GuardianDecision): string {
  switch (decision) {
    case "auto_allow":
      return "Allowed";
    case "require_human":
      return "Escalated";
    case "auto_deny":
      return "Denied";
  }
}

function formatAuditTime(ms: number): string {
  if (!ms) return "";
  try {
    return new Date(ms).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit", second: "2-digit" });
  } catch {
    return "";
  }
}

function PatternListEditor({
  title,
  description,
  placeholder,
  items,
  disabled,
  onChange,
}: {
  title: string;
  description: string;
  placeholder: string;
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
    <div className="space-y-2.5 px-4 py-3">
      <div>
        <div className="text-foreground text-[13px] font-medium">{title}</div>
        <div className="text-muted-foreground mt-0.5 text-[11.5px] leading-snug">{description}</div>
      </div>

      <div className="flex items-center gap-2">
        <input
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
          className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 h-[30px] min-w-0 flex-1 rounded-lg border px-2.5 font-mono text-[12px] outline-none focus:border-foreground/35 disabled:opacity-50"
        />
        <button
          type="button"
          disabled={disabled || !draft.trim()}
          onClick={handleAdd}
          className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[30px] shrink-0 cursor-pointer items-center gap-1 rounded-lg border px-2.5 text-[12px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-45"
        >
          <Plus className="h-[12.5px] w-[12.5px]" />
          Add
        </button>
      </div>

      {items.length > 0 && (
        <div className="flex flex-wrap gap-1.5 pt-0.5">
          {items.map((item) => (
            <span
              key={item}
              className="border-border bg-muted/60 text-foreground inline-flex items-center gap-1 rounded-md border px-2 py-0.5 font-mono text-[11.5px]"
            >
              <span className="max-w-[260px] truncate">{item}</span>
              <button
                type="button"
                disabled={disabled}
                aria-label={`Remove ${item}`}
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

  const commitConfig = (patch: Partial<GuardianPolicyConfig>) => {
    if (!config) return;
    void orbitActions.updateGuardianConfig(hostId, { ...config, ...patch });
  };

  const effectiveMaxRisk: "low" | "medium" =
    config?.auto_approve_max_risk === "medium" ? "medium" : "low";

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="Guardian"
        description="Fail-closed risk classifier for Auto mode. High-risk commands, secrets, and outside-workspace writes always require approval."
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
              Refresh
            </button>
          ) : undefined
        }
      />

      {!isOnline ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-6 text-center text-[13px]">
            Connect to this host to configure Guardian auto-approval policy.
          </div>
        </SettingCard>
      ) : isLoading ? (
        <SettingCard>
          <div className="text-muted-foreground flex items-center justify-center gap-2 px-4 py-8 text-[12.5px]">
            <Loader2 className="h-[14px] w-[14px] animate-spin" />
            <span>Loading Guardian policy…</span>
          </div>
        </SettingCard>
      ) : !config ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-6 text-center text-[13px]">
            {entry?.error || "Unable to load Guardian configuration from this host."}
          </div>
        </SettingCard>
      ) : (
        <>
          {entry?.error && (
            <div className="rounded-lg border border-rose-500/25 bg-rose-500/[0.07] px-3 py-2 text-[12px] text-rose-700 dark:text-rose-300">
              {entry.error}
            </div>
          )}

          <div className="space-y-2">
            <SettingGroupLabel>Policy</SettingGroupLabel>
            <SettingCard>
              <div className="divide-border divide-y">
                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      Enable Guardian Auto-Approval
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11.5px]">
                      Filter permission requests automatically for sessions in Auto mode
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.enabled}
                    disabled={isSaving}
                    ariaLabel="Enable Guardian Auto-Approval"
                    onChange={(enabled) => commitConfig({ enabled })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      Auto-approve threshold
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11.5px]">
                      High and Critical risk actions always escalate to human approval
                    </div>
                  </div>
                  <SettingSelect
                    value={effectiveMaxRisk}
                    options={MAX_RISK_OPTIONS}
                    disabled={isSaving || !config.enabled}
                    onChange={(val) => commitConfig({ auto_approve_max_risk: val })}
                    triggerClassName="w-[176px]"
                    menuClassName="w-[260px]"
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      Read-only workspace tools
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11.5px]">
                      Auto-allow file reads, search, and read-only Git inspection inside workspace
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.allow_read_only_tools}
                    disabled={isSaving || !config.enabled}
                    ariaLabel="Allow read-only workspace tools"
                    onChange={(allow_read_only_tools) => commitConfig({ allow_read_only_tools })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      Workspace build, lint &amp; test commands
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11.5px]">
                      Auto-allow single commands like cargo test, pytest, go test, and tsc --noEmit
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.allow_workspace_tests}
                    disabled={isSaving || !config.enabled}
                    ariaLabel="Allow workspace build and test commands"
                    onChange={(allow_workspace_tests) => commitConfig({ allow_workspace_tests })}
                  />
                </div>

                <div className="flex items-center justify-between gap-4 px-4 py-3">
                  <div className="min-w-0">
                    <div className="text-foreground text-[13px] font-medium">
                      Secondary rubric classifier for Medium risk
                    </div>
                    <div className="text-muted-foreground mt-0.5 text-[11.5px]">
                      Evaluates scoped edits within a {config.classifier_timeout_ms}ms fail-closed timeout
                    </div>
                  </div>
                  <ToggleSwitch
                    checked={config.use_ai_classifier_for_medium}
                    disabled={isSaving || !config.enabled}
                    ariaLabel="Use secondary rubric classifier for Medium risk"
                    onChange={(use_ai_classifier_for_medium) =>
                      commitConfig({ use_ai_classifier_for_medium })
                    }
                  />
                </div>
              </div>
            </SettingCard>
          </div>

          <div className="space-y-2">
            <SettingGroupLabel>Custom Rules</SettingGroupLabel>
            <SettingCard>
              <div className="divide-border divide-y">
                <PatternListEditor
                  title="Trusted command prefixes"
                  description="Single non-chained commands starting with these prefixes are treated as Low risk (never bypasses credential or destructive checks)."
                  placeholder="e.g. just test, make check"
                  items={config.trusted_command_prefixes}
                  disabled={isSaving || !config.enabled}
                  onChange={(trusted_command_prefixes) =>
                    commitConfig({ trusted_command_prefixes })
                  }
                />
                <PatternListEditor
                  title="Blocked command patterns"
                  description="Matching requests always escalate to human approval. Prefix with deny: to auto-deny."
                  placeholder="e.g. cargo publish, deny:terraform destroy"
                  items={config.blocked_command_patterns}
                  disabled={isSaving}
                  onChange={(blocked_command_patterns) =>
                    commitConfig({ blocked_command_patterns })
                  }
                />
              </div>
            </SettingCard>
          </div>

          <div className="space-y-2">
            <SettingGroupLabel>Recent Decisions</SettingGroupLabel>
            <SettingCard>
              {audits.length === 0 ? (
                <div className="text-muted-foreground flex items-center justify-center gap-2 px-4 py-7 text-[12.5px]">
                  <ShieldCheck className="h-[14px] w-[14px] opacity-65" />
                  <span>No Guardian decisions recorded on this host yet.</span>
                </div>
              ) : (
                <div className="divide-border max-h-[280px] divide-y overflow-y-auto">
                  {audits.slice(0, 40).map((item) => {
                    const risk = formatRiskBadge(item.assessment.risk_level);
                    return (
                      <div key={item.id} className="space-y-1 px-4 py-2.5">
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
                            <span className="text-foreground shrink-0 text-[11.5px] font-medium">
                              {formatDecisionLabel(item.assessment.decision)}
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
