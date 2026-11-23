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
  label: string;
  hint: string;
}[] = [
  { mode: "always", label: "Always", hint: "Injected on every prompt" },
  { mode: "glob_auto_attach", label: "Glob", hint: "Attached when mentioned/edited files match" },
  { mode: "agent_decided", label: "Agent", hint: "Indexed by description; agent reads if relevant" },
  { mode: "manual", label: "Manual", hint: "Attached via @rule:<name> in Composer" },
];

const SCOPE_OPTIONS: SettingSelectOption<RuleScope>[] = [
  { value: "project_orbit", label: ".orbit/rules/*.md", description: "Project Orbit rule" },
  { value: "project_claude", label: ".claude/rules/*.md", description: "Project Claude rule" },
  { value: "project_cursor", label: ".cursor/rules/*.mdc", description: "Project Cursor rule" },
  { value: "user_global", label: "~/.orbit/rules/*.md", description: "User global across projects" },
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
      return "Global";
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

function basenameOf(path: string): string {
  const clean = path.replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) || clean : clean;
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
  const activeHint = TRIGGER_GROUPS.find((g) => g.mode === triggerMode)?.hint ?? "";

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === "Escape" && !busy) {
        e.preventDefault();
        if (confirmDiscard) {
          setConfirmDiscard(false);
        } else {
          requestClose();
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, confirmDiscard, requestClose]);

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
        "Name must use A-Z, 0-9, `-`, `_` (optional `/` subdirectories up to 3 levels).",
      );
      return;
    }
    const parsedGlobs = triggerMode === "glob_auto_attach" ? parseGlobsInput(globsText) : [];
    if (triggerMode === "glob_auto_attach" && parsedGlobs.length === 0) {
      setError("Add at least one glob pattern (e.g. **/*.rs).");
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
        if (!nextOpen) requestClose();
      }}
    >
      <DialogContent
        closeDisabled={busy}
        className="flex max-h-[88vh] max-w-[540px] flex-col gap-0 p-0"
      >
        <DialogHeader className="px-5 pt-5 pr-12 pb-4">
          <DialogTitle>{editing ? `Edit ${editing.name}` : "New Path Rule"}</DialogTitle>
          {editing && (
            <DialogDescription className="truncate font-mono text-[11.5px]">
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
              <FieldLabel>Name</FieldLabel>
              <Input
                type="text"
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
              <FieldLabel>Scope</FieldLabel>
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
              <FieldLabel>Trigger Mode</FieldLabel>
              <span className="text-ink-muted text-[11.5px]">{activeHint}</span>
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
                  <TabsTrigger key={g.mode} value={g.mode} disabled={busy} className="text-[12px]">
                    {g.label}
                  </TabsTrigger>
                ))}
              </TabsList>
            </Tabs>
          </Field>

          {triggerMode === "glob_auto_attach" && (
            <Field>
              <FieldLabel htmlFor="rule-globs">Glob Patterns</FieldLabel>
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
                className="font-mono text-[12px]"
              />
              <FieldDescription className="text-[11px]">
                Comma-separated globs. Supports <code className="font-mono">**</code>,{" "}
                <code className="font-mono">{"{ts,tsx}"}</code>, and{" "}
                <code className="font-mono">!negation</code>.
              </FieldDescription>
            </Field>
          )}

          <Field>
            <FieldLabel htmlFor="rule-description">Description</FieldLabel>
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
                  ? "When should the agent read this rule?"
                  : "Short summary (optional)"
              }
            />
          </Field>

          <Field>
            <div className="flex items-center justify-between">
              <FieldLabel htmlFor="rule-body">Rule Instructions (Markdown)</FieldLabel>
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
              placeholder="Write concise, actionable engineering constraints..."
              className="min-h-[150px] resize-y font-mono text-[12px] leading-relaxed"
            />
          </Field>

          <Field orientation="horizontal" className="pt-0.5">
            <FieldLabel className="text-[12.5px] font-normal">Enabled</FieldLabel>
            <ToggleSwitch
              checked={enabled}
              disabled={busy}
              onChange={(next) => {
                setEnabled(next);
                setConfirmDiscard(false);
              }}
              ariaLabel="Enable rule"
            />
          </Field>

          {error && (
            <div className="rounded-[8px] border border-red-500/30 bg-red-500/8 px-3 py-2 text-[12px] text-red-600 dark:text-red-400">
              {error}
            </div>
          )}

          {confirmDiscard && (
            <div className="flex items-center justify-between gap-2 rounded-[8px] border border-amber-500/30 bg-amber-500/10 px-3 py-2 text-[12px]">
              <span className="text-amber-700 dark:text-amber-300">Discard unsaved changes?</span>
              <div className="flex items-center gap-1.5">
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  onClick={() => setConfirmDiscard(false)}
                >
                  Keep editing
                </Button>
                <Button
                  type="button"
                  size="xs"
                  onClick={onClose}
                  className="bg-amber-600 text-white hover:bg-amber-700"
                >
                  Discard
                </Button>
              </div>
            </div>
          )}

          <div className="border-border mt-1 flex items-center justify-between gap-2 border-t pt-3.5">
            <div>
              {editing && !confirmDelete && (
                <Button
                  type="button"
                  variant="ghost"
                  size="sm"
                  disabled={busy}
                  onClick={() => setConfirmDelete(true)}
                  className="text-muted-foreground hover:text-red-600 dark:hover:text-red-400"
                >
                  <Trash2 className="h-[13.5px] w-[13.5px]" />
                  Delete
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
                    Delete
                  </Button>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    disabled={busy}
                    onClick={() => setConfirmDelete(false)}
                  >
                    Keep
                  </Button>
                </div>
              )}
            </div>

            <div className="flex items-center gap-2">
              <Button type="button" variant="outline" size="sm" disabled={busy} onClick={requestClose}>
                Cancel
              </Button>
              <Button type="submit" size="sm" disabled={busy}>
                {saving && <Loader2 className="h-3.5 w-3.5 animate-spin" />}
                Save Rule
              </Button>
            </div>
          </div>
        </form>
      </DialogContent>
    </Dialog>
  );
}

