import { memo, useCallback, useEffect, useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  Globe,
  Loader2,
  Play,
  Plus,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Terminal,
  Trash2,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingDrawer,
  SettingResourceRow,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
  useHostIsOnline,
} from "@/components/settings/settings-primitives";
import { Button } from "@/components/ui/button";
import { IconButton } from "@/components/ui/icon-button";
import {
  Field,
  FieldDescription,
  FieldGroup,
  FieldLabel,
} from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { type MessageKey, getLocale, t, useI18n } from "@/lib/i18n";
import type {
  HookAction,
  HookDecision,
  HookEventKind,
  HookExecutionRecord,
  HookScope,
  LifecycleHookRule,
} from "@/lib/orbit-ipc";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

const EMPTY_HOOKS: readonly LifecycleHookRule[] = [];
const EMPTY_EXECUTIONS: readonly HookExecutionRecord[] = [];
const DEFAULT_TIMEOUT_MS = 10_000;
const MAX_TIMEOUT_MS = 30_000;

const HOOK_EVENTS: {
  id: HookEventKind;
  labelKey: MessageKey;
  hintKey: MessageKey;
  matcherPlaceholder: string;
  samplePayload: Record<string, unknown>;
}[] = [
  {
    id: "session_start",
    labelKey: "hooks.eventSessionStart",
    hintKey: "hooks.eventSessionStartHint",
    matcherPlaceholder: "claude_code|codex",
    samplePayload: { provider: "codex", target: "codex" },
  },
  {
    id: "pre_prompt_submit",
    labelKey: "hooks.eventPrePromptSubmit",
    hintKey: "hooks.eventPrePromptSubmitHint",
    matcherPlaceholder: "claude_code|codex",
    samplePayload: {
      provider: "codex",
      target: "codex",
      prompt: "Dry-run test prompt",
    },
  },
  {
    id: "pre_tool_use",
    labelKey: "hooks.eventPreToolUse",
    hintKey: "hooks.eventPreToolUseHint",
    matcherPlaceholder: "Bash|shell|file_edit|create_agent",
    samplePayload: {
      tool_name: "shell",
      target: "Bash",
      tool_input: { command: "cargo test" },
    },
  },
  {
    id: "post_tool_use",
    labelKey: "hooks.eventPostToolUse",
    hintKey: "hooks.eventPostToolUseHint",
    matcherPlaceholder: "Bash|shell|file_edit|create_agent",
    samplePayload: {
      tool_name: "shell",
      target: "Bash",
      status: "completed",
      exit_code: 0,
      output_preview: "ok",
      modified_files: [],
    },
  },
  {
    id: "post_file_edit",
    labelKey: "hooks.eventPostFileEdit",
    hintKey: "hooks.eventPostFileEditHint",
    matcherPlaceholder: "*.rs|*.ts",
    samplePayload: { path: "src/lib.rs", target: "src/lib.rs" },
  },
  {
    id: "notification",
    labelKey: "hooks.eventNotification",
    hintKey: "hooks.eventNotificationHint",
    matcherPlaceholder: "awaiting_permission|agent_error|turn_completed",
    samplePayload: {
      kind: "awaiting_permission",
      target: "awaiting_permission",
      agent_title: "Dry-run session",
      message: "Awaiting tool approval",
    },
  },
  {
    id: "turn_stop",
    labelKey: "hooks.eventTurnStop",
    hintKey: "hooks.eventTurnStopHint",
    matcherPlaceholder: "claude_code|codex",
    samplePayload: { provider: "claude_code", target: "claude_code" },
  },
  {
    id: "session_end",
    labelKey: "hooks.eventSessionEnd",
    hintKey: "hooks.eventSessionEndHint",
    matcherPlaceholder: "claude_code|codex",
    samplePayload: {
      provider: "claude_code",
      target: "claude_code",
      reason: "archived",
    },
  },
];

function eventMeta(kind: HookEventKind) {
  return HOOK_EVENTS.find((e) => e.id === kind) ?? HOOK_EVENTS[0];
}

