import { useEffect, useMemo, useState } from "react";
import { Plus, Search, Shield, ShieldBan, ShieldCheck, Trash2, X } from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSelect,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { ConfirmDialog } from "@/components/ui/confirm-dialog";
import { Empty, EmptyDescription, EmptyHeader, EmptyMedia, EmptyTitle } from "@/components/ui/empty";
import { Field, FieldError, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { OrbitSpinner } from "@/components/ui/orbit-spinner";
import { SimpleTooltip } from "@/components/ui/overlays";
import {
  formatRelativeTimeLabel,
  useSharedRelativeNow,
} from "@/components/stage/stage-helpers";
import { useI18n } from "@/lib/i18n";
import type {
  PermissionRuleDecision,
  PermissionRulePatternKind,
  PermissionRuleRecord,
  PermissionRuleScopeKind,
  PermissionRuleToolKind,
  ProviderKind,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";
import {
  basenameFromPath,
  countAlwaysAllowRules,
  createHostPermissionRulesSelector,
  createInitialPermissionRuleDraft,
  defaultPatternKindForTool,
  filterPermissionRules,
  formatProviderShortLabel,
  type PermissionRuleDraft,
  type PermissionRuleFilterTab,
} from "./permission-rules-model";

const EMPTY_HOST_WORKSPACES: readonly { cwd: string; title: string }[] = Object.freeze([]);

function RuleRelativeTime({
  isoTimestamp,
  prefixKey,
}: {
  isoTimestamp: string;
  prefixKey?: "permissionRules.lastMatched";
}) {
  const { t, locale } = useI18n();
  const parsedMs = useMemo(() => {
    const ms = Date.parse(isoTimestamp);
    return Number.isFinite(ms) ? ms : 0;
  }, [isoTimestamp]);
  const nowMs = useSharedRelativeNow(parsedMs || Date.now());
  if (!parsedMs) return null;
  const rel = formatRelativeTimeLabel(parsedMs, nowMs, locale);
  return <span>{prefixKey ? t(prefixKey, { time: rel }) : rel}</span>;
}

export function PermissionRulesPanel({
  hostId,
  isOnline,
}: {
  hostId: string;
  isOnline: boolean;
}) {
  const { t } = useI18n();
  const rulesSelector = useMemo(() => createHostPermissionRulesSelector(hostId), [hostId]);
  const rulesSnap = useOrbitSelector(rulesSelector);
  const hostWorkspaces = useOrbitSelector(
    (s) => s.hostDetails[hostId]?.workspaces ?? EMPTY_HOST_WORKSPACES,
  );

  const uniqueWorkspacePaths = useMemo(() => {
    const seen = new Set<string>();
    const paths: string[] = [];
    for (const ws of hostWorkspaces) {
      const cleaned = ws.cwd.trim();
      if (cleaned && !seen.has(cleaned)) {
        seen.add(cleaned);
        paths.push(cleaned);
      }
    }
    return paths;
  }, [hostWorkspaces]);

  const [query, setQuery] = useState("");
  const [filterTab, setFilterTab] = useState<PermissionRuleFilterTab>("all");
  const [showAddForm, setShowAddForm] = useState(false);
  const [draft, setDraft] = useState<PermissionRuleDraft>(() =>
    createInitialPermissionRuleDraft(uniqueWorkspacePaths[0] ?? ""),
  );
  const [formError, setFormError] = useState<string | null>(null);
  const [revokeTarget, setRevokeTarget] = useState<PermissionRuleRecord | null>(null);
  const [confirmRevokeAllOpen, setConfirmRevokeAllOpen] = useState(false);

  useEffect(() => {
    if (isOnline && hostId) {
      void orbitActions.loadPermissionRules(hostId);
    }
  }, [isOnline, hostId]);

  const filteredRules = useMemo(
    () => filterPermissionRules(rulesSnap.rules, query, filterTab),
    [rulesSnap.rules, query, filterTab],
  );
  const alwaysAllowCount = useMemo(
    () => countAlwaysAllowRules(rulesSnap.rules),
    [rulesSnap.rules],
  );

  const filterOptions = useMemo<SettingSelectOption<PermissionRuleFilterTab>[]>(
    () => [
      { value: "all", label: t("permissionRules.filterAll") },
      { value: "always_allow", label: t("permissionRules.filterAlwaysAllow") },
      { value: "manual", label: t("permissionRules.filterManual") },
      { value: "deny", label: t("permissionRules.filterDeny") },
    ],
    [t],
  );

  const decisionOptions = useMemo<SettingSelectOption<PermissionRuleDecision>[]>(
    () => [
      { value: "allow", label: t("permissionRules.decisionAllow") },
      { value: "deny", label: t("permissionRules.decisionDeny") },
    ],
    [t],
  );

  const scopeOptions = useMemo<SettingSelectOption<PermissionRuleScopeKind>[]>(
    () => [
      { value: "project", label: t("permissionRules.scopeProject") },
      { value: "host", label: t("permissionRules.scopeHost") },
    ],
    [t],
  );

  const toolOptions = useMemo<SettingSelectOption<PermissionRuleToolKind>[]>(
    () => [
      { value: "shell", label: t("permissionRules.toolShell") },
      { value: "file_write", label: t("permissionRules.toolFileWrite") },
      { value: "file_read", label: t("permissionRules.toolFileRead") },
      { value: "mcp_tool", label: t("permissionRules.toolMcpTool") },
      { value: "network", label: t("permissionRules.toolNetwork") },
      { value: "any", label: t("permissionRules.toolAny") },
    ],
    [t],
  );

  const patternKindOptions = useMemo<SettingSelectOption<PermissionRulePatternKind>[]>(
    () => [
      { value: "command_prefix", label: t("permissionRules.patternCommandPrefix") },
      { value: "exact_command", label: t("permissionRules.patternExactCommand") },
      { value: "path_glob", label: t("permissionRules.patternPathGlob") },
      { value: "tool_name", label: t("permissionRules.patternToolName") },
    ],
    [t],
  );

  const providerOptions = useMemo<SettingSelectOption<ProviderKind | "all">[]>(
    () => [
      { value: "all", label: t("permissionRules.providerAll") },
      { value: "claude_code", label: "Claude Code" },
      { value: "codex", label: "Codex" },
      { value: "antigravity", label: "Antigravity" },
      { value: "pi", label: "Pi" },
      { value: "opencode", label: "OpenCode" },
      { value: "gemini_cli", label: "Gemini CLI" },
      { value: "cursor_agent", label: "Cursor" },
      { value: "acp", label: "ACP" },
    ],
    [t],
  );

  const toolKindLabel = (kind: PermissionRuleToolKind): string => {
    switch (kind) {
      case "shell":
        return t("permissionRules.toolShell");
      case "file_write":
        return t("permissionRules.toolFileWrite");
      case "file_read":
        return t("permissionRules.toolFileRead");
      case "mcp_tool":
        return t("permissionRules.toolMcpTool");
      case "network":
        return t("permissionRules.toolNetwork");
      case "any":
        return t("permissionRules.toolAny");
    }
  };

  const patternKindLabel = (kind: PermissionRulePatternKind): string => {
    switch (kind) {
      case "command_prefix":
        return t("permissionRules.patternCommandPrefix");
      case "exact_command":
        return t("permissionRules.patternExactCommand");
      case "path_glob":
        return t("permissionRules.patternPathGlob");
      case "tool_name":
        return t("permissionRules.patternToolName");
    }
  };

  const patternPlaceholder = (): string => {
    switch (draft.patternKind) {
      case "command_prefix":
        return t("permissionRules.patternPlaceholderCommand");
      case "exact_command":
        return t("permissionRules.patternPlaceholderExact");
      case "path_glob":
        return t("permissionRules.patternPlaceholderGlob");
      case "tool_name":
        return t("permissionRules.patternPlaceholderTool");
    }
  };

  const formatMatchCount = (count: number): string => {
    if (count <= 0) return t("permissionRules.matchCountZero");
    if (count === 1) return t("permissionRules.matchCountOne");
    return t("permissionRules.matchCountMany", { count });
  };

  const openAddForm = () => {
    setFormError(null);
    setDraft((prev) => ({
      ...createInitialPermissionRuleDraft(uniqueWorkspacePaths[0] ?? prev.projectCwd),
    }));
    setShowAddForm(true);
  };

  const handleSaveRule = async () => {
    const trimmedPattern = draft.pattern.trim();
    if (!trimmedPattern) {
      setFormError(t("permissionRules.errorPatternRequired"));
      return;
    }
    const trimmedProject = draft.projectCwd.trim();
    if (draft.scopeKind === "project" && !trimmedProject) {
      setFormError(t("permissionRules.errorProjectPathRequired"));
      return;
    }
    setFormError(null);
    const saved = await orbitActions.upsertPermissionRule(hostId, {
      scope_kind: draft.scopeKind,
      project_cwd: draft.scopeKind === "project" ? trimmedProject : null,
      provider: draft.provider === "all" ? null : draft.provider,
      tool_kind: draft.toolKind,
      pattern_kind: draft.patternKind,
      pattern: trimmedPattern,
      decision: draft.decision,
      source: "manual",
      note: draft.note.trim() || null,
    });
    if (saved) {
      setShowAddForm(false);
      setDraft(createInitialPermissionRuleDraft(uniqueWorkspacePaths[0] ?? ""));
      orbitActions.showToast(t("permissionRules.ruleSavedToast"));
    }
  };

  const handleConfirmRevokeSingle = async () => {
    if (!revokeTarget) return;
    const ok = await orbitActions.deletePermissionRule(hostId, revokeTarget.id);
    if (ok) {
      orbitActions.showToast(t("permissionRules.ruleRevokedToast"));
    }
  };

  const handleConfirmRevokeAllAlwaysAllow = async () => {
    const targets = rulesSnap.rules.filter((r) => r.source === "always_allow_grant");
    if (targets.length === 0) return;
    let removed = 0;
    for (const rule of targets) {
      const ok = await orbitActions.deletePermissionRule(hostId, rule.id);
      if (ok) removed += 1;
    }
    if (removed > 0) {
      orbitActions.showToast(
        t("permissionRules.rulesRevokedBatchToast", { count: removed }),
      );
    }
  };

  const isLoading = rulesSnap.status === "loading" && rulesSnap.rules.length === 0;

  return (
    <div className="flex flex-col gap-2">
      <SettingGroupLabel
        action={
          isOnline ? (
            <div className="flex items-center gap-1.5">
              {alwaysAllowCount >= 2 && (
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  disabled={rulesSnap.saving}
                  onClick={() => setConfirmRevokeAllOpen(true)}
                  className="text-ink-muted hover:text-signal-danger"
                >
                  {t("permissionRules.revokeAllAlwaysAllow", { count: alwaysAllowCount })}
                </Button>
              )}
              <Button
                type="button"
                variant="outline"
                size="xs"
                disabled={rulesSnap.saving}
                onClick={() => (showAddForm ? setShowAddForm(false) : openAddForm())}
              >
                {showAddForm ? (
                  <X data-icon="inline-start" />
                ) : (
                  <Plus data-icon="inline-start" />
                )}
                <span>{showAddForm ? t("common.cancel") : t("permissionRules.addRule")}</span>
              </Button>
            </div>
          ) : undefined
        }
      >
        <span className="inline-flex items-center gap-1.5">
          <span>{t("permissionRules.sectionTitle")}</span>
          {rulesSnap.rules.length > 0 && (
            <span className="text-ink-faint font-mono tabular-nums">
              {rulesSnap.rules.length}
            </span>
          )}
        </span>
      </SettingGroupLabel>

      <SettingCard>
        <div className="flex flex-col gap-2.5 px-4 py-3">
          <div className="text-ink-secondary text-[13px] leading-[1.45]">
            {t("permissionRules.sectionSubtitle")}
          </div>

          <div className="flex flex-wrap items-center gap-2">
            <div className="relative min-w-[200px] flex-1">
              <Search className="text-ink-faint pointer-events-none absolute top-1/2 left-2.5 size-3.5 -translate-y-1/2" />
              <Input
                type="text"
                value={query}
                disabled={!isOnline}
                onChange={(e) => setQuery(e.target.value)}
                placeholder={t("permissionRules.searchPlaceholder")}
                className="h-[30px] pl-8 text-[13px]"
              />
            </div>
            <SettingSelect
              value={filterTab}
              options={filterOptions}
              disabled={!isOnline}
              size="sm"
              onChange={setFilterTab}
              triggerClassName="w-[144px]"
              menuClassName="w-[168px]"
            />
          </div>

          {rulesSnap.error && (
            <div
              role="alert"
              className="border-signal-danger-border bg-signal-danger-bg text-signal-danger rounded-lg border px-3 py-2 text-[13px]"
            >
              {rulesSnap.error}
            </div>
          )}
        </div>

        {showAddForm && (
          <div className="bg-surface-recessed/55 px-4 py-3.5">
            <FieldGroup className="gap-3">
              <div className="grid grid-cols-2 gap-2.5 sm:grid-cols-3">
                <Field>
                  <FieldLabel>{t("permissionRules.decisionLabel")}</FieldLabel>
                  <SettingSelect
                    value={draft.decision}
                    options={decisionOptions}
                    disabled={rulesSnap.saving}
                    size="sm"
                    align="start"
                    onChange={(decision) => setDraft((prev) => ({ ...prev, decision }))}
                    triggerClassName="w-full"
                  />
                </Field>
                <Field>
                  <FieldLabel>{t("permissionRules.scopeLabel")}</FieldLabel>
                  <SettingSelect
                    value={draft.scopeKind}
                    options={scopeOptions}
                    disabled={rulesSnap.saving}
                    size="sm"
                    align="start"
                    onChange={(scopeKind) => setDraft((prev) => ({ ...prev, scopeKind }))}
                    triggerClassName="w-full"
                  />
                </Field>
                <Field>
                  <FieldLabel>{t("permissionRules.providerLabel")}</FieldLabel>
                  <SettingSelect
                    value={draft.provider}
                    options={providerOptions}
                    disabled={rulesSnap.saving}
                    size="sm"
                    align="start"
                    onChange={(provider) => setDraft((prev) => ({ ...prev, provider }))}
                    triggerClassName="w-full"
                  />
                </Field>
                <Field>
                  <FieldLabel>{t("permissionRules.toolLabel")}</FieldLabel>
                  <SettingSelect
                    value={draft.toolKind}
                    options={toolOptions}
                    disabled={rulesSnap.saving}
                    size="sm"
                    align="start"
                    onChange={(toolKind) =>
                      setDraft((prev) => ({
                        ...prev,
                        toolKind,
                        patternKind: defaultPatternKindForTool(toolKind),
                      }))
                    }
                    triggerClassName="w-full"
                  />
                </Field>
                <Field>
                  <FieldLabel>{t("permissionRules.patternKindLabel")}</FieldLabel>
                  <SettingSelect
                    value={draft.patternKind}
                    options={patternKindOptions}
                    disabled={rulesSnap.saving}
                    size="sm"
                    align="start"
                    onChange={(patternKind) => setDraft((prev) => ({ ...prev, patternKind }))}
                    triggerClassName="w-full"
                  />
                </Field>
              </div>

              {draft.scopeKind === "project" && (
                <Field data-invalid={Boolean(formError && !draft.projectCwd.trim()) || undefined}>
                  <FieldLabel htmlFor="orbit-perm-rule-project">
                    {t("permissionRules.projectPathLabel")}
                  </FieldLabel>
                  <Input
                    id="orbit-perm-rule-project"
                    type="text"
                    value={draft.projectCwd}
                    disabled={rulesSnap.saving}
                    aria-invalid={Boolean(formError && !draft.projectCwd.trim()) || undefined}
                    onChange={(e) => setDraft((prev) => ({ ...prev, projectCwd: e.target.value }))}
                    placeholder={t("permissionRules.projectPathPlaceholder")}
                    className="h-[30px] font-mono text-[13px]"
                  />
                  {uniqueWorkspacePaths.length > 0 && (
                    <div className="flex flex-wrap items-center gap-1 pt-0.5">
                      {uniqueWorkspacePaths.slice(0, 5).map((wsPath) => (
                        <button
                          key={wsPath}
                          type="button"
                          onClick={() => setDraft((prev) => ({ ...prev, projectCwd: wsPath }))}
                          className={cn(
                            "border-border-subtle hover:bg-surface-recessed cursor-pointer rounded-[6px] border px-2 py-0.5 font-mono text-[11px] transition-colors",
                            draft.projectCwd.trim() === wsPath
                              ? "bg-surface-recessed text-ink-primary font-medium"
                              : "bg-surface-raised text-ink-muted",
                          )}
                        >
                          {basenameFromPath(wsPath)}
                        </button>
                      ))}
                    </div>
                  )}
                </Field>
              )}

              <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                <Field data-invalid={Boolean(formError && !draft.pattern.trim()) || undefined}>
                  <FieldLabel htmlFor="orbit-perm-rule-pattern">
                    {t("permissionRules.patternLabel")}
                  </FieldLabel>
                  <Input
                    id="orbit-perm-rule-pattern"
                    type="text"
                    value={draft.pattern}
                    disabled={rulesSnap.saving}
                    aria-invalid={Boolean(formError && !draft.pattern.trim()) || undefined}
                    onChange={(e) => setDraft((prev) => ({ ...prev, pattern: e.target.value }))}
                    onKeyDown={(e) => {
                      if (e.key === "Enter") {
                        e.preventDefault();
                        void handleSaveRule();
                      }
                    }}
                    placeholder={patternPlaceholder()}
                    className="h-[30px] font-mono text-[13px]"
                  />
                </Field>

                <Field>
                  <FieldLabel htmlFor="orbit-perm-rule-note">
                    {t("permissionRules.noteLabel")}
                  </FieldLabel>
                  <Input
                    id="orbit-perm-rule-note"
                    type="text"
                    value={draft.note}
                    disabled={rulesSnap.saving}
                    onChange={(e) => setDraft((prev) => ({ ...prev, note: e.target.value }))}
                    placeholder={t("permissionRules.notePlaceholder")}
                    className="h-[30px] text-[13px]"
                  />
                </Field>
              </div>

              {formError && <FieldError>{formError}</FieldError>}

              <div className="flex items-center justify-end gap-2">
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={rulesSnap.saving}
                  onClick={() => {
                    setShowAddForm(false);
                    setFormError(null);
                  }}
                >
                  {t("common.cancel")}
                </Button>
                <Button
                  type="button"
                  size="sm"
                  disabled={rulesSnap.saving || !draft.pattern.trim()}
                  onClick={() => void handleSaveRule()}
                >
                  {rulesSnap.saving && <OrbitSpinner size={12} />}
                  <span>
                    {rulesSnap.saving
                      ? t("permissionRules.savingRule")
                      : t("permissionRules.saveRule")}
                  </span>
                </Button>
              </div>
            </FieldGroup>
          </div>
        )}

        {isLoading ? (
          <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-8 text-[13px]">
            <OrbitSpinner size={14} />
            <span>{t("permissionRules.loading")}</span>
          </div>
        ) : rulesSnap.rules.length === 0 ? (
          <Empty className="py-7">
            <EmptyHeader>
              <EmptyMedia variant="icon">
                <Shield />
              </EmptyMedia>
              <EmptyTitle className="text-[13px]">{t("permissionRules.emptyTitle")}</EmptyTitle>
              <EmptyDescription>{t("permissionRules.emptyDescription")}</EmptyDescription>
            </EmptyHeader>
          </Empty>
        ) : filteredRules.length === 0 ? (
          <div className="text-ink-muted px-4 py-6 text-center text-[13px]">
            {t("permissionRules.noFilterMatches")}
          </div>
        ) : (
          <div className="orbit-native-scroll divide-border-subtle max-h-[340px] divide-y overflow-y-auto overscroll-contain">
            {filteredRules.map((rule) => {
              const isDeny = rule.decision === "deny";
              const providerLabel =
                formatProviderShortLabel(rule.provider) ?? t("permissionRules.providerAll");
              const projectName =
                rule.scope_kind === "project" ? basenameFromPath(rule.project_cwd) : null;

              return (
                <div
                  key={rule.id}
                  className="flex items-start justify-between gap-3 px-4 py-3 transition-colors hover:bg-surface-recessed/35"
                >
                  <div className="flex min-w-0 flex-1 flex-col gap-1.5">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <Badge variant={isDeny ? "destructive" : "success"}>
                        {isDeny ? (
                          <ShieldBan className="size-3" />
                        ) : (
                          <ShieldCheck className="size-3" />
                        )}
                        <span>
                          {isDeny
                            ? t("permissionRules.decisionDeny")
                            : t("permissionRules.decisionAllow")}
                        </span>
                      </Badge>
                      <code className="text-ink-primary min-w-0 truncate font-mono text-[13px] font-medium">
                        {rule.pattern}
                      </code>
                    </div>

                    <div className="text-ink-muted flex flex-wrap items-center gap-1.5 text-[11px]">
                      {rule.scope_kind === "project" ? (
                        <SimpleTooltip label={rule.project_cwd ?? ""} side="top">
                          <Badge variant="outline" className="font-mono text-[11px]">
                            {t("permissionRules.scopeProject")}: {projectName || rule.project_cwd}
                          </Badge>
                        </SimpleTooltip>
                      ) : (
                        <Badge variant="outline" className="text-[11px]">
                          {t("permissionRules.scopeHost")}
                        </Badge>
                      )}

                      <Badge variant="secondary" className="text-[11px]">
                        {rule.source === "always_allow_grant"
                          ? t("permissionRules.sourceAlwaysAllow")
                          : t("permissionRules.sourceManual")}
                      </Badge>

                      <span>
                        {toolKindLabel(rule.tool_kind)} · {patternKindLabel(rule.pattern_kind)}
                      </span>
                      <span aria-hidden="true">·</span>
                      <span>{providerLabel}</span>
                      <span aria-hidden="true">·</span>
                      <span className="font-mono tabular-nums">
                        {formatMatchCount(rule.match_count)}
                      </span>
                      {rule.last_matched_at ? (
                        <>
                          <span aria-hidden="true">·</span>
                          <RuleRelativeTime
                            isoTimestamp={rule.last_matched_at}
                            prefixKey="permissionRules.lastMatched"
                          />
                        </>
                      ) : (
                        <>
                          <span aria-hidden="true">·</span>
                          <RuleRelativeTime isoTimestamp={rule.created_at} />
                        </>
                      )}
                    </div>

                    {rule.note && (
                      <div className="text-ink-tertiary truncate text-[11px]">{rule.note}</div>
                    )}
                  </div>

                  <SimpleTooltip label={t("permissionRules.revokeAction")} side="top">
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon-xs"
                      disabled={rulesSnap.saving}
                      aria-label={`${t("permissionRules.revokeAction")} ${rule.pattern}`}
                      onClick={() => setRevokeTarget(rule)}
                      className="text-ink-muted hover:bg-signal-danger-bg hover:text-signal-danger shrink-0"
                    >
                      <Trash2 className="size-3.5" />
                    </Button>
                  </SimpleTooltip>
                </div>
              );
            })}
          </div>
        )}
      </SettingCard>

      <ConfirmDialog
        open={revokeTarget !== null}
        onOpenChange={(open) => {
          if (!open) setRevokeTarget(null);
        }}
        title={t("permissionRules.revokeTitle")}
        description={t("permissionRules.revokeDescription")}
        detail={
          revokeTarget ? (
            <div className="flex flex-col gap-1">
              <div className="text-ink-primary font-mono text-[13px] font-medium break-all">
                {revokeTarget.pattern}
              </div>
              <div className="text-ink-muted text-[11px]">
                {revokeTarget.scope_kind === "project"
                  ? `${t("permissionRules.scopeProject")}: ${revokeTarget.project_cwd ?? ""}`
                  : t("permissionRules.scopeHost")}
              </div>
            </div>
          ) : undefined
        }
        confirmLabel={t("permissionRules.revokeAction")}
        tone="danger"
        onConfirm={handleConfirmRevokeSingle}
      />

      <ConfirmDialog
        open={confirmRevokeAllOpen}
        onOpenChange={setConfirmRevokeAllOpen}
        title={t("permissionRules.revokeAllTitle")}
        description={t("permissionRules.revokeAllDescription", { count: alwaysAllowCount })}
        confirmLabel={t("permissionRules.revokeAction")}
        tone="danger"
        onConfirm={handleConfirmRevokeAllAlwaysAllow}
      />
    </div>
  );
}
