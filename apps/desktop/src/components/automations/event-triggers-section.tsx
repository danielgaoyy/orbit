import { memo, useCallback, useEffect, useMemo, useRef, useState } from "react";
import {
  ArrowUpRight,
  Check,
  ChevronRight,
  Copy,
  Pencil,
  Play,
  Plus,
  RefreshCw,
  Trash2,
  Webhook,
} from "lucide-react";
import {
  PROVIDER_LIST,
  formatProviderLabel,
  type EventTriggerDelivery,
  type EventTriggerRecord,
  type ProviderKind,
  type SessionMode,
  type TriggerDispatchTarget,
  type TriggerSourceKind,
  type UpsertEventTriggerRequest,
} from "@/lib/orbit-ipc";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog-tabs";
import { ScrollArea } from "@/components/ui/scroll-area";
import { SimpleTooltip } from "@/components/ui/overlays";
import {
  SettingSelect,
  ToggleSwitch,
  type SettingSelectOption,
} from "@/components/settings/settings-primitives";
import { orbitActions, useOrbitSelector } from "@/state/orbit-store";
import { cn } from "@/lib/utils";

type SimpleSessionMode = "auto" | "accept_edits" | "full_access" | "plan" | "default";

interface AgentChoice {
  agent_id: string;
  title: string;
  cwd: string;
}

interface ProjectChoice {
  path: string;
  name: string;
  branch: string | null;
}

interface EventTriggersSectionProps {
  hostId: string | null;
  availableProviders: ProviderKind[];
  activeAgents: AgentChoice[];
  hostProjects: ProjectChoice[];
  defaultCwd: string;
  defaultProvider: ProviderKind;
}

const SOURCE_OPTIONS: SettingSelectOption<TriggerSourceKind>[] = [
  { value: "github", label: "GitHub", description: "HMAC-SHA256 webhook + PR polling" },
  { value: "slack", label: "Slack", description: "v0 signature + 5m replay window" },
  { value: "linear", label: "Linear", description: "Linear-Signature HMAC-SHA256" },
  { value: "generic_webhook", label: "Webhook", description: "HMAC-SHA256 or Bearer token" },
];

const TARGET_KIND_OPTIONS: SettingSelectOption<"spawn_workspace_agent" | "existing_agent">[] = [
  {
    value: "spawn_workspace_agent",
    label: "Spawn workspace agent",
    description: "Start a fresh session in a project directory",
  },
  {
    value: "existing_agent",
    label: "Existing agent",
    description: "Queue event prompt into an active session",
  },
];

const PERMISSION_OPTIONS: SettingSelectOption<SimpleSessionMode>[] = [
  { value: "auto", label: "Auto", description: "Guardian risk filter" },
  { value: "accept_edits", label: "Accept Edits", description: "Auto-approve file edits" },
  { value: "full_access", label: "Full Access", description: "Unattended edits & commands" },
  { value: "plan", label: "Plan (Read-only)", description: "Inspect without modifying files" },
  { value: "default", label: "Ask", description: "Prompt before tool calls" },
];

const DEFAULT_PROMPT_TEMPLATES: Record<TriggerSourceKind, string> = {
  github:
    "Handle GitHub {{event}} on {{repo}} by {{actor}}.\nTitle: {{title}}\nURL: {{url}}\n\n{{body}}",
  slack: "Respond to Slack {{event}} in {{repo}} from {{actor}}:\n\n{{body}}",
  linear:
    "Triage Linear {{event}} ({{repo}}) by {{actor}}.\nTitle: {{title}}\nURL: {{url}}\n\n{{body}}",
  generic_webhook: "Process webhook event `{{event}}`:\nTitle: {{title}}\n\n{{body}}",
};

const DEFAULT_EVENT_FILTERS: Record<TriggerSourceKind, string> = {
  github: "pull_request.opened, pull_request.synchronize",
  slack: "app_mention",
  linear: "Issue.create",
  generic_webhook: "",
};

function sourceBadgeLabel(source: TriggerSourceKind): string {
  switch (source) {
    case "github":
      return "GitHub";
    case "slack":
      return "Slack";
    case "linear":
      return "Linear";
    case "generic_webhook":
      return "Webhook";
  }
}

function sourceBadgeClass(source: TriggerSourceKind): string {
  switch (source) {
    case "github":
      return "bg-foreground/8 text-foreground/85 border-border/75";
    case "slack":
      return "bg-violet-500/10 text-violet-600 dark:text-violet-300 border-violet-500/20";
    case "linear":
      return "bg-sky-500/10 text-sky-600 dark:text-sky-300 border-sky-500/20";
    case "generic_webhook":
      return "bg-amber-500/10 text-amber-700 dark:text-amber-300 border-amber-500/20";
  }
}

function deliveryStatusClass(status: string): string {
  switch (status) {
    case "dispatched":
      return "bg-emerald-500/12 text-emerald-600 dark:text-emerald-400";
    case "filtered":
    case "duplicate":
      return "bg-muted/80 text-muted-foreground";
    case "rejected_signature":
    case "rate_limited":
    case "error":
      return "bg-rose-500/12 text-rose-600 dark:text-rose-400";
    default:
      return "bg-muted/80 text-muted-foreground";
  }
}

function shortWorkspaceName(cwd: string): string {
  if (!cwd || cwd === "~") return "~";
  const clean = cwd.replace(/\/+$/, "");
  const idx = clean.lastIndexOf("/");
  return idx >= 0 ? clean.slice(idx + 1) || "/" : clean;
}