function formatActionPreview(action: HookAction): string {
  if (action.type === "command") {
    const joined = [action.command, ...action.args].join(" ").trim();
    return joined || "(command)";
  }
  return `POST ${action.url}`;
}

function formatTimeoutLabel(ms: number): string {
  if (ms >= 1000 && ms % 1000 === 0) return `${ms / 1000}s`;
  return `${ms}ms`;
}

function formatRelativeTime(tsMs: number): string {
  const diffSec = Math.max(0, Math.floor((Date.now() - tsMs) / 1000));
  const relative = new Intl.RelativeTimeFormat(getLocale(), {
    numeric: "auto",
    style: "short",
  });
  if (diffSec < 60) return relative.format(-diffSec, "second");
  const minutes = Math.floor(diffSec / 60);
  if (minutes < 60) return relative.format(-minutes, "minute");
  const hours = Math.floor(minutes / 60);
  return hours < 24
    ? relative.format(-hours, "hour")
    : relative.format(-Math.floor(hours / 24), "day");
}

function decisionPillClass(decision: HookDecision): string {
  switch (decision) {
    case "allow":
      return "bg-signal-success-bg text-signal-success";
    case "block":
      return "bg-signal-danger-bg text-signal-danger";
    case "modify":
      return "bg-signal-warning-bg text-signal-warning";
  }
}

function decisionLabel(decision: HookDecision): string {
  switch (decision) {
    case "allow":
      return t("approval.allow");
    case "block":
      return t("devTools.block");
    case "modify":
      return t("devTools.modify");
  }
}

function serializeKeyValueLines(
  map: Record<string, string>,
  sep: string,
): string {
  return Object.entries(map)
    .map(([k, v]) => `${k}${sep}${v}`)
    .join("\n");
}

function parseKeyValueLines(
  raw: string,
  sep: "=" | ":",
): { values: Record<string, string>; invalidLine: string | null } {
  const values: Record<string, string> = {};
  for (const line of raw.split("\n")) {
    const trimmed = line.trim();
    if (!trimmed) continue;
    const idx = trimmed.indexOf(sep);
    if (idx <= 0) {
      return { values, invalidLine: trimmed };
    }
    const key = trimmed.slice(0, idx).trim();
    const val = trimmed.slice(idx + 1).trim();
    if (!key) {
      return { values, invalidLine: trimmed };
    }
    values[key] = val;
  }
  return { values, invalidLine: null };
}

function hasMaskedValues(raw: string): boolean {
  return raw.includes("••••");
}

