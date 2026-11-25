import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  AlertTriangle,
  BookOpen,
  Loader2,
  Pencil,
  Plus,
  RefreshCw,
  Trash2,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import { t, type MessageKey } from "@/lib/i18n";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  Tabs,
  TabsList,
  TabsTrigger,
} from "@/components/ui/dialog-tabs";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import type {
  PathScopedRule,
  RuleScope,
  RuleTriggerMode,
  UpsertPathRuleParams,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const WARN_TOKEN_BUDGET = 8_000;
const HARD_CAP_TOKENS = 24_000;
const RULE_PATH_REGEX =
  /^[A-Za-z0-9][A-Za-z0-9_-]{0,63}(?:\/[A-Za-z0-9][A-Za-z0-9_-]{0,63}){0,3}$/;

const TRIGGER_GROUPS: {
  mode: RuleTriggerMode;
  label: MessageKey;
  hint: MessageKey;
}[] = [
  { mode: "always", label: "context.triggerAlways", hint: "context.triggerAlwaysHint" },
  { mode: "glob_auto_attach", label: "context.triggerGlob", hint: "context.triggerGlobHint" },
  { mode: "agent_decided", label: "context.triggerAgent", hint: "context.triggerAgentHint" },
  { mode: "manual", label: "context.triggerManual", hint: "context.triggerManualHint" },
];

const SCOPE_OPTIONS: SettingSelectOption<RuleScope>[] = [
  { value: "project_orbit", label: ".orbit/rules/*.md" },
  { value: "project_claude", label: ".claude/rules/*.md" },
  { value: "project_cursor", label: ".cursor/rules/*.mdc" },
  { value: "user_global", label: "~/.orbit/rules/*.md" },
];

function scopeBadgeLabel(scope: RuleScope): string {
  switch (scope) {
    case "project_orbit":
      return ".orbit";
    case "project_claude":
      return ".claude";
    case "project_cursor":
      return ".cursor";
    case "user_global":
      return t("context.scopeGlobal");
  }
}

function estimateTokensClient(text: string): number {
  if (!text.trim()) return 0;
  let ascii = 0;
  let nonAscii = 0;
  for (const ch of text) {
    if (ch.charCodeAt(0) < 128) ascii += 1;
    else nonAscii += 1;
  }
  return Math.ceil((ascii + nonAscii * 3) / 4);
}

function parseGlobsInput(raw: string): string[] {
  const out: string[] = [];
  for (const line of raw.split("\n")) {
    let cur = "";
    let braceDepth = 0;
    for (let i = 0; i < line.length; i += 1) {
      const ch = line[i];
      if (ch === "{") {
        braceDepth += 1;
        cur += ch;
      } else if (ch === "}") {
        braceDepth = Math.max(0, braceDepth - 1);
        cur += ch;
      } else if (ch === "," && braceDepth === 0) {
        const trimmed = cur.trim();
        if (trimmed) out.push(trimmed);
        cur = "";
      } else {
        cur += ch;
      }
    }
    const tail = cur.trim();
    if (tail) out.push(tail);
  }
  return Array.from(new Set(out));
}

function RuleEditorModal({
  hostId,
  cwd,
  hasProjectCwd,
  editing,
  onClose,
  onSave,
  onDelete,
}: {
  hostId: string;
  cwd: string;
  hasProjectCwd: boolean;
  editing: PathScopedRule | null;
  onClose: () => void;
  onSave: (params: UpsertPathRuleParams) => Promise<void>;
  onDelete: (rule: PathScopedRule) => Promise<void>;
}) {
  const initialScope: RuleScope =
    editing?.scope ?? (hasProjectCwd ? "project_orbit" : "user_global");
  const initialName = editing?.name ?? "";
  const initialTrigger: RuleTriggerMode = editing?.trigger_mode ?? "glob_auto_attach";
  const initialGlobs = editing ? editing.globs.join(", ") : "";
  const initialDesc = editing?.description ?? "";
  const initialBody = editing?.body ?? "";
  const initialEnabled = editing?.enabled ?? true;

  const [scope, setScope] = useState<RuleScope>(initialScope);
  const [name, setName] = useState(initialName);
  const [triggerMode, setTriggerMode] = useState<RuleTriggerMode>(initialTrigger);
  const [globsText, setGlobsText] = useState(initialGlobs);
  const [description, setDescription] = useState(initialDesc);
  const [body, setBody] = useState(initialBody);
  const [enabled, setEnabled] = useState(initialEnabled);
  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [confirmDiscard, setConfirmDiscard] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const busy = saving || deleting;
  const isDirty =
    scope !== initialScope ||
    name !== initialName ||
    triggerMode !== initialTrigger ||
    globsText !== initialGlobs ||
    description !== initialDesc ||
    body !== initialBody ||
    enabled !== initialEnabled;

  const requestClose = useCallback(() => {
    if (busy) return;
    if (isDirty) {
      setConfirmDiscard(true);
    } else {
      onClose();
    }
  }, [busy, isDirty, onClose]);

  const approxTokens = useMemo(() => estimateTokensClient(body), [body]);
  const activeHint = TRIGGER_GROUPS.find((g) => g.mode === triggerMode)?.hint;

  const scopeOptions = useMemo(
    () =>
      SCOPE_OPTIONS.map((opt) => ({
        ...opt,
        disabled: !hasProjectCwd && opt.value !== "user_global",
      })),
    [hasProjectCwd],
  );

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanName = name.trim();
    if (!editing && !RULE_PATH_REGEX.test(cleanName)) {
      setError(
        t("context.ruleNameError"),
      );
      return;
    }
    const parsedGlobs = triggerMode === "glob_auto_attach" ? parseGlobsInput(globsText) : [];
    if (triggerMode === "glob_auto_attach" && parsedGlobs.length === 0) {
      setError(t("context.ruleGlobError"));
      return;
    }

    setError(null);
    setConfirmDiscard(false);
    setSaving(true);
    try {
      await onSave({
        hostId,
        cwd,
        scope,
        name: cleanName,
        description: description.trim(),
        triggerMode,
        globs: parsedGlobs,
        enabled,
        body,
        expectedEtag: editing?.etag ?? null,
      });
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
    } finally {
      setSaving(false);
    }
  };

  const handleDelete = async () => {
    if (!editing) return;
    setError(null);
    setDeleting(true);
    try {
      await onDelete(editing);
      onClose();
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err));
      setConfirmDelete(false);
    } finally {
      setDeleting(false);
    }
  };

  return (
    <Dialog
      open
      onOpenChange={(nextOpen) => {
        if (!nextOpen) {
          if (confirmDiscard && !busy) setConfirmDiscard(false);
          else requestClose();
        }
      }}
    >
      <DialogContent
        closeDisabled={busy}
        className="flex max-h-[88vh] max-w-[540px] flex-col gap-0 p-0"
      >
        <DialogHeader className="px-5 pt-5 pr-12 pb-4">
          <DialogTitle>{editing ? t("context.editRule", { name: editing.name }) : t("context.newRule")}</DialogTitle>
          {editing && (
            <DialogDescription className="truncate font-mono text-[11px]">
              {editing.relative_path}
            </DialogDescription>
          )}
        </DialogHeader>

        <form
          onSubmit={(e) => void handleSubmit(e)}
          className="orbit-native-scroll flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto overscroll-contain px-5 pb-5"
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <Field>
              <FieldLabel htmlFor="rule-name">{t("context.name")}</FieldLabel>
              <Input
                id="rule-name" type="text"
                value={name}
                disabled={Boolean(editing) || busy}
                onChange={(e) => {
                  setName(e.target.value);
                  setConfirmDiscard(false);
                }}
                placeholder="rust-guidelines"
                className="font-mono"
              />
            </Field>

            <Field>
              <FieldLabel>{t("context.scope")}</FieldLabel>
              <SettingSelect
                value={scope}
                options={scopeOptions}
                disabled={Boolean(editing) || busy}
                onChange={(next) => {
                  setScope(next);
                  setConfirmDiscard(false);
                }}
                triggerClassName="h-[32px] w-full"
              />
            </Field>
          </div>

          <Field>
            <div className="flex items-center justify-between">
              <FieldLabel>{t("context.triggerMode")}</FieldLabel>
              <span className="text-ink-muted text-[11px]">{activeHint && t(activeHint)}</span>
            </div>
            <Tabs
              value={triggerMode}
              onValueChange={(next) => {
                setTriggerMode(next as RuleTriggerMode);
                setConfirmDiscard(false);
              }}
            >
              <TabsList className="grid w-full grid-cols-4">
                {TRIGGER_GROUPS.map((g) => (
                  <TabsTrigger key={g.mode} value={g.mode} disabled={busy} className="text-[13px]">
                    {t(g.label)}
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
          </Field>

          {triggerMode === "glob_auto_attach" && (
            <Field>
              <FieldLabel htmlFor="rule-globs">{t("context.globPatterns")}</FieldLabel>
              <Input
                id="rule-globs"
                type="text"
                value={globsText}
                disabled={busy}
                onChange={(e) => {
                  setGlobsText(e.target.value);
                  setConfirmDiscard(false);
                }}
                placeholder="**/*.rs, !**/tests/**"
                className="font-mono text-[13px]"
              />
              <FieldDescription className="text-[11px]">
                {t("context.globHint")}
              </FieldDescription>
            </Field>
          )}

          <Field>
            <FieldLabel htmlFor="rule-description">{t("context.ruleDescription")}</FieldLabel>
            <Input
              id="rule-description"
              type="text"
              value={description}
              disabled={busy}
              onChange={(e) => {
                setDescription(e.target.value);
                setConfirmDiscard(false);
              }}
              placeholder={
                triggerMode === "agent_decided"
                  ? t("context.whenToRead")
                  : t("context.optionalSummary")
              }
            />
          </Field>

          <Field>
            <div className="flex items-center justify-between">
              <FieldLabel htmlFor="rule-body">{t("context.ruleBody")}</FieldLabel>
              <span className="text-ink-muted text-[11px] tabular-nums">
                ~{approxTokens.toLocaleString()} tok
              </span>
            </div>
            <Textarea
              id="rule-body"
              value={body}
              disabled={busy}
              onChange={(e) => {
                setBody(e.target.value);
                setConfirmDiscard(false);
              }}
              rows={8}
              placeholder={t("context.rulePlaceholder")}
              className="min-h-[150px] resize-y font-mono text-[13px] leading-relaxed"
            />
          </Field>

          <Field orientation="horizontal" className="pt-0.5">
            <FieldLabel className="text-[13px] font-normal">{t("context.enabled")}</FieldLabel>
            <ToggleSwitch
              checked={enabled}
              disabled={busy}
              onChange={(next) => {
                setEnabled(next);
                setConfirmDiscard(false);
              }}
              ariaLabel={t("context.enabled")}
            />
          </Field>

          {error && (
            <div className="rounded-[8px] border border-signal-danger-border bg-signal-danger-bg px-3 py-2 text-[13px] text-signal-danger">
              {error}
            </div>
          )}

          {confirmDiscard && (
            <div className="flex items-center justify-between gap-2 rounded-[8px] border border-signal-warning-border bg-signal-warning-bg px-3 py-2 text-[13px]">
              <span className="text-signal-warning">{t("context.discardTitle")}</span>
              <div className="flex items-center gap-1.5">
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  onClick={() => setConfirmDiscard(false)}
                >
                  {t("context.keepEditing")}
                </Button>
                <Button
                  type="button"
                  size="xs"
                  onClick={onClose}
                  className="border-signal-warning-border bg-signal-warning-bg text-signal-warning border hover:bg-surface-raised"
                >
                  {t("review.discard")}
                </Button>
              </div>
            </div>
          )}

          <div className="border-border-subtle mt-1 flex items-center justify-between gap-2 border-t pt-3.5">
            <div>
              {editing && !confirmDelete && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onClick={() => setConfirmDelete(true)}
                  className="text-ink-muted hover:text-signal-danger"
                >
                  <Trash2 className="h-[14px] w-[14px]" />
                  {t("common.delete")}
                </Button>
              )}
              {editing && confirmDelete && (
                <div className="flex items-center gap-1.5">
                  <Button
                    type="button"
                    variant="destructive"
                    size="sm"
                    disabled={busy}
                    onClick={() => void handleDelete()}
                  >
                    {deleting && <Loader2 className="h-3 w-3 animate-spin" />}
                    {t("common.delete")}
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={busy}
                    onClick={() => setConfirmDelete(false)}
                  >
                    {t("common.cancel")}
                  </Button>
                </div>
              )}
            </div>

            <div className="flex items-center gap-2">
              <Button type="button" variant="outline" size="sm" disabled={busy} onClick={requestClose}>
                {t("common.cancel")}
              </Button>
              <Button type="submit" size="sm" disabled={busy}>
                {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                {t("permissionRules.saveRule")}
              </Button>
            </div>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function HostRulesSection({ hostId, cwd: activeCwd }: { hostId: string; cwd: string }) {
  const pathRulesByScope = useOrbitSelector((s) => s.pathRulesByScope);
  const refreshSeqRef = useRef(0);
  const [loading, setLoading] = useState(false);
  const [togglingIds, setTogglingIds] = useState<Set<string>>(() => new Set());
  const [modalState, setModalState] = useState<
    { open: false } | { open: true; editing: PathScopedRule | null }
  >({ open: false });

  const scopeKey = `${hostId}::${activeCwd}`;
  const snapshot = pathRulesByScope[scopeKey] ?? null;

  const refresh = useCallback(async () => {
    const seq = refreshSeqRef.current + 1;
    refreshSeqRef.current = seq;
    setLoading(true);
    try {
      await orbitActions.refreshPathRules(hostId, activeCwd);
    } finally {
      if (refreshSeqRef.current === seq) {
        setLoading(false);
      }
    }
  }, [hostId, activeCwd]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const rules = snapshot?.rules ?? [];
  const totalAlwaysTokens = snapshot?.total_always_tokens ?? 0;
  const budgetWarning = snapshot?.token_budget_warning ?? false;
  const budgetRatio = Math.min(100, Math.round((totalAlwaysTokens / WARN_TOKEN_BUDGET) * 100));

  const groupedRules = useMemo(() => {
    return TRIGGER_GROUPS.map((group) => ({
      ...group,
      items: rules.filter((r) => r.trigger_mode === group.mode),
    })).filter((g) => g.items.length > 0);
  }, [rules]);

  const handleToggleRule = async (rule: PathScopedRule, nextEnabled: boolean) => {
    setTogglingIds((prev) => {
      const next = new Set(prev);
      next.add(rule.id);
      return next;
    });
    try {
      await orbitActions.upsertPathRule({
        hostId,
        cwd: activeCwd,
        scope: rule.scope,
        name: rule.name,
        description: rule.description,
        triggerMode: rule.trigger_mode,
        globs: rule.globs,
        enabled: nextEnabled,
        body: rule.body,
        expectedEtag: rule.etag,
      });
    } catch (err) {
      orbitActions.showToast(err instanceof Error ? err.message : String(err));
    } finally {
      setTogglingIds((prev) => {
        const next = new Set(prev);
        next.delete(rule.id);
        return next;
      });
    }
  };

  return (
    <div data-setting-id="settings.subTabRules" className="space-y-6">
      <SettingSectionHeader
        subpage
        title={t("context.pathRules")}
        description={t("context.pathRulesDescription")}
        action={
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              aria-label={t("common.refresh")}
              onClick={() => void refresh()}
              disabled={loading}
              className="border-border-subtle bg-surface-base hover:bg-surface-recessed text-ink-muted hover:text-ink-primary inline-flex h-[30px] w-[30px] cursor-pointer items-center justify-center rounded-[8px] border transition-colors disabled:opacity-45"
            >
              <RefreshCw className={cn("h-[14px] w-[14px]", loading && "animate-spin")} />
            </button>
            <button
              type="button"
              onClick={() => setModalState({ open: true, editing: null })}
              className="bg-ink-primary text-surface-base hover:opacity-90 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-[8px] px-2.5 text-[13px] font-medium transition-opacity"
            >
              <Plus className="h-[14px] w-[14px] stroke-[2]" />
              {t("context.newRule")}
            </button>
          </div>
        }
      />

      <SettingCard>
        <div data-setting-id="context.ruleBudget" className="space-y-2 px-4 py-3">
          <div className="flex items-center justify-between gap-2 text-[13px]">
            <span className="text-ink-muted flex items-center gap-1.5">
              {t("context.ruleBudget")}
              {budgetWarning && (
                <span className="inline-flex items-center gap-1 rounded bg-signal-warning-bg px-1.5 py-0.5 text-[11px] font-medium text-signal-warning">
                  <AlertTriangle className="h-3 w-3" />
                  {t("context.highBudget")}
                </span>
              )}
            </span>
            <span className="text-ink-secondary font-mono text-[11px] tabular-nums">
              {totalAlwaysTokens.toLocaleString()} / {WARN_TOKEN_BUDGET.toLocaleString()} tok
              <span className="text-ink-muted">
                {" "}
                ({t("context.maxTokens", { count: HARD_CAP_TOKENS.toLocaleString() })})
              </span>
            </span>
          </div>
          <div className="bg-surface-recessed h-[4px] w-full overflow-hidden rounded-full">
            <div
              style={{ width: `${budgetRatio}%` }}
              className={cn(
                "h-full rounded-full transition-none",
                totalAlwaysTokens > HARD_CAP_TOKENS
                  ? "bg-signal-danger"
                  : budgetWarning
                    ? "bg-signal-warning"
                    : "bg-ink-primary/65",
              )}
            />
          </div>
        </div>
      </SettingCard>

      {/* Rule List Grouped by Trigger Mode */}
      {loading && !snapshot ? (
        <div className="text-ink-muted flex items-center justify-center gap-2 py-12 text-[13px]">
          <Loader2 className="h-4 w-4 animate-spin" />
          {t("common.loading")}
        </div>
      ) : groupedRules.length === 0 ? (
        <SettingCard>
          <div className="flex flex-col items-center justify-center gap-2 px-6 py-10 text-center">
            <BookOpen className="text-ink-muted h-6 w-6 stroke-[1.5]" />
            <div className="text-ink-primary text-[13px] font-medium">{t("context.noRules")}</div>
            <p className="text-ink-muted max-w-[360px] text-[13px] leading-relaxed">
              {t("context.noRulesHint")}
            </p>
            <button
              type="button"
              onClick={() => setModalState({ open: true, editing: null })}
              className="border-border-subtle bg-surface-base hover:bg-surface-recessed mt-1.5 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-[8px] border px-3 text-[13px] font-medium transition-colors"
            >
              <Plus className="h-3.5 w-3.5" />
              {t("permissionRules.addRule")}
            </button>
          </div>
        </SettingCard>
      ) : (
        <div className="space-y-5">
          {groupedRules.map((group) => (
            <div key={group.mode} className="space-y-2">
              <SettingGroupLabel
                action={
                  <span className="text-ink-muted text-[11px]">{t(group.hint)}</span>
                }
              >
                {t(group.label)} ({group.items.length})
              </SettingGroupLabel>
              <SettingCard>
                {group.items.map((rule) => (
                  <div
                    key={rule.id}
                    className={cn(
                      "flex items-start justify-between gap-3 px-4 py-3 transition-opacity",
                      !rule.enabled && "opacity-55",
                    )}
                  >
                    <div className="min-w-0 flex-1 space-y-1">
                      <div className="flex flex-wrap items-center gap-1.5">
                        <span className="text-ink-primary font-mono text-[13px] font-medium">
                          {rule.name}
                        </span>
                        <span className="bg-surface-recessed text-ink-muted rounded px-1.5 py-0.5 text-[11px] font-medium">
                          {scopeBadgeLabel(rule.scope)}
                        </span>
                        <span className="text-ink-muted font-mono text-[11px] tabular-nums">
                          ~{rule.approx_tokens.toLocaleString()} tok
                        </span>
                      </div>

                      {rule.description && (
                        <p className="text-ink-muted text-[13px] leading-snug">
                          {rule.description}
                        </p>
                      )}

                      {rule.globs.length > 0 && (
                        <div className="flex flex-wrap gap-1 pt-0.5">
                          {rule.globs.map((g) => (
                            <span
                              key={g}
                              className="border-border-subtle bg-surface-base text-ink-muted rounded-[5px] border px-1.5 py-0.5 font-mono text-[11px]"
                            >
                              {g}
                            </span>
                          ))}
                        </div>
                      )}
                    </div>

                    <div className="flex shrink-0 items-center gap-2 pt-0.5">
                      <button
                        type="button"
                        aria-label={t("context.editRule", { name: rule.name })}
                        onClick={() => setModalState({ open: true, editing: rule })}
                        className="text-ink-muted hover:text-ink-primary hover:bg-surface-recessed inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-[7px] transition-colors"
                      >
                        <Pencil className="h-[13px] w-[13px]" />
                      </button>
                      <ToggleSwitch
                        checked={rule.enabled}
                        disabled={togglingIds.has(rule.id)}
                        onChange={(next) => void handleToggleRule(rule, next)}
                        ariaLabel={t("context.toggleRule", { name: rule.name })}
                      />
                    </div>
                  </div>
                ))}
              </SettingCard>
            </div>
          ))}
        </div>
      )}

      {modalState.open && (
        <RuleEditorModal
          hostId={hostId}
          cwd={activeCwd}
          hasProjectCwd={activeCwd !== "~"}
          editing={modalState.editing}
          onClose={() => setModalState({ open: false })}
          onSave={async (params) => {
            await orbitActions.upsertPathRule(params);
            orbitActions.showToast(
              t("permissionRules.ruleSavedToast"),
            );
          }}
          onDelete={async (rule) => {
            await orbitActions.deletePathRule({
              hostId,
              cwd: activeCwd,
              path: rule.path,
            });
            orbitActions.showToast(t("context.deletedRule", { name: rule.name }));
          }}
        />
      )}
    </div>
  );
}
