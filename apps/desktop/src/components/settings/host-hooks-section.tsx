import { useCallback, useEffect, useMemo, useState } from "react";
import {
  Check,
  ChevronDown,
  ChevronRight,
  ChevronsUpDown,
  Globe,
  Loader2,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  ShieldAlert,
  ShieldCheck,
  Terminal,
  Trash2,
  X,
} from "lucide-react";
import {
  SettingCard,
  SettingGroupLabel,
  SettingSectionHeader,
  SettingSelect,
  type SettingSelectOption,
  ToggleSwitch,
} from "@/components/settings/settings-primitives";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/overlays";
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
  label: string;
  hint: string;
  samplePayload: Record<string, unknown>;
}[] = [
  {
    id: "session_start",
    label: "Session Start",
    hint: "When an agent session initializes",
    samplePayload: { provider: "codex", target: "codex" },
  },
  {
    id: "pre_prompt_submit",
    label: "Pre-Prompt Submit",
    hint: "Before dispatching a prompt (can block or modify)",
    samplePayload: { provider: "codex", target: "codex", prompt: "Dry-run test prompt" },
  },
  {
    id: "pre_tool_use",
    label: "Pre-Tool Use",
    hint: "Before an agent invokes a tool",
    samplePayload: { tool: "Bash", target: "Bash" },
  },
  {
    id: "post_tool_use",
    label: "Post-Tool Use",
    hint: "After a tool execution completes",
    samplePayload: { tool: "Bash", target: "Bash" },
  },
  {
    id: "post_file_edit",
    label: "Post-File Edit",
    hint: "After a workspace file is saved or edited",
    samplePayload: { path: "src/lib.rs", target: "src/lib.rs" },
  },
  {
    id: "notification",
    label: "Notification",
    hint: "On agent runtime notices and warnings",
    samplePayload: { level: "warning", target: "warning" },
  },
  {
    id: "turn_stop",
    label: "Turn Stop",
    hint: "When an agent turn settles to idle",
    samplePayload: { provider: "claude_code", target: "claude_code" },
  },
  {
    id: "session_end",
    label: "Session End",
    hint: "When a session exits or is archived",
    samplePayload: { provider: "claude_code", target: "claude_code", reason: "archived" },
  },
];

const EVENT_OPTIONS: SettingSelectOption<HookEventKind>[] = HOOK_EVENTS.map((ev) => ({
  value: ev.id,
  label: ev.label,
  description: ev.hint,
}));

function eventMeta(kind: HookEventKind) {
  return HOOK_EVENTS.find((e) => e.id === kind) ?? HOOK_EVENTS[0];
}

function formatWorkspaceShortName(cwd: string): string {
  const clean = cwd.replace(/\/+$/, "");
  const parts = clean.split("/");
  return parts[parts.length - 1] || clean || "~";
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
  if (diffSec < 5) return "just now";
  if (diffSec < 60) return `${diffSec}s ago`;
  const diffMin = Math.floor(diffSec / 60);
  if (diffMin < 60) return `${diffMin}m ago`;
  const diffHr = Math.floor(diffMin / 60);
  if (diffHr < 24) return `${diffHr}h ago`;
  return `${Math.floor(diffHr / 24)}d ago`;
}

function decisionPillClass(decision: HookDecision): string {
  switch (decision) {
    case "allow":
      return "bg-emerald-500/12 text-emerald-700 dark:text-emerald-300";
    case "block":
      return "bg-rose-500/12 text-rose-700 dark:text-rose-300";
    case "modify":
      return "bg-amber-500/12 text-amber-700 dark:text-amber-300";
  }
}

function decisionLabel(decision: HookDecision): string {
  switch (decision) {
    case "allow":
      return "Allow";
    case "block":
      return "Block";
    case "modify":
      return "Modify";
  }
}