function HookEditorDrawer({
  open,
  onClosed,
  hasProjectCwd,
  editing,
  onClose,
  onSave,
  onDelete,
}: {
  open: boolean;
  onClosed: () => void;
  hasProjectCwd: boolean;
  editing: LifecycleHookRule | null;
  onClose: () => void;
  onSave: (rule: LifecycleHookRule) => Promise<void>;
  onDelete: (rule: LifecycleHookRule) => Promise<void>;
}) {
  const { t } = useI18n();
  const scope: HookScope =
    editing?.scope ?? (hasProjectCwd ? "project_orbit" : "user_global");
  const [event, setEvent] = useState<HookEventKind>(
    () => editing?.event ?? "pre_tool_use",
  );
  const eventOptions: SettingSelectOption<HookEventKind>[] = useMemo(
    () =>
      HOOK_EVENTS.map((ev) => ({
        value: ev.id,
        label: t(ev.labelKey),
        description: t(ev.hintKey),
      })),
    [t],
  );
  const [name, setName] = useState(() => editing?.name ?? "");
  const [description, setDescription] = useState(
    () => editing?.description ?? "",
  );
  const [matcher, setMatcher] = useState(() => editing?.matcher ?? "");
  const [timeoutMs, setTimeoutMs] = useState(() =>
    String(editing?.timeout_ms ?? DEFAULT_TIMEOUT_MS),
  );
  const [enabled, setEnabled] = useState(() => editing?.enabled ?? true);

  const [actionKind, setActionKind] = useState<"command" | "http_webhook">(
    () => editing?.action.type ?? "command",
  );
  const [command, setCommand] = useState(() =>
    editing?.action.type === "command" ? editing.action.command : "",
  );
  const [argsText, setArgsText] = useState(() =>
    editing?.action.type === "command" ? editing.action.args.join("\n") : "",
  );
  const [envText, setEnvText] = useState(() =>
    editing?.action.type === "command"
      ? serializeKeyValueLines(editing.action.env, "=")
      : "",
  );
  const [webhookUrl, setWebhookUrl] = useState(() =>
    editing?.action.type === "http_webhook" ? editing.action.url : "",
  );
  const [headersText, setHeadersText] = useState(() =>
    editing?.action.type === "http_webhook"
      ? serializeKeyValueLines(editing.action.headers, ": ")
      : "",
  );

  const [saving, setSaving] = useState(false);
  const [deleting, setDeleting] = useState(false);
  const [confirmDelete, setConfirmDelete] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const busy = saving || deleting;

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanName = name.trim();
    if (!cleanName) {
      setError(t("devTools.requireName"));
      return;
    }

    const parsedTimeout = Number.parseInt(timeoutMs, 10);
    const boundedTimeout = Number.isFinite(parsedTimeout)
      ? Math.min(MAX_TIMEOUT_MS, Math.max(100, parsedTimeout))
      : DEFAULT_TIMEOUT_MS;

    let action: HookAction;
    if (actionKind === "command") {
      const cleanCmd = command.trim();
      if (!cleanCmd) {
        setError(t("devTools.requireCommand"));
        return;
      }
      const args = argsText
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      const parsedEnv = parseKeyValueLines(envText, "=");
      if (parsedEnv.invalidLine) {
        setError(t("devTools.requireEnvLines"));
        return;
      }
      action = {
        type: "command",
        command: cleanCmd,
        args,
        env: parsedEnv.values,
      };
    } else {
      const cleanUrl = webhookUrl.trim();
      if (!cleanUrl) {
        setError(t("devTools.requireHttp"));
        return;
      }
      const parsedHeaders = parseKeyValueLines(headersText, ":");
      if (parsedHeaders.invalidLine) {
        setError(t("devTools.requireHeaderLines"));
        return;
      }
      action = {
        type: "http_webhook",
        url: cleanUrl,
        headers: parsedHeaders.values,
      };
    }

    const rule: LifecycleHookRule = {
      id: editing?.id ?? "",
      name: cleanName,
      scope,
      event,
      matcher: matcher.trim() || null,
      action,
      timeout_ms: boundedTimeout,
      enabled,
      trusted: editing?.trusted ?? scope === "user_global",
      description: description.trim() || null,
    };

    setError(null);
    setSaving(true);
    try {
      await onSave(rule);
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
    <SettingDrawer
      open={open}
      onOpenChangeComplete={(visible) => {
        if (!visible) onClosed();
      }}
      onOpenChange={(open) => {
        if (!open && !busy) onClose();
      }}
      title={
        editing
          ? t("hooks.editHookTitle", { name: editing.name })
          : t("hooks.addHookTitle")
      }
      description={
        scope === "project_orbit" ? t("devTools.project") : t("devTools.global")
      }
    >
      <form
        onSubmit={(e) => void handleSubmit(e)}
        className="flex flex-col gap-[16px]"
      >
        <Field>
          <FieldLabel htmlFor="hook-name">{t("devTools.name")}</FieldLabel>
          <Input
            type="text"
            value={name}
            disabled={busy}
            onChange={(e) => setName(e.target.value)}
            id="hook-name"
            placeholder="rustfmt"
          />
        </Field>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field>
            <FieldLabel>{t("devTools.event")}</FieldLabel>
            <SettingSelect
              ariaLabel={t("devTools.event")}
              value={event}
              options={eventOptions}
              disabled={busy}
              onChange={setEvent}
              triggerClassName="h-[32px] w-full"
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="hook-timeout">
              {t("devTools.timeout")}
            </FieldLabel>
            <Input
              id="hook-timeout"
              type="number"
              min={100}
              max={MAX_TIMEOUT_MS}
              step={1}
              value={timeoutMs}
              disabled={busy}
              onChange={(e) => setTimeoutMs(e.target.value)}
              className="font-mono"
            />
          </Field>
        </div>

        <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
          <Field>
            <FieldLabel htmlFor="hook-matcher">
              {t("devTools.matcher")}
            </FieldLabel>
            <Input
              id="hook-matcher"
              type="text"
              value={matcher}
              disabled={busy}
              onChange={(e) => setMatcher(e.target.value)}
              placeholder={eventMeta(event).matcherPlaceholder}
              className="font-mono text-[13px]"
            />
          </Field>

          <Field>
            <FieldLabel htmlFor="hook-description">
              {t("devTools.summary")}
            </FieldLabel>
            <Input
              id="hook-description"
              type="text"
              value={description}
              disabled={busy}
              onChange={(e) => setDescription(e.target.value)}
            />
          </Field>
        </div>

        {/* Action Kind Switcher */}
        <FieldGroup className="gap-2.5">
          <div className="flex items-center justify-between gap-2">
            <FieldLabel>{t("devTools.action")}</FieldLabel>
            <SettingSelect
              ariaLabel={t("devTools.action")}
              value={actionKind}
              options={[
                { value: "command", label: t("devTools.command") },
                { value: "http_webhook", label: "HTTP Webhook" },
              ]}
              onChange={setActionKind}
              disabled={busy}
            />
          </div>

          {actionKind === "command" ? (
            <FieldGroup className="gap-2.5">
              <Field>
                <FieldLabel htmlFor="hook-command" className="text-[11px]">
                  {t("devTools.executable")}
                </FieldLabel>
                <Input
                  id="hook-command"
                  type="text"
                  value={command}
                  disabled={busy}
                  onChange={(e) => setCommand(e.target.value)}
                  placeholder="rustfmt"
                  className="font-mono text-[13px]"
                />
              </Field>
              <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                <Field>
                  <FieldLabel htmlFor="hook-args" className="text-[11px]">
                    {t("devTools.argsLines")}
                  </FieldLabel>
                  <Textarea
                    id="hook-args"
                    rows={3}
                    value={argsText}
                    disabled={busy}
                    onChange={(e) => setArgsText(e.target.value)}
                    placeholder={"--edition\n2024"}
                    className="min-h-[72px] resize-none px-2.5 py-1.5 font-mono text-[11px]"
                  />
                </Field>
                <Field>
                  <FieldLabel htmlFor="hook-env" className="text-[11px]">
                    {t("devTools.envLines")}
                  </FieldLabel>
                  <Textarea
                    id="hook-env"
                    rows={3}
                    value={envText}
                    disabled={busy}
                    onChange={(e) => setEnvText(e.target.value)}
                    placeholder={"RUST_LOG=warn"}
                    className="min-h-[72px] resize-none px-2.5 py-1.5 font-mono text-[11px]"
                  />
                </Field>
              </div>
              <FieldDescription className="text-[11px]">
                {t("hooks.commandStdinHint")}
                {editing && hasMaskedValues(envText)
                  ? ` ${t("devTools.keepMasked")}`
                  : ""}
              </FieldDescription>
            </FieldGroup>
          ) : (
            <FieldGroup className="gap-2.5">
              <Field>
                <FieldLabel htmlFor="hook-webhook-url" className="text-[11px]">
                  {t("devTools.endpoint")}
                </FieldLabel>
                <Input
                  id="hook-webhook-url"
                  type="url"
                  value={webhookUrl}
                  disabled={busy}
                  onChange={(e) => setWebhookUrl(e.target.value)}
                  placeholder="https://hooks.example.com/orbit"
                  className="font-mono text-[13px]"
                />
              </Field>
              <Field>
                <FieldLabel htmlFor="hook-headers" className="text-[11px]">
                  {t("devTools.headerLines")}
                </FieldLabel>
                <Textarea
                  id="hook-headers"
                  rows={3}
                  value={headersText}
                  disabled={busy}
                  onChange={(e) => setHeadersText(e.target.value)}
                  placeholder={"Authorization: Bearer …"}
                  className="min-h-[72px] resize-none px-2.5 py-1.5 font-mono text-[11px]"
                />
                {editing && hasMaskedValues(headersText) && (
                  <FieldDescription className="text-[11px]">
                    {t("devTools.keepMasked")}
                  </FieldDescription>
                )}
              </Field>
            </FieldGroup>
          )}
        </FieldGroup>

        <Field orientation="horizontal" className="pt-0.5">
          <FieldLabel className="text-ink-primary text-[13px]">
            {t("devTools.enabled")}
          </FieldLabel>
          <ToggleSwitch
            checked={enabled}
            onChange={setEnabled}
            disabled={busy}
            ariaLabel={t("devTools.enabled")}
          />
        </Field>

        {error && (
          <div className="rounded-lg border border-signal-danger-border bg-signal-danger-bg px-3 py-2 text-[13px] text-signal-danger">
            {error}
          </div>
        )}

        <div className="border-border-subtle/70 mt-1 flex items-center justify-between gap-2 border-t pt-3">
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
                <Trash2 className="h-[13px] w-[13px]" />
                {t("common.delete")}
              </Button>
            )}
            {editing && confirmDelete && (
              <div className="flex items-center gap-1.5">
                <Button
                  type="button"
                  variant="destructive"
                  size="xs"
                  disabled={busy}
                  onClick={() => void handleDelete()}
                >
                  {deleting && (
                    <Loader2 className="h-[12px] w-[12px] animate-spin" />
                  )}
                  {t("common.delete")}
                </Button>
                <Button
                  type="button"
                  variant="ghost"
                  size="xs"
                  disabled={busy}
                  onClick={() => setConfirmDelete(false)}
                >
                  {t("common.cancel")}
                </Button>
              </div>
            )}
          </div>

          <div className="flex items-center gap-2">
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={onClose}
            >
              {t("common.cancel")}
            </Button>
            <Button type="submit" size="sm" disabled={busy}>
              {saving && <Loader2 className="h-[12px] w-[12px] animate-spin" />}
              {editing ? t("common.save") : t("hooks.addHook")}
            </Button>
          </div>
        </div>
      </form>
    </SettingDrawer>
  );
}