function formatRelativeTime(ms: number): string {
  const diffSec = Math.max(0, Math.round((Date.now() - ms) / 1000));
  if (diffSec < 60) return `${diffSec}s ago`;
  const mins = Math.round(diffSec / 60);
  if (mins < 60) return `${mins}m ago`;
  const hrs = Math.round(mins / 60);
  if (hrs < 24) return `${hrs}h ago`;
  return new Date(ms).toLocaleDateString();
}

function buildSampleTestEvent(trigger: EventTriggerRecord): {
  eventType: string;
  payload: Record<string, unknown>;
} {
  const firstFilter = trigger.event_filters.find((f) => f.trim() && f.trim() !== "*");
  const cleanFilter = firstFilter?.replace(/\.\*$/, ".opened") ?? null;
  switch (trigger.source) {
    case "github":
      return {
        eventType: cleanFilter ?? "pull_request.opened",
        payload: {
          action: "opened",
          pull_request: {
            title: "Sample pull request verification",
            body: "Triggered via Orbit Event Trigger test.",
            html_url: `https://github.com/${trigger.github_poll?.repo || "owner/repo"}/pull/1`,
          },
          sender: { login: "orbit-test" },
          repository: { full_name: trigger.github_poll?.repo || "owner/repo" },
        },
      };
    case "slack":
      return {
        eventType: cleanFilter ?? "app_mention",
        payload: {
          type: "event_callback",
          event: {
            type: cleanFilter ?? "app_mention",
            user: "U_TEST",
            channel: "C_ENG",
            text: "@orbit Please check the latest staging status.",
          },
        },
      };
    case "linear":
      return {
        eventType: cleanFilter ?? "Issue.create",
        payload: {
          type: "Issue",
          action: "create",
          url: "https://linear.app/team/issue/ENG-1",
          actor: { name: "orbit-test" },
          data: {
            title: "Sample Linear issue",
            description: "Triggered via Orbit Event Trigger test.",
            team: { key: "ENG" },
          },
        },
      };
    case "generic_webhook":
      return {
        eventType: cleanFilter ?? "webhook.test",
        payload: {
          event: cleanFilter ?? "webhook.test",
          title: "Sample webhook event",
          body: "Triggered via Orbit Event Trigger test.",
          actor: "orbit-test",
        },
      };
  }
}

function normalizeSimpleMode(mode: SessionMode): SimpleSessionMode {
  if (typeof mode !== "string") return "accept_edits";
  return mode;
}