export function HostRulesSection({ hostId }: { hostId: string }) {
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId]);
  const fleet = useOrbitSelector((s) => s.fleet);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);
  const pathRulesByScope = useOrbitSelector((s) => s.pathRulesByScope);

  const projectCwds = useMemo(() => {
    const seen = new Set<string>();
    const list: { path: string; label: string }[] = [];
    const add = (raw: string | null | undefined, label?: string) => {
      const clean = raw?.trim();
      if (!clean || clean === "~" || seen.has(clean)) return;
      seen.add(clean);
      list.push({ path: clean, label: label || basenameOf(clean) });
    };

    if (selectedHostId === hostId && selectedCwd) {
      add(selectedCwd);
    }
    for (const proj of fleet.projects ?? []) {
      if (proj.host_id === hostId) {
        for (const dir of proj.directories) {
          add(dir, proj.name);
        }
      }
    }
    for (const ws of hostDetail?.workspaces ?? []) {
      add(ws.cwd, ws.title);
    }
    for (const dp of hostDetail?.discovered_projects ?? []) {
      add(dp.path, dp.name);
    }
    return list;
  }, [hostId, selectedHostId, selectedCwd, fleet.projects, hostDetail]);

  const explicitGlobalRef = useRef(false);
  const [activeCwd, setActiveCwd] = useState<string>(() => projectCwds[0]?.path ?? "~");

  useEffect(() => {
    if (activeCwd === "~") {
      if (!explicitGlobalRef.current && projectCwds.length > 0) {
        setActiveCwd(projectCwds[0].path);
      }
    } else if (projectCwds.length > 0 && !projectCwds.some((p) => p.path === activeCwd)) {
      setActiveCwd(projectCwds[0].path);
    }
  }, [activeCwd, projectCwds]);

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

  const workspaceOptions = useMemo<SettingSelectOption<string>[]>(() => {
    const opts: SettingSelectOption<string>[] = projectCwds.map((p) => ({
      value: p.path,
      label: p.label,
      description: p.path,
      mono: false,
    }));
    opts.push({
      value: "~",
      label: "User Global Only",
      description: "~/.orbit/rules/*.md",
    });
    return opts;
  }, [projectCwds]);

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
    <div className="space-y-6">
      <SettingSectionHeader
        title="Rules"
        description="Path-scoped instructions from .orbit/rules, .claude/rules, and .cursor/rules."
        action={
          <div className="flex items-center gap-1.5">
            <button
              type="button"
              aria-label="Refresh rules"
              onClick={() => void refresh()}
              disabled={loading}
              className="border-border bg-background hover:bg-accent/70 text-muted-foreground hover:text-foreground inline-flex h-[30px] w-[30px] cursor-pointer items-center justify-center rounded-[8px] border transition-colors disabled:opacity-45"
            >
              <RefreshCw className={cn("h-[13.5px] w-[13.5px]", loading && "animate-spin")} />
            </button>
            <button
              type="button"
              onClick={() => setModalState({ open: true, editing: null })}
              className="bg-foreground text-background hover:opacity-90 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-[8px] px-2.5 text-[12.5px] font-medium transition-opacity"
            >
              <Plus className="h-[13.5px] w-[13.5px] stroke-[2]" />
              New Rule
            </button>
          </div>
        }
      />

      {/* Workspace Selector + Token Budget Meter */}
      <SettingCard>
        <div className="flex flex-col gap-3 px-4 py-3 sm:flex-row sm:items-center sm:justify-between">
          <div className="min-w-0">
            <div className="text-foreground text-[13px] font-medium">Workspace</div>
            <div className="text-muted-foreground truncate font-mono text-[11.5px]">
              {activeCwd === "~" ? "~/.orbit/rules" : activeCwd}
            </div>
          </div>
          <SettingSelect
            value={activeCwd}
            options={workspaceOptions}
            onChange={(next) => {
              explicitGlobalRef.current = next === "~";
              setActiveCwd(next);
            }}
            triggerClassName="max-w-[220px]"
            menuClassName="max-w-[320px]"
          />
        </div>

        <div className="space-y-2 px-4 py-3">
          <div className="flex items-center justify-between gap-2 text-[12px]">
            <span className="text-muted-foreground flex items-center gap-1.5">
              Always-active budget
              {budgetWarning && (
                <span className="inline-flex items-center gap-1 rounded bg-amber-500/12 px-1.5 py-0.5 text-[10.5px] font-medium text-amber-600 dark:text-amber-400">
                  <AlertTriangle className="h-3 w-3" />
                  High budget
                </span>
              )}
            </span>
            <span className="text-foreground/85 font-mono text-[11.5px] tabular-nums">
              {totalAlwaysTokens.toLocaleString()} / {WARN_TOKEN_BUDGET.toLocaleString()} tok
              <span className="text-muted-foreground/70">
                {" "}
                (cap {(HARD_CAP_TOKENS / 1000).toFixed(0)}k)
              </span>
            </span>
          </div>
          <div className="bg-muted h-[4px] w-full overflow-hidden rounded-full">
            <div
              style={{ width: `${budgetRatio}%` }}
              className={cn(
                "h-full rounded-full transition-all duration-200",
                totalAlwaysTokens > HARD_CAP_TOKENS
                  ? "bg-red-500"
                  : budgetWarning
                    ? "bg-amber-500"
                    : "bg-foreground/65",
              )}
            />
          </div>
        </div>
      </SettingCard>

      {/* Rule List Grouped by Trigger Mode */}
      {loading && !snapshot ? (
        <div className="text-muted-foreground flex items-center justify-center gap-2 py-12 text-[13px]">
          <Loader2 className="h-4 w-4 animate-spin" />
          Loading rules…
        </div>
      ) : groupedRules.length === 0 ? (
        <SettingCard>
          <div className="flex flex-col items-center justify-center gap-2 px-6 py-10 text-center">
            <BookOpen className="text-muted-foreground/50 h-6 w-6 stroke-[1.5]" />
            <div className="text-foreground text-[13.5px] font-medium">No rules discovered</div>
            <p className="text-muted-foreground max-w-[360px] text-[12.5px] leading-relaxed">
              Add glob-scoped rules to automatically attach conventions when editing matching files,
              or reference manual rules with <code className="font-mono">@rule:name</code>.
            </p>
            <button
              type="button"
              onClick={() => setModalState({ open: true, editing: null })}
              className="border-border bg-background hover:bg-accent/70 mt-1.5 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-[8px] border px-3 text-[12.5px] font-medium transition-colors"
            >
              <Plus className="h-3.5 w-3.5" />
              Create first rule
            </button>
          </div>
        </SettingCard>
      ) : (
        <div className="space-y-5">
          {groupedRules.map((group) => (
            <div key={group.mode} className="space-y-2">
              <SettingGroupLabel
                action={
                  <span className="text-muted-foreground/75 text-[11px]">{group.hint}</span>
                }
              >
                {group.label} ({group.items.length})
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
                        <span className="text-foreground font-mono text-[13px] font-medium">
                          {rule.name}
                        </span>
                        <span className="bg-muted text-muted-foreground rounded px-1.5 py-0.5 text-[10.5px] font-medium">
                          {scopeBadgeLabel(rule.scope)}
                        </span>
                        <span className="text-muted-foreground/80 font-mono text-[10.5px] tabular-nums">
                          ~{rule.approx_tokens.toLocaleString()} tok
                        </span>
                      </div>

                      {rule.description && (
                        <p className="text-muted-foreground text-[12px] leading-snug">
                          {rule.description}
                        </p>
                      )}

                      {rule.globs.length > 0 && (
                        <div className="flex flex-wrap gap-1 pt-0.5">
                          {rule.globs.map((g) => (
                            <span
                              key={g}
                              className="border-border/80 bg-background/80 text-muted-foreground rounded-[5px] border px-1.5 py-0.5 font-mono text-[11px]"
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
                        aria-label={`Edit ${rule.name}`}
                        onClick={() => setModalState({ open: true, editing: rule })}
                        className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-[7px] transition-colors"
                      >
                        <Pencil className="h-[13px] w-[13px]" />
                      </button>
                      <ToggleSwitch
                        checked={rule.enabled}
                        disabled={togglingIds.has(rule.id)}
                        onChange={(next) => void handleToggleRule(rule, next)}
                        ariaLabel={`Toggle ${rule.name}`}
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
              modalState.editing ? `Updated rule ${params.name}` : `Created rule ${params.name}`,
            );
          }}
          onDelete={async (rule) => {
            await orbitActions.deletePathRule({
              hostId,
              cwd: activeCwd,
              path: rule.path,
            });
            orbitActions.showToast(`Deleted rule ${rule.name}`);
          }}
        />
      )}
    </div>
  );
}