export const HostHooksSection = memo(function HostHooksSection({
  hostId,
  workspaceCwd: effectiveCwd = null,
}: {
  hostId: string;
  workspaceCwd?: string | null;
}) {
  const { t } = useI18n();
  const isOnline = useHostIsOnline(hostId);
  const scopeKey = `${hostId}::${effectiveCwd ?? "__global__"}`;
  const entry = useOrbitSelector(
    (s) => s.lifecycleHooksByScope[scopeKey] ?? null,
  );
  const hooks = entry?.hooks ?? EMPTY_HOOKS;
  const recentExecutions = entry?.recentExecutions ?? EMPTY_EXECUTIONS;
  const projectTrusted = entry?.projectHooksTrusted ?? false;
  const isLoading = entry?.status === "loading";

  const [editor, setEditor] = useState<{
    open: boolean;
    rule: LifecycleHookRule | null;
  } | null>(null);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [trustBusy, setTrustBusy] = useState(false);
  const [runsExpanded, setRunsExpanded] = useState(false);
  const [actionError, setActionError] = useState<string | null>(null);

  const refresh = useCallback(() => {
    if (!isOnline) return;
    setActionError(null);
    void orbitActions.loadLifecycleHooks(hostId, effectiveCwd);
  }, [isOnline, hostId, effectiveCwd]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  const projectHooks = useMemo(
    () => hooks.filter((h) => h.scope === "project_orbit"),
    [hooks],
  );
  const hasUntrustedProjectHooks =
    Boolean(effectiveCwd) && projectHooks.length > 0 && !projectTrusted;

  const groupedByEvent = useMemo(() => {
    const map = new Map<HookEventKind, LifecycleHookRule[]>();
    for (const h of hooks) {
      if ((h.scope === "project_orbit") !== Boolean(effectiveCwd)) continue;
      const list = map.get(h.event) ?? [];
      list.push(h);
      map.set(h.event, list);
    }
    return HOOK_EVENTS.map((ev) => ({
      event: ev,
      rules: map.get(ev.id) ?? [],
    })).filter((g) => g.rules.length > 0);
  }, [hooks, effectiveCwd]);

  const handleToggle = async (rule: LifecycleHookRule, next: boolean) => {
    if (togglingId) return;
    setActionError(null);
    setTogglingId(`${rule.scope}:${rule.id}`);
    try {
      await orbitActions.upsertLifecycleHook(hostId, effectiveCwd, {
        ...rule,
        enabled: next,
      });
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setTogglingId(null);
    }
  };

  const handleTest = async (rule: LifecycleHookRule) => {
    if (testingId) return;
    setActionError(null);
    const scopedId = `${rule.scope}:${rule.id}`;
    setTestingId(scopedId);
    try {
      const sample = eventMeta(rule.event).samplePayload;
      await orbitActions.testLifecycleHook(
        hostId,
        effectiveCwd,
        scopedId,
        sample,
      );
      setRunsExpanded(true);
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setTestingId(null);
    }
  };

  const handleTrustChange = async (trusted: boolean) => {
    if (!effectiveCwd || trustBusy) return;
    setActionError(null);
    setTrustBusy(true);
    try {
      await orbitActions.setProjectHooksTrust(hostId, effectiveCwd, trusted);
    } catch (err) {
      setActionError(err instanceof Error ? err.message : String(err));
    } finally {
      setTrustBusy(false);
    }
  };

  return (
    <section data-setting-id="hooks.title" className="space-y-2">
      <SettingGroupLabel
        action={
          <div className="flex items-center gap-2">
            <IconButton
              disabled={isLoading || !isOnline}
              tooltip={t("hooks.refreshAria")}
              onClick={refresh}
            >
              <RefreshCw className={cn(isLoading && "animate-spin")} />
            </IconButton>
            <Button
              data-setting-id="hooks.addHook"
              disabled={!isOnline}
              size="sm"
              variant="outline"
              className="text-[13px]"
              onClick={() => {
                setEditor({ open: true, rule: null });
              }}
            >
              <Plus /> {t("hooks.addHook")}
            </Button>
          </div>
        }
      >
        {t("hooks.title")}
      </SettingGroupLabel>

      {!isOnline ? (
        <SettingCard>
          <div className="text-ink-muted px-4 py-6 text-center text-[13px]">
            {t("hooks.offline")}
          </div>
        </SettingCard>
      ) : (
        <>
          {/* Project Trust Banner */}
          {hasUntrustedProjectHooks && (
            <div className="flex items-center justify-between gap-3 rounded-[12px] border border-signal-warning-border bg-signal-warning-bg px-3.5 py-2.5">
              <div className="flex min-w-0 items-start gap-2.5">
                <ShieldAlert className="mt-0.5 h-[15px] w-[15px] shrink-0 text-signal-warning" />
                <div className="min-w-0">
                  <div className="text-ink-primary text-[13px] font-medium">
                    {t("devTools.untrusted")} · .orbit/hooks.json
                  </div>
                  <p className="text-ink-muted text-[11px] leading-snug">
                    {t("devTools.trustHint")}
                  </p>
                </div>
              </div>
              <button
                type="button"
                disabled={trustBusy}
                onClick={() => void handleTrustChange(true)}
                className="inline-flex h-[28px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg bg-action-primary-bg px-2.5 text-[11px] font-medium text-action-primary-fg hover:bg-action-primary-hover disabled:opacity-50"
              >
                {trustBusy && (
                  <Loader2 className="h-[12px] w-[12px] animate-spin" />
                )}
                {t("devTools.trustHooks")}
              </button>
            </div>
          )}

          {Boolean(effectiveCwd) &&
            projectHooks.length > 0 &&
            projectTrusted && (
              <div className="border-border-subtle/80 bg-surface-raised/50 flex items-center justify-between gap-3 rounded-[11px] border px-3.5 py-2">
                <div className="text-ink-muted flex min-w-0 items-center gap-2 text-[13px]">
                  <ShieldCheck className="h-[14px] w-[14px] shrink-0 text-signal-success" />
                  <span className="truncate">{t("devTools.trusted")}</span>
                </div>
                <button
                  type="button"
                  disabled={trustBusy}
                  onClick={() => void handleTrustChange(false)}
                  className="text-ink-muted hover:text-ink-primary shrink-0 cursor-pointer text-[11px] font-medium transition-colors disabled:opacity-50"
                >
                  {t("permissionRules.revokeAction")}
                </button>
              </div>
            )}

          {(actionError || (entry?.status === "error" && entry.error)) && (
            <div className="rounded-lg border border-signal-danger-border bg-signal-danger-bg px-3 py-2 text-[13px] text-signal-danger">
              {actionError ?? entry?.error}
            </div>
          )}

          {/* Event-Grouped Hook List */}
          {isLoading && hooks.length === 0 ? (
            <SettingCard>
              <div className="text-ink-muted flex items-center justify-center gap-2 px-4 py-8 text-[13px]">
                <Loader2 className="h-[14px] w-[14px] animate-spin" />
                <span>{t("hooks.loading")}</span>
              </div>
            </SettingCard>
          ) : groupedByEvent.length === 0 ? (
            <SettingCard>
              <div className="flex flex-col items-center gap-2 px-4 py-8 text-center">
                <p className="text-ink-muted text-[13px]">{t("hooks.empty")}</p>
              </div>
            </SettingCard>
          ) : (
            <div className="space-y-4">
              {groupedByEvent.map(({ event, rules }) => (
                <div key={event.id} className="space-y-1.5">
                  <SettingGroupLabel>
                    {t(event.labelKey)}{" "}
                    <span className="font-normal opacity-70">
                      · {rules.length}
                    </span>
                  </SettingGroupLabel>
                  <SettingCard>
                    {rules.map((rule) => {
                      const isProject = rule.scope === "project_orbit";
                      const untrusted = isProject && !rule.trusted;
                      const scopedKey = `${rule.scope}:${rule.id}`;
                      const isTesting = testingId === scopedKey;
                      const isToggling = Boolean(togglingId);
                      return (
                        <SettingResourceRow
                          key={scopedKey}
                          title={rule.name}
                          icon={
                            rule.action.type === "command" ? (
                              <Terminal />
                            ) : (
                              <Globe />
                            )
                          }
                          description={
                            <span className="font-mono">
                              {formatActionPreview(rule.action)}
                            </span>
                          }
                          status={
                            <span
                              className="block max-w-[160px] truncate"
                              title={rule.matcher ?? undefined}
                            >
                              {[
                                rule.matcher,
                                formatTimeoutLabel(rule.timeout_ms),
                                untrusted ? t("devTools.untrusted") : null,
                              ]
                                .filter(Boolean)
                                .join(" · ")}
                            </span>
                          }
                          onClick={() => {
                            setEditor({ open: true, rule });
                          }}
                          action={
                            <div className="flex items-center gap-2">
                              <IconButton
                                tooltip={t("devTools.test")}
                                disabled={untrusted || Boolean(testingId)}
                                onClick={() => void handleTest(rule)}
                              >
                                {isTesting ? (
                                  <Loader2 className="animate-spin" />
                                ) : (
                                  <Play />
                                )}
                              </IconButton>
                              <ToggleSwitch
                                checked={rule.enabled}
                                disabled={isToggling}
                                ariaLabel={t("devTools.toggle", {
                                  name: rule.name,
                                })}
                                onChange={(enabled) =>
                                  void handleToggle(rule, enabled)
                                }
                              />
                            </div>
                          }
                        />
                      );
                    })}
                  </SettingCard>
                </div>
              ))}
            </div>
          )}

          {/* Collapsible Recent Runs Audit Log */}
          <details
            data-setting-id="devTools.recentRuns"
            className="space-y-1.5 pt-1"
            open={runsExpanded}
            onToggle={(event) => setRunsExpanded(event.currentTarget.open)}
          >
            <summary className="text-ink-muted hover:text-ink-primary flex cursor-pointer items-center gap-1.5 px-0.5 text-[13px] font-medium transition-colors">
              {runsExpanded ? (
                <ChevronDown className="h-[14px] w-[14px]" />
              ) : (
                <ChevronRight className="h-[14px] w-[14px]" />
              )}
              <span>{t("devTools.recentRuns")}</span>
              <span className="tabular-nums opacity-70">
                ({recentExecutions.length})
              </span>
            </summary>

            {runsExpanded && (
              <SettingCard>
                {recentExecutions.length === 0 ? (
                  <div className="text-ink-muted px-4 py-5 text-center text-[13px]">
                    {t("devTools.emptyRuns")}
                  </div>
                ) : (
                  recentExecutions.slice(0, 30).map((run) => {
                    const preview =
                      run.reason || run.stderr_preview || run.stdout_preview;
                    return (
                      <div key={run.id} className="space-y-1 px-4 py-2.5">
                        <div className="flex items-center justify-between gap-2">
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span
                              className={cn(
                                "shrink-0 rounded px-1.5 py-0.5 text-[11px] font-medium",
                                decisionPillClass(run.decision),
                              )}
                            >
                              {decisionLabel(run.decision)}
                            </span>
                            <span className="text-ink-primary truncate text-[13px] font-medium">
                              {run.hook_name}
                            </span>
                            <span className="text-ink-muted shrink-0 text-[11px]">
                              {t(eventMeta(run.event).labelKey)}
                            </span>
                            {run.matched_target && (
                              <span className="text-ink-muted/80 truncate font-mono text-[11px]">
                                ({run.matched_target})
                              </span>
                            )}
                          </div>
                          <div className="text-ink-muted flex shrink-0 items-center gap-2 text-[11px] tabular-nums">
                            {run.exit_code !== null &&
                              run.exit_code !== undefined && (
                                <span className="font-mono">
                                  exit {run.exit_code}
                                </span>
                              )}
                            <span>{run.duration_ms}ms</span>
                            <span className="opacity-70">
                              {formatRelativeTime(run.executed_at_ms)}
                            </span>
                          </div>
                        </div>
                        {preview && (
                          <div className="text-ink-muted bg-surface-recessed/45 truncate rounded px-2 py-1 font-mono text-[11px]">
                            {preview}
                          </div>
                        )}
                      </div>
                    );
                  })
                )}
              </SettingCard>
            )}
          </details>
        </>
      )}

      {editor && (
        <HookEditorDrawer
          open={editor.open}
          onClosed={() => setEditor(null)}
          hasProjectCwd={Boolean(effectiveCwd)}
          editing={editor.rule}
          onClose={() =>
            setEditor((state) => state && { ...state, open: false })
          }
          onSave={async (rule) => {
            await orbitActions.upsertLifecycleHook(hostId, effectiveCwd, rule);
          }}
          onDelete={async (rule) => {
            await orbitActions.deleteLifecycleHook(
              hostId,
              effectiveCwd,
              rule.id,
              rule.scope,
            );
          }}
        />
      )}
    </section>
  );
});