export const EventTriggersSection = memo(function EventTriggersSection({
  hostId,
  availableProviders,
  activeAgents,
  hostProjects,
  defaultCwd,
  defaultProvider,
}: EventTriggersSectionProps) {
  const entry = useOrbitSelector((s) => (hostId ? s.eventTriggersByHost[hostId] : undefined));
  const triggers = entry?.triggers ?? [];
  const recentDeliveries = entry?.recentDeliveries ?? [];
  const baseWebhookUrl = entry?.baseWebhookUrl || "http://127.0.0.1:6868";
  const loading = entry?.status === "loading";

  const hostIdRef = useRef<string | null>(hostId);
  hostIdRef.current = hostId;

  const [copiedId, setCopiedId] = useState<string | null>(null);
  const [testingId, setTestingId] = useState<string | null>(null);
  const [togglingId, setTogglingId] = useState<string | null>(null);
  const [confirmDeleteId, setConfirmDeleteId] = useState<string | null>(null);
  const [deliveriesOpen, setDeliveriesOpen] = useState(false);

  // Create / Edit modal state
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingTrigger, setEditingTrigger] = useState<EventTriggerRecord | null>(null);
  const [saving, setSaving] = useState(false);

  const [name, setName] = useState("");
  const [source, setSource] = useState<TriggerSourceKind>("github");
  const [eventFiltersText, setEventFiltersText] = useState(DEFAULT_EVENT_FILTERS.github);
  const [secret, setSecret] = useState("");
  const [keepExistingSecret, setKeepExistingSecret] = useState(false);
  const [enabled, setEnabled] = useState(true);

  const [targetKind, setTargetKind] = useState<"spawn_workspace_agent" | "existing_agent">(
    "spawn_workspace_agent",
  );
  const [targetAgentId, setTargetAgentId] = useState("");
  const [targetCwd, setTargetCwd] = useState("");
  const [targetProvider, setTargetProvider] = useState<ProviderKind>(defaultProvider);
  const [targetModel, setTargetModel] = useState("");
  const [targetMode, setTargetMode] = useState<SimpleSessionMode>("accept_edits");
  const [promptTemplate, setPromptTemplate] = useState(DEFAULT_PROMPT_TEMPLATES.github);

  // GitHub PR Polling Fallback state
  const [pollEnabled, setPollEnabled] = useState(false);
  const [pollRepo, setPollRepo] = useState("");
  const [pollIntervalSecs, setPollIntervalSecs] = useState("120");
  const [pollWatchPrs, setPollWatchPrs] = useState(true);
  const [pollWatchIssues, setPollWatchIssues] = useState(false);
  const [pollWatchCi, setPollWatchCi] = useState(false);
  const [githubToken, setGithubToken] = useState("");
  const [keepExistingToken, setKeepExistingToken] = useState(false);

  useEffect(() => {
    setDialogOpen(false);
    setEditingTrigger(null);
    setConfirmDeleteId(null);
    setDeliveriesOpen(false);
    setSaving(false);
    if (!hostId) return;
    void orbitActions.loadEventTriggers(hostId);
  }, [hostId]);

  const providerOptions = useMemo<SettingSelectOption<ProviderKind>[]>(() => {
    const base =
      availableProviders.length > 0
        ? PROVIDER_LIST.filter((p) => availableProviders.includes(p.id)).map((p) => ({
            value: p.id,
            label: p.label,
          }))
        : PROVIDER_LIST.map((p) => ({ value: p.id, label: p.label }));
    if (!base.some((o) => o.value === targetProvider)) {
      base.push({ value: targetProvider, label: formatProviderLabel(targetProvider) });
    }
    return base;
  }, [availableProviders, targetProvider]);

  const projectOptions = useMemo<SettingSelectOption<string>[]>(() => {
    const opts: SettingSelectOption<string>[] = hostProjects.map((p) => ({
      value: p.path,
      label: p.name,
      description: p.branch ?? undefined,
    }));
    if (targetCwd && !opts.some((o) => o.value === targetCwd)) {
      opts.unshift({ value: targetCwd, label: shortWorkspaceName(targetCwd) });
    }
    return opts;
  }, [hostProjects, targetCwd]);

  const agentOptions = useMemo<SettingSelectOption<string>[]>(() => {
    const opts: SettingSelectOption<string>[] = activeAgents.map((a) => ({
      value: a.agent_id,
      label: a.title || a.agent_id,
      description: shortWorkspaceName(a.cwd),
    }));
    if (targetAgentId && !opts.some((o) => o.value === targetAgentId)) {
      opts.unshift({
        value: targetAgentId,
        label: targetAgentId,
        description: "Not currently active",
      });
    }
    return opts;
  }, [activeAgents, targetAgentId]);

  const openCreateDialog = useCallback(() => {
    setEditingTrigger(null);
    setName("");
    setSource("github");
    setEventFiltersText(DEFAULT_EVENT_FILTERS.github);
    setSecret("");
    setKeepExistingSecret(false);
    setEnabled(true);
    setTargetKind(hostProjects.length > 0 ? "spawn_workspace_agent" : "existing_agent");
    setTargetAgentId(activeAgents[0]?.agent_id ?? "");
    setTargetCwd(hostProjects[0]?.path ?? (defaultCwd !== "~" ? defaultCwd : ""));
    setTargetProvider(defaultProvider);
    setTargetModel("");
    setTargetMode("accept_edits");
    setPromptTemplate(DEFAULT_PROMPT_TEMPLATES.github);
    setPollEnabled(false);
    setPollRepo("");
    setPollIntervalSecs("120");
    setPollWatchPrs(true);
    setPollWatchIssues(false);
    setPollWatchCi(false);
    setGithubToken("");
    setKeepExistingToken(false);
    setDialogOpen(true);
  }, [activeAgents, defaultCwd, defaultProvider, hostProjects]);

  const openEditDialog = useCallback(
    (trigger: EventTriggerRecord) => {
      setEditingTrigger(trigger);
      setName(trigger.name);
      setSource(trigger.source);
      setEventFiltersText(trigger.event_filters.join(", "));
      setSecret("");
      setKeepExistingSecret(trigger.has_secret);
      setEnabled(trigger.enabled);
      if (trigger.target.type === "existing_agent") {
        setTargetKind("existing_agent");
        setTargetAgentId(trigger.target.agent_id);
        setTargetCwd(hostProjects[0]?.path ?? "");
        setTargetProvider(defaultProvider);
        setTargetModel("");
        setTargetMode("accept_edits");
      } else {
        setTargetKind("spawn_workspace_agent");
        setTargetAgentId(activeAgents[0]?.agent_id ?? "");
        setTargetCwd(trigger.target.cwd);
        setTargetProvider(trigger.target.provider);
        setTargetModel(trigger.target.model ?? "");
        setTargetMode(normalizeSimpleMode(trigger.target.mode));
      }
      setPromptTemplate(trigger.prompt_template);
      if (trigger.github_poll) {
        setPollEnabled(true);
        setPollRepo(trigger.github_poll.repo);
        setPollIntervalSecs(String(trigger.github_poll.poll_interval_secs));
        setPollWatchPrs(trigger.github_poll.watch_prs);
        setPollWatchIssues(trigger.github_poll.watch_issues);
        setPollWatchCi(trigger.github_poll.watch_ci_failures);
        setGithubToken("");
        setKeepExistingToken(trigger.github_poll.has_token);
      } else {
        setPollEnabled(false);
        setPollRepo("");
        setPollIntervalSecs("120");
        setPollWatchPrs(true);
        setPollWatchIssues(false);
        setPollWatchCi(false);
        setGithubToken("");
        setKeepExistingToken(false);
      }
      setDialogOpen(true);
    },
    [activeAgents, defaultProvider, hostProjects],
  );

  const handleSourceChange = useCallback(
    (next: TriggerSourceKind) => {
      if (!editingTrigger) {
        if (promptTemplate.trim() === DEFAULT_PROMPT_TEMPLATES[source].trim()) {
          setPromptTemplate(DEFAULT_PROMPT_TEMPLATES[next]);
        }
        if (eventFiltersText.trim() === DEFAULT_EVENT_FILTERS[source].trim()) {
          setEventFiltersText(DEFAULT_EVENT_FILTERS[next]);
        }
      }
      setSource(next);
    },
    [editingTrigger, eventFiltersText, promptTemplate, source],
  );

  const copyWebhookUrl = useCallback(
    async (trigger: EventTriggerRecord) => {
      const fullUrl = `${baseWebhookUrl.replace(/\/+$/, "")}${trigger.webhook_path}`;
      try {
        await navigator.clipboard.writeText(fullUrl);
        setCopiedId(trigger.trigger_id);
        orbitActions.showToast("Webhook URL copied");
        window.setTimeout(() => {
          setCopiedId((cur) => (cur === trigger.trigger_id ? null : cur));
        }, 1500);
      } catch {
        orbitActions.showToast("Failed to copy webhook URL");
      }
    },
    [baseWebhookUrl],
  );

  const handleToggleEnabled = useCallback(
    async (trigger: EventTriggerRecord, nextEnabled: boolean) => {
      if (!hostId || togglingId) return;
      setTogglingId(trigger.trigger_id);
      try {
        await orbitActions.upsertEventTrigger({
          host_id: hostId,
          trigger_id: trigger.trigger_id,
          name: trigger.name,
          source: trigger.source,
          event_filters: trigger.event_filters,
          prompt_template: trigger.prompt_template,
          target: trigger.target,
          secret: null,
          keep_existing_secret: trigger.has_secret,
          github_poll: trigger.github_poll ?? null,
          github_token: null,
          keep_existing_token: Boolean(trigger.github_poll?.has_token),
          enabled: nextEnabled,
        });
      } catch {
        // Error toast already surfaced by orbitActions.upsertEventTrigger
      } finally {
        setTogglingId(null);
      }
    },
    [hostId, togglingId],
  );

  const handleTest = useCallback(
    async (trigger: EventTriggerRecord) => {
      if (!hostId || testingId) return;
      const reqHostId = hostId;
      setTestingId(trigger.trigger_id);
      try {
        const sample = buildSampleTestEvent(trigger);
        await orbitActions.testEventTrigger(
          reqHostId,
          trigger.trigger_id,
          sample.eventType,
          sample.payload,
        );
        if (hostIdRef.current === reqHostId) {
          setDeliveriesOpen(true);
        }
      } catch {
        // Error toast already surfaced by orbitActions.testEventTrigger
      } finally {
        setTestingId(null);
      }
    },
    [hostId, testingId],
  );

  const handleDelete = useCallback(
    async (triggerId: string) => {
      if (!hostId) return;
      setConfirmDeleteId(null);
      try {
        await orbitActions.deleteEventTrigger(hostId, triggerId);
      } catch {
        // Error toast already surfaced by orbitActions.deleteEventTrigger
      }
    },
    [hostId],
  );

  const handleSubmit = useCallback(async () => {
    if (!hostId || saving) return;
    const cleanName = name.trim();
    if (!cleanName) {
      orbitActions.showToast("Enter a trigger name");
      return;
    }
    const cleanPrompt = promptTemplate.trim();
    if (!cleanPrompt) {
      orbitActions.showToast("Enter a prompt template");
      return;
    }

    const isGithubPollActive = source === "github" && pollEnabled;
    const hasEffectiveSecret = Boolean(secret.trim() || keepExistingSecret);
    if (!isGithubPollActive && !hasEffectiveSecret) {
      orbitActions.showToast("Enter a signing secret to authenticate inbound webhooks");
      return;
    }

    let target: TriggerDispatchTarget;
    if (targetKind === "existing_agent") {
      if (!targetAgentId) {
        orbitActions.showToast("Select a target agent");
        return;
      }
      target = { type: "existing_agent", agent_id: targetAgentId };
    } else {
      const cleanCwd = targetCwd.trim();
      if (!cleanCwd || cleanCwd === "~") {
        orbitActions.showToast("Select a registered project workspace");
        return;
      }
      target = {
        type: "spawn_workspace_agent",
        cwd: cleanCwd,
        provider: targetProvider,
        model: targetModel.trim() || null,
        mode: targetMode,
      };
    }

    const parsedFilters = eventFiltersText
      .split(",")
      .map((s) => s.trim())
      .filter(Boolean);

    let githubPollPayload: UpsertEventTriggerRequest["github_poll"] = null;
    if (isGithubPollActive) {
      const cleanRepo = pollRepo.trim();
      if (!cleanRepo || !cleanRepo.includes("/")) {
        orbitActions.showToast("Enter GitHub repository as owner/repo");
        return;
      }
      const interval = Math.max(30, Math.min(3600, Math.floor(Number(pollIntervalSecs) || 120)));
      githubPollPayload = {
        repo: cleanRepo,
        poll_interval_secs: interval,
        watch_prs: pollWatchPrs,
        watch_issues: pollWatchIssues,
        watch_ci_failures: pollWatchCi,
        has_token: Boolean(githubToken.trim() || keepExistingToken),
      };
    }

    const req: UpsertEventTriggerRequest = {
      host_id: hostId,
      trigger_id: editingTrigger?.trigger_id ?? null,
      name: cleanName,
      source,
      event_filters: parsedFilters,
      prompt_template: cleanPrompt,
      target,
      secret: secret.trim() ? secret.trim() : null,
      keep_existing_secret: !secret.trim() && keepExistingSecret,
      github_poll: githubPollPayload,
      github_token: isGithubPollActive && githubToken.trim() ? githubToken.trim() : null,
      keep_existing_token: isGithubPollActive && !githubToken.trim() && keepExistingToken,
      enabled,
    };

    setSaving(true);
    try {
      await orbitActions.upsertEventTrigger(req);
      orbitActions.showToast(editingTrigger ? "Trigger updated" : "Trigger created");
      setDialogOpen(false);
    } catch {
      // Error toast already surfaced by orbitActions.upsertEventTrigger
    } finally {
      setSaving(false);
    }
  }, [
    editingTrigger,
    enabled,
    eventFiltersText,
    githubToken,
    hostId,
    keepExistingSecret,
    keepExistingToken,
    name,
    pollEnabled,
    pollIntervalSecs,
    pollRepo,
    pollWatchCi,
    pollWatchIssues,
    pollWatchPrs,
    promptTemplate,
    saving,
    secret,
    source,
    targetAgentId,
    targetCwd,
    targetKind,
    targetMode,
    targetModel,
    targetProvider,
  ]);

  const activeTriggerCount = useMemo(
    () => triggers.filter((t) => t.enabled).length,
    [triggers],
  );

  return (
    <div className="space-y-2">
      <div className="flex items-center justify-between gap-2 px-0.5">
        <div className="text-muted-foreground/80 flex items-center gap-1.5 text-[11.5px] font-medium">
          <span>Event Triggers</span>
          {triggers.length > 0 && (
            <span className="text-muted-foreground/60 tabular-nums">
              {activeTriggerCount}/{triggers.length} active
            </span>
          )}
        </div>
        <div className="flex items-center gap-1.5">
          {hostId && (
            <SimpleTooltip label="Refresh triggers">
              <button
                type="button"
                aria-label="Refresh triggers"
                disabled={loading}
                onClick={() => void orbitActions.loadEventTriggers(hostId)}
                className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors disabled:opacity-40"
              >
                <RefreshCw className={cn("size-3.5 stroke-[1.75]", loading && "animate-spin")} />
              </button>
            </SimpleTooltip>
          )}
          <button
            type="button"
            disabled={!hostId}
            onClick={openCreateDialog}
            className="border-border/80 bg-elevated/50 hover:bg-accent text-foreground/85 inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-[7px] border px-2 text-[11.5px] font-medium transition-colors disabled:cursor-not-allowed disabled:opacity-40"
          >
            <Plus className="size-3 stroke-[2]" />
            <span>New Trigger</span>
          </button>
        </div>
      </div>

      {triggers.length === 0 ? (
        <div className="border-border/65 text-muted-foreground/75 rounded-xl border border-dashed px-4 py-3 text-[12px]">
          No external event triggers yet. Connect GitHub, Slack, Linear, or generic webhooks to
          dispatch agents automatically.
        </div>
      ) : (
        <div className="border-border/80 bg-elevated/35 divide-border/70 divide-y overflow-hidden rounded-xl border">
          {triggers.map((trigger) => {
            const fullUrl = `${baseWebhookUrl.replace(/\/+$/, "")}${trigger.webhook_path}`;
            const isConfirming = confirmDeleteId === trigger.trigger_id;
            const isTesting = testingId === trigger.trigger_id;
            const isCopied = copiedId === trigger.trigger_id;
            const target = trigger.target;
            const targetDesc =
              target.type === "existing_agent"
                ? activeAgents.find((a) => a.agent_id === target.agent_id)?.title ??
                  target.agent_id
                : `${shortWorkspaceName(target.cwd)} · ${formatProviderLabel(target.provider)}`;

            return (
              <div
                key={trigger.trigger_id}
                className="group flex items-center justify-between gap-3 px-4 py-2.5 transition-colors hover:bg-black/[0.02] dark:hover:bg-white/[0.025]"
              >
                <div className="flex min-w-0 items-start gap-2.5">
                  <Webhook
                    className={cn(
                      "mt-0.5 size-3.5 shrink-0 stroke-[1.75]",
                      trigger.enabled ? "text-foreground/75" : "text-muted-foreground/45",
                    )}
                  />
                  <div className="min-w-0 space-y-0.5">
                    <div className="flex flex-wrap items-center gap-1.5">
                      <span
                        className={cn(
                          "max-w-[220px] truncate text-[13px] font-medium",
                          trigger.enabled ? "text-foreground" : "text-muted-foreground",
                        )}
                      >
                        {trigger.name}
                      </span>
                      <span
                        className={cn(
                          "shrink-0 rounded-md border px-1.5 py-0.5 text-[10.5px] font-medium",
                          sourceBadgeClass(trigger.source),
                        )}
                      >
                        {sourceBadgeLabel(trigger.source)}
                      </span>
                      {trigger.github_poll && (
                        <span className="border-border/75 bg-muted/60 text-muted-foreground shrink-0 rounded-md border px-1.5 py-0.5 font-mono text-[10.5px]">
                          Poll · {trigger.github_poll.repo}
                        </span>
                      )}
                      <span className="text-muted-foreground/70 tabular-nums shrink-0 text-[11px]">
                        {trigger.trigger_count}{" "}
                        {trigger.trigger_count === 1 ? "delivery" : "deliveries"}
                      </span>
                    </div>

                    <div className="text-muted-foreground/80 flex flex-wrap items-center gap-1.5 text-[11.5px]">
                      <button
                        type="button"
                        onClick={() => void copyWebhookUrl(trigger)}
                        title={`Copy ${fullUrl}`}
                        className="hover:text-foreground inline-flex max-w-[280px] cursor-pointer items-center gap-1 truncate font-mono text-[11px] transition-colors"
                      >
                        <span className="truncate">{fullUrl}</span>
                        {isCopied ? (
                          <Check className="size-3 shrink-0 text-emerald-500" />
                        ) : (
                          <Copy className="size-3 shrink-0 opacity-60" />
                        )}
                      </button>
                      <span>·</span>
                      <span className="max-w-[200px] truncate">{targetDesc}</span>
                      {trigger.has_secret ? (
                        <>
                          <span>·</span>
                          <span className="font-mono text-[10.5px] opacity-75">
                            HMAC {trigger.masked_secret ?? "••••"}
                          </span>
                        </>
                      ) : (
                        <>
                          <span>·</span>
                          <span className="text-[10.5px] text-amber-600 dark:text-amber-400">
                            No secret (polling only)
                          </span>
                        </>
                      )}
                    </div>
                  </div>
                </div>

                {hostId && (
                  <div className="flex shrink-0 items-center gap-1.5">
                    {isConfirming ? (
                      <div className="flex items-center gap-1">
                        <button
                          type="button"
                          onClick={() => void handleDelete(trigger.trigger_id)}
                          className="cursor-pointer rounded-md bg-rose-500/12 px-2 py-1 text-[11.5px] font-medium text-rose-600 transition-colors hover:bg-rose-500/20 dark:text-rose-400"
                        >
                          Delete
                        </button>
                        <button
                          type="button"
                          onClick={() => setConfirmDeleteId(null)}
                          className="text-muted-foreground hover:text-foreground cursor-pointer rounded-md px-1.5 py-1 text-[11.5px]"
                        >
                          Cancel
                        </button>
                      </div>
                    ) : (
                      <>
                        <SimpleTooltip label="Send sample test event">
                          <button
                            type="button"
                            disabled={isTesting}
                            onClick={() => void handleTest(trigger)}
                            className="text-muted-foreground hover:text-foreground hover:bg-accent inline-flex h-[26px] cursor-pointer items-center gap-1 rounded-md px-2 text-[11.5px] font-medium transition-colors disabled:opacity-40"
                          >
                            <Play className="size-3 fill-current" />
                            <span>{isTesting ? "Testing…" : "Test"}</span>
                          </button>
                        </SimpleTooltip>

                        <ToggleSwitch
                          checked={trigger.enabled}
                          disabled={togglingId === trigger.trigger_id}
                          onChange={(next) => void handleToggleEnabled(trigger, next)}
                          ariaLabel={`Toggle ${trigger.name}`}
                        />

                        <SimpleTooltip label="Edit trigger">
                          <button
                            type="button"
                            aria-label="Edit trigger"
                            onClick={() => openEditDialog(trigger)}
                            className="text-muted-foreground/75 hover:text-foreground hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors"
                          >
                            <Pencil className="size-3.5 stroke-[1.75]" />
                          </button>
                        </SimpleTooltip>

                        <SimpleTooltip label="Delete trigger">
                          <button
                            type="button"
                            aria-label="Delete trigger"
                            onClick={() => setConfirmDeleteId(trigger.trigger_id)}
                            className="text-muted-foreground/75 hover:bg-accent inline-flex size-[26px] cursor-pointer items-center justify-center rounded-md transition-colors hover:text-rose-500"
                          >
                            <Trash2 className="size-3.5 stroke-[1.75]" />
                          </button>
                        </SimpleTooltip>
                      </>
                    )}
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}

      {/* Collapsible Recent Deliveries Log */}
      {recentDeliveries.length > 0 && (
        <div className="pt-0.5">
          <button
            type="button"
            onClick={() => setDeliveriesOpen((v) => !v)}
            className="text-muted-foreground/80 hover:text-foreground inline-flex cursor-pointer items-center gap-1 px-0.5 text-[11.5px] font-medium transition-colors"
          >
            <ChevronRight
              className={cn(
                "size-3.5 transition-transform duration-150",
                deliveriesOpen && "rotate-90",
              )}
            />
            <span>Recent Deliveries</span>
            <span className="text-muted-foreground/60 tabular-nums">
              ({recentDeliveries.length})
            </span>
          </button>

          {deliveriesOpen && (
            <div className="border-border/75 bg-elevated/25 divide-border/65 mt-1.5 divide-y overflow-hidden rounded-xl border">
              {recentDeliveries.slice(0, 20).map((d: EventTriggerDelivery) => (
                <div
                  key={d.delivery_id}
                  className="flex items-center justify-between gap-3 px-3.5 py-2 text-[11.5px]"
                >
                  <div className="flex min-w-0 items-center gap-2">
                    <span
                      className={cn(
                        "shrink-0 rounded-md px-1.5 py-0.5 font-mono text-[10.5px] font-medium",
                        deliveryStatusClass(d.status),
                      )}
                    >
                      {d.status}
                    </span>
                    <span className="text-muted-foreground/80 shrink-0">
                      {sourceBadgeLabel(d.source)}
                    </span>
                    <span className="text-foreground/90 shrink-0 font-mono text-[11px]">
                      {d.event_type}
                    </span>
                    <span className="text-muted-foreground truncate">{d.summary}</span>
                  </div>
                  <div className="flex shrink-0 items-center gap-2">
                    <span className="text-muted-foreground/65 tabular-nums text-[11px]">
                      {formatRelativeTime(d.received_at_ms)}
                    </span>
                    {hostId && d.dispatched_agent_id && (
                      <button
                        type="button"
                        onClick={() =>
                          orbitActions.selectAgent(hostId, d.dispatched_agent_id!)
                        }
                        className="text-muted-foreground hover:text-foreground inline-flex cursor-pointer items-center gap-0.5 font-medium transition-colors"
                      >
                        <span>Open</span>
                        <ArrowUpRight className="size-3" />
                      </button>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}
        </div>
      )}

      {/* Create / Edit Trigger Dialog */}
      <Dialog
        open={dialogOpen}
        onOpenChange={(nextOpen) => {
          if (!saving) setDialogOpen(nextOpen);
        }}
      >
        <DialogContent
          closeDisabled={saving}
          onKeyDown={(e) => {
            if (e.key === "Escape" && (e.nativeEvent.isComposing || e.keyCode === 229)) {
              e.stopPropagation();
            }
          }}
          className="max-w-[500px] gap-0 p-0"
        >
          <DialogHeader className="px-5 pt-5 pb-3">
            <DialogTitle className="text-[15px]">
              {editingTrigger ? "Edit event trigger" : "New event trigger"}
            </DialogTitle>
            <DialogDescription className="text-[12px]">
              Dispatch an agent from verified webhooks or GitHub PR polling.
            </DialogDescription>
          </DialogHeader>

          <ScrollArea viewportClassName="max-h-[min(72vh,580px)] px-5 pb-4">
            <div className="flex flex-col gap-3.5">
              <div className="grid grid-cols-2 gap-3">
                <label className="flex flex-col gap-1.5 text-[12px]">
                  <span className="text-muted-foreground font-medium">Name</span>
                  <input
                    value={name}
                    onChange={(e) => setName(e.target.value)}
                    placeholder="PR Auto-Review"
                    className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[34px] w-full rounded-lg border px-3 text-[12.5px] outline-none"
                  />
                </label>

                <label className="flex flex-col gap-1.5 text-[12px]">
                  <span className="text-muted-foreground font-medium">Source</span>
                  <SettingSelect
                    value={source}
                    onChange={handleSourceChange}
                    options={SOURCE_OPTIONS}
                  />
                </label>
              </div>

              <label className="flex flex-col gap-1.5 text-[12px]">
                <span className="text-muted-foreground font-medium">
                  Event filters{" "}
                  <span className="font-normal opacity-70">(comma-separated, blank = all)</span>
                </span>
                <input
                  value={eventFiltersText}
                  onChange={(e) => setEventFiltersText(e.target.value)}
                  placeholder="pull_request.opened, issues.*"
                  className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[34px] w-full rounded-lg border px-3 font-mono text-[12px] outline-none"
                />
              </label>

              <div className="flex flex-col gap-1.5 text-[12px]">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-muted-foreground font-medium">
                    Signing secret{" "}
                    <span className="font-normal opacity-70">
                      {keepExistingSecret
                        ? "(stored · leave blank to keep ••••)"
                        : source === "github" && pollEnabled
                          ? "(required for inbound webhooks)"
                          : "(required · HMAC-SHA256 / Bearer)"}
                    </span>
                  </span>
                  {editingTrigger?.has_secret && keepExistingSecret && !secret.trim() && (
                    <button
                      type="button"
                      onClick={() => setKeepExistingSecret(false)}
                      className="text-muted-foreground hover:text-rose-500 cursor-pointer text-[11px] font-medium transition-colors"
                    >
                      Clear
                    </button>
                  )}
                </div>
                <input
                  type="password"
                  value={secret}
                  onChange={(e) => {
                    const nextVal = e.target.value;
                    setSecret(nextVal);
                    if (nextVal.trim()) {
                      setKeepExistingSecret(false);
                    } else if (editingTrigger?.has_secret) {
                      setKeepExistingSecret(true);
                    }
                  }}
                  placeholder={keepExistingSecret ? "••••" : "whsec_..."}
                  className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[34px] w-full rounded-lg border px-3 font-mono text-[12px] outline-none"
                />
              </div>

              {/* Optional GitHub PR Polling Fallback */}
              {source === "github" && (
                <div className="border-border/75 bg-elevated/35 space-y-3 rounded-xl border p-3">
                  <div className="flex items-center justify-between gap-2">
                    <div>
                      <div className="text-foreground text-[12px] font-medium">
                        GitHub PR polling fallback
                      </div>
                      <div className="text-muted-foreground text-[11px]">
                        Poll GitHub REST API with ETag when webhooks cannot reach this host.
                      </div>
                    </div>
                    <ToggleSwitch
                      checked={pollEnabled}
                      onChange={setPollEnabled}
                      ariaLabel="Enable GitHub polling fallback"
                    />
                  </div>

                  {pollEnabled && (
                    <div className="space-y-2.5 pt-1">
                      <div className="grid grid-cols-[1fr_110px] gap-2.5">
                        <label className="flex flex-col gap-1 text-[11.5px]">
                          <span className="text-muted-foreground font-medium">Repository</span>
                          <input
                            value={pollRepo}
                            onChange={(e) => setPollRepo(e.target.value)}
                            placeholder="owner/repo"
                            className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[32px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none"
                          />
                        </label>
                        <label className="flex flex-col gap-1 text-[11.5px]">
                          <span className="text-muted-foreground font-medium">Interval (s)</span>
                          <input
                            type="number"
                            min={30}
                            max={3600}
                            value={pollIntervalSecs}
                            onChange={(e) => setPollIntervalSecs(e.target.value)}
                            className="border-border bg-canvas text-foreground focus:border-foreground/30 h-[32px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none"
                          />
                        </label>
                      </div>

                      <div className="flex flex-wrap items-center gap-3 text-[11.5px]">
                        <label className="inline-flex cursor-pointer items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={pollWatchPrs}
                            onChange={(e) => setPollWatchPrs(e.target.checked)}
                            className="accent-foreground"
                          />
                          <span>Pull requests</span>
                        </label>
                        <label className="inline-flex cursor-pointer items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={pollWatchIssues}
                            onChange={(e) => setPollWatchIssues(e.target.checked)}
                            className="accent-foreground"
                          />
                          <span>Issues</span>
                        </label>
                        <label className="inline-flex cursor-pointer items-center gap-1.5">
                          <input
                            type="checkbox"
                            checked={pollWatchCi}
                            onChange={(e) => setPollWatchCi(e.target.checked)}
                            className="accent-foreground"
                          />
                          <span>CI failures</span>
                        </label>
                      </div>

                      <div className="flex flex-col gap-1 text-[11.5px]">
                        <div className="flex items-center justify-between gap-2">
                          <span className="text-muted-foreground font-medium">
                            GitHub token{" "}
                            <span className="font-normal opacity-70">
                              {keepExistingToken
                                ? "(stored · leave blank to keep)"
                                : "(optional · required for private repos)"}
                            </span>
                          </span>
                          {editingTrigger?.github_poll?.has_token &&
                            keepExistingToken &&
                            !githubToken.trim() && (
                              <button
                                type="button"
                                onClick={() => setKeepExistingToken(false)}
                                className="text-muted-foreground hover:text-rose-500 cursor-pointer text-[11px] font-medium transition-colors"
                              >
                                Clear
                              </button>
                            )}
                        </div>
                        <input
                          type="password"
                          value={githubToken}
                          onChange={(e) => {
                            const nextVal = e.target.value;
                            setGithubToken(nextVal);
                            if (nextVal.trim()) {
                              setKeepExistingToken(false);
                            } else if (editingTrigger?.github_poll?.has_token) {
                              setKeepExistingToken(true);
                            }
                          }}
                          placeholder={keepExistingToken ? "••••" : "ghp_..."}
                          className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[32px] w-full rounded-lg border px-2.5 font-mono text-[12px] outline-none"
                        />
                      </div>
                    </div>
                  )}
                </div>
              )}

              {/* Dispatch Target */}
              <label className="flex flex-col gap-1.5 text-[12px]">
                <span className="text-muted-foreground font-medium">Dispatch target</span>
                <SettingSelect
                  value={targetKind}
                  onChange={setTargetKind}
                  options={TARGET_KIND_OPTIONS}
                />
              </label>

              {targetKind === "existing_agent" ? (
                <label className="flex flex-col gap-1.5 text-[12px]">
                  <span className="text-muted-foreground font-medium">Target agent</span>
                  <SettingSelect
                    value={targetAgentId}
                    onChange={setTargetAgentId}
                    options={agentOptions}
                    placeholder="Select active agent"
                  />
                </label>
              ) : (
                <div className="space-y-3">
                  <label className="flex flex-col gap-1.5 text-[12px]">
                    <span className="text-muted-foreground font-medium">Project workspace</span>
                    <SettingSelect
                      value={targetCwd}
                      onChange={setTargetCwd}
                      options={projectOptions}
                      placeholder="Select workspace"
                    />
                  </label>

                  <div className="grid grid-cols-2 gap-3">
                    <label className="flex flex-col gap-1.5 text-[12px]">
                      <span className="text-muted-foreground font-medium">Provider</span>
                      <SettingSelect
                        value={targetProvider}
                        onChange={setTargetProvider}
                        options={providerOptions}
                      />
                    </label>
                    <label className="flex flex-col gap-1.5 text-[12px]">
                      <span className="text-muted-foreground font-medium">Permissions</span>
                      <SettingSelect
                        value={targetMode}
                        onChange={setTargetMode}
                        options={PERMISSION_OPTIONS}
                      />
                    </label>
                  </div>

                  <label className="flex flex-col gap-1.5 text-[12px]">
                    <span className="text-muted-foreground font-medium">
                      Model <span className="font-normal opacity-70">(optional)</span>
                    </span>
                    <input
                      value={targetModel}
                      onChange={(e) => setTargetModel(e.target.value)}
                      placeholder="Provider default"
                      className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 h-[34px] w-full rounded-lg border px-3 font-mono text-[12px] outline-none"
                    />
                  </label>
                </div>
              )}

              <label className="flex flex-col gap-1.5 text-[12px]">
                <span className="text-muted-foreground font-medium">Prompt template</span>
                <textarea
                  rows={4}
                  value={promptTemplate}
                  onChange={(e) => setPromptTemplate(e.target.value)}
                  className="border-border bg-canvas text-foreground placeholder:text-muted-foreground/50 focus:border-foreground/30 w-full resize-none rounded-lg border px-3 py-2 font-mono text-[12px] outline-none"
                />
                <span className="text-muted-foreground/70 font-mono text-[10.5px]">
                  Variables: {"{{source}} {{event}} {{title}} {{body}} {{url}} {{actor}} {{repo}}"}
                </span>
              </label>

              <div className="flex items-center justify-between pt-0.5">
                <span className="text-muted-foreground text-[12px] font-medium">Enabled</span>
                <ToggleSwitch checked={enabled} onChange={setEnabled} ariaLabel="Trigger enabled" />
              </div>
            </div>
          </ScrollArea>

          <div className="border-border/70 flex items-center justify-end gap-2 border-t px-5 py-3">
            <button
              type="button"
              disabled={saving}
              onClick={() => setDialogOpen(false)}
              className="text-muted-foreground hover:text-foreground h-[30px] cursor-pointer rounded-lg px-3 text-[12.5px] font-medium transition-colors disabled:opacity-40"
            >
              Cancel
            </button>
            <button
              type="button"
              disabled={saving}
              onClick={() => void handleSubmit()}
              className="bg-foreground text-background hover:opacity-90 inline-flex h-[30px] cursor-pointer items-center gap-1.5 rounded-lg px-3.5 text-[12.5px] font-medium transition-opacity disabled:opacity-40"
            >
              {saving ? "Saving…" : editingTrigger ? "Save changes" : "Create trigger"}
            </button>
          </div>
        </DialogContent>
      </Dialog>
    </div>
  );
});