function serializeKeyValueLines(map: Record<string, string>, sep: string): string {
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

function HookEditorModal({
  hasProjectCwd,
  editing,
  onClose,
  onSave,
  onDelete,
}: {
  hasProjectCwd: boolean;
  editing: LifecycleHookRule | null;
  onClose: () => void;
  onSave: (rule: LifecycleHookRule) => Promise<void>;
  onDelete: (rule: LifecycleHookRule) => Promise<void>;
}) {
  const [scope, setScope] = useState<HookScope>(
    () => editing?.scope ?? (hasProjectCwd ? "project_orbit" : "user_global"),
  );
  const [event, setEvent] = useState<HookEventKind>(() => editing?.event ?? "pre_prompt_submit");
  const [name, setName] = useState(() => editing?.name ?? "");
  const [description, setDescription] = useState(() => editing?.description ?? "");
  const [matcher, setMatcher] = useState(() => editing?.matcher ?? "");
  const [timeoutMs, setTimeoutMs] = useState(() => String(editing?.timeout_ms ?? DEFAULT_TIMEOUT_MS));
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
    editing?.action.type === "command" ? serializeKeyValueLines(editing.action.env, "=") : "",
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

  const scopeOptions: SettingSelectOption<HookScope>[] = useMemo(
    () => [
      {
        value: "user_global",
        label: "User (~/.orbit/hooks.json)",
        description: "Global across all workspaces on this host",
      },
      {
        value: "project_orbit",
        label: "Project (.orbit/hooks.json)",
        description: "Scoped to the selected workspace (requires trust)",
        disabled: !hasProjectCwd,
      },
    ],
    [hasProjectCwd],
  );

  useEffect(() => {
    const onKeyDown = (e: KeyboardEvent) => {
      if (e.isComposing || e.keyCode === 229) return;
      if (e.key === "Escape" && !busy) {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, [busy, onClose]);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    const cleanName = name.trim();
    if (!cleanName) {
      setError("Hook name is required.");
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
        setError("Executable command is required.");
        return;
      }
      const args = argsText
        .split("\n")
        .map((line) => line.trim())
        .filter((line) => line.length > 0);
      const parsedEnv = parseKeyValueLines(envText, "=");
      if (parsedEnv.invalidLine) {
        setError(`Invalid env var line (expected KEY=VALUE): ${parsedEnv.invalidLine}`);
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
        setError("Webhook URL is required.");
        return;
      }
      const parsedHeaders = parseKeyValueLines(headersText, ":");
      if (parsedHeaders.invalidLine) {
        setError(`Invalid header line (expected Header: Value): ${parsedHeaders.invalidLine}`);
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
    <div
      onMouseDown={(e) => {
        if (e.target === e.currentTarget && !busy) onClose();
      }}
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/45 p-4 backdrop-blur-[2px]"
    >
      <div
        role="dialog"
        aria-modal="true"
        aria-label={editing ? "Edit lifecycle hook" : "Add lifecycle hook"}
        className="border-border bg-elevated text-foreground floating-surface flex max-h-[88vh] w-full max-w-[540px] flex-col rounded-[14px] border p-5"
      >
        <div className="mb-4 flex items-center justify-between gap-2">
          <div className="min-w-0">
            <h3 className="text-[15px] font-semibold tracking-[-0.01em]">
              {editing ? `Edit ${editing.name}` : "Add Lifecycle Hook"}
            </h3>
            <p className="text-muted-foreground text-[12px]">
              {eventMeta(event).hint}
            </p>
          </div>
          <button
            type="button"
            aria-label="Close"
            disabled={busy}
            onClick={onClose}
            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-[8px] transition-colors"
          >
            <X className="h-[14px] w-[14px]" />
          </button>
        </div>

        <form
          onSubmit={(e) => void handleSubmit(e)}
          className="flex min-h-0 flex-1 flex-col gap-3.5 overflow-y-auto pr-0.5"
        >
          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <label className="text-muted-foreground block text-[12px] font-medium">Name</label>
              <input
                type="text"
                value={name}
                disabled={busy}
                onChange={(e) => setName(e.target.value)}
                placeholder="Format Rust files"
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 h-[32px] w-full rounded-[8px] border px-2.5 text-[12.5px] outline-none"
              />
            </div>

            <div className="space-y-1">
              <label className="text-muted-foreground block text-[12px] font-medium">Scope</label>
              <SettingSelect
                value={scope}
                options={scopeOptions}
                disabled={Boolean(editing) || busy}
                onChange={setScope}
                triggerClassName="h-[32px] w-full"
              />
            </div>
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <label className="text-muted-foreground block text-[12px] font-medium">Event</label>
              <SettingSelect
                value={event}
                options={EVENT_OPTIONS}
                disabled={busy}
                onChange={setEvent}
                triggerClassName="h-[32px] w-full"
              />
            </div>

            <div className="space-y-1">
              <label className="text-muted-foreground block text-[12px] font-medium">
                Timeout (ms, max 30000)
              </label>
              <input
                type="number"
                min={100}
                max={MAX_TIMEOUT_MS}
                step={500}
                value={timeoutMs}
                disabled={busy}
                onChange={(e) => setTimeoutMs(e.target.value)}
                className="border-border bg-background text-foreground focus:border-foreground/35 h-[32px] w-full rounded-[8px] border px-2.5 font-mono text-[12.5px] outline-none"
              />
            </div>
          </div>

          <div className="grid grid-cols-1 gap-3 sm:grid-cols-2">
            <div className="space-y-1">
              <label className="text-muted-foreground block text-[12px] font-medium">
                Matcher <span className="font-normal opacity-70">(optional)</span>
              </label>
              <input
                type="text"
                value={matcher}
                disabled={busy}
                onChange={(e) => setMatcher(e.target.value)}
                placeholder="*.rs|*.ts or codex"
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 h-[32px] w-full rounded-[8px] border px-2.5 font-mono text-[12px] outline-none"
              />
            </div>

            <div className="space-y-1">
              <label className="text-muted-foreground block text-[12px] font-medium">
                Description <span className="font-normal opacity-70">(optional)</span>
              </label>
              <input
                type="text"
                value={description}
                disabled={busy}
                onChange={(e) => setDescription(e.target.value)}
                placeholder="Short summary"
                className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 h-[32px] w-full rounded-[8px] border px-2.5 text-[12.5px] outline-none"
              />
            </div>
          </div>

          {/* Action Kind Switcher */}
          <div className="space-y-2">
            <div className="flex items-center justify-between">
              <label className="text-muted-foreground text-[12px] font-medium">Action</label>
              <div className="border-border bg-muted/45 inline-flex rounded-[8px] border p-0.5">
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setActionKind("command")}
                  className={cn(
                    "inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-[6px] px-2.5 text-[11.5px] font-medium transition-colors",
                    actionKind === "command"
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <Terminal className="h-[11.5px] w-[11.5px]" />
                  Command
                </button>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setActionKind("http_webhook")}
                  className={cn(
                    "inline-flex h-[24px] cursor-pointer items-center gap-1.5 rounded-[6px] px-2.5 text-[11.5px] font-medium transition-colors",
                    actionKind === "http_webhook"
                      ? "bg-background text-foreground shadow-2xs"
                      : "text-muted-foreground hover:text-foreground",
                  )}
                >
                  <Globe className="h-[11.5px] w-[11.5px]" />
                  HTTP Webhook
                </button>
              </div>
            </div>

            {actionKind === "command" ? (
              <div className="space-y-2.5">
                <div className="space-y-1">
                  <label className="text-muted-foreground block text-[11.5px]">
                    Executable (spawned directly, no shell)
                  </label>
                  <input
                    type="text"
                    value={command}
                    disabled={busy}
                    onChange={(e) => setCommand(e.target.value)}
                    placeholder="rustfmt"
                    className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 h-[32px] w-full rounded-[8px] border px-2.5 font-mono text-[12px] outline-none"
                  />
                </div>
                <div className="grid grid-cols-1 gap-2.5 sm:grid-cols-2">
                  <div className="space-y-1">
                    <label className="text-muted-foreground block text-[11.5px]">
                      Arguments <span className="opacity-70">(one per line)</span>
                    </label>
                    <textarea
                      rows={3}
                      value={argsText}
                      disabled={busy}
                      onChange={(e) => setArgsText(e.target.value)}
                      placeholder={"--edition\n2024"}
                      className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 w-full resize-none rounded-[8px] border px-2.5 py-1.5 font-mono text-[11.5px] outline-none"
                    />
                  </div>
                  <div className="space-y-1">
                    <label className="text-muted-foreground block text-[11.5px]">
                      Env Vars <span className="opacity-70">(KEY=VALUE)</span>
                    </label>
                    <textarea
                      rows={3}
                      value={envText}
                      disabled={busy}
                      onChange={(e) => setEnvText(e.target.value)}
                      placeholder={"RUST_LOG=warn"}
                      className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 w-full resize-none rounded-[8px] border px-2.5 py-1.5 font-mono text-[11.5px] outline-none"
                    />
                  </div>
                </div>
                <p className="text-muted-foreground/75 text-[11px] leading-snug">
                  Event JSON is piped to stdin. Exit <code className="font-mono">0</code> allows (or
                  returns JSON decision), exit <code className="font-mono">2</code> blocks.
                  {editing && hasMaskedValues(envText)
                    ? " Leave masked •••• values unchanged to preserve stored secrets."
                    : ""}
                </p>
              </div>
            ) : (
              <div className="space-y-2.5">
                <div className="space-y-1">
                  <label className="text-muted-foreground block text-[11.5px]">Webhook URL</label>
                  <input
                    type="url"
                    value={webhookUrl}
                    disabled={busy}
                    onChange={(e) => setWebhookUrl(e.target.value)}
                    placeholder="https://hooks.example.com/orbit"
                    className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 h-[32px] w-full rounded-[8px] border px-2.5 font-mono text-[12px] outline-none"
                  />
                </div>
                <div className="space-y-1">
                  <label className="text-muted-foreground block text-[11.5px]">
                    Headers <span className="opacity-70">(Header: Value per line)</span>
                  </label>
                  <textarea
                    rows={3}
                    value={headersText}
                    disabled={busy}
                    onChange={(e) => setHeadersText(e.target.value)}
                    placeholder={"Authorization: Bearer …"}
                    className="border-border bg-background text-foreground placeholder:text-muted-foreground/60 focus:border-foreground/35 w-full resize-none rounded-[8px] border px-2.5 py-1.5 font-mono text-[11.5px] outline-none"
                  />
                  {editing && hasMaskedValues(headersText) && (
                    <p className="text-muted-foreground/75 text-[11px] leading-snug">
                      Leave masked •••• values unchanged to preserve stored secrets.
                    </p>
                  )}
                </div>
              </div>
            )}
          </div>

          <div className="flex items-center justify-between pt-0.5">
            <span className="text-foreground text-[12.5px] font-medium">Enabled</span>
            <ToggleSwitch
              checked={enabled}
              onChange={setEnabled}
              disabled={busy}
              ariaLabel="Enable hook"
            />
          </div>

          {error && (
            <div className="rounded-lg border border-rose-500/25 bg-rose-500/[0.07] px-3 py-2 text-[12px] text-rose-700 dark:text-rose-300">
              {error}
            </div>
          )}

          <div className="border-border/70 mt-1 flex items-center justify-between gap-2 border-t pt-3">
            <div>
              {editing && !confirmDelete && (
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => setConfirmDelete(true)}
                  className="text-muted-foreground hover:text-rose-600 dark:hover:text-rose-400 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-2 text-[12px] transition-colors"
                >
                  <Trash2 className="h-[13px] w-[13px]" />
                  Delete
                </button>
              )}
              {editing && confirmDelete && (
                <div className="flex items-center gap-1.5">
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => void handleDelete()}
                    className="inline-flex h-[28px] cursor-pointer items-center gap-1 rounded-lg bg-rose-600 px-2.5 text-[11.5px] font-medium text-white hover:bg-rose-700 disabled:opacity-50"
                  >
                    {deleting && <Loader2 className="h-[11.5px] w-[11.5px] animate-spin" />}
                    Confirm Delete
                  </button>
                  <button
                    type="button"
                    disabled={busy}
                    onClick={() => setConfirmDelete(false)}
                    className="text-muted-foreground hover:text-foreground h-[28px] cursor-pointer px-2 text-[11.5px]"
                  >
                    Cancel
                  </button>
                </div>
              )}
            </div>

            <div className="flex items-center gap-2">
              <button
                type="button"
                disabled={busy}
                onClick={onClose}
                className="border-border bg-background hover:bg-accent text-foreground h-[30px] cursor-pointer rounded-lg border px-3 text-[12px] font-medium transition-colors disabled:opacity-50"
              >
                Cancel
              </button>
              <button
                type="submit"
                disabled={busy}
                className="bg-foreground text-background hover:opacity-90 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[12px] font-medium transition-opacity disabled:opacity-50"
              >
                {saving && <Loader2 className="h-[12px] w-[12px] animate-spin" />}
                {editing ? "Save" : "Add Hook"}
              </button>
            </div>
          </div>
        </form>
      </div>
    </div>
  );
}

export function HostHooksSection({ hostId }: { hostId: string }) {
  const hostCard = useOrbitSelector(
    (s) => s.fleet.hosts.find((h) => h.host_id === hostId) ?? null,
  );
  const hostDetail = useOrbitSelector((s) => s.hostDetails[hostId] ?? null);
  const selectedHostId = useOrbitSelector((s) => s.selectedHostId);
  const selectedWorkspaceCwd = useOrbitSelector((s) => s.selectedWorkspaceCwd);

  const workspaces = useMemo(() => {
    const seen = new Set<string>();
    const list: Array<{ cwd: string; label: string }> = [];
    if (selectedHostId === hostId && selectedWorkspaceCwd && selectedWorkspaceCwd !== "~") {
      seen.add(selectedWorkspaceCwd);
      list.push({
        cwd: selectedWorkspaceCwd,
        label: formatWorkspaceShortName(selectedWorkspaceCwd),
      });
    }
    for (const ws of hostDetail?.workspaces ?? []) {
      if (!ws.cwd || ws.cwd === "~" || ws.archived_at || seen.has(ws.cwd)) continue;
      seen.add(ws.cwd);
      list.push({
        cwd: ws.cwd,
        label: ws.title || formatWorkspaceShortName(ws.cwd),
      });
    }
    for (const proj of hostDetail?.discovered_projects ?? []) {
      if (!proj.path || proj.path === "~" || seen.has(proj.path)) continue;
      seen.add(proj.path);
      list.push({
        cwd: proj.path,
        label: proj.name || formatWorkspaceShortName(proj.path),
      });
    }
    return list;
  }, [hostDetail, hostId, selectedHostId, selectedWorkspaceCwd]);

  const [pickedCwd, setPickedCwd] = useState<string>(() => workspaces[0]?.cwd ?? "");

  useEffect(() => {
    if (workspaces.length === 0) {
      if (pickedCwd) setPickedCwd("");
    } else if (pickedCwd && !workspaces.some((w) => w.cwd === pickedCwd)) {
      setPickedCwd(workspaces[0].cwd);
    }
  }, [pickedCwd, workspaces]);

  const effectiveCwd = pickedCwd || null;
  const scopeKey = `${hostId}::${effectiveCwd ?? "__global__"}`;
  const entry = useOrbitSelector((s) => s.lifecycleHooksByScope[scopeKey] ?? null);
  const hooks = entry?.hooks ?? EMPTY_HOOKS;
  const recentExecutions = entry?.recentExecutions ?? EMPTY_EXECUTIONS;
  const projectTrusted = entry?.projectHooksTrusted ?? false;
  const isLoading = entry?.status === "loading";

  const isOnline =
    hostCard?.connection_state === "online_direct" ||
    hostCard?.connection_state === "online_relay";

  const [modalOpen, setModalOpen] = useState(false);
  const [editingRule, setEditingRule] = useState<LifecycleHookRule | null>(null);
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
  const hasUntrustedProjectHooks = Boolean(effectiveCwd) && projectHooks.length > 0 && !projectTrusted;

  const groupedByEvent = useMemo(() => {
    const map = new Map<HookEventKind, LifecycleHookRule[]>();
    for (const h of hooks) {
      const list = map.get(h.event) ?? [];
      list.push(h);
      map.set(h.event, list);
    }
    return HOOK_EVENTS.map((ev) => ({
      event: ev,
      rules: map.get(ev.id) ?? [],
    })).filter((g) => g.rules.length > 0);
  }, [hooks]);

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
      await orbitActions.testLifecycleHook(hostId, effectiveCwd, scopedId, sample);
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

  const activeWorkspace = workspaces.find((w) => w.cwd === pickedCwd) ?? null;

  return (
    <div className="space-y-5">
      <SettingSectionHeader
        title="Lifecycle Hooks"
        description="Deterministic commands and webhooks triggered on session, prompt, edit, and turn events."
        action={
          isOnline ? (
            <div className="flex items-center gap-1.5">
              <button
                type="button"
                disabled={isLoading}
                onClick={refresh}
                aria-label="Refresh hooks"
                className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[28px] w-[28px] cursor-pointer items-center justify-center rounded-lg border transition-colors disabled:opacity-50"
              >
                <RefreshCw className={cn("h-[12.5px] w-[12.5px]", isLoading && "animate-spin")} />
              </button>
              <button
                type="button"
                onClick={() => {
                  setEditingRule(null);
                  setModalOpen(true);
                }}
                className="bg-foreground text-background hover:opacity-90 inline-flex h-[28px] cursor-pointer items-center gap-1 rounded-lg px-2.5 text-[12px] font-medium transition-opacity"
              >
                <Plus className="h-[13px] w-[13px]" />
                Add Hook
              </button>
            </div>
          ) : undefined
        }
      />

      {!isOnline ? (
        <SettingCard>
          <div className="text-muted-foreground px-4 py-6 text-center text-[13px]">
            Connect to this host to manage lifecycle hooks.
          </div>
        </SettingCard>
      ) : (
        <>
          {/* Scope Picker */}
          <div className="flex items-center justify-between gap-3">
            <span className="text-muted-foreground text-[12px] font-medium">Scope</span>
            <DropdownMenu>
              <DropdownMenuTrigger asChild>
                <button
                  type="button"
                  className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[30px] max-w-[360px] cursor-pointer items-center gap-2 rounded-lg border px-2.5 text-[12.5px] transition-colors outline-none"
                >
                  <span className="truncate font-medium">
                    {activeWorkspace ? activeWorkspace.label : "User Global only"}
                  </span>
                  {pickedCwd && (
                    <span className="text-muted-foreground/75 truncate font-mono text-[11px]">
                      {pickedCwd}
                    </span>
                  )}
                  <ChevronsUpDown className="text-muted-foreground h-[12.5px] w-[12.5px] shrink-0" />
                </button>
              </DropdownMenuTrigger>
              <DropdownMenuContent align="end" className="max-h-[260px] w-[340px] overflow-auto p-1">
                <DropdownMenuItem
                  onSelect={() => setPickedCwd("")}
                  className="flex items-center justify-between gap-2 py-1.5"
                >
                  <span className="min-w-0 flex-1">
                    <span className="block truncate text-[12.5px] font-medium">
                      User Global only
                    </span>
                    <span className="text-muted-foreground block truncate font-mono text-[10.5px]">
                      ~/.orbit/hooks.json
                    </span>
                  </span>
                  {pickedCwd === "" && <Check className="h-[13px] w-[13px] shrink-0" />}
                </DropdownMenuItem>
                {workspaces.map((ws) => (
                  <DropdownMenuItem
                    key={ws.cwd}
                    onSelect={() => setPickedCwd(ws.cwd)}
                    className="flex items-center justify-between gap-2 py-1.5"
                  >
                    <span className="min-w-0 flex-1">
                      <span className="block truncate text-[12.5px] font-medium">{ws.label}</span>
                      <span className="text-muted-foreground block truncate font-mono text-[10.5px]">
                        {ws.cwd}
                      </span>
                    </span>
                    {ws.cwd === pickedCwd && <Check className="h-[13px] w-[13px] shrink-0" />}
                  </DropdownMenuItem>
                ))}
              </DropdownMenuContent>
            </DropdownMenu>
          </div>

          {/* Project Trust Banner */}
          {hasUntrustedProjectHooks && (
            <div className="flex items-center justify-between gap-3 rounded-[12px] border border-amber-500/30 bg-amber-500/[0.07] px-3.5 py-2.5">
              <div className="flex min-w-0 items-start gap-2.5">
                <ShieldAlert className="mt-0.5 h-[15px] w-[15px] shrink-0 text-amber-600 dark:text-amber-400" />
                <div className="min-w-0">
                  <div className="text-foreground text-[12.5px] font-medium">
                    Untrusted project hooks in{" "}
                    <code className="font-mono text-[11.5px]">.orbit/hooks.json</code>
                  </div>
                  <p className="text-muted-foreground text-[11.5px] leading-snug">
                    Repository hooks will not run until you approve their current SHA-256 digest.
                  </p>
                </div>
              </div>
              <button
                type="button"
                disabled={trustBusy}
                onClick={() => void handleTrustChange(true)}
                className="inline-flex h-[28px] shrink-0 cursor-pointer items-center gap-1.5 rounded-lg bg-amber-600 px-2.5 text-[11.5px] font-medium text-white hover:bg-amber-700 disabled:opacity-50"
              >
                {trustBusy && <Loader2 className="h-[11.5px] w-[11.5px] animate-spin" />}
                Trust Project
              </button>
            </div>
          )}

          {Boolean(effectiveCwd) && projectHooks.length > 0 && projectTrusted && (
            <div className="border-border/80 bg-background/50 flex items-center justify-between gap-3 rounded-[11px] border px-3.5 py-2">
              <div className="text-muted-foreground flex min-w-0 items-center gap-2 text-[12px]">
                <ShieldCheck className="h-[14px] w-[14px] shrink-0 text-emerald-600 dark:text-emerald-400" />
                <span className="truncate">
                  Project <code className="font-mono text-[11px]">.orbit/hooks.json</code> is
                  trusted (SHA-256 verified)
                </span>
              </div>
              <button
                type="button"
                disabled={trustBusy}
                onClick={() => void handleTrustChange(false)}
                className="text-muted-foreground hover:text-foreground shrink-0 cursor-pointer text-[11.5px] font-medium transition-colors disabled:opacity-50"
              >
                Revoke
              </button>
            </div>
          )}

          {(actionError || (entry?.status === "error" && entry.error)) && (
            <div className="rounded-lg border border-rose-500/25 bg-rose-500/[0.07] px-3 py-2 text-[12px] text-rose-700 dark:text-rose-300">
              {actionError ?? entry?.error}
            </div>
          )}

          {/* Event-Grouped Hook List */}
          {isLoading && hooks.length === 0 ? (
            <SettingCard>
              <div className="text-muted-foreground flex items-center justify-center gap-2 px-4 py-8 text-[12.5px]">
                <Loader2 className="h-[14px] w-[14px] animate-spin" />
                <span>Loading hooks…</span>
              </div>
            </SettingCard>
          ) : groupedByEvent.length === 0 ? (
            <SettingCard>
              <div className="flex flex-col items-center gap-2 px-4 py-8 text-center">
                <p className="text-muted-foreground text-[12.5px]">
                  No lifecycle hooks configured yet.
                </p>
                <button
                  type="button"
                  onClick={() => {
                    setEditingRule(null);
                    setModalOpen(true);
                  }}
                  className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[28px] cursor-pointer items-center gap-1 rounded-lg border px-2.5 text-[12px] font-medium transition-colors"
                >
                  <Plus className="h-[12.5px] w-[12.5px]" />
                  Add Hook
                </button>
              </div>
            </SettingCard>
          ) : (
            <div className="space-y-4">
              {groupedByEvent.map(({ event, rules }) => (
                <div key={event.id} className="space-y-1.5">
                  <SettingGroupLabel>
                    {event.label}{" "}
                    <span className="font-normal opacity-70">· {rules.length}</span>
                  </SettingGroupLabel>
                  <SettingCard>
                    {rules.map((rule) => {
                      const isProject = rule.scope === "project_orbit";
                      const untrusted = isProject && !rule.trusted;
                      const scopedKey = `${rule.scope}:${rule.id}`;
                      const isTesting = testingId === scopedKey;
                      const isToggling = togglingId === scopedKey;
                      return (
                        <div
                          key={scopedKey}
                          className="flex items-center justify-between gap-3 px-4 py-2.5"
                        >
                          <div className="min-w-0 flex-1">
                            <div className="flex flex-wrap items-center gap-1.5">
                              <span className="text-foreground truncate text-[13px] font-medium">
                                {rule.name}
                              </span>
                              <span className="bg-muted text-muted-foreground shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium">
                                {isProject ? "Project" : "User"}
                              </span>
                              {untrusted && (
                                <span className="rounded bg-amber-500/15 px-1.5 py-0.5 text-[10.5px] font-medium text-amber-700 dark:text-amber-300">
                                  Untrusted
                                </span>
                              )}
                              {rule.matcher && (
                                <span className="border-border text-muted-foreground max-w-[160px] truncate rounded border px-1.5 py-0.5 font-mono text-[10px]">
                                  {rule.matcher}
                                </span>
                              )}
                            </div>
                            <div className="text-muted-foreground mt-0.5 flex items-center gap-1.5 truncate text-[11.5px]">
                              {rule.action.type === "command" ? (
                                <Terminal className="h-[11px] w-[11px] shrink-0 opacity-70" />
                              ) : (
                                <Globe className="h-[11px] w-[11px] shrink-0 opacity-70" />
                              )}
                              <span className="truncate font-mono text-[11px]">
                                {formatActionPreview(rule.action)}
                              </span>
                              <span className="opacity-40">·</span>
                              <span className="shrink-0 tabular-nums opacity-75">
                                {formatTimeoutLabel(rule.timeout_ms)}
                              </span>
                            </div>
                          </div>

                          <div className="flex shrink-0 items-center gap-1.5">
                            <button
                              type="button"
                              disabled={untrusted || Boolean(testingId)}
                              title={
                                untrusted
                                  ? "Trust project hooks before testing"
                                  : `Dry-run ${rule.name}`
                              }
                              onClick={() => void handleTest(rule)}
                              className="border-border bg-background hover:bg-accent text-foreground inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md border px-2 text-[11.5px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-45"
                            >
                              {isTesting ? (
                                <Loader2 className="h-[11px] w-[11px] animate-spin" />
                              ) : (
                                <Play className="h-[10.5px] w-[10.5px]" />
                              )}
                              Test
                            </button>
                            <button
                              type="button"
                              aria-label={`Edit ${rule.name}`}
                              onClick={() => {
                                setEditingRule(rule);
                                setModalOpen(true);
                              }}
                              className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] w-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                            >
                              <Pencil className="h-[12px] w-[12px]" />
                            </button>
                            <ToggleSwitch
                              checked={rule.enabled}
                              disabled={isToggling}
                              ariaLabel={`Toggle ${rule.name}`}
                              onChange={(next) => void handleToggle(rule, next)}
                            />
                          </div>
                        </div>
                      );
                    })}
                  </SettingCard>
                </div>
              ))}
            </div>
          )}

          {/* Collapsible Recent Runs Audit Log */}
          <div className="space-y-1.5 pt-1">
            <button
              type="button"
              onClick={() => setRunsExpanded((v) => !v)}
              className="text-muted-foreground hover:text-foreground flex cursor-pointer items-center gap-1.5 px-0.5 text-[12px] font-medium transition-colors"
            >
              {runsExpanded ? (
                <ChevronDown className="h-[13.5px] w-[13.5px]" />
              ) : (
                <ChevronRight className="h-[13.5px] w-[13.5px]" />
              )}
              <span>Recent Runs</span>
              <span className="tabular-nums opacity-70">({recentExecutions.length})</span>
            </button>

            {runsExpanded && (
              <SettingCard>
                {recentExecutions.length === 0 ? (
                  <div className="text-muted-foreground px-4 py-5 text-center text-[12px]">
                    No hook executions recorded yet. Use <strong>Test</strong> on any rule to run a
                    dry-run check.
                  </div>
                ) : (
                  recentExecutions.slice(0, 30).map((run) => {
                    const preview = run.reason || run.stderr_preview || run.stdout_preview;
                    return (
                      <div key={run.id} className="space-y-1 px-4 py-2.5">
                        <div className="flex items-center justify-between gap-2">
                          <div className="flex min-w-0 items-center gap-1.5">
                            <span
                              className={cn(
                                "shrink-0 rounded px-1.5 py-0.5 text-[10.5px] font-medium",
                                decisionPillClass(run.decision),
                              )}
                            >
                              {decisionLabel(run.decision)}
                            </span>
                            <span className="text-foreground truncate text-[12.5px] font-medium">
                              {run.hook_name}
                            </span>
                            <span className="text-muted-foreground shrink-0 text-[11px]">
                              {eventMeta(run.event).label}
                            </span>
                            {run.matched_target && (
                              <span className="text-muted-foreground/80 truncate font-mono text-[10.5px]">
                                ({run.matched_target})
                              </span>
                            )}
                          </div>
                          <div className="text-muted-foreground flex shrink-0 items-center gap-2 text-[11px] tabular-nums">
                            {run.exit_code !== null && run.exit_code !== undefined && (
                              <span className="font-mono">exit {run.exit_code}</span>
                            )}
                            <span>{run.duration_ms}ms</span>
                            <span className="opacity-70">
                              {formatRelativeTime(run.executed_at_ms)}
                            </span>
                          </div>
                        </div>
                        {preview && (
                          <div className="text-muted-foreground bg-muted/45 truncate rounded px-2 py-1 font-mono text-[11px]">
                            {preview}
                          </div>
                        )}
                      </div>
                    );
                  })
                )}
              </SettingCard>
            )}
          </div>
        </>
      )}

      {modalOpen && (
        <HookEditorModal
          hasProjectCwd={Boolean(effectiveCwd)}
          editing={editingRule}
          onClose={() => {
            setModalOpen(false);
            setEditingRule(null);
          }}
          onSave={async (rule) => {
            await orbitActions.upsertLifecycleHook(hostId, effectiveCwd, rule);
          }}
          onDelete={async (rule) => {
            await orbitActions.deleteLifecycleHook(hostId, effectiveCwd, rule.id, rule.scope);
          }}
        />
      )}
    </div>
  );
}
